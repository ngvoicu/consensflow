#![cfg(unix)]

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use cf_panes::arbiter::{EnterTiming, InputArbiter};
use cf_panes::input_queue::{InputQueue, InputWork, PageInputGroup};
use cf_panes::pty::{PaneKey, PaneTable};
use futures_executor::block_on;
use portable_pty::PtySize;

#[allow(
    clippy::unwrap_used,
    reason = "a test fixture failure must fail the test"
)]
fn recorder(count: usize) -> (Arc<PaneTable>, InputQueue, PaneKey, mpsc::Receiver<Vec<u8>>) {
    let panes = Arc::new(PaneTable::new());
    let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
    let inputs = InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter));
    let opened = panes
        .open(
            Path::new("/tmp"),
            &[
                "/bin/sh".to_owned(),
                "-c".to_owned(),
                format!("stty raw -echo; printf READY; dd bs=1 count={count} 2>/dev/null"),
            ],
            &HashMap::new(),
            PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            },
        )
        .unwrap();
    let key = opened.key;
    arbiter.register(&key).unwrap();
    inputs.open(&key).unwrap();
    let (ready_send, ready_receive) = mpsc::channel();
    let (send, receive) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = opened.reader;
        let mut ready = [0; 5];
        reader.read_exact(&mut ready).unwrap();
        assert_eq!(&ready, b"READY");
        ready_send.send(()).unwrap();
        let mut bytes = vec![0; count];
        reader.read_exact(&mut bytes).unwrap();
        send.send(bytes).unwrap();
    });
    ready_receive.recv_timeout(Duration::from_secs(5)).unwrap();
    (panes, inputs, key, receive)
}

#[allow(clippy::unwrap_used, reason = "a test admission must succeed")]
fn admitted(
    inputs: &InputQueue,
    page: &str,
    key: &PaneKey,
    seq: u64,
    bytes: &[u8],
    group: Option<PageInputGroup>,
) -> String {
    inputs
        .enqueue_page(
            page,
            key.clone(),
            seq,
            InputWork::Typed(bytes.to_vec()),
            true,
            group,
        )
        .unwrap()
}

#[allow(clippy::unwrap_used, reason = "a test completion must succeed")]
fn finished(inputs: &InputQueue, page: &str, ticket: &str) {
    block_on(inputs.page_completion(page, ticket).unwrap().receiver).unwrap();
}

#[test]
fn a_delayed_old_page_retry_cannot_write_into_the_new_page() {
    let (panes, inputs, key, received) = recorder(2);
    inputs.begin_page("old").unwrap();
    let first = admitted(&inputs, "old", &key, 1, b"A", None);
    finished(&inputs, "old", &first);
    inputs.begin_page("new").unwrap();
    let late = inputs.enqueue_page(
        "old",
        key.clone(),
        1,
        InputWork::Typed(b"A".to_vec()),
        true,
        None,
    );
    let next = admitted(&inputs, "new", &key, 1, b"B", None);
    finished(&inputs, "new", &next);
    let bytes = received.recv_timeout(Duration::from_secs(5)).unwrap();
    inputs.close_and_drain();
    panes.kill(&key).unwrap();
    assert_eq!(late.unwrap_err(), "pane-input-stale-page");
    assert_eq!(bytes, b"AB");
}

#[test]
fn a_new_pages_equal_first_key_is_not_confused_with_a_stale_call() {
    let (panes, inputs, key, received) = recorder(3);
    inputs.begin_page("old").unwrap();
    let original = admitted(&inputs, "old", &key, 1, b"A", None);
    finished(&inputs, "old", &original);
    inputs.begin_page("new").unwrap();
    let late = inputs.enqueue_page(
        "old",
        key.clone(),
        1,
        InputWork::Typed(b"A".to_vec()),
        true,
        None,
    );
    let fresh = admitted(&inputs, "new", &key, 1, b"A", None);
    let next = admitted(&inputs, "new", &key, 2, b"B", None);
    finished(&inputs, "new", &fresh);
    finished(&inputs, "new", &next);
    let bytes = received.recv_timeout(Duration::from_secs(5)).unwrap();
    inputs.close_and_drain();
    panes.kill(&key).unwrap();
    assert_eq!(late.unwrap_err(), "pane-input-stale-page");
    assert_eq!(bytes, b"AAB");
}

#[test]
fn a_reply_cannot_cross_an_active_paste_on_the_real_pty() {
    let mut paste = b"\x1b[200~".to_vec();
    paste.extend(vec![b'p'; 32761]);
    let cut = paste.len();
    paste.extend_from_slice(b"\x1b[201~");
    let reply = b"\x1b[10;20R";
    let (panes, inputs, key, received) = recorder(paste.len() + reply.len() + 1);
    inputs.begin_page("page").unwrap();
    let start = admitted(
        &inputs,
        "page",
        &key,
        1,
        &paste[..cut],
        Some(PageInputGroup {
            id: 1,
            start: true,
            end: false,
        }),
    );
    finished(&inputs, "page", &start);
    let reply_ticket = inputs
        .enqueue_page(
            "page",
            key.clone(),
            2,
            InputWork::Reply(reply.to_vec()),
            false,
            None,
        )
        .unwrap();
    let reply_done = inputs.page_completion("page", &reply_ticket).unwrap();
    let (sent, done) = mpsc::channel();
    let waiter = thread::spawn(move || sent.send(block_on(reply_done.receiver)).unwrap());
    let early = done.recv_timeout(Duration::from_millis(100));
    let end = admitted(
        &inputs,
        "page",
        &key,
        3,
        &paste[cut..],
        Some(PageInputGroup {
            id: 1,
            start: false,
            end: true,
        }),
    );
    finished(&inputs, "page", &end);
    if early.is_err() {
        done.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
    }
    waiter.join().unwrap();
    let enter = admitted(&inputs, "page", &key, 4, b"\r", None);
    finished(&inputs, "page", &enter);
    let bytes = received.recv_timeout(Duration::from_secs(5)).unwrap();
    inputs.close_and_drain();
    panes.kill(&key).unwrap();
    assert!(
        early.is_err(),
        "the reply was written before the paste ended"
    );
    assert_eq!(bytes, [paste.as_slice(), reply, b"\r"].concat());
}

#[test]
fn page_replacement_releases_an_unfinished_paste_gate() {
    let (panes, inputs, key, received) = recorder(2);
    inputs.begin_page("old").unwrap();
    let start = admitted(
        &inputs,
        "old",
        &key,
        1,
        b"A",
        Some(PageInputGroup {
            id: 1,
            start: true,
            end: false,
        }),
    );
    finished(&inputs, "old", &start);
    inputs.begin_page("new").unwrap();
    let reply = inputs
        .enqueue_page(
            "new",
            key.clone(),
            1,
            InputWork::Reply(b"R".to_vec()),
            false,
            None,
        )
        .unwrap();
    finished(&inputs, "new", &reply);
    let bytes = received.recv_timeout(Duration::from_secs(5)).unwrap();
    inputs.close_and_drain();
    panes.kill(&key).unwrap();
    assert_eq!(bytes, b"AR");
}

#[test]
fn an_interrupted_bracketed_paste_closes_before_fresh_page_typing() {
    let opening = b"\x1b[200~A";
    let expected = [opening.as_slice(), b"\x1b[201~", b"B"].concat();
    let (panes, inputs, key, received) = recorder(expected.len());
    inputs.begin_page("old").unwrap();
    let first = admitted(
        &inputs,
        "old",
        &key,
        1,
        opening,
        Some(PageInputGroup {
            id: 1,
            start: true,
            end: false,
        }),
    );
    finished(&inputs, "old", &first);
    inputs.begin_page("new").unwrap();
    let next = admitted(&inputs, "new", &key, 1, b"B", None);
    finished(&inputs, "new", &next);
    let bytes = received.recv_timeout(Duration::from_secs(2));
    inputs.close_and_drain();
    panes.kill(&key).unwrap();
    assert_eq!(
        bytes.expect("fresh typing follows a closed paste"),
        expected
    );
}

#[test]
fn an_interrupted_bracketed_paste_closes_before_a_terminal_reply() {
    let opening = b"\x1b[200~A";
    let expected = [opening.as_slice(), b"\x1b[201~", b"R", b"B"].concat();
    let (panes, inputs, key, received) = recorder(expected.len());
    inputs.begin_page("old").unwrap();
    let first = admitted(
        &inputs,
        "old",
        &key,
        1,
        opening,
        Some(PageInputGroup {
            id: 1,
            start: true,
            end: false,
        }),
    );
    finished(&inputs, "old", &first);
    inputs.begin_page("new").unwrap();
    let reply = inputs
        .enqueue_page(
            "new",
            key.clone(),
            1,
            InputWork::Reply(b"R".to_vec()),
            false,
            None,
        )
        .unwrap();
    finished(&inputs, "new", &reply);
    let next = admitted(&inputs, "new", &key, 2, b"B", None);
    finished(&inputs, "new", &next);
    let bytes = received.recv_timeout(Duration::from_secs(2));
    inputs.close_and_drain();
    panes.kill(&key).unwrap();
    assert_eq!(bytes.expect("reply follows a closed paste"), expected);
}

#[test]
fn a_completed_paste_is_not_closed_again_after_page_replacement() {
    let expected = b"\x1b[200~A\x1b[201~B";
    let (panes, inputs, key, received) = recorder(expected.len());
    inputs.begin_page("old").unwrap();
    let opening = admitted(
        &inputs,
        "old",
        &key,
        1,
        b"\x1b[200~A",
        Some(PageInputGroup {
            id: 1,
            start: true,
            end: false,
        }),
    );
    let closing = admitted(
        &inputs,
        "old",
        &key,
        2,
        b"\x1b[201~",
        Some(PageInputGroup {
            id: 1,
            start: false,
            end: true,
        }),
    );
    finished(&inputs, "old", &opening);
    finished(&inputs, "old", &closing);
    inputs.begin_page("new").unwrap();
    let next = admitted(&inputs, "new", &key, 1, b"B", None);
    finished(&inputs, "new", &next);
    let bytes = received.recv_timeout(Duration::from_secs(5));
    inputs.close_and_drain();
    panes.kill(&key).unwrap();
    assert_eq!(bytes.unwrap(), expected);
}

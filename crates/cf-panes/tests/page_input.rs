#![cfg(unix)]

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use cf_panes::arbiter::{EnterTiming, InputArbiter};
use cf_panes::input_queue::{InputQueue, InputWork};
use cf_panes::pty::{PaneKey, PaneTable};
use cf_panes::validation::MAX_PAGE_INPUT_BYTES;
use futures_executor::block_on;
use portable_pty::PtySize;

/// The page calls at the host's own epoch, which these tests do not move except by navigating.
trait Pages {
    fn enqueue(
        &self,
        page: &str,
        key: PaneKey,
        sequence: u64,
        work: InputWork,
        human: bool,
    ) -> Result<String, String>;
    fn skip(&self, page: &str, key: &PaneKey, sequence: u64) -> Result<u64, String>;
}

impl Pages for InputQueue {
    fn enqueue(
        &self,
        page: &str,
        key: PaneKey,
        sequence: u64,
        work: InputWork,
        human: bool,
    ) -> Result<String, String> {
        self.enqueue_page(page, self.epoch(), key, sequence, work, human)
    }

    fn skip(&self, page: &str, key: &PaneKey, sequence: u64) -> Result<u64, String> {
        self.skip_page_sequence(page, self.epoch(), key, sequence)
    }
}

#[allow(
    clippy::unwrap_used,
    reason = "a test fixture failure must fail the test"
)]
fn recorder(count: usize) -> (Arc<PaneTable>, InputQueue, PaneKey, mpsc::Receiver<Vec<u8>>) {
    recorder_after(0, count)
}

/// A program that reads nothing for `wait` seconds after it says it is ready.
#[allow(
    clippy::unwrap_used,
    reason = "a test fixture failure must fail the test"
)]
fn recorder_after(
    wait: u64,
    count: usize,
) -> (Arc<PaneTable>, InputQueue, PaneKey, mpsc::Receiver<Vec<u8>>) {
    let panes = Arc::new(PaneTable::new());
    let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
    let inputs = InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter));
    let opened = panes
        .open(
            Path::new("/tmp"),
            &[
                "/bin/sh".to_owned(),
                "-c".to_owned(),
                format!("stty raw -echo; printf READY; sleep {wait}; head -c {count}"),
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
fn admitted(inputs: &InputQueue, page: &str, key: &PaneKey, seq: u64, bytes: &[u8]) -> String {
    inputs
        .enqueue(
            page,
            key.clone(),
            seq,
            InputWork::Typed(bytes.to_vec()),
            true,
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
    let first = admitted(&inputs, "old", &key, 1, b"A");
    finished(&inputs, "old", &first);
    inputs.begin_page("new").unwrap();
    let late = inputs.enqueue("old", key.clone(), 1, InputWork::Typed(b"A".to_vec()), true);
    let next = admitted(&inputs, "new", &key, 1, b"B");
    finished(&inputs, "new", &next);
    let bytes = received.recv_timeout(Duration::from_secs(5)).unwrap();
    inputs.close_and_drain();
    panes.kill(&key).unwrap();
    // The page that went is not heard again: its call is refused, and writes nothing.
    assert_eq!(late.unwrap_err(), "pane-input-stale-page");
    assert_eq!(bytes, b"AB");
}

#[test]
fn a_new_pages_equal_first_key_is_not_confused_with_a_stale_call() {
    let (panes, inputs, key, received) = recorder(3);
    inputs.begin_page("old").unwrap();
    let original = admitted(&inputs, "old", &key, 1, b"A");
    finished(&inputs, "old", &original);
    inputs.begin_page("new").unwrap();
    let late = inputs.enqueue("old", key.clone(), 1, InputWork::Typed(b"A".to_vec()), true);
    let fresh = admitted(&inputs, "new", &key, 1, b"A");
    let next = admitted(&inputs, "new", &key, 2, b"B");
    finished(&inputs, "new", &fresh);
    finished(&inputs, "new", &next);
    let bytes = received.recv_timeout(Duration::from_secs(5)).unwrap();
    inputs.close_and_drain();
    panes.kill(&key).unwrap();
    // The page that went is not heard again: its call is refused, and writes nothing.
    assert_eq!(late.unwrap_err(), "pane-input-stale-page");
    assert_eq!(bytes, b"AAB");
}

fn reply(inputs: &InputQueue, key: &PaneKey, seq: u64, bytes: &[u8]) -> Result<String, String> {
    inputs.enqueue(
        "page",
        key.clone(),
        seq,
        InputWork::Reply(bytes.to_vec()),
        false,
    )
}

/// T-1769's schedule, on the at-most-once rule: A's call is slow and ends
/// unknown at the page, which skips it and goes on with B and a terminal
/// reply in the same order. The original comes late and writes nothing.
#[test]
#[allow(clippy::unwrap_used, reason = "a test step must succeed")]
fn a_late_original_after_its_skip_writes_nothing_and_the_reply_arrives() {
    let (_panes, inputs, key, received) = recorder(4);
    inputs.begin_page("page").unwrap();
    inputs.skip("page", &key, 1).unwrap();
    let b = admitted(&inputs, "page", &key, 2, b"B");
    // A reply has its own writer behind the keys it may pass; B is written
    // before it is admitted so the bytes' order is the admission order.
    finished(&inputs, "page", &b);
    let r = reply(&inputs, &key, 3, b"\x1b[R").unwrap();
    assert_eq!(
        inputs
            .enqueue(
                "page",
                key.clone(),
                1,
                InputWork::Typed(b"A".to_vec()),
                true
            )
            .unwrap_err(),
        "pane-input-sequence-stale"
    );
    finished(&inputs, "page", &r);
    assert_eq!(
        received.recv_timeout(Duration::from_secs(5)).unwrap(),
        b"B\x1b[R"
    );
    inputs.close_and_drain();
}

/// The same schedule with the original arriving before the skip: it is
/// written once, the skip moves nothing, and the next number goes on.
#[test]
#[allow(clippy::unwrap_used, reason = "a test step must succeed")]
fn an_original_that_beat_its_skip_is_written_once() {
    let (_panes, inputs, key, received) = recorder(5);
    inputs.begin_page("page").unwrap();
    let a = admitted(&inputs, "page", &key, 1, b"A");
    inputs.skip("page", &key, 1).unwrap();
    let b = admitted(&inputs, "page", &key, 2, b"B");
    finished(&inputs, "page", &b);
    let r = reply(&inputs, &key, 3, b"\x1b[R").unwrap();
    // The page never learned A's ticket; the others finish as usual.
    assert!(inputs.page_completion("page", &a).is_err());
    finished(&inputs, "page", &r);
    assert_eq!(
        received.recv_timeout(Duration::from_secs(5)).unwrap(),
        b"AB\x1b[R"
    );
    inputs.close_and_drain();
}

const PASTE_OPEN: &[u8] = b"\x1b[200~";
const PASTE_CLOSE: &[u8] = b"\x1b[201~";

fn bracketed(body: &[u8]) -> Vec<u8> {
    [PASTE_OPEN, body, PASTE_CLOSE].concat()
}

/// A paste is one call with both of its markers, so the program gets all of
/// it in one write, and a reply that waits behind it stays whole too.
#[test]
#[allow(clippy::unwrap_used, reason = "a test step must succeed")]
fn a_paste_goes_in_as_one_call_with_both_markers() {
    let paste = bracketed(&[b"echo one\rtwo".as_slice(), &vec![b'p'; 100_000]].concat());
    let (_panes, inputs, key, received) = recorder(paste.len() + 2);
    inputs.begin_page("page").unwrap();
    let ticket = admitted(&inputs, "page", &key, 1, &paste);
    finished(&inputs, "page", &ticket);
    let next = admitted(&inputs, "page", &key, 2, b"\r");
    finished(&inputs, "page", &next);
    let reply = reply(&inputs, &key, 3, b"R").unwrap();
    finished(&inputs, "page", &reply);
    let bytes = received.recv_timeout(Duration::from_secs(5)).unwrap();
    inputs.close_and_drain();
    assert_eq!(bytes, [paste.as_slice(), b"\rR"].concat());
}

/// A terminal reply that comes while a paste is going in waits for all of it:
/// the paste is one write, so the reply is never inside it. The paste has begun when the
/// reader has its first byte, which a reply is not sent before (a sleep left a runner that
/// had not yet started the paste's write to write the reply first).
#[test]
#[allow(clippy::unwrap_used, reason = "a test step must succeed")]
fn a_reply_that_comes_during_a_paste_goes_after_all_of_it() {
    let paste = bracketed(&vec![b'p'; 100_000]);
    let panes = Arc::new(PaneTable::new());
    let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
    let inputs = InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter));
    let (key, begun, finished_reading, release) =
        begun_reader(&panes, &arbiter, &inputs, paste.len() + 1);
    inputs.begin_page("page").unwrap();
    let ticket = admitted(&inputs, "page", &key, 1, &paste);
    assert_eq!(begun.recv_timeout(Duration::from_secs(8)).unwrap(), 0x1b);
    let answer = reply(&inputs, &key, 2, b"R").unwrap();
    release.release();
    finished(&inputs, "page", &ticket);
    finished(&inputs, "page", &answer);
    let bytes = finished_reading
        .recv_timeout(Duration::from_secs(20))
        .unwrap();
    inputs.close_and_drain();
    panes.kill(&key).unwrap();
    assert_eq!(bytes, [paste.as_slice(), b"R"].concat());
}

/// T-1771's schedule: the call that carried the paste is lost, and the page
/// skips it. Nothing of the paste is ever written, not its opening, its
/// payload or its Enter, whether the original comes late or never.
#[test]
#[allow(clippy::unwrap_used, reason = "a test step must succeed")]
fn a_lost_paste_writes_none_of_itself_even_when_it_comes_late() {
    let paste = bracketed(b"echo PASTE_ONLY\rmore");
    let (_panes, inputs, key, received) = recorder(2);
    inputs.begin_page("page").unwrap();
    inputs.skip("page", &key, 1).unwrap();
    let late = inputs.enqueue("page", key.clone(), 1, InputWork::Typed(paste), true);
    assert_eq!(late.unwrap_err(), "pane-input-sequence-stale");
    let z = admitted(&inputs, "page", &key, 2, b"Z");
    finished(&inputs, "page", &z);
    let r = reply(&inputs, &key, 3, b"R").unwrap();
    finished(&inputs, "page", &r);
    assert_eq!(
        received.recv_timeout(Duration::from_secs(5)).unwrap(),
        b"ZR"
    );
    inputs.close_and_drain();
}

/// A paste over the cap is refused whole before anything of it is queued.
/// The refusal is final for its number, and the next key goes in at the next.
#[test]
#[allow(clippy::unwrap_used, reason = "a test step must succeed")]
fn a_paste_over_the_cap_is_refused_whole_and_spends_its_number() {
    let (_panes, inputs, key, received) = recorder(1);
    inputs.begin_page("page").unwrap();
    let refused = inputs.enqueue(
        "page",
        key.clone(),
        1,
        InputWork::Typed(vec![b'x'; MAX_PAGE_INPUT_BYTES + 1]),
        true,
    );
    assert_eq!(
        refused.unwrap_err(),
        format!("pane input exceeds {MAX_PAGE_INPUT_BYTES} bytes")
    );
    let again = inputs.enqueue(
        "page",
        key.clone(),
        1,
        InputWork::Typed(b"x".to_vec()),
        true,
    );
    assert_eq!(again.unwrap_err(), "pane-input-sequence-stale");
    let next = admitted(&inputs, "page", &key, 2, b"A");
    finished(&inputs, "page", &next);
    assert_eq!(received.recv_timeout(Duration::from_secs(5)).unwrap(), b"A");
    inputs.close_and_drain();
}

/// Calls that come as 2, 3, 1 reach the program as A, B, C: the first is not refused as stale,
/// and none overtakes another (D4's red 2, at a real terminal).
#[test]
#[allow(clippy::unwrap_used, reason = "a test step must succeed")]
fn calls_that_come_out_of_order_reach_the_program_in_order() {
    let (_panes, inputs, key, received) = recorder(3);
    inputs.begin_page("page").unwrap();
    let b = admitted(&inputs, "page", &key, 2, b"B");
    let c = admitted(&inputs, "page", &key, 3, b"C");
    let a = admitted(&inputs, "page", &key, 1, b"A");
    for ticket in [a, b, c] {
        finished(&inputs, "page", &ticket);
    }
    assert_eq!(
        received.recv_timeout(Duration::from_secs(5)).unwrap(),
        b"ABC"
    );
    inputs.close_and_drain();
}

/// The reload (D4's red 3, with the real queue and an owned PTY): A's call is on its way when
/// the page goes, B and C had come, and the next page types D at once. The program reads
/// A, B, C, D.
#[test]
#[allow(clippy::unwrap_used, reason = "a test step must succeed")]
fn a_reload_cancels_what_waited_in_order_at_a_real_terminal() {
    let (_panes, inputs, key, received) = recorder(1);
    inputs.begin_page("s:1").unwrap();
    admitted(&inputs, "s:1", &key, 2, b"B");
    admitted(&inputs, "s:1", &key, 3, b"C");
    inputs.end_page().unwrap();
    inputs.begin_page("s:2").unwrap();
    let d = admitted(&inputs, "s:2", &key, 1, b"D");
    // The old page's number 1 comes after the reload: refused, never written.
    assert!(inputs
        .enqueue("s:1", key.clone(), 1, InputWork::Typed(b"A".to_vec()), true)
        .is_err());
    finished(&inputs, "s:2", &d);
    assert_eq!(received.recv_timeout(Duration::from_secs(5)).unwrap(), b"D");
    inputs.close_and_drain();
}

/// The largest paste a window takes goes in whole, in order, with the key
/// typed after it.
#[test]
#[allow(clippy::unwrap_used, reason = "a test step must succeed")]
fn the_largest_paste_goes_in_whole() {
    let body = vec![b'p'; MAX_PAGE_INPUT_BYTES - PASTE_OPEN.len() - PASTE_CLOSE.len()];
    let paste = bracketed(&body);
    assert_eq!(paste.len(), MAX_PAGE_INPUT_BYTES);
    let (_panes, inputs, key, received) = recorder(paste.len() + 1);
    inputs.begin_page("page").unwrap();
    let ticket = admitted(&inputs, "page", &key, 1, &paste);
    finished(&inputs, "page", &ticket);
    let next = admitted(&inputs, "page", &key, 2, b"Z");
    finished(&inputs, "page", &next);
    let bytes = received.recv_timeout(Duration::from_secs(60)).unwrap();
    inputs.close_and_drain();
    assert!(bytes[..paste.len()] == paste[..] && bytes[paste.len()] == b'Z');
}

/// A pane whose program reads one byte, says it did, waits until the test lets it go, then
/// reads the rest (`count` bytes in all): the reader's first byte proves its writer is
/// writing, and the test, not a timer, decides when the program reads on. The last is the
/// way to let it go.
#[allow(
    clippy::unwrap_used,
    reason = "a test fixture failure must fail the test"
)]
#[allow(clippy::disallowed_methods, reason = "a test's own FIFO")]
fn begun_reader(
    panes: &Arc<PaneTable>,
    arbiter: &Arc<InputArbiter>,
    inputs: &InputQueue,
    count: usize,
) -> (PaneKey, mpsc::Receiver<u8>, mpsc::Receiver<Vec<u8>>, Hold) {
    static FIFOS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let hold = std::env::temp_dir().join(format!(
        "cf-panes-hold-{}-{}",
        std::process::id(),
        FIFOS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    assert!(std::process::Command::new("/usr/bin/mkfifo")
        .arg(&hold)
        .status()
        .unwrap()
        .success());
    let opened = panes
        .open(
            Path::new("/tmp"),
            &[
                "/bin/sh".to_owned(),
                "-c".to_owned(),
                if count == 1 {
                    "stty raw -echo; printf READY; dd bs=1 count=1 2>/dev/null".to_owned()
                } else {
                    format!(
                        "stty raw -echo; printf READY; dd bs=1 count=1 2>/dev/null; cat '{}' >/dev/null; head -c {}",
                        hold.display(),
                        count - 1
                    )
                },
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
    let mut reader = opened.reader;
    let mut ready = [0; 5];
    reader.read_exact(&mut ready).unwrap();
    assert_eq!(&ready, b"READY");
    let (first, begun) = mpsc::channel();
    let (done, finished) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = vec![0u8; count];
        reader.read_exact(&mut bytes[..1]).unwrap();
        first.send(bytes[0]).unwrap();
        reader.read_exact(&mut bytes[1..]).unwrap();
        done.send(bytes).unwrap();
    });
    (
        key,
        begun,
        finished,
        Hold {
            path: hold,
            writes: count > 1,
        },
    )
}

/// The FIFO a reader waits on: letting it go writes what the reader waits for, and the FIFO is
/// removed when this goes, whether the test passed or failed.
struct Hold {
    path: std::path::PathBuf,
    writes: bool,
}

impl Hold {
    #[allow(clippy::unwrap_used, reason = "a test step must succeed")]
    fn release(&self) {
        if self.writes {
            std::fs::write(&self.path, b"").unwrap();
        }
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// R13 (and P4-D). The paste has begun, proved by the reader's first byte and not by a sleep;
/// then a terminal reply is admitted on its own allowance and reaches the program after the
/// whole paste, with no tear and no resend.
#[test]
#[allow(clippy::unwrap_used, reason = "a test step must succeed")]
fn a_reply_after_the_pastes_first_byte_reaches_the_program_after_the_whole_paste() {
    let paste = bracketed(&vec![
        b'p';
        MAX_PAGE_INPUT_BYTES
            - PASTE_OPEN.len()
            - PASTE_CLOSE.len()
    ]);
    let panes = Arc::new(PaneTable::new());
    let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
    let inputs = InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter));
    let (key, begun, finished_reading, release) =
        begun_reader(&panes, &arbiter, &inputs, paste.len() + 1);
    inputs.begin_page("page").unwrap();
    let ticket = admitted(&inputs, "page", &key, 1, &paste);
    let first = begun.recv_timeout(Duration::from_secs(8)).unwrap();
    assert_eq!(first, 0x1b, "the reader saw the paste's first byte");
    let answer = reply(&inputs, &key, 2, b"R").unwrap();
    // The reply is admitted while the reader still holds the paste: now it reads on.
    release.release();
    finished(&inputs, "page", &ticket);
    finished(&inputs, "page", &answer);
    let bytes = finished_reading
        .recv_timeout(Duration::from_secs(20))
        .unwrap();
    inputs.close_and_drain();
    panes.kill(&key).unwrap();
    assert_eq!(bytes[..paste.len()], paste[..], "the whole paste, untorn");
    assert_eq!(bytes[paste.len()], b'R');
}

/// R6. A call already handed to a window's writer (a key behind a paste that is writing) is
/// written, in order, though the page reloads; the new page's key follows it; and another
/// window goes on meanwhile.
#[test]
#[allow(clippy::unwrap_used, reason = "a test step must succeed")]
fn a_call_handed_to_a_blocked_writer_is_written_before_the_new_pages_key_and_other_windows_go_on() {
    let paste = bracketed(&vec![
        b'p';
        MAX_PAGE_INPUT_BYTES
            - PASTE_OPEN.len()
            - PASTE_CLOSE.len()
    ]);
    let panes = Arc::new(PaneTable::new());
    let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
    let inputs = InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter));
    let (w, w_begun, w_done, release_w) = begun_reader(&panes, &arbiter, &inputs, paste.len() + 2);
    let (x, _x_begun, x_done, _release_x) = begun_reader(&panes, &arbiter, &inputs, 1);
    inputs.begin_page("s:1").unwrap();
    let paste_ticket = admitted(&inputs, "s:1", &w, 1, &paste);
    assert_eq!(w_begun.recv_timeout(Duration::from_secs(8)).unwrap(), 0x1b);
    // K is handed to the writer's queue behind the paste, and the page reloads.
    let k = admitted(&inputs, "s:1", &w, 2, b"K");
    inputs.end_page().unwrap();
    inputs.begin_page("s:2").unwrap();
    let d = admitted(&inputs, "s:2", &w, 1, b"D");
    // Another window is not held by the blocked one.
    let z = admitted(&inputs, "s:2", &x, 1, b"Z");
    finished(&inputs, "s:2", &z);
    assert_eq!(
        x_done.recv_timeout(Duration::from_secs(3)).unwrap(),
        b"Z",
        "window X gets its key while W is blocked"
    );
    assert!(
        w_done.try_recv().is_err(),
        "W is still blocked on its paste"
    );
    release_w.release();
    finished(&inputs, "s:2", &d);
    let _ = (paste_ticket, k);
    let bytes = w_done.recv_timeout(Duration::from_secs(30)).unwrap();
    inputs.close_and_drain();
    panes.kill(&w).unwrap();
    panes.kill(&x).unwrap();
    assert_eq!(bytes[..paste.len()], paste[..], "the whole paste");
    assert_eq!(&bytes[paste.len()..], b"KD", "then K, then D");
}

//! The host's side of a page that comes back: the boundary a subscription fixes, the credit it
//! settles, the journals of ended windows.

#[cfg(unix)]
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use super::{PaneKey, PaneOutput, PaneTable};
use crate::journal::Journal;
use crate::output_hub::{OutputHub, PaneOutputMessage};
use crate::pane_handlers::forward_journaled;

type Live = Arc<Mutex<Vec<(u64, Vec<u8>)>>>;
type Kept = Arc<Mutex<Vec<(Page, u64, Vec<u8>, bool)>>>;
#[cfg(unix)]
type Seen = Arc<Mutex<Vec<(u64, usize, u64)>>>;

/// A page's sink: what it was given, whether it is still there.
#[derive(Clone)]
struct Page {
    alive: Arc<AtomicBool>,
    got: Live,
}

impl Page {
    fn new() -> Self {
        Self {
            alive: Arc::new(AtomicBool::new(true)),
            got: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn sink(&self) -> crate::output_hub::OutputSink {
        let page = self.clone();
        Arc::new(move |message: PaneOutputMessage| {
            if !page.alive.load(Ordering::SeqCst) {
                return false;
            }
            page.got.lock().unwrap().push((
                message.offset.expect("a journaled message has its offset"),
                message.bytes,
            ));
            true
        })
    }

    fn live_bytes(&self) -> Vec<u8> {
        self.got
            .lock()
            .unwrap()
            .iter()
            .flat_map(|(_, bytes)| bytes.clone())
            .collect()
    }
}

fn read(key: &PaneKey, seq: u64, bytes: &[u8]) -> PaneOutput {
    PaneOutput {
        key: key.clone(),
        seq,
        bytes: bytes.to_vec(),
    }
}

/// A window's consumer as the app runs it, without a pty: reads in, the journal and the hub.
struct Window {
    key: PaneKey,
    journal: Arc<Journal>,
    hub: Arc<OutputHub>,
    send: Option<mpsc::Sender<PaneOutput>>,
    consumer: Option<std::thread::JoinHandle<()>>,
    seq: u64,
}

impl Window {
    fn start(hub: &Arc<OutputHub>) -> Self {
        let key = PaneKey::new("p1-chief", 1);
        let journal = Arc::new(Journal::new((24, 80)));
        let (send, stream) = mpsc::channel();
        let consumer = {
            let (journal, hub) = (Arc::clone(&journal), Arc::clone(hub));
            std::thread::spawn(move || forward_journaled(&stream, &journal, &hub, |_| {}))
        };
        Self {
            key,
            journal,
            hub: Arc::clone(hub),
            send: Some(send),
            consumer: Some(consumer),
            seq: 0,
        }
    }

    fn print(&mut self, bytes: &[u8]) {
        self.seq += 1;
        self.send
            .as_ref()
            .unwrap()
            .send(read(&self.key, self.seq, bytes))
            .unwrap();
    }

    fn end(mut self) -> Arc<Journal> {
        drop(self.send.take());
        self.consumer.take().unwrap().join().unwrap();
        Arc::clone(&self.journal)
    }

    /// A subscription, as `PaneTable::attach_page` makes it for this one window: the history
    /// is what the page is given, the rest comes on the sink.
    fn subscribe(&self, page: &Page) -> (u64, Vec<u8>) {
        let boundaries = self.hub.attach_page(page.sink());
        let boundary = boundaries.get(&self.key).copied().unwrap_or(0);
        self.journal.begin_page(boundary);
        let mut history = Vec::new();
        let mut from = None;
        loop {
            let piece = self.journal.piece(from);
            history.extend_from_slice(&piece.bytes);
            from = Some(piece.from + piece.bytes.len() as u64);
            if piece.done {
                return (boundary, history);
            }
        }
    }
}

fn wait_for(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(std::time::Instant::now() < deadline, "waited for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// One owner for every byte (T-2121 finding 7): what had been handed to a page is the history
/// of the next, replayed muted, and what was parked for it is its first live message.
#[test]
fn a_subscription_splits_the_bytes_into_the_history_and_the_live_path_with_no_byte_twice() {
    let hub = Arc::new(OutputHub::new());
    let mut window = Window::start(&hub);
    let old = Page::new();
    window.subscribe(&old);
    window.print(b"first ");
    window.print(b"second ");
    wait_for("the old page's bytes", || old.live_bytes().len() == 13);
    old.alive.store(false, Ordering::SeqCst);
    window.print(b"\x1b[5n parked");
    wait_for("the journal", || window.journal.end() == 13 + 11);
    let new = Page::new();
    let (boundary, history) = window.subscribe(&new);
    assert_eq!(boundary, 13, "what was handed to a sink that took it");
    assert_eq!(history, b"first second ");
    assert_eq!(
        *new.got.lock().unwrap(),
        vec![(13, b"\x1b[5n parked".to_vec())],
        "the question parked for the new page is the new page's, from the boundary"
    );
    window.print(b" later");
    wait_for("later", || new.live_bytes().len() == 11 + 6);
    assert_eq!(old.live_bytes(), b"first second ");
}

/// The boundary is fixed in the step that swaps the sink, as publication is made: with a
/// consumer publishing all the while and pages subscribing between its messages, each page's
/// history and live messages are the stream, in order, with no byte twice and none missing.
#[test]
fn each_of_many_subscriptions_against_a_running_consumer_has_the_stream_exactly_once() {
    let hub = Arc::new(OutputHub::new());
    let mut window = Window::start(&hub);
    let mut all = Vec::new();
    let pages: Kept = Arc::new(Mutex::new(Vec::new()));
    let done = Arc::new(AtomicBool::new(false));
    let subscriber = {
        let (hub, journal, key) = (
            Arc::clone(&hub),
            Arc::clone(&window.journal),
            window.key.clone(),
        );
        let (pages, done) = (Arc::clone(&pages), Arc::clone(&done));
        std::thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                let page = Page::new();
                let boundaries = hub.attach_page(page.sink());
                let boundary = boundaries.get(&key).copied().unwrap_or(0);
                journal.begin_page(boundary);
                let mut history = Vec::new();
                let mut from = None;
                loop {
                    let piece = journal.piece(from);
                    history.extend_from_slice(&piece.bytes);
                    from = Some(piece.from + piece.bytes.len() as u64);
                    if piece.done {
                        break;
                    }
                }
                // Whether the consumer was still printing when this page subscribed.
                let during = !done.load(Ordering::SeqCst);
                pages
                    .lock()
                    .unwrap()
                    .push((page, boundary, history, during));
                std::thread::sleep(Duration::from_micros(150));
            }
        })
    };
    for i in 0..6000u32 {
        let bytes = format!("<{i}>").into_bytes();
        all.extend_from_slice(&bytes);
        window.print(&bytes);
        if i % 4 == 0 {
            std::thread::sleep(Duration::from_micros(100));
        }
    }
    wait_for("the journal", || window.journal.end() == all.len() as u64);
    done.store(true, Ordering::SeqCst);
    subscriber.join().unwrap();
    let held = pages.lock().unwrap();
    assert!(
        held.len() >= 30,
        "{} subscriptions while it printed",
        held.len()
    );
    let mut seen = 0;
    for (page, boundary, history, _) in held.iter() {
        assert_eq!(history.len() as u64, *boundary);
        assert!(
            history[..] == all[..*boundary as usize],
            "the history is the stream"
        );
        let live = page.got.lock().unwrap().clone();
        let mut at = *boundary;
        for (offset, bytes) in &live {
            assert_eq!(
                *offset, at,
                "the live messages continue at the boundary, in order"
            );
            at += bytes.len() as u64;
        }
        seen += live.len();
        // A page's sink stays attached until the next subscription: what it was handed ends
        // at the next page's boundary, so what it holds is a prefix of the stream's rest.
        let joined: Vec<u8> = live.iter().flat_map(|(_, bytes)| bytes.clone()).collect();
        assert!(joined[..] == all[*boundary as usize..*boundary as usize + joined.len()]);
    }
    assert!(seen > 0, "some pages were given live messages");
    let _ = window.end();
}

#[test]
fn a_window_that_has_not_been_handed_a_byte_has_boundary_zero_and_all_of_it_is_live() {
    let hub = Arc::new(OutputHub::new());
    let mut window = Window::start(&hub);
    window.print(b"parked before any page");
    wait_for("the journal", || window.journal.end() == 22);
    let page = Page::new();
    let (boundary, history) = window.subscribe(&page);
    assert_eq!((boundary, history.len()), (0, 0));
    assert_eq!(page.live_bytes(), b"parked before any page");
}

#[test]
fn the_journals_of_ended_windows_are_sixteen_and_a_page_forgets_one_by_asking() {
    let table = PaneTable::new();
    for generation in 1..=20 {
        let journal = Arc::new(Journal::new((24, 80)));
        journal.append(&[b'a'; 100]);
        journal.finish();
        table.keep_ended_journal(PaneKey::new("p1-chief", generation), journal);
    }
    assert_eq!(table.ended_journals_held(), 16);
    assert!(table.history(&PaneKey::new("p1-chief", 4), None).is_err());
    assert!(table.history(&PaneKey::new("p1-chief", 5), None).is_ok());
    // Asked twice, the same bytes: a page that reloaded before it took the last piece asks again.
    let key = PaneKey::new("p1-chief", 20);
    let first = table.history(&key, None).unwrap();
    assert_eq!(first, table.history(&key, None).unwrap());
    table.forget_ended_journal(&key);
    assert!(table.history(&key, None).is_err());
    assert_eq!(table.ended_journals_held(), 15);
}

/// Credit (T-2121, `four-credit.log`): without a subscription the program stops at 1 MiB of
/// output not acknowledged; a subscription settles what the old page was owed and the program
/// goes on, up to the next MiB, until the new page acknowledges. Unix only: the program is
/// python's.
#[cfg(unix)]
#[test]
fn a_program_stops_at_one_mebibyte_and_a_subscription_settles_the_room_the_old_page_left() {
    use super::PaneEnvironment;
    use std::sync::mpsc::RecvTimeoutError;
    const MIB: usize = 1 << 20;
    let table = Arc::new(PaneTable::new());
    table.keep_journals();
    let hub = OutputHub::new();
    let key = PaneKey::new("credit", 1);
    let program = "import os\nfor c,n in [(b'A',1<<20),(b'B',1<<20),(b'Z',1)]:\n v=memoryview(c*n)\n while v:v=v[os.write(1,v):]\n";
    let pane = table
        .open_streamed_at_while(
            &|| true,
            key.clone(),
            &std::env::current_dir().unwrap(),
            &[
                "/usr/bin/python3".into(),
                "-I".into(),
                "-S".into(),
                "-c".into(),
                program.into(),
            ],
            PaneEnvironment::new(&HashMap::new(), &[]),
            portable_pty::PtySize::default(),
            MIB,
        )
        .unwrap();
    assert!(pane.journal.is_some());
    let take = |want: u8| {
        let mut bytes = 0;
        let mut last = 0;
        while bytes < MIB {
            let got = pane.output.recv_timeout(Duration::from_secs(10)).unwrap();
            assert!(got.bytes.iter().all(|byte| *byte == want));
            assert!(got.seq > last);
            last = got.seq;
            bytes += got.bytes.len();
        }
        assert_eq!(bytes, MIB);
        assert_eq!(
            pane.output
                .recv_timeout(Duration::from_millis(250))
                .unwrap_err(),
            RecvTimeoutError::Timeout,
            "the program waits on its page's room: no read after 1 MiB"
        );
        last
    };
    take(b'A');
    table.attach_page(&hub, Arc::new(|_| true));
    let second = take(b'B');
    table.ack(&key, second).unwrap();
    let last = pane.output.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(last.bytes, b"Z");
    table.ack(&key, last.seq).unwrap();
    let end = table
        .wait_ended(&key, &pane.ended, Duration::from_secs(10))
        .unwrap();
    assert_eq!(end.code, 0);
}

/// No page is let go, and no timer: a page that does not acknowledge holds its window at the
/// room main gives it, 1 MiB, however long it takes, and stays the destination. The
/// acknowledgements, when they come, let the program go on, and every byte reaches the page
/// live and in order (T-2121 finding 2: a stale 250 ms threshold let a page that kept up go).
#[cfg(unix)]
#[test]
fn a_page_that_does_not_acknowledge_is_waited_for_and_never_let_go() {
    use super::PaneEnvironment;
    const MIB: usize = 1 << 20;
    let table = Arc::new(PaneTable::new());
    table.keep_journals();
    let hub = Arc::new(OutputHub::new());
    let key = PaneKey::new("waited", 1);
    let program = "import os\nfor n in [1<<20, 1<<20, 1<<20]:\n v=memoryview(b'A'*n)\n while v:v=v[os.write(1,v):]\n";
    let pane = table
        .open_streamed_at_while(
            &|| true,
            key.clone(),
            &std::env::current_dir().unwrap(),
            &[
                "/usr/bin/python3".into(),
                "-I".into(),
                "-S".into(),
                "-c".into(),
                program.into(),
            ],
            PaneEnvironment::new(&HashMap::new(), &[]),
            portable_pty::PtySize::default(),
            MIB,
        )
        .unwrap();
    let journal = pane.journal.clone().unwrap();
    let got: Seen = Arc::new(Mutex::new(Vec::new()));
    let refused = Arc::new(AtomicBool::new(false));
    table.attach_page(&hub, {
        let (got, refused) = (Arc::clone(&got), Arc::clone(&refused));
        Arc::new(move |message: PaneOutputMessage| {
            if refused.load(Ordering::SeqCst) {
                return false;
            }
            got.lock()
                .unwrap()
                .push((message.offset.unwrap(), message.bytes.len(), message.seq()));
            true
        })
    });
    let consumer = {
        let (journal, hub) = (Arc::clone(&journal), Arc::clone(&hub));
        std::thread::spawn(move || forward_journaled(&pane.output, &journal, &hub, |_| {}))
    };
    let held = |got: &Mutex<Vec<(u64, usize, u64)>>| -> usize {
        got.lock().unwrap().iter().map(|(_, len, _)| len).sum()
    };
    wait_for("the first MiB", || held(&got) == MIB);
    // Longer than the old let-go's first threshold of 2 s: the program is still held and the
    // page is still the destination.
    std::thread::sleep(Duration::from_millis(2600));
    assert_eq!(held(&got), MIB, "the program waits on its page's room");
    // The page draws and acknowledges, a message at a time, as a page does.
    let mut acked = 0;
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while held(&got) < 3 * MIB || acked < got.lock().unwrap().len() {
        assert!(std::time::Instant::now() < deadline, "the stream goes on");
        let seqs: Vec<u64> = got.lock().unwrap().iter().map(|(_, _, seq)| *seq).collect();
        if acked < seqs.len() {
            table.ack(&key, seqs[seqs.len() - 1]).unwrap();
            acked = seqs.len();
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    consumer.join().unwrap();
    let got = got.lock().unwrap();
    let mut at = 0;
    for (offset, len, _) in got.iter() {
        assert_eq!(*offset, at, "in order, from where the last ended");
        at += *len as u64;
    }
    assert_eq!(at as usize, 3 * MIB);
    assert_eq!(journal.published_to(), at);
}

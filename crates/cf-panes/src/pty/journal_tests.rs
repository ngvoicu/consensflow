//! The host's side of a page that comes back: the boundary a subscription fixes, the credit it
//! settles, the journals of ended windows.

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
        let boundary = boundaries
            .get(&self.key)
            .and_then(|taken| taken.end)
            .unwrap_or(0);
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

#[cfg(unix)]
/// A sink that takes every message and notes its offset, length and sequence.
fn seeing(seen: &Seen) -> crate::output_hub::OutputSink {
    let seen = Arc::clone(seen);
    Arc::new(move |message: PaneOutputMessage| {
        seen.lock().unwrap().push((
            message.offset.expect("a journaled message has its offset"),
            message.bytes.len(),
            message.seq(),
        ));
        true
    })
}

#[cfg(unix)]
fn held(seen: &Seen) -> usize {
    seen.lock().unwrap().iter().map(|(_, len, _)| len).sum()
}

#[cfg(unix)]
fn last_seq(seen: &Seen) -> u64 {
    seen.lock().unwrap().last().expect("a message").2
}

#[cfg(unix)]
/// The journal's published bytes once they have held still for 600 ms.
fn settled(journal: &Journal) -> usize {
    let deadline = std::time::Instant::now() + Duration::from_secs(40);
    let mut last = journal.published_to();
    let mut still = std::time::Instant::now();
    while still.elapsed() < Duration::from_millis(600) {
        assert!(
            std::time::Instant::now() < deadline,
            "the window never settled"
        );
        std::thread::sleep(Duration::from_millis(10));
        let now = journal.published_to();
        if now != last {
            last = now;
            still = std::time::Instant::now();
        }
    }
    last as usize
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
                let boundary = boundaries
                    .get(&key)
                    .and_then(|taken| taken.end)
                    .unwrap_or(0);
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
    table.attach_page("page", &OutputHub::new(), Arc::new(|_| true));
    for generation in 1..=20 {
        let journal = Arc::new(Journal::new((24, 80)));
        journal.append(&[b'a'; 100]);
        journal.finish();
        table.keep_ended_journal(PaneKey::new("p1-chief", generation), journal);
    }
    assert_eq!(table.ended_journals_held(), 16);
    assert!(table
        .history("page", &PaneKey::new("p1-chief", 4), None)
        .is_err());
    assert!(table
        .history("page", &PaneKey::new("p1-chief", 5), None)
        .is_ok());
    // Asked twice, the same bytes: a page that reloaded before it took the last piece asks again.
    let key = PaneKey::new("p1-chief", 20);
    let first = table.history("page", &key, None).unwrap();
    assert_eq!(first, table.history("page", &key, None).unwrap());
    assert!(table.forget_consumed_journal(&key, 100));
    assert!(table.history("page", &key, None).is_err());
    assert_eq!(table.ended_journals_held(), 15);
}

/// Credit (T-2121, `four-credit.log`): without an acknowledgement the program stops at 1 MiB
/// handed to its page; a new page's subscription settles what the page before was handed and the
/// program goes on, up to the next MiB, until the new page acknowledges. Unix only: the program
/// is python's.
#[cfg(unix)]
#[test]
fn a_program_stops_at_one_mebibyte_and_a_subscription_settles_what_the_page_before_was_handed() {
    use super::PaneEnvironment;
    const MIB: usize = 1 << 20;
    let table = Arc::new(PaneTable::new());
    table.keep_journals();
    let hub = Arc::new(OutputHub::new());
    let key = PaneKey::new("credit", 1);
    // It stays up after its last byte: a program that ends with output unread loses it.
    let program = "import os,time\nfor c,n in [(b'A',1<<20),(b'B',1<<20),(b'Z',1)]:\n v=memoryview(c*n)\n while v:v=v[os.write(1,v):]\ntime.sleep(60)\n";
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
            crate::pane_handlers::default_backlog_bytes(),
        )
        .unwrap();
    assert!(pane.journal.is_some());
    let journal = pane.journal.clone().unwrap();
    let first: Seen = Arc::new(Mutex::new(Vec::new()));
    table.attach_page("one", &hub, seeing(&first));
    let consumer = {
        let (journal, hub) = (Arc::clone(&journal), Arc::clone(&hub));
        std::thread::spawn(move || forward_journaled(&pane.output, &journal, &hub, |_| {}))
    };
    wait_for("the first MiB", || held(&first) == MIB);
    assert_eq!(settled(&journal), MIB, "no read after 1 MiB unacknowledged");
    let second: Seen = Arc::new(Mutex::new(Vec::new()));
    table.attach_page("two", &hub, seeing(&second));
    wait_for("the second MiB", || held(&second) == MIB);
    assert_eq!(settled(&journal), 2 * MIB);
    table.ack(&key, last_seq(&second)).unwrap();
    wait_for("the last byte", || held(&second) == MIB + 1);
    table.ack(&key, last_seq(&second)).unwrap();
    table.kill(&key).unwrap();
    consumer.join().unwrap();
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
    table.attach_page("page", &hub, {
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

/// T-2229 finding 2: until the page acknowledges a live message the host reads at most one room
/// past the boundary its subscription fixed, so the journal's start can't pass that boundary and
/// the first live question is the page's. Unix only: the program is python's.
#[cfg(unix)]
#[test]
fn a_flood_before_the_first_pull_keeps_the_boundary_in_the_journal_and_the_first_question_live() {
    use super::PaneEnvironment;
    let table = Arc::new(PaneTable::new());
    table.keep_journals();
    let hub = Arc::new(OutputHub::new());
    let key = PaneKey::new("flood", 1);
    // 80 bytes of history, then a question and 8 MiB, which a page that has not pulled cannot
    // have drawn.
    let program = "import os,sys,tty\ntty.setraw(1)\ndef out(b):\n v=memoryview(b)\n while v:v=v[os.write(1,v):]\nout(b'H'*80)\nsys.stdin.buffer.readline()\nout(b'\\x1b[5n'+bytes(8<<20)+b'TAIL')\n";
    let live: Live = Arc::new(Mutex::new(Vec::new()));
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let sink = || -> crate::output_hub::OutputSink {
        let (live, seen) = (Arc::clone(&live), Arc::clone(&seen));
        Arc::new(move |message: PaneOutputMessage| {
            let offset = message.offset.unwrap();
            seen.lock()
                .unwrap()
                .push((offset, message.bytes.len(), message.seq()));
            live.lock().unwrap().push((offset, message.bytes));
            true
        })
    };
    table.attach_page("one", &hub, sink());
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
            crate::pane_handlers::default_backlog_bytes(),
        )
        .unwrap();
    let journal = pane.journal.clone().unwrap();
    let consumer = {
        let (journal, hub) = (Arc::clone(&journal), Arc::clone(&hub));
        std::thread::spawn(move || forward_journaled(&pane.output, &journal, &hub, |_| {}))
    };
    wait_for("the history", || journal.published_to() == 80);
    table
        .ack(&key, seen.lock().unwrap().last().unwrap().2)
        .unwrap();
    // The new page subscribes at 80 and has not pulled; its program floods.
    seen.lock().unwrap().clear();
    live.lock().unwrap().clear();
    let boundary = table.attach_page("two", &hub, sink())[&key];
    assert_eq!(boundary, 80);
    table.write(&key, b"go\n").unwrap();
    // The flood stops at the room, with no acknowledgement.
    settled(&journal);
    let piece = table.history("two", &key, None).unwrap();
    assert!(
        piece.from <= boundary,
        "the history starts at or before the boundary: from {} past B {boundary}",
        piece.from
    );
    // The first live question is the first live bytes, once.
    let got: Vec<u8> = live
        .lock()
        .unwrap()
        .iter()
        .flat_map(|(_, b)| b.clone())
        .collect();
    assert!(got.starts_with(b"\x1b[5n"));
    assert_eq!(live.lock().unwrap()[0].0, boundary);
    let _ = table.kill(&key);
    consumer.join().unwrap();
}

/// T-2229 finding 4: a subscription settles the room only up to what the page before was handed.
/// What was parked stays charged until the page that gets it acknowledges it, so two
/// subscriptions whose sinks refuse leave at most 1 MiB waiting, and the page that works gets at
/// most 1 MiB with no acknowledgement. Unix only: the program is python's.
#[cfg(unix)]
#[test]
fn subscriptions_that_refuse_do_not_grow_the_room_of_a_window() {
    use super::PaneEnvironment;
    const MIB: usize = 1 << 20;
    let table = Arc::new(PaneTable::new());
    table.keep_journals();
    let hub = Arc::new(OutputHub::new());
    let key = PaneKey::new("refused", 1);
    let program = "import os\nv=memoryview(b'A'*(8<<20))\nwhile v:v=v[os.write(1,v):]\n";
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
            crate::pane_handlers::default_backlog_bytes(),
        )
        .unwrap();
    let journal = pane.journal.clone().unwrap();
    let consumer = {
        let (journal, hub) = (Arc::clone(&journal), Arc::clone(&hub));
        std::thread::spawn(move || forward_journaled(&pane.output, &journal, &hub, |_| {}))
    };
    // No page yet: everything is parked, and the program stops at the room.
    assert_eq!(settled(&journal), MIB);
    for _ in 0..2 {
        table.attach_page("refusing", &hub, Arc::new(|_| false));
        assert_eq!(
            settled(&journal),
            MIB,
            "a subscription that refused settled nothing"
        );
    }
    let got: Seen = Arc::new(Mutex::new(Vec::new()));
    table.attach_page("working", &hub, seeing(&got));
    assert_eq!(settled(&journal), MIB, "what was parked is still charged");
    let handed: usize = got.lock().unwrap().iter().map(|(_, len, _)| len).sum();
    assert_eq!(handed, MIB, "at most 1 MiB is handed over unacknowledged");
    // The page that works acknowledges, and the program goes on.
    let seq = got.lock().unwrap().last().unwrap().2;
    table.ack(&key, seq).unwrap();
    assert_eq!(settled(&journal), 2 * MIB);
    let _ = table.kill(&key);
    consumer.join().unwrap();
}

/// T-2275: the flow's count of acknowledged bytes is the journal's offset. Reads are joined into
/// messages (the consumer is slow) and each message is acknowledged by its number: what the
/// journal holds as acknowledged is the offset past that message's last byte. Unix only: the
/// program is python's.
#[cfg(unix)]
#[test]
fn what_is_acknowledged_is_the_offset_past_the_acknowledged_messages_last_byte() {
    use super::PaneEnvironment;
    const MIB: usize = 1 << 20;
    let table = Arc::new(PaneTable::new());
    table.keep_journals();
    let hub = Arc::new(OutputHub::new());
    let key = PaneKey::new("counted", 1);
    let program =
        "import os,time\nfor n in range(3000):\n os.write(1,b'x'*(1+n%97))\ntime.sleep(60)\n";
    let total: usize = (0..3000).map(|n| 1 + n % 97).sum();
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
    table.attach_page("page", &hub, seeing(&got));
    let consumer = {
        let (journal, hub) = (Arc::clone(&journal), Arc::clone(&hub));
        // Slow enough that reads wait and are joined.
        std::thread::spawn(move || {
            forward_journaled(&pane.output, &journal, &hub, |_| {
                std::thread::sleep(Duration::from_micros(300));
            });
        })
    };
    wait_for("all of it", || held(&got) == total);
    let messages = got.lock().unwrap().clone();
    assert!(
        messages.len() < 3000,
        "some reads were joined: {} messages",
        messages.len()
    );
    for (offset, len, seq) in messages {
        table.ack(&key, seq).unwrap();
        assert_eq!(
            journal.acknowledged_to(),
            offset + len as u64,
            "acknowledged by message {seq}"
        );
    }
    table.kill(&key).unwrap();
    consumer.join().unwrap();
}

/// A window with a journal, its consumer as the app's, and the sink its page has: the program
/// prints `HEADER`, then, once told, `ESC [ 5 n TAIL`.
#[cfg(unix)]
struct Rig {
    table: Arc<PaneTable>,
    hub: Arc<OutputHub>,
    key: PaneKey,
    journal: Arc<Journal>,
    consumer: Option<std::thread::JoinHandle<()>>,
    seen: Seen,
    live: Live,
    alive: Arc<AtomicBool>,
}

#[cfg(unix)]
const QUESTION: &[u8] = b"\x1b[5nTAIL";

#[cfg(unix)]
impl Rig {
    fn start(name: &str, backlog: usize, program: &str) -> Self {
        use super::PaneEnvironment;
        let table = Arc::new(PaneTable::new());
        table.keep_journals();
        let hub = Arc::new(OutputHub::new());
        let key = PaneKey::new(name, 1);
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
                portable_pty::PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
                backlog,
            )
            .unwrap();
        let journal = pane.journal.clone().unwrap();
        let consumer = {
            let (journal, hub) = (Arc::clone(&journal), Arc::clone(&hub));
            std::thread::spawn(move || forward_journaled(&pane.output, &journal, &hub, |_| {}))
        };
        Self {
            table,
            hub,
            key,
            journal,
            consumer: Some(consumer),
            seen: Arc::new(Mutex::new(Vec::new())),
            live: Arc::new(Mutex::new(Vec::new())),
            alive: Arc::new(AtomicBool::new(true)),
        }
    }

    /// The page `id` subscribes: what it is handed is kept; its boundary for the window.
    fn subscribe(&mut self, id: &str) -> u64 {
        self.seen = Arc::new(Mutex::new(Vec::new()));
        self.live = Arc::new(Mutex::new(Vec::new()));
        self.alive = Arc::new(AtomicBool::new(true));
        let (seen, live, alive) = (
            Arc::clone(&self.seen),
            Arc::clone(&self.live),
            Arc::clone(&self.alive),
        );
        let boundaries = self.table.attach_page(
            id,
            &self.hub,
            Arc::new(move |message: PaneOutputMessage| {
                if !alive.load(Ordering::SeqCst) {
                    return false;
                }
                let offset = message.offset.unwrap();
                seen.lock()
                    .unwrap()
                    .push((offset, message.bytes.len(), message.seq()));
                live.lock().unwrap().push((offset, message.bytes));
                true
            }),
        );
        boundaries.get(&self.key).copied().unwrap_or(0)
    }

    /// `count` widths one after another, 81 and 80, each at `at`.
    fn widths(&self, count: usize, at: u64) {
        for i in 0..count {
            let cols = if i % 2 == 0 { 81 } else { 80 };
            self.table.resize_at(&self.key, 24, cols, Some(at)).unwrap();
        }
    }

    fn go(&self) {
        self.table.write(&self.key, b"go\n").unwrap();
    }

    fn live_bytes(&self) -> Vec<u8> {
        self.live
            .lock()
            .unwrap()
            .iter()
            .flat_map(|(_, bytes)| bytes.clone())
            .collect()
    }

    fn end(mut self) {
        let _ = self.table.kill(&self.key);
        self.consumer.take().unwrap().join().unwrap();
    }
}

#[cfg(unix)]
const ASKING: &str = "import os,sys,time,tty\ntty.setraw(1)\nos.write(1,b'HEADER')\nsys.stdin.buffer.readline()\nos.write(1,b'\\x1b[5nTAIL')\ntime.sleep(60)\n";

/// T-2269, before the next page subscribed: the old page holds 256 sizes, its sink refuses a
/// message with a question in it, and a size it had already sent completes while it is still the
/// current page. The next page's history starts at or before its boundary and holds that size,
/// and the parked bytes come to it live: the journal never starts past what is acknowledged.
#[cfg(unix)]
#[test]
fn a_size_that_completes_after_the_sink_refused_does_not_move_the_history_past_the_boundary() {
    for sizes in [255, 256] {
        let mut rig = Rig::start(&format!("refused{sizes}"), 1 << 20, ASKING);
        rig.subscribe("old");
        wait_for("HEADER", || held(&rig.seen) == 6);
        rig.table.ack(&rig.key, last_seq(&rig.seen)).unwrap();
        rig.widths(sizes, 6);
        rig.alive.store(false, Ordering::SeqCst);
        rig.go();
        let journal = Arc::clone(&rig.journal);
        wait_for("the question", || journal.published_to() == 14);
        rig.table.resize_at(&rig.key, 24, 82, Some(6)).unwrap();
        let boundary = rig.subscribe("new");
        assert_eq!(boundary, 6);
        let piece = rig.table.history("new", &rig.key, None).unwrap();
        assert!(
            piece.from <= boundary,
            "{sizes}: the history starts at {} past B {boundary}",
            piece.from
        );
        assert!(
            piece
                .resizes
                .iter()
                .any(|size| size.cols == 82 && size.at == 6)
                || piece.size.1 == 82,
            "{sizes}: the size the old page sent is in the history: {:?}",
            piece.resizes
        );
        assert_eq!(rig.live_bytes(), QUESTION, "{sizes}");
        rig.end();
    }
}

/// T-2269, after the next page subscribed with live bytes handed over: a size the page before
/// had sent is refused `pane-input-stale-page` (its emulator is gone), the terminal keeps its
/// size, nothing is journaled, and the history starts at or before the boundary.
#[cfg(unix)]
#[test]
fn a_size_from_a_page_that_has_gone_is_refused_and_changes_nothing() {
    use crate::arbiter::{EnterTiming, InputArbiter};
    use crate::input_queue::InputQueue;
    let mut rig = Rig::start("gone", 1 << 20, ASKING);
    let inputs = InputQueue::new(
        Arc::clone(&rig.table),
        Arc::new(InputArbiter::new(EnterTiming::fixed(0))),
    );
    inputs.begin_page("old").unwrap();
    rig.subscribe("old");
    wait_for("HEADER", || held(&rig.seen) == 6);
    rig.table.ack(&rig.key, last_seq(&rig.seen)).unwrap();
    rig.widths(256, 6);
    inputs.begin_page("new").unwrap();
    let boundary = rig.subscribe("new");
    rig.go();
    wait_for("the question", || held(&rig.seen) == 8);
    let answer = crate::page::resize(
        &inputs,
        &rig.table,
        Some("old"),
        "gone",
        1,
        (82, 24),
        Some(6),
    );
    assert_eq!(answer["error"], "pane-input-stale-page");
    assert_eq!(rig.table.terminal_size(&rig.key), Some((24, 80)));
    let none = crate::page::resize(&inputs, &rig.table, None, "gone", 1, (83, 24), None);
    assert_eq!(none["error"], "pane-input-stale-page");
    let piece = rig.table.history("new", &rig.key, None).unwrap();
    assert!(
        piece.from <= boundary,
        "from {} past B {boundary}",
        piece.from
    );
    assert!(
        piece.resizes.iter().all(|size| size.cols < 82),
        "{:?}",
        piece.resizes
    );
    rig.end();
}

/// A `pane.open` that names an 8 MiB room: a flood with no acknowledgement after a subscription
/// at B cuts the journal for its cap, and not past what is acknowledged.
#[cfg(unix)]
#[test]
fn an_8_mebibyte_room_does_not_cut_the_journal_past_what_is_acknowledged() {
    const ROOM: usize = 8 << 20;
    let program = "import os,sys,time,tty\ntty.setraw(1)\ndef out(b):\n v=memoryview(b)\n while v:v=v[os.write(1,v):]\nout(b'H'*80)\nsys.stdin.buffer.readline()\nout(bytes(8<<20))\ntime.sleep(60)\n";
    let mut rig = Rig::start("big", ROOM, program);
    rig.subscribe("one");
    wait_for("the history", || held(&rig.seen) == 80);
    rig.table.ack(&rig.key, last_seq(&rig.seen)).unwrap();
    let boundary = rig.subscribe("two");
    assert_eq!(boundary, 80);
    rig.go();
    settled(&rig.journal);
    let piece = rig.table.history("two", &rig.key, None).unwrap();
    assert!(
        piece.from <= boundary,
        "from {} past B {boundary}",
        piece.from
    );
    rig.end();
}

/// A window nobody draws takes 4,096 sizes and no more: with 4,096 held and a byte not
/// acknowledged, the next is refused `pane-sizes-held` before its terminal takes it; once the
/// output is acknowledged the next is taken.
#[test]
fn a_window_nobody_draws_refuses_a_size_past_4096_and_keeps_its_terminals_size() {
    use super::PaneEnvironment;
    #[cfg(unix)]
    let argv: Vec<String> = vec!["/bin/sh".into(), "-c".into(), "sleep 60".into()];
    #[cfg(windows)]
    let argv = super::conpty_test::powershell("Start-Sleep 60");
    let table = Arc::new(PaneTable::new());
    table.keep_journals();
    let key = PaneKey::new("sizes", 1);
    // The window's output is not read: only what the test appends is in the journal.
    let pane = table
        .open_streamed_at_while(
            &|| true,
            key.clone(),
            &std::env::current_dir().unwrap(),
            &argv,
            PaneEnvironment::new(&HashMap::new(), &[]),
            portable_pty::PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            },
            1 << 20,
        )
        .unwrap();
    let journal = pane.journal.clone().unwrap();
    journal.append(b"x");
    let width = |i: usize| if i.is_multiple_of(2) { 81 } else { 80 };
    for i in 0..4096 {
        table.resize_at(&key, 24, width(i), Some(0)).unwrap();
    }
    let refused = table.resize_at(&key, 30, 90, Some(0));
    assert_eq!(
        refused.map_err(|error| error.to_string()),
        Err("pane-sizes-held".to_string())
    );
    assert_eq!(
        table.terminal_size(&key),
        Some((24, 80)),
        "its terminal keeps its size"
    );
    journal.acknowledged(1);
    table.resize_at(&key, 30, 90, Some(1)).unwrap();
    assert_eq!(table.terminal_size(&key), Some((30, 90)));
    table.kill(&key).unwrap();
}

/// T-2284's finding 16: a terminal that is slow to resize holds up its own window's sizes and
/// nothing else: not another window's keys, not the page's lock (admission of a call, a
/// subscription), not the journal (a read, the end), not an acknowledgement. Each is tried while
/// the native resize is blocked, and has a bound only so that a failure is a failure and not a
/// hang.
#[cfg(unix)]
#[test]
fn a_slow_resize_holds_nothing_of_another_window_the_page_the_journal_or_an_acknowledgement() {
    use super::PaneEnvironment;
    use crate::arbiter::{EnterTiming, InputArbiter};
    use crate::input_queue::InputQueue;

    let rig = Rig::start("slow-resize", 1 << 20, ASKING);
    let other = PaneKey::new("other-window", 1);
    let _other_output = rig
        .table
        .open_streamed_at_while(
            &|| true,
            other.clone(),
            &std::env::current_dir().unwrap(),
            &[
                "/usr/bin/python3".into(),
                "-I".into(),
                "-S".into(),
                "-c".into(),
                "import time\ntime.sleep(60)\n".into(),
            ],
            PaneEnvironment::new(&HashMap::new(), &[]),
            portable_pty::PtySize::default(),
            1 << 20,
        )
        .unwrap();
    let inputs = Arc::new(InputQueue::new(
        Arc::clone(&rig.table),
        Arc::new(InputArbiter::new(EnterTiming::fixed(0))),
    ));
    inputs.begin_page("page").unwrap();
    let (entered, inside) = mpsc::channel::<()>();
    let (release, released) = mpsc::channel::<()>();
    let released = Mutex::new(released);
    rig.table.on_native_resize(Arc::new(move || {
        entered.send(()).unwrap();
        released.lock().unwrap().recv().unwrap();
    }));
    let resizing = {
        let (inputs, table, key) = (Arc::clone(&inputs), Arc::clone(&rig.table), rig.key.clone());
        std::thread::spawn(move || {
            crate::page::resize(
                &inputs,
                &table,
                Some("page"),
                &key.id,
                key.generation,
                (82, 24),
                Some(0),
            )
        })
    };
    inside
        .recv_timeout(Duration::from_secs(10))
        .expect("the native resize began");
    // Each of these must finish while the resize is blocked.
    type Try = Box<dyn FnOnce() + Send>;
    let tries: Vec<(&str, Try)> = vec![
        ("a key of another window", {
            let (table, other) = (Arc::clone(&rig.table), other.clone());
            Box::new(move || table.write(&other, b"k").map(|_| ()).unwrap())
        }),
        ("the page's lock", {
            let inputs = Arc::clone(&inputs);
            Box::new(move || inputs.page_is_current("page").unwrap())
        }),
        ("the table's list", {
            let table = Arc::clone(&rig.table);
            Box::new(move || {
                table.list().unwrap();
            })
        }),
        ("the journal's end", {
            let journal = Arc::clone(&rig.journal);
            Box::new(move || {
                journal.end();
            })
        }),
        ("an acknowledgement", {
            let (table, key) = (Arc::clone(&rig.table), rig.key.clone());
            Box::new(move || {
                let _ = table.ack(&key, 0);
            })
        }),
    ];
    let mut held = Vec::new();
    for (what, work) in tries {
        let (done, finished) = mpsc::channel::<()>();
        std::thread::spawn(move || {
            work();
            let _ = done.send(());
        });
        if finished.recv_timeout(Duration::from_secs(3)).is_err() {
            held.push(what);
        }
    }
    // Let the resize go, so that nothing is left blocked, then say what was held.
    release.send(()).unwrap();
    let answer = resizing.join().unwrap();
    assert!(
        held.is_empty(),
        "these waited on another window's slow resize: {held:?}"
    );
    assert_eq!(
        answer["ok"], true,
        "the slow resize itself completes: {answer}"
    );
    assert_eq!(rig.table.terminal_size(&rig.key), Some((24, 82)));
    rig.end();
}

/// A size whose page is replaced while its terminal resizes is refused when it comes to be
/// recorded, and the terminal goes back to the size it had: no page's emulator took it.
#[cfg(unix)]
#[test]
fn a_size_whose_page_was_replaced_while_its_terminal_resized_is_undone() {
    use crate::arbiter::{EnterTiming, InputArbiter};
    use crate::input_queue::InputQueue;

    let rig = Rig::start("replaced-while-resizing", 1 << 20, ASKING);
    let inputs = Arc::new(InputQueue::new(
        Arc::clone(&rig.table),
        Arc::new(InputArbiter::new(EnterTiming::fixed(0))),
    ));
    inputs.begin_page("old").unwrap();
    let (entered, inside) = mpsc::channel::<()>();
    let (release, released) = mpsc::channel::<()>();
    let released = Mutex::new(released);
    rig.table.on_native_resize(Arc::new(move || {
        entered.send(()).unwrap();
        released.lock().unwrap().recv().unwrap();
    }));
    let resizing = {
        let (inputs, table, key) = (Arc::clone(&inputs), Arc::clone(&rig.table), rig.key.clone());
        std::thread::spawn(move || {
            crate::page::resize(
                &inputs,
                &table,
                Some("old"),
                &key.id,
                key.generation,
                (82, 24),
                Some(0),
            )
        })
    };
    inside.recv_timeout(Duration::from_secs(10)).unwrap();
    // The next page takes the page's lock while the old one's terminal is still resizing.
    let (began, begun) = mpsc::channel::<()>();
    {
        let inputs = Arc::clone(&inputs);
        std::thread::spawn(move || {
            inputs.begin_page("new").unwrap();
            let _ = began.send(());
        });
    }
    let held = begun.recv_timeout(Duration::from_secs(3)).is_err();
    release.send(()).unwrap();
    let answer = resizing.join().unwrap();
    assert!(!held, "the next page waited on the old page's slow resize");
    assert_eq!(answer["ok"], false);
    assert_eq!(answer["error"], "pane-input-stale-page");
    assert_eq!(
        rig.table.terminal_size(&rig.key),
        Some((24, 80)),
        "the terminal went back to its size"
    );
    let piece = rig.table.history("new", &rig.key, None);
    assert!(
        piece.is_err() || piece.unwrap().resizes.is_empty(),
        "no size was journaled"
    );
    rig.end();
}

/// With 4,096 sizes held and output acknowledged the next size is checked and goes to the
/// terminal; a read that comes in meanwhile leaves no room when it is recorded, and the size
/// is undone as the check would have refused it.
#[cfg(unix)]
#[test]
fn a_size_that_finds_no_room_when_it_is_recorded_is_undone() {
    use super::PaneEnvironment;
    let table = Arc::new(PaneTable::new());
    table.keep_journals();
    let key = PaneKey::new("no-room", 1);
    let pane = table
        .open_streamed_at_while(
            &|| true,
            key.clone(),
            &std::env::current_dir().unwrap(),
            &["/bin/sh".into(), "-c".into(), "exec sleep 60".into()],
            PaneEnvironment::new(&HashMap::new(), &[]),
            portable_pty::PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            },
            1 << 20,
        )
        .unwrap();
    let journal = pane.journal.clone().unwrap();
    journal.append(b"x");
    for i in 0..4096 {
        table
            .resize_at(&key, 24, if i % 2 == 0 { 81 } else { 80 }, Some(0))
            .unwrap();
    }
    journal.acknowledged(1);
    let in_between = Arc::clone(&journal);
    table.on_native_resize(Arc::new(move || in_between.append(b"y")));
    let refused = table.resize_at(&key, 30, 90, Some(1));
    assert_eq!(
        refused.map_err(|error| error.to_string()),
        Err("pane-sizes-held".to_string())
    );
    assert_eq!(
        table.terminal_size(&key),
        Some((24, 80)),
        "its terminal went back to its size"
    );
    table.kill(&key).unwrap();
}

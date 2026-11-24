//! A native pty, a real pane, the host's own consumer: what a paste's Enter, a counted key and
//! the input box see while a read of the window's output is on its way to the screen (N17,
//! the cases of T-2132's review). The child is a Python program that records its input, and
//! prints on the test's word.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cf_bridge::{Bridge, BridgeBuilder};
use cf_proto::bridge::Role;
use portable_pty::PtySize;
use serde_json::{json, Value};

use super::feeding_tests::{hold_at, Manual};
use super::{snapshot_of, stream_to_page};
use crate::arbiter::{EnterTiming, InputArbiter};
use crate::headless::ENTER;
use crate::input_queue::{wait_for_input_blocking, InputQueue};
use crate::output_hub::{OutputHub, PaneOutputMessage};
use crate::pty::{PaneEnvironment, PaneKey, PaneTable};
use crate::arbiter::At;
use crate::screen::{FeedStage, PaneScreen, Seen};

const BOUND: Duration = Duration::from_secs(20);

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// A raw-mode child: it draws an empty box, takes what the test sends, records it, and prints
/// only when a gate file appears. `enter`: takes a bracketed paste, then prints "echo" and "Z"
/// on the gates `echo` and `paint`. `split`: takes the human's `X`, then prints the cursor
/// move of its echo on `prefix` and the `X` on `paint`. It ends when it has taken a carriage
/// return, recording what it took in `bytes`.
const CHILD: &str = r#"
import os, select, sys, time, tty
from pathlib import Path
directory = Path(sys.argv[1]); kind = sys.argv[2]
deadline = time.monotonic() + 40
tty.setraw(0)
def gate(name):
    while not (directory / name).exists():
        if time.monotonic() > deadline: raise TimeoutError(name)
        time.sleep(0.003)
def take_until(end):
    data = bytearray()
    while not data.endswith(end):
        if not select.select([0], [], [], max(0, deadline - time.monotonic()))[0]: raise TimeoutError(end)
        chunk = os.read(0, 1)
        if not chunk: raise EOFError(end)
        data.extend(chunk)
    return bytes(data)
border = "─" * 40
os.write(1, f"{border}\r\n❯ \r\n{border}\x1b[2;3H".encode())
if kind == "enter":
    initial = take_until(b"\x1b[201~")
    (directory / "took-input").touch()
    gate("echo"); os.write(1, b"echo")
    gate("paint"); os.write(1, b"Z")
elif kind == "recovery":
    initial = b""
    (directory / "took-input").touch()
    gate("repaint"); os.write(1, b"\x1b[0m")
else:
    initial = take_until(b"X")
    (directory / "took-input").touch()
    gate("prefix"); os.write(1, b"\x1b[2;3H")
    gate("paint"); os.write(1, b"X")
data = initial + take_until(b"\r")
with (directory / "bytes.tmp").open("wb") as stream: stream.write(data)
(directory / "bytes.tmp").rename(directory / "bytes")
"#;

struct Window {
    key: PaneKey,
    directory: PathBuf,
    panes: Arc<PaneTable>,
    arbiter: Arc<InputArbiter>,
    inputs: Arc<InputQueue>,
    screen: Arc<PaneScreen>,
    peer: BufReader<UnixStream>,
    bridge: Bridge,
    _published: mpsc::Receiver<PaneOutputMessage>,
}

impl Window {
    fn new(kind: &str, arbiter: InputArbiter) -> Self {
        static MADE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "cf-n17-{kind}-{}-{}",
            std::process::id(),
            MADE.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir(&directory).unwrap();
        let key = PaneKey::new(format!("n17-{kind}"), 1);
        let panes = Arc::new(PaneTable::new());
        let argv = vec![
            "/usr/bin/python3".to_owned(),
            "-c".to_owned(),
            CHILD.to_owned(),
            directory.to_string_lossy().into_owned(),
            kind.to_owned(),
        ];
        let streamed = panes
            .open_streamed_at(
                key.clone(),
                &directory,
                &argv,
                PaneEnvironment::new(&HashMap::new(), &[]),
                PtySize {
                    rows: 8,
                    cols: 64,
                    pixel_width: 0,
                    pixel_height: 0,
                },
                1 << 20,
            )
            .expect("open the child");
        let screen = Arc::clone(&streamed.screen);
        let arbiter = Arc::new(arbiter);
        let clock = arbiter
            .register_counted(&key, None, &streamed.counts)
            .expect("register");
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        inputs.open(&key).expect("open the input");
        let (published, output) = mpsc::channel();
        let hub = Arc::new(OutputHub::new());
        hub.register_sink(Arc::new(move |message| published.send(message).is_ok()));
        let (host, peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(BOUND)).unwrap();
        let bridge = BridgeBuilder::new(Role::Host, 1 << 20)
            .serve(host.try_clone().unwrap(), host, &json!({"fixture": true}))
            .unwrap();
        let mut peer = BufReader::new(peer);
        let mut hello = String::new();
        peer.read_line(&mut hello).unwrap();
        stream_to_page(
            streamed,
            bridge.clone(),
            Arc::clone(&panes),
            clock,
            Arc::clone(&inputs),
            hub,
        );
        let window = Self {
            key,
            directory,
            panes,
            arbiter,
            inputs,
            screen,
            peer,
            bridge,
            _published: output,
        };
        window.until(|snapshot| {
            snapshot["bottom"].to_string().contains('\u{276f}') && is_current(snapshot)
        });
        window
    }

    fn gate(&self, name: &str) {
        fs::write(self.directory.join(name), b"").unwrap();
    }

    fn took_input(&self) {
        self.wait_for(&self.directory.join("took-input"));
    }

    fn wait_for(&self, path: &Path) {
        let until = Instant::now() + BOUND;
        while !path.exists() {
            assert!(Instant::now() < until, "waiting for {}", path.display());
            thread::sleep(ms(3));
        }
    }

    fn counts_only(&self) -> Value {
        snapshot_of(
            &self.arbiter,
            &self.key,
            Some(Arc::clone(&self.screen)),
            None,
            None,
        )
        .unwrap()
    }

    fn with_lines(&self) -> Value {
        snapshot_of(
            &self.arbiter,
            &self.key,
            Some(Arc::clone(&self.screen)),
            None,
            Some(8),
        )
        .unwrap()
    }

    /// Looks, with the lines, until `done` says so.
    fn until(&self, done: impl Fn(&Value) -> bool) -> Value {
        let until = Instant::now() + BOUND;
        loop {
            let snapshot = self.with_lines();
            if done(&snapshot) {
                return snapshot;
            }
            assert!(
                Instant::now() < until,
                "the window never showed it: {snapshot}"
            );
            thread::sleep(ms(3));
        }
    }

    fn bytes(&self) -> Option<Vec<u8>> {
        fs::read(self.directory.join("bytes")).ok()
    }

    fn end(self) {
        let _ = fs::remove_dir_all(&self.directory);
        self.inputs.close_and_drain();
        drop(self.peer);
        let _ = self.bridge.wait_closed();
    }
}

fn is_current(snapshot: &Value) -> bool {
    snapshot["outputRead"] == snapshot["outputDrawn"]
}

/// Whether the adapter's reader (`read_unsent`, cf-harness `shared/input_box.rs`) would take
/// the box for empty and let a paste go over the human's key: the key settled, the window
/// printed since, the screen current, and its box showing none of what was typed.
fn reads_empty_over_a_key(snapshot: &Value) -> bool {
    let typed_quiet = snapshot["typedQuietMs"].as_u64().unwrap();
    let printed_since = snapshot["outputQuietMs"]
        .as_u64()
        .is_some_and(|quiet| quiet < typed_quiet);
    let shows_the_key = snapshot["bottom"]
        .as_array()
        .unwrap()
        .iter()
        .any(|line| line.as_str().unwrap().contains('X'));
    typed_quiet >= 1_000 && printed_since && is_current(snapshot) && !shows_the_key
}

/// A paste has gone in and the window has echoed it; the next read is held after the pty gave
/// it and before the screen has it. Returns the held feed's release.
fn hold_the_next_read(
    window: &Window,
    hold_at_stage: FeedStage,
) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
    hold_at(&window.screen, hold_at_stage)
}

fn paste_in_a_thread(
    window: &Window,
) -> mpsc::Receiver<Result<(), crate::input_queue::InputError>> {
    let (inputs, key) = (Arc::clone(&window.inputs), window.key.clone());
    let (told, result) = mpsc::channel();
    thread::spawn(move || {
        let sent = inputs
            .paste(key, b"hello".to_vec(), None, None, None)
            .and_then(wait_for_input_blocking);
        let _ = told.send(sent);
    });
    result
}

fn timing(most_ms: u64) -> EnterTiming {
    EnterTiming {
        least_ms: 10,
        drawn_after_ms: 0,
        quiet_ms: 120,
        most_ms,
    }
}

/// T-2132 1: the Enter does not go while a byte the pty gave waits for the screen's lock (or
/// for the consumer's other work), however long, and goes once the byte is drawn and 120 ms of
/// quiet have passed. At 158 ms, and at the 600 ms cap, the child had already its Enter.
#[test]
fn the_enter_waits_for_a_byte_the_pty_gave_and_the_screen_has_not() {
    let window = Window::new("enter", InputArbiter::new(timing(600)));
    let pasted = paste_in_a_thread(&window);
    window.took_input();
    window.gate("echo");
    window
        .until(|snapshot| snapshot["bottom"].to_string().contains("echo") && is_current(snapshot));
    let (held, release) = hold_the_next_read(&window, FeedStage::Read);
    window.gate("paint");
    held.recv_timeout(BOUND).expect("the feed holds Z");
    let counts = window.counts_only();
    assert!(
        counts["outputRead"].as_u64() > counts["outputDrawn"].as_u64(),
        "Z is read and not drawn: {counts}"
    );
    thread::sleep(ms(900));
    assert!(
        window.bytes().is_none(),
        "the child had its Enter while Z waited"
    );
    assert!(
        pasted.try_recv().is_err(),
        "the paste was answered while Z waited"
    );
    release.send(()).unwrap();
    assert!(pasted.recv_timeout(BOUND).unwrap().is_ok());
    window.wait_for(&window.directory.join("bytes"));
    assert!(window.bytes().unwrap().ends_with(b"\x1b[201~\r"));
    window.end();
}

/// T-2132 2, T-2139 8: output pending past the cap and its grace ends the wait as it did on
/// main: the Enter goes, once, and the paste is answered as made, not as uncertain (an
/// uncertain answer sent the delivery into a recovery that pressed Enter over the pending
/// bytes or pasted the message again).
#[test]
fn the_cap_sends_one_enter_and_no_uncertain_answer() {
    let window = Window::new("enter", InputArbiter::new(timing(200)));
    let pasted = paste_in_a_thread(&window);
    window.took_input();
    window.gate("echo");
    window
        .until(|snapshot| snapshot["bottom"].to_string().contains("echo") && is_current(snapshot));
    let (held, release) = hold_the_next_read(&window, FeedStage::Locked);
    window.gate("paint");
    held.recv_timeout(BOUND)
        .expect("the feed holds Z inside the lock");
    thread::sleep(ms(1_500));
    assert!(
        window.bytes().is_none(),
        "the child had its Enter before the cap and its grace"
    );
    let answer = pasted
        .recv_timeout(ms(10_000))
        .expect("the wait ends in bounded time");
    assert!(answer.is_ok(), "the paste is answered as made: {answer:?}");
    window.wait_for(&window.directory.join("bytes"));
    assert!(
        window.bytes().unwrap().ends_with(b"\x1b[201~\r"),
        "the paste and one Enter"
    );
    release.send(()).unwrap();
    window.end();
}

/// T-2139 1: the Enter passed its quiet check and waited on the pane's writer while another
/// read arrived. The counts are looked at again with the writer held, where the Enter is
/// written: it did not go (259 read, 258 drawn) and went once `Z` was drawn.
#[test]
fn the_enter_looks_at_the_counts_again_with_the_writer_held() {
    let window = Window::new("enter", InputArbiter::new(timing(600)));
    let pasted = paste_in_a_thread(&window);
    window.took_input();
    window.gate("echo");
    window
        .until(|snapshot| snapshot["bottom"].to_string().contains("echo") && is_current(snapshot));
    let writer = window.panes.writer_of(&window.key);
    let held_writer = writer.lock().unwrap();
    // Quiet for 120 ms: the wait passes its check and the Enter waits for the writer.
    thread::sleep(ms(400));
    let (held, release) = hold_the_next_read(&window, FeedStage::Read);
    window.gate("paint");
    held.recv_timeout(BOUND).expect("the feed holds Z");
    thread::sleep(ms(100));
    drop(held_writer);
    thread::sleep(ms(700));
    assert!(
        window.bytes().is_none(),
        "the child had its Enter with Z read and not drawn"
    );
    assert!(pasted.try_recv().is_err());
    release.send(()).unwrap();
    assert!(pasted.recv_timeout(BOUND).unwrap().is_ok());
    window.wait_for(&window.directory.join("bytes"));
    assert!(window.bytes().unwrap().ends_with(b"\x1b[201~\r"));
    window.end();
}

/// T-2139 3: the reader's answer of the cursor prefix alone (the box empty, 6 bytes read) carries
/// the look's `seen`. The `X` came after in a read of its own, pending and then drawn: the paste
/// with that look is refused both times, and the child took only the `X`.
#[test]
fn an_answer_read_before_output_that_came_since_does_not_admit_a_paste() {
    let clock = Manual::new();
    let window = Window::new("split", InputArbiter::with_clock(ENTER, clock.clone()));
    clock.advance(ms(300));
    wait_for_input_blocking(
        window
            .inputs
            .typed(window.key.clone(), b"X".to_vec())
            .unwrap(),
    )
    .unwrap();
    window.took_input();
    clock.advance(ms(1_200));
    let before = window.counts_only()["outputRead"].as_u64().unwrap();
    window.gate("prefix");
    let looked = window.until(|snapshot| {
        snapshot["outputDrawn"].as_u64() == Some(before + 6) && is_current(snapshot)
    });
    let at = look_of(&looked);
    // The answer was read here; the `X` comes in a read of its own, held before the screen.
    let (held, release) = hold_the_next_read(&window, FeedStage::Read);
    window.gate("paint");
    held.recv_timeout(BOUND).expect("the feed holds X");
    let paste_over = |window: &Window| {
        window
            .inputs
            .paste(
                window.key.clone(),
                b"hello".to_vec(),
                Some(at),
                Some(at.typed),
                None,
            )
            .and_then(wait_for_input_blocking)
    };
    let pending = paste_over(&window).expect_err("X is pending: the answer is stale");
    assert_eq!(pending.paste_answer()["admitted"], false, "{pending}");
    assert_eq!(pending.paste_answer()["error"], "look-again");
    assert_eq!(pending.paste_answer()["cause"], "output-pending");
    release.send(()).unwrap();
    window.until(|snapshot| is_current(snapshot) && snapshot["bottom"].to_string().contains('X'));
    let drawn = paste_over(&window).expect_err("X is drawn: the answer is still stale");
    assert_eq!(drawn.paste_answer()["admitted"], false, "{drawn}");
    assert_eq!(drawn.paste_answer()["cause"], "box-changed");
    wait_for_input_blocking(
        window
            .inputs
            .write(window.key.clone(), b"\r".to_vec())
            .unwrap(),
    )
    .unwrap();
    window.wait_for(&window.directory.join("bytes"));
    assert_eq!(
        window.bytes().unwrap(),
        b"X\r",
        "no paste went over the key"
    );
    window.end();
}

/// T-2160 3: the recovery Enter of a look. A byte the pty gave (a repaint) is pending when the key
/// is taken: the child gets no Enter while it is pending, and one once it is drawn; never one at
/// the grace.
#[test]
fn the_enter_of_a_look_is_not_written_while_a_byte_is_pending_and_is_after_it_is_drawn() {
    let clock = Manual::new();
    let window = Window::new("recovery", InputArbiter::with_clock(ENTER, clock.clone()));
    let at = look_of(&window.until(is_current));
    let (held, release) = hold_the_next_read(&window, FeedStage::Read);
    window.gate("repaint");
    held.recv_timeout(BOUND).expect("the feed holds the repaint");
    let (inputs, key) = (Arc::clone(&window.inputs), window.key.clone());
    let entering = thread::spawn(move || {
        inputs
            .enter(key, at)
            .and_then(wait_for_input_blocking)
    });
    thread::sleep(ms(150));
    assert!(window.bytes().is_none(), "the child has no Enter while a byte is pending");
    release.send(()).unwrap();
    entering.join().unwrap().expect("drawn: the Enter goes");
    window.wait_for(&window.directory.join("bytes"));
    assert_eq!(window.bytes().unwrap(), b"\r");
    window.end();
}

/// The look a `pane.snapshot` that asked for the lines is.
fn look_of(snapshot: &Value) -> At {
    At {
        seen: Seen::parse(snapshot["seen"].as_str().expect("a snapshot with lines has a seen"))
            .expect("sixteen hex digits"),
        typed: snapshot["typed"].as_u64().unwrap(),
    }
}

/// T-2132 3: the child echoes the human's key in two reads, the cursor move and then the `X`.
/// With the second held after the host has it, the snapshot says output is pending, and a
/// reader that takes pending output for a box that reads empty is wrong.
#[test]
fn a_split_echo_is_not_a_box_that_reads_empty() {
    let clock = Manual::new();
    let window = Window::new("split", InputArbiter::with_clock(ENTER, clock.clone()));
    clock.advance(ms(300));
    wait_for_input_blocking(
        window
            .inputs
            .typed(window.key.clone(), b"X".to_vec())
            .unwrap(),
    )
    .unwrap();
    window.took_input();
    clock.advance(ms(1_200));
    let before = window.counts_only()["outputRead"].as_u64().unwrap();
    window.gate("prefix");
    // The cursor move of the echo is six bytes, which may come in more than one read.
    window.until(|snapshot| {
        snapshot["outputDrawn"].as_u64() == Some(before + 6) && is_current(snapshot)
    });
    let (held, release) = hold_the_next_read(&window, FeedStage::Read);
    window.gate("paint");
    held.recv_timeout(BOUND).expect("the feed holds X");
    let snapshot = window.with_lines();
    assert!(
        snapshot["outputRead"].as_u64() > snapshot["outputDrawn"].as_u64(),
        "{snapshot}"
    );
    assert_eq!(
        snapshot["bottom"][1].as_str().unwrap().trim_end(),
        "\u{276f}"
    );
    assert!(!reads_empty_over_a_key(&snapshot), "{snapshot}");
    release.send(()).unwrap();
    let snapshot = window
        .until(|snapshot| is_current(snapshot) && snapshot["bottom"].to_string().contains('X'));
    assert!(!reads_empty_over_a_key(&snapshot), "{snapshot}");
    wait_for_input_blocking(
        window
            .inputs
            .write(window.key.clone(), b"\r".to_vec())
            .unwrap(),
    )
    .unwrap();
    window.wait_for(&window.directory.join("bytes"));
    assert_eq!(window.bytes().unwrap(), b"X\r");
    window.end();
}

/// T-2132 4: a panic in the feed ends the pane's generation. Its program is ended, nothing is
/// admitted against the screen the panic left, and the pane's exit is told as for a program
/// that ended.
#[test]
fn a_panic_in_the_feed_ends_the_pane_and_admits_no_input() {
    let clock = Manual::new();
    let mut window = Window::new("split", InputArbiter::with_clock(ENTER, clock.clone()));
    clock.advance(ms(300));
    wait_for_input_blocking(
        window
            .inputs
            .typed(window.key.clone(), b"X".to_vec())
            .unwrap(),
    )
    .unwrap();
    window.took_input();
    clock.advance(ms(1_200));
    window.screen.hook.set(|stage| {
        assert_ne!(stage, FeedStage::Locked, "an injected panic in the feed");
    });
    window.gate("prefix");
    let mut line = String::new();
    assert!(
        window.peer.read_line(&mut line).unwrap() > 0,
        "no pane.exit came"
    );
    let event: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(event["op"], "pane.exit");
    // Not admitted: a paste against the screen the panic left is refused before anything is
    // written, the pane's own record gone with its generation.
    let refused = window
        .inputs
        .paste(window.key.clone(), b"again".to_vec(), None, Some(1), None)
        .and_then(wait_for_input_blocking);
    let error = refused.expect_err("no input is admitted against the broken screen");
    assert_eq!(error.paste_answer()["admitted"], false, "{error}");
    let deadline = Instant::now() + BOUND;
    while window.panes.open_count() != 0 {
        assert!(Instant::now() < deadline, "the pane stayed in the table");
        thread::sleep(ms(5));
    }
    window.end();
}

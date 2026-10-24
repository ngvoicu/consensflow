//! What stands between the daemon and the pane host: every frame one writes is
//! read here, kept for the case to look at, and passed on to the other, as the
//! app's own bridge does. A frame is kept before it is passed on, so that a
//! case that has seen the effect of a frame can find the frame.
//!
//! The rig is also a page and a terminal. A request the case makes of the
//! daemon is the page's (`r-test-N`), one it makes of the pane host the
//! desktop app's own (`n-test-N`), and the answers to them stop here: they
//! were asked by no one on the other side. A program may ask where the cursor
//! is (a ConPTY made to inherit its cursor asks before a program may print at
//! all; the pane host's are not); the page's xterm answers, and with no page
//! the rig does. And the page takes each frame a pane printed and says so
//! (`pane.ack`), which frees the pane to print on: the host reads no more of a
//! window whose unread output fills its budget (`backlogBytes`, 1 MiB unless the
//! open names one). The rig says so too, as it gathers the frames, of the latest once
//! [`ACK_EVERY`] bytes of them are taken (the host's answer to each ack shares its way
//! out with the frames, and one to each frame filled it: `bridge writer queue is
//! full`); till T-139 it said nothing, and every window of a rig froze once it had
//! printed 1 MiB. A window opened with a budget under [`ACK_EVERY`] would freeze: the
//! rig opens none.
//!
//! What a window printed is kept as its last [`KEPT`] bytes or so, and how much was let
//! go of before them: a long run's windows print for hours.
//!
//! And a case can catch one paste of the daemon's ([`Catch`]): passed to the pane
//! host or kept from it, its answer never reaches the daemon, which goes on
//! waiting for it, as a daemon stopped between a paste and its receipt left it.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde_json::{json, Value};

use super::report::plain;
use crate::live::members::{Identity, Machine, ThisMachine};
use crate::process::Sink;
use crate::wire::{self, Pending};

/// Where a terminal asks for the cursor's place.
const CURSOR_QUERY: [u8; 4] = [0x1b, b'[', b'6', b'n'];

/// How much of what a window printed is kept: its last 8 MiB, give or take a line.
pub(super) const KEPT: usize = 8 * 1024 * 1024;

/// How much more a window prints before what it printed first is let go of.
const SLACK: usize = 2 * 1024 * 1024;

/// How much of a window's output the rig takes before it says so to the pane host.
pub(super) const ACK_EVERY: usize = 32 * 1024;

/// What the page's xterm answers: the cursor is at row 1, column 1.
const CURSOR_REPLY: [u8; 6] = [0x1b, b'[', b'1', b';', b'1', b'R'];

/// The prefix of the id of a request the case makes of the daemon.
pub(super) const PAGE: &str = "r-test-";

/// The prefix of the id of a request the case makes of the pane host.
pub(super) const HOST: &str = "n-test-";

/// A key the daemon pressed into a window: a `pane.input` request it made of
/// the pane host, and when the rig saw it sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Press {
    /// The window's id.
    pub pane: String,
    /// The bytes of the key (Escape is `[27]`).
    pub bytes: Vec<u8>,
    /// When it was seen, in milliseconds since the epoch: the clock the daemon's
    /// event file and a harness's status file keep their times by.
    pub at_ms: i64,
}

/// A window the pane host said it opened (`id`, `generation`), and which process its
/// program is: read once, as the host answered the open, and kept. A live check holds
/// what it finds of the window to this: a window whose process has gone has nothing that
/// descends from it, whatever has its number since (Astra and Sol on round 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Root {
    pub id: Value,
    pub generation: Value,
    /// None where the host said no process, or it had gone by the time it was read.
    pub identity: Option<Identity>,
}

/// What a window printed: the last [`KEPT`] bytes of it or so, from the start of a line,
/// and how many characters of its screen the bytes let go of before them made; and how
/// many bytes were taken since the rig last said so to the pane host.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Printed {
    pub kept: Vec<u8>,
    pub dropped: usize,
    untold: usize,
}

impl Printed {
    /// `bytes` taken: whether that makes [`ACK_EVERY`] since the rig last said so.
    fn taken(&mut self, bytes: usize) -> bool {
        self.untold += bytes;
        let due = self.untold >= ACK_EVERY;
        if due {
            self.untold = 0;
        }
        due
    }

    /// `bytes`, printed after what is kept: once what is kept is past `kept` and `slack`
    /// more, what came first is let go of, up to the start of a line at least `kept` from
    /// the end and no more than `slack` before that (a line's start ends no character and
    /// no control sequence of a screen's text), else to `kept` from the end; and the text
    /// it made is counted.
    fn push(&mut self, bytes: &[u8], kept: usize, slack: usize) {
        self.kept.extend_from_slice(bytes);
        if self.kept.len() <= kept + slack {
            return;
        }
        let from = self.kept.len() - kept;
        let earliest = from.saturating_sub(slack);
        let cut = self.kept[earliest..from]
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(from, |at| earliest + at + 1);
        self.dropped += plain(&self.kept[..cut]).chars().count();
        self.kept.drain(..cut);
    }
}

/// The window a frame of the pane host's says it opened, its process read by `read` as it
/// says so: none for any other frame.
fn root_of(frame: &Value, read: impl FnOnce(u32) -> Option<Identity>) -> Option<Root> {
    if frame["kind"] != "res" || frame["op"] != "pane.open" || frame["body"]["id"].is_null() {
        return None;
    }
    let body = &frame["body"];
    let pid = body["pid"].as_u64().and_then(|pid| u32::try_from(pid).ok());
    Some(Root {
        id: body["id"].clone(),
        generation: body["generation"].clone(),
        identity: pid.and_then(read),
    })
}

/// What the frames showed so far. A frame is kept for what a case reads of it and
/// no more: a long run's windows print for hours, and every frame kept whole (a
/// window's output is a list of numbers, one to a byte) would hold gigabytes.
#[derive(Debug, Default)]
pub(super) struct Seen {
    /// How many frames the daemon wrote, and how many the pane host wrote.
    pub daemon: usize,
    pub host: usize,
    /// The body of each `pane.open` the daemon asked, in order.
    pub opened: Vec<Value>,
    /// The `pane.open` requests the pane host has not answered yet, by the
    /// request's id: the window each is for.
    opening: HashMap<String, Value>,
    /// The process each window was opened on, as the pane host answered its
    /// `pane.open`: `{ id, generation, pid }`, in the order they were answered.
    pub pids: Vec<Value>,
    /// The body of each `pane.kill` the daemon asked, in order: the windows it
    /// closed itself.
    pub kills: Vec<Value>,
    /// Every key the daemon pressed into a window, in order.
    pub presses: Vec<Press>,
    /// What each pane printed, the end of it ([`Printed`]), by the pane's id.
    pub printed: HashMap<String, Printed>,
    /// The body of each `pane.exit` the pane host told.
    pub exits: Vec<Value>,
    /// When the rig saw each of `exits`, in milliseconds since the epoch: a
    /// window's end as the host told it, not when a case next looked.
    pub exit_times: Vec<i64>,
    /// Each window the pane host answered the open of, and its process ([`Root`]).
    pub roots: Vec<Root>,
    /// Lines a program wrote that are no frame, which a failure shows.
    pub stray: Vec<String>,
    /// Whether the pane host's output has ended: every frame it wrote, its last
    /// exits among them, has been read.
    pub host_ended: bool,
}

/// Where the paste a case catches goes ([`crate::rig::Rig::catch_paste`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pasted {
    /// To the pane host, which pastes it into the window.
    IntoTheWindow,
    /// Nowhere: the window never has it.
    Nowhere,
}

/// The paste a case set out to catch: what it holds and where it goes, until one
/// is caught, and the request caught, whose answer stops here.
#[derive(Debug, Default)]
pub(super) struct Catch {
    armed: Option<(String, Pasted)>,
    caught: Option<String>,
}

/// What the routers share with the case.
#[derive(Debug, Default)]
pub(super) struct Shared {
    seen: Mutex<Seen>,
    pub pending: Pending,
    /// The numbers of the requests made of the daemon and of the pane host.
    pages: AtomicU64,
    hosts: AtomicU64,
    catch: Mutex<Catch>,
}

impl Shared {
    pub fn seen(&self) -> MutexGuard<'_, Seen> {
        self.seen.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The id for the next request of the page's.
    pub fn next_page_id(&self) -> String {
        format!("{PAGE}{}", self.pages.fetch_add(1, Ordering::Relaxed) + 1)
    }

    /// The id for the next request of the pane host's.
    pub fn next_host_id(&self) -> String {
        format!("{HOST}{}", self.hosts.fetch_add(1, Ordering::Relaxed) + 1)
    }

    /// The daemon's next paste that holds `words` is caught, and goes as `pasted`
    /// says; a catch set before replaces one not yet made.
    pub fn catch_paste(&self, words: &str, pasted: Pasted) {
        let mut catch = self.catch.lock().unwrap_or_else(PoisonError::into_inner);
        catch.armed = Some((words.to_owned(), pasted));
        catch.caught = None;
    }

    /// Whether the paste set out to be caught was.
    pub fn paste_caught(&self) -> bool {
        self.catch
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .caught
            .is_some()
    }

    /// Whether `frame`, a frame of the daemon's, is the paste set out to be caught:
    /// where it goes, once it is caught.
    fn caught(&self, frame: &Value) -> Option<Pasted> {
        if frame["kind"] != "req" || frame["op"] != "pane.write_paste" {
            return None;
        }
        let mut catch = self.catch.lock().unwrap_or_else(PoisonError::into_inner);
        let holds = |words: &str| {
            frame["body"]["body"]
                .as_str()
                .is_some_and(|body| body.contains(words))
        };
        let (_, pasted) = catch.armed.take_if(|(words, _)| holds(words))?;
        catch.caught = frame["id"].as_str().map(str::to_owned);
        Some(pasted)
    }

    /// Whether `frame`, a frame of the pane host's, answers the paste caught.
    fn answers_the_caught(&self, frame: &Value) -> bool {
        let catch = self.catch.lock().unwrap_or_else(PoisonError::into_inner);
        frame["kind"] == "res"
            && catch
                .caught
                .as_deref()
                .is_some_and(|caught| frame["id"] == caught)
    }

    /// A line the daemon wrote: kept, and passed to the pane host unless it is
    /// the answer to a request of the case's, or a paste caught to go nowhere.
    pub fn route_daemon(&self, line: &str, host: &Sink) {
        let Ok(frame) = serde_json::from_str::<Value>(line) else {
            self.seen().stray.push(line.to_owned());
            return;
        };
        let id = frame["id"].as_str().unwrap_or_default();
        {
            let mut seen = self.seen();
            seen.daemon += 1;
            if frame["kind"] == "req" && frame["op"] == "pane.open" {
                seen.opened.push(frame["body"].clone());
                seen.opening.insert(
                    id.to_owned(),
                    json!({ "id": frame["body"]["id"], "generation": frame["body"]["generation"] }),
                );
            }
            if frame["kind"] == "req" && frame["op"] == "pane.kill" {
                seen.kills.push(frame["body"].clone());
            }
            if frame["kind"] == "req" && frame["op"] == "pane.input" {
                if let Some(bytes) = bytes_of(&frame["body"]["bytes"]) {
                    seen.presses.push(Press {
                        pane: frame["body"]["id"].as_str().unwrap_or_default().to_owned(),
                        bytes,
                        at_ms: jiff::Timestamp::now().as_millisecond(),
                    });
                }
            }
        }
        if frame["kind"] == "res"
            && (self.pending.answer(id, &frame["body"]) || id.starts_with(PAGE))
        {
            return;
        }
        if self.caught(&frame) == Some(Pasted::Nowhere) {
            return;
        }
        host.send(format!("{line}\n"));
    }

    /// A line the pane host wrote: kept, a pane's output gathered and the
    /// cursor query answered, and passed to the daemon unless it is the answer
    /// to a request of the case's, or to the paste caught.
    pub fn route_host(&self, line: &str, host: &Sink, daemon: &Sink) {
        let Ok(frame) = serde_json::from_str::<Value>(line) else {
            self.seen().stray.push(line.to_owned());
            return;
        };
        let id = frame["id"].as_str().unwrap_or_default();
        let mut reply = None;
        let mut taken = None;
        // The window's process, read as the host says it opened it, before the answer goes on.
        let root = root_of(&frame, |pid| ThisMachine.identity(pid).ok().flatten());
        {
            let mut seen = self.seen();
            seen.host += 1;
            if let Some(root) = root {
                seen.roots.push(root);
            }
            if frame["kind"] == "res" {
                if let Some(mut window) = seen.opening.remove(id) {
                    if let Some(pid) = frame["body"]["pid"].as_u64() {
                        window["pid"] = json!(pid);
                        seen.pids.push(window);
                    }
                }
            }
            if frame["op"] == "pane.output" {
                if let Some(bytes) = bytes_of(&frame["body"]["bytes"]) {
                    let pane = frame["body"]["id"].as_str().unwrap_or_default().to_owned();
                    if bytes
                        .windows(CURSOR_QUERY.len())
                        .any(|at| at == CURSOR_QUERY)
                    {
                        reply = Some(json!({
                            "id": frame["body"]["id"],
                            "generation": frame["body"]["generation"],
                            "bytes": CURSOR_REPLY,
                        }));
                    }
                    let printed = seen.printed.entry(pane).or_default();
                    printed.push(&bytes, KEPT, SLACK);
                    let due = printed.taken(bytes.len());
                    taken = frame["body"]["seq"]
                        .as_u64()
                        .filter(|seq| due && *seq > 0)
                        .map(|seq| {
                            json!({
                                "id": frame["body"]["id"],
                                "generation": frame["body"]["generation"],
                                "seq": seq,
                            })
                        });
                }
            }
            if frame["op"] == "pane.exit" {
                seen.exits.push(frame["body"].clone());
                seen.exit_times
                    .push(jiff::Timestamp::now().as_millisecond());
            }
        }
        if let Some(body) = reply {
            // Nobody waits for the answer, which stops at the next frame's check.
            host.send(wire::request(&self.next_host_id(), "pane.reply", &body));
        }
        if let Some(body) = taken {
            // The frame is taken, as the page says once it drew it; nor is this answer waited for.
            host.send(wire::request(&self.next_host_id(), "pane.ack", &body));
        }
        if frame["kind"] == "res"
            && (self.pending.answer(id, &frame["body"]) || id.starts_with(HOST))
        {
            return;
        }
        if self.answers_the_caught(&frame) {
            return;
        }
        daemon.send(format!("{line}\n"));
    }
}

/// The bytes a frame carries as a list of numbers.
fn bytes_of(list: &Value) -> Option<Vec<u8>> {
    list.as_array()?
        .iter()
        .map(|byte| byte.as_u64().and_then(|byte| u8::try_from(byte).ok()))
        .collect()
}

/// The tests start `cat` for the programs on the other side of the rig.
#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    use crate::process::Run;

    /// Two sinks whose lines come out of a `cat` each, which the test reads.
    fn sinks() -> (Sink, mpsc::Receiver<String>, Sink, mpsc::Receiver<String>) {
        fn cat() -> (Sink, mpsc::Receiver<String>, crate::process::Spawned) {
            let mut process = Run::new("/bin/cat").spawn().unwrap();
            let sink = process.sink().unwrap();
            let output = process.take_output().unwrap();
            let (sender, lines) = mpsc::channel();
            wire::read_each(
                output,
                move |line| {
                    let _ = sender.send(line);
                    true
                },
                || {},
            );
            (sink, lines, process)
        }
        let (host, host_lines, host_process) = cat();
        let (daemon, daemon_lines, daemon_process) = cat();
        // The programs live as long as the test.
        std::mem::forget(host_process);
        std::mem::forget(daemon_process);
        (host, host_lines, daemon, daemon_lines)
    }

    #[test]
    fn a_frame_the_daemon_wrote_is_kept_and_passed_to_the_pane_host() {
        let shared = Shared::default();
        let (host, host_lines, _daemon, _) = sinks();
        let open = r#"{"v":1,"id":"n-1","kind":"req","op":"pane.open","body":{"id":"p1-chief","argv":["x"]}}"#;
        shared.route_daemon(open, &host);
        assert_eq!(
            host_lines.recv_timeout(Duration::from_secs(10)).unwrap(),
            open
        );
        let seen = shared.seen();
        assert_eq!(seen.daemon, 1);
        assert_eq!(seen.opened, [json!({"id": "p1-chief", "argv": ["x"]})]);
    }

    #[test]
    fn the_process_a_window_was_opened_on_and_the_windows_the_daemon_closed_are_kept() {
        let shared = Shared::default();
        let (host, _host_lines, daemon, _daemon_lines) = sinks();
        let open = |frame: &str, pane: &str| {
            json!({
                "v": 1, "id": frame, "kind": "req", "op": "pane.open",
                "body": {"id": pane, "generation": 2, "argv": ["claude"]},
            })
            .to_string()
        };
        shared.route_daemon(&open("d-7", "p1-worker-a"), &host);
        shared.route_daemon(&open("d-8", "p1-worker-b"), &host);
        // The host answers the second first, and the first with no process (it failed).
        let answer = |frame: &str, body: Value| {
            json!({ "v": 1, "id": frame, "kind": "res", "op": "pane.open", "body": body })
                .to_string()
        };
        shared.route_host(
            &answer(
                "d-8",
                json!({"ok": true, "id": "p1-worker-b", "generation": 2, "pid": 4242}),
            ),
            &host,
            &daemon,
        );
        shared.route_host(
            &answer("d-7", json!({"ok": false, "error": "no such program"})),
            &host,
            &daemon,
        );
        let kill = json!({
            "v": 1, "id": "d-9", "kind": "req", "op": "pane.kill",
            "body": {"id": "p1-worker-b", "generation": 2},
        });
        shared.route_daemon(&kill.to_string(), &host);
        let seen = shared.seen();
        assert_eq!(
            seen.pids,
            [json!({"id": "p1-worker-b", "generation": 2, "pid": 4242})]
        );
        assert!(seen.opening.is_empty(), "every open was answered");
        assert_eq!(seen.kills, [json!({"id": "p1-worker-b", "generation": 2})]);
        assert_eq!((seen.daemon, seen.host), (3, 2));
    }

    #[test]
    fn a_key_the_daemon_pressed_is_kept_with_the_time_it_was_seen_sent() {
        let shared = Shared::default();
        let (host, host_lines, _daemon, _) = sinks();
        let before = jiff::Timestamp::now().as_millisecond();
        let key = |id: &str, bytes: &str| {
            format!(
                r#"{{"v":1,"id":"n-1","kind":"req","op":"pane.input","body":{{"id":"{id}","generation":1,"bytes":{bytes}}}}}"#
            )
        };
        for line in [
            key("p1-chief", "[27]"),
            key("p1-worker", "[3]"),
            key("p1-chief", "[27]"),
        ] {
            shared.route_daemon(&line, &host);
            // Each is passed on, as every frame is.
            assert_eq!(
                host_lines.recv_timeout(Duration::from_secs(10)).unwrap(),
                line
            );
        }
        // Another request of the daemon's is no key, and nor is an answer.
        shared.route_daemon(
            r#"{"v":1,"id":"n-2","kind":"req","op":"pane.write_paste","body":{"id":"p1-chief","bytes":[27]}}"#,
            &host,
        );
        let after = jiff::Timestamp::now().as_millisecond();
        let seen = shared.seen();
        let kept: Vec<(&str, &[u8])> = seen
            .presses
            .iter()
            .map(|press| (press.pane.as_str(), press.bytes.as_slice()))
            .collect();
        assert_eq!(
            kept,
            [
                ("p1-chief", &[27][..]),
                ("p1-worker", &[3][..]),
                ("p1-chief", &[27][..])
            ]
        );
        assert!(seen
            .presses
            .iter()
            .all(|press| (before..=after).contains(&press.at_ms)));
        assert!(seen.presses[0].at_ms <= seen.presses[2].at_ms);
    }

    #[test]
    fn the_answer_to_a_request_of_the_cases_stops_where_it_is_read() {
        let shared = Shared::default();
        let (host, host_lines, _daemon, _) = sinks();
        let id = shared.next_page_id();
        assert_eq!(id, "r-test-1");
        let answer = shared.pending.expect(&id);
        shared.route_daemon(
            &format!(r#"{{"v":1,"id":"{id}","kind":"res","op":"ping","body":{{"ok":true}}}}"#),
            &host,
        );
        assert_eq!(
            answer.recv_timeout(Duration::from_secs(10)).unwrap(),
            json!({"ok": true})
        );
        // One that nobody waits for any more stops too.
        shared.route_daemon(
            r#"{"v":1,"id":"r-test-9","kind":"res","op":"ping","body":{"ok":true}}"#,
            &host,
        );
        // An answer to the host's own request goes on.
        let theirs = r#"{"v":1,"id":"n-7","kind":"res","op":"pane.open","body":{"ok":true}}"#;
        shared.route_daemon(theirs, &host);
        assert_eq!(
            host_lines.recv_timeout(Duration::from_secs(10)).unwrap(),
            theirs
        );
        assert!(host_lines.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn what_a_pane_prints_is_gathered_its_exit_kept_and_the_cursor_asked_for_is_told() {
        let shared = Shared::default();
        let (host, host_lines, daemon, daemon_lines) = sinks();
        let printed = json!({
            "v": 1, "id": "e-1", "kind": "evt", "op": "pane.output",
            "body": {"id": "p1-chief", "generation": 5, "bytes": [104, 105, 0x1b, 91, 54, 110]},
        });
        shared.route_host(&printed.to_string(), &host, &daemon);
        let more = json!({
            "v": 1, "id": "e-2", "kind": "evt", "op": "pane.output",
            "body": {"id": "p1-chief", "generation": 5, "bytes": [33]},
        });
        shared.route_host(&more.to_string(), &host, &daemon);
        let exit = json!({
            "v": 1, "id": "e-3", "kind": "evt", "op": "pane.exit",
            "body": {"id": "p1-chief", "generation": 5, "code": 0},
        });
        let before = jiff::Timestamp::now().as_millisecond();
        shared.route_host(&exit.to_string(), &host, &daemon);
        let after = jiff::Timestamp::now().as_millisecond();
        {
            let seen = shared.seen();
            // When the host told it, which a look that comes later reads.
            assert_eq!(seen.exit_times.len(), 1);
            assert!((before..=after).contains(&seen.exit_times[0]));
            assert_eq!(
                seen.printed["p1-chief"].kept,
                [104, 105, 0x1b, 91, 54, 110, 33]
            );
            assert_eq!(
                seen.exits,
                [json!({"id": "p1-chief", "generation": 5, "code": 0})]
            );
            assert_eq!(seen.host, 3);
        }
        // The page's reply to the query goes to the pane host, as a request of the rig's.
        let reply: Value =
            serde_json::from_str(&host_lines.recv_timeout(Duration::from_secs(10)).unwrap())
                .unwrap();
        assert_eq!(reply["op"], "pane.reply");
        assert_eq!(reply["id"], "n-test-1");
        assert_eq!(
            reply["body"],
            json!({"id": "p1-chief", "generation": 5, "bytes": [27, 91, 49, 59, 49, 82]})
        );
        // Every frame went on to the daemon, in order.
        let passed: Vec<String> = (0..3)
            .map(|_| daemon_lines.recv_timeout(Duration::from_secs(10)).unwrap())
            .collect();
        assert_eq!(
            passed,
            [printed.to_string(), more.to_string(), exit.to_string()]
        );
    }

    #[test]
    fn each_frame_a_pane_printed_is_taken_and_said_so_to_the_host_as_the_page_says_it() {
        // T-139: the host reads no more of a window whose unread output fills its budget,
        // and the rig said of none that it was read: every window of a rig froze at 1 MiB.
        let shared = Shared::default();
        let (host, host_lines, daemon, daemon_lines) = sinks();
        // A frame of half the bytes the rig takes before it says so, one of a quarter.
        let printed = |seq: u64, bytes: usize| {
            json!({
                "v": 1, "id": format!("e-{seq}"), "kind": "evt", "op": "pane.output",
                "body": {"id": "p1-zeus", "generation": 2, "seq": seq, "bytes": vec![104; bytes]},
            })
        };
        let frames = [
            printed(1, ACK_EVERY / 2),
            printed(2, ACK_EVERY / 4),
            printed(3, ACK_EVERY / 4),
            printed(4, ACK_EVERY / 2),
            printed(5, ACK_EVERY / 2),
        ];
        for frame in &frames {
            shared.route_host(&frame.to_string(), &host, &daemon);
        }
        // Said of the third, the latest of the first ACK_EVERY bytes, and of the fifth.
        for seq in [3, 5] {
            let ack: Value =
                serde_json::from_str(&host_lines.recv_timeout(Duration::from_secs(10)).unwrap())
                    .unwrap();
            assert_eq!(ack["kind"], "req");
            assert_eq!(ack["op"], "pane.ack");
            assert_eq!(
                ack["body"],
                json!({"id": "p1-zeus", "generation": 2, "seq": seq})
            );
        }
        assert!(host_lines.recv_timeout(Duration::from_millis(100)).is_err());
        // The host's answer to an ack is the rig's own, and goes no further.
        let answer = json!({"v": 1, "id": "n-test-1", "kind": "res", "op": "pane.ack", "body": {"ok": true}});
        shared.route_host(&answer.to_string(), &host, &daemon);
        let passed: Vec<String> = (0..frames.len())
            .map(|_| daemon_lines.recv_timeout(Duration::from_secs(10)).unwrap())
            .collect();
        let frames: Vec<String> = frames.iter().map(Value::to_string).collect();
        assert_eq!(passed, frames);
        assert!(daemon_lines
            .recv_timeout(Duration::from_millis(100))
            .is_err());
        assert_eq!(shared.seen().printed["p1-zeus"].kept.len(), 2 * ACK_EVERY);
    }

    #[test]
    fn what_a_window_printed_is_kept_as_its_end_from_the_start_of_a_line_and_what_went_counted() {
        // T-139: every byte a window printed was kept for the rig's life.
        let mut printed = Printed::default();
        for line in 1..=10 {
            printed.push(format!("line {line:02}\r\n").as_bytes(), 20, 10);
        }
        // Nine bytes a line: kept to 30 at most, then let go of to a line's start 20 or
        // more from the end.
        assert_eq!(printed.kept, b"line 08\r\nline 09\r\nline 10\r\n");
        assert_eq!(printed.dropped, plain(b"line 01\r\n").chars().count() * 7);
        // A line longer than what is kept is cut where the bound falls.
        let mut long = Printed::default();
        long.push(&[b'x'; 40], 20, 10);
        assert_eq!(long.kept, [b'x'; 20]);
        assert_eq!(long.dropped, 20);
    }

    /// A `pane.write_paste` of the daemon's, numbered `id`, of `body`, and the host's
    /// answer to it.
    fn paste(id: &str, body: &str) -> (String, String) {
        (
            json!({"v":1,"id":id,"kind":"req","op":"pane.write_paste","body":{"id":"p1-zeus","generation":1,"body":body}}).to_string(),
            json!({"v":1,"id":id,"kind":"res","op":"pane.write_paste","body":{"ok":true}}).to_string(),
        )
    }

    #[test]
    fn a_paste_caught_into_the_window_goes_to_the_host_and_its_answer_never_to_the_daemon() {
        let shared = Shared::default();
        let (host, host_lines, daemon, daemon_lines) = sinks();
        shared.catch_paste("Resumed:", Pasted::IntoTheWindow);
        // A paste that does not hold the words goes, and its answer comes back.
        let (other, answered) = paste("n-1", "[ConsensFlow m-3]\nhello");
        shared.route_daemon(&other, &host);
        assert_eq!(
            host_lines.recv_timeout(Duration::from_secs(10)).unwrap(),
            other
        );
        shared.route_host(&answered, &host, &daemon);
        assert_eq!(
            daemon_lines.recv_timeout(Duration::from_secs(10)).unwrap(),
            answered
        );
        assert!(!shared.paste_caught());

        let (caught, answer) = paste("n-2", "[ConsensFlow m-4]\nResumed: Go on");
        shared.route_daemon(&caught, &host);
        assert_eq!(
            host_lines.recv_timeout(Duration::from_secs(10)).unwrap(),
            caught
        );
        assert!(shared.paste_caught());
        shared.route_host(&answer, &host, &daemon);
        assert!(daemon_lines
            .recv_timeout(Duration::from_millis(100))
            .is_err());

        // It is caught once: the next that holds the words goes, answer and all.
        let (again, answered) = paste("n-3", "[ConsensFlow m-5]\nResumed: Go on");
        shared.route_daemon(&again, &host);
        assert_eq!(
            host_lines.recv_timeout(Duration::from_secs(10)).unwrap(),
            again
        );
        shared.route_host(&answered, &host, &daemon);
        assert_eq!(
            daemon_lines.recv_timeout(Duration::from_secs(10)).unwrap(),
            answered
        );
    }

    #[test]
    fn a_paste_caught_to_go_nowhere_never_reaches_the_host() {
        let shared = Shared::default();
        let (host, host_lines, _daemon, _) = sinks();
        shared.catch_paste("Resumed:", Pasted::Nowhere);
        let (caught, _) = paste("n-1", "[ConsensFlow m-4]\nResumed: Go on");
        shared.route_daemon(&caught, &host);
        assert!(shared.paste_caught());
        assert!(host_lines.recv_timeout(Duration::from_millis(100)).is_err());
        assert_eq!(shared.seen().daemon, 1, "it is counted, as every frame is");
    }

    #[test]
    fn a_windows_process_is_read_as_the_host_answers_its_open_and_kept() {
        let shared = Shared::default();
        let (host, _, daemon, _) = sinks();
        let own = std::process::id();
        let opened = |id: &str, pid: Option<u32>| {
            let mut body = json!({ "ok": true, "id": id, "generation": 1 });
            if let Some(pid) = pid {
                body["pid"] = json!(pid);
            }
            json!({ "v": 1, "id": "n-3", "kind": "res", "op": "pane.open", "body": body })
                .to_string()
        };
        shared.route_host(&opened("p1-chief", Some(own)), &host, &daemon);
        shared.route_host(&opened("p1-zeus", None), &host, &daemon);
        // Another answer is no window's open.
        shared.route_host(
            r#"{"v":1,"id":"n-4","kind":"res","op":"pane.snapshot","body":{"id":"p1-chief","generation":1,"pid":7}}"#,
            &host,
            &daemon,
        );
        let seen = shared.seen();
        assert_eq!(
            seen.roots,
            [
                Root {
                    id: json!("p1-chief"),
                    generation: json!(1),
                    identity: ThisMachine.identity(own).unwrap(),
                },
                Root {
                    id: json!("p1-zeus"),
                    generation: json!(1),
                    identity: None,
                },
            ]
        );
        assert!(seen.roots[0].identity.is_some());
    }

    #[test]
    fn the_answer_to_a_request_of_the_cases_to_the_pane_host_stops_there_and_a_stray_line_is_kept()
    {
        let shared = Shared::default();
        let (host, _, daemon, daemon_lines) = sinks();
        let id = shared.next_host_id();
        assert_eq!(id, "n-test-1");
        let answer = shared.pending.expect(&id);
        shared.route_host(
            &format!(
                r#"{{"v":1,"id":"{id}","kind":"res","op":"pane.input","body":{{"ok":true}}}}"#
            ),
            &host,
            &daemon,
        );
        assert_eq!(
            answer.recv_timeout(Duration::from_secs(10)).unwrap(),
            json!({"ok": true})
        );
        shared.route_host("not a frame", &host, &daemon);
        assert_eq!(shared.seen().stray, ["not a frame"]);
        assert!(daemon_lines
            .recv_timeout(Duration::from_millis(100))
            .is_err());
    }
}

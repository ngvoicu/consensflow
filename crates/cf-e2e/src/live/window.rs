//! A real harness's window in the pane host of a rig, opened as the app opens
//! one, for the live tests that look at what a window does and not at what a
//! project does: in a folder of its own under the home the rig gives its windows
//! (the machine's, for the machine's harnesses), on the harness's cheap model or
//! the agent a test names, with its screen and its own record to read.
//!
//! The window is opened by the pane host, with no daemon in between ([`Open`]):
//! the command line is the harness's own for a new conversation (`harness-ask`,
//! which is the product's code), a Claude window has the settings file the app
//! writes for a launch and the flags that shut the human's MCP servers and
//! browser out, and Claude Code is asked, in a window of its own that is closed
//! after, to trust the folder first ([`trust_claude`]). A Devin or Pi window may
//! be a member's ([`Member`]): the launch is then the adapter's whole, Devin's
//! config and hooks and prompt file, Pi's extension and role skill, among it. What goes in goes in as
//! the daemon sends it ([`Window::send`]: a bracketed paste, then Enter once the
//! window has drawn it), and nothing presses Enter a second time. What a window
//! printed is read as the text of its screen, and as it came ([`raw_end`]), which
//! tells a blank screen from a window that printed nothing.

mod open;
mod raw;
#[cfg(test)]
mod tests;
mod trust;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::ask::{self, Interrupt, Item};
use super::last_lines;
use super::machine::Harness;
use super::members::Identity;
use crate::rig::Rig;
use crate::Result;

pub use open::{pasted_installs, session_for, without_the_daemon, Member, Open};
pub use raw::{raw_end, RawEnd};
pub use trust::claude as trust_claude;

/// How long a window may take to draw its prompt, and a sent message its answer.
pub const READY: Duration = Duration::from_secs(120);
pub const ANSWER: Duration = Duration::from_secs(180);

/// How long a window holds still before it counts as drawn, or done with its turn.
const STILL: Duration = Duration::from_secs(3);

/// How often the screen of a window is looked at while it is waited for, and the
/// record of one while an answer is.
const LOOK: Duration = Duration::from_millis(200);
const ASKING: Duration = Duration::from_millis(500);

/// The key that interrupts a turn, and the gap of a double press: the daemon's
/// own (`crates/cf-engine/src/windows.rs`).
const ESCAPE: u8 = 27;
const DOUBLE_PRESS: Duration = Duration::from_millis(150);

/// What came of sending a message to a window.
#[derive(Debug, Clone, PartialEq)]
pub struct Sent {
    /// What the pane host answered the paste with.
    pub written: Value,
    /// Whether the answer was seen, on the screen or in the harness's record.
    pub shown: bool,
    /// How long after the paste it was (or the wait ran out), in seconds.
    pub seconds: String,
}

/// A harness's window, open in the pane host of a rig. It is killed when it is
/// dropped, whatever ended the test.
pub struct Window<'r> {
    rig: &'r Rig,
    harness: Harness,
    kind: String,
    id: String,
    workspace: PathBuf,
    argv: Vec<String>,
    session: RefCell<Option<String>>,
    opened_at_ms: i64,
    env: Vec<(String, String)>,
    trust: Option<&'static str>,
    killed: Cell<bool>,
}

impl Window<'_> {
    /// Which harness it is of.
    pub fn harness(&self) -> Harness {
        self.harness
    }

    /// Its id, which is its pane's.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Which process its program is, as the pane host said it opened it (`pane.open`'s
    /// `pid`), read once as the host answered and kept ([`Rig::window_process`]): what
    /// descends from it is the window's own tree, and nothing does once it has gone. None
    /// where the host did not say, or the process had gone when it was read.
    pub fn process(&self) -> Option<Identity> {
        self.rig.window_process(&self.pane())
    }

    /// Its pane, as the pane host takes it in a request.
    pub fn pane(&self) -> Value {
        json!({ "id": self.id, "generation": 1 })
    }

    /// The folder it runs in.
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// The command line the pane host was asked to open it on: the harness's
    /// program, what a Claude window has besides (its settings and the flags that
    /// shut MCP servers and the browser out), and the harness's own arguments for
    /// a new conversation (its model among them).
    pub fn argv(&self) -> &[String] {
        &self.argv
    }

    /// What came of asking Claude to trust its folder: none for a harness that
    /// asks nothing of it.
    pub fn trust(&self) -> Option<&'static str> {
        self.trust
    }

    /// The conversation the window opened on, where its harness lets the app name
    /// it (Claude, Pi), and otherwise the one found in its store since it
    /// opened, once there is one.
    pub fn session(&self) -> Option<String> {
        self.session.borrow().clone()
    }

    /// The window shows the conversation `session` now, as a `/clear` in it has it
    /// begin one: what [`Window::record`] reads.
    pub fn follow(&self, session: &str) {
        *self.session.borrow_mut() = Some(session.to_owned());
    }

    /// What it printed so far, as the text on its screen ([`Rig::screen`]: the last 8
    /// MiB or so of it).
    pub fn screen(&self) -> String {
        self.rig.screen(&self.id)
    }

    /// How much it printed so far, in characters of its screen, those the rig let go
    /// of among them: a mark to ask what it printed after ([`Window::screen_after`]).
    pub fn mark(&self) -> usize {
        let (screen, dropped) = self.rig.screen_and_dropped(&self.id);
        dropped + screen.chars().count()
    }

    /// What it printed after the mark `mark`: all the rig keeps of it, where the rig
    /// let go of the place.
    pub fn screen_after(&self, mark: usize) -> String {
        let (screen, dropped) = self.rig.screen_and_dropped(&self.id);
        after(&screen, mark.saturating_sub(dropped))
    }

    /// What it printed so far, as it came, bytes and all: the end of it ([`raw_end`]) tells
    /// a blank screen from a window that printed nothing.
    pub fn printed(&self) -> Vec<u8> {
        self.rig.printed(&self.id)
    }

    /// The end of its screen, as a line ([`last_lines`]): what a report says of a
    /// window to say why it did not open or did not answer.
    pub fn tail(&self) -> String {
        last_lines(&self.screen())
    }

    /// Whether the window has drawn its prompt, waiting for it for [`READY`]
    /// ([`Window::still`]); where it has not, the end of its screen, which says
    /// why: a login page, or a dialog that nobody answers.
    pub fn ready(&self) -> std::result::Result<(), String> {
        if self.still(READY) {
            Ok(())
        } else {
            Err(self.tail())
        }
    }

    /// The Claude config folder the window's Claude Code keeps its sessions and
    /// its transcripts in ([`super::claude`] reads them there): the
    /// `CLAUDE_CONFIG_DIR` the window has, and else `.claude` in its home.
    pub fn claude_config(&self) -> PathBuf {
        claude_config_of(&self.env)
    }

    /// Whether it has ended.
    pub fn closed(&self) -> bool {
        self.rig.exits().iter().any(|exit| exit["id"] == self.id)
    }

    /// Until the window has drawn something and then held still for three
    /// seconds, or `within` has passed: whether it did. A window that ended did
    /// not, and one that printed only blanks has drawn nothing ([`drawn`]).
    pub fn still(&self, within: Duration) -> bool {
        held_still(
            || drawn(&self.screen()),
            || self.closed(),
            within,
            STILL,
            LOOK,
        )
    }

    /// `body` as the app gives it to this window: Devin on Windows gets its marks
    /// in ASCII.
    pub fn given(&self, body: &str) -> Result<String> {
        let text = ask::window_text(body)?;
        if self.harness == Harness::Devin && cfg!(windows) {
            ask::console_text(&text)
        } else {
            Ok(text)
        }
    }

    /// What the harness's own record says so far, as the daemon reads it: its
    /// items, and whether its turn is over. None while there is no record, or none
    /// can be read.
    pub fn record(&self) -> Option<ask::Record> {
        let known = self.session.borrow().clone();
        let session = known.or_else(|| {
            let found =
                ask::newest_session(&self.kind, &self.workspace, self.opened_at_ms, &self.env)
                    .ok()
                    .flatten();
            self.session.borrow_mut().clone_from(&found);
            found
        });
        session.and_then(|session| ask::records(&self.kind, &session, &self.env).ok().flatten())
    }

    /// What the harness's own record holds so far, as the daemon reads it:
    /// nothing while there is no record, or none can be read.
    pub fn recorded(&self) -> Vec<Item> {
        self.record().map(|record| record.items).unwrap_or_default()
    }

    /// Marks the window as the daemon does where a turn begins (`pane.mark`), with the way
    /// its harness keeps its servers as the product's adapter says it on this platform
    /// ([`servers_of`]): what the pane host answered.
    pub fn mark_turn(&self) -> Result<Value> {
        let mut mark = json!({ "id": self.id, "generation": 1, "harness": self.harness.name() });
        if let Some(servers) = servers_of(self.harness, cfg!(windows)) {
            mark["servers"] = json!(servers);
        }
        self.rig.host("pane.mark", mark)
    }

    /// How many commands of its harness the window's tree runs now, as the pane host tells
    /// them (`pane.commands`): none where it cannot tell, or did not answer.
    pub fn commands(&self) -> Option<u64> {
        self.commands_after(None).map(|(total, _)| total)
    }

    /// [`Window::commands`], less what began before `after` (milliseconds since the epoch),
    /// as the daemon asks where no rest proves a monitor: how many, and the first of them
    /// the host lists, each its number and command line.
    pub fn commands_after(&self, after: Option<i64>) -> Option<(u64, Vec<(u32, String)>)> {
        let mut asked = json!({ "id": self.id, "generation": 1 });
        if let Some(after) = after {
            asked["after"] = json!(after);
        }
        let answer = self.rig.host("pane.commands", asked).ok()?;
        let total = answer["commands"]["total"].as_u64()?;
        let listed = answer["commands"]["processes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|process| {
                Some((
                    u32::try_from(process["pid"].as_u64()?).ok()?,
                    process["command"].as_str()?.to_owned(),
                ))
            })
            .collect();
        Some((total, listed))
    }

    /// Pastes `body` as the daemon does (`pane.write_paste`: a bracketed paste,
    /// then Enter once the window has drawn it): what the pane host answered.
    pub fn paste(&self, body: &str) -> Result<Value> {
        self.rig.host(
            "pane.write_paste",
            json!({ "id": self.id, "generation": 1, "body": body }),
        )
    }

    /// Presses the key `bytes` (Escape is `[27]`, Control-C `[3]`) as the daemon
    /// does (`pane.input`): what the pane host answered.
    pub fn press(&self, bytes: &[u8]) -> Result<Value> {
        self.rig.host(
            "pane.input",
            json!({ "id": self.id, "generation": 1, "bytes": bytes }),
        )
    }

    /// Types `bytes` into the window as the human does at the app's page
    /// ([`Rig::types`]): the pane host counts what they leave unsent.
    pub fn types(&self, bytes: &[u8]) -> Result<()> {
        self.rig.types(&self.pane(), bytes)
    }

    /// What the pane host says of the window now (`pane.snapshot`), with the
    /// last `bottom` lines of its screen as drawn.
    pub fn snapshot(&self, bottom: usize) -> Result<Value> {
        self.rig.host(
            "pane.snapshot",
            json!({ "id": self.id, "generation": 1, "bottom": bottom }),
        )
    }

    /// Interrupts the window as the daemon does: Escape as many times in a row
    /// (150 ms apart, the daemon's own gap) as the harness's adapter asks for
    /// ([`ask::interrupt`]) and, where those presses open a dialog at a turn that
    /// had just ended (Devin's rewind, which the next message's Enter would
    /// confirm, cutting the conversation back), one more after a pause, which
    /// closes it and is nothing anywhere else.
    pub fn interrupt(&self, how: &Interrupt) -> Result<()> {
        for press in 0..how.presses {
            if press > 0 {
                sleep(DOUBLE_PRESS);
            }
            self.press(&[ESCAPE])?;
        }
        if let Some(after) = how.close_after {
            sleep(after);
            self.press(&[ESCAPE])?;
        }
        Ok(())
    }

    /// Sends `body` ([`Window::paste`]), and waits for `answer` on the screen or
    /// in the record, for [`ANSWER`]: Windows' console host may draw "5555" in
    /// two strokes, which the screen's text then splits. Nothing presses Enter a
    /// second time.
    pub fn send(&self, body: &str, answer: &str) -> Result<Sent> {
        self.sent(answer, || self.paste(body))
    }

    /// [`Window::send`], the paste telling the pane host that the input box read
    /// empty when it had taken `typed` writes of the human's keys (`readEmpty`), as
    /// an adapter that reads its harness's box sends one.
    pub fn send_read_empty(&self, body: &str, answer: &str, typed: u64) -> Result<Sent> {
        self.sent(answer, || {
            self.rig.host(
                "pane.write_paste",
                json!({ "id": self.id, "generation": 1, "body": body, "readEmpty": typed }),
            )
        })
    }

    /// What came of the paste `paste` makes, `answer` waited for.
    fn sent(&self, answer: &str, paste: impl FnOnce() -> Result<Value>) -> Result<Sent> {
        let mark = self.mark();
        let pasted = Instant::now();
        let written = paste()?;
        let mut shown = false;
        while !shown && pasted.elapsed() < ANSWER && !self.closed() {
            sleep(ASKING);
            shown = answered(&self.screen_after(mark), &self.recorded(), answer);
        }
        Ok(Sent {
            written,
            shown,
            seconds: super::seconds(
                i64::try_from(pasted.elapsed().as_millis()).unwrap_or(i64::MAX),
            ),
        })
    }

    /// Ends the window.
    pub fn kill(&self) {
        if !self.killed.replace(true) {
            let _ = self.rig.host("pane.kill", self.pane());
        }
    }
}

impl Drop for Window<'_> {
    /// A window the test did not end is not left running.
    fn drop(&mut self) {
        self.kill();
    }
}

/// What a screen printed after the mark `mark` (a count of characters).
fn after(screen: &str, mark: usize) -> String {
    screen.chars().skip(mark).collect()
}

/// The Claude config folder of an environment: its `CLAUDE_CONFIG_DIR`, and
/// else `.claude` in its `HOME` (the last of a name given twice is the one).
fn claude_config_of(env: &[(String, String)]) -> PathBuf {
    let var = |name: &str| {
        env.iter()
            .rev()
            .find(|(given, value)| given == name && !value.is_empty())
            .map(|(_, value)| value.as_str())
    };
    match (var("CLAUDE_CONFIG_DIR"), var("HOME")) {
        (Some(config), _) => PathBuf::from(config),
        (None, Some(home)) => Path::new(home).join(".claude"),
        (None, None) => PathBuf::from(".claude"),
    }
}

/// Whether the answer is on what the screen printed since the message was
/// pasted, or in an answer of the record.
fn answered(printed: &str, items: &[Item], answer: &str) -> bool {
    printed.contains(answer)
        || items
            .iter()
            .any(|item| item.role == "assistant" && item.text.contains(answer))
}

/// How much of a window's screen is drawn: its characters that are not blanks.
/// Windows' console host opens a window with a jump of the cursor, which the
/// screen's text has as a line break, and a harness that takes seconds to start
/// (Devin) draws nothing after it for as long: a window that printed only that
/// has not drawn its prompt.
pub fn drawn(screen: &str) -> usize {
    screen.chars().filter(|at| !at.is_whitespace()).count()
}

/// How `harness` keeps its servers, as the product's adapter says it in the window's mark
/// (each adapter's `Window::servers`, `cf_proto::panes::Servers`), on Windows where `windows`
/// says so; none where the adapter says it does not know, and the mark then says nothing of it,
/// as the daemon's does. Windows has no process groups: there Claude Code runs each command of
/// its agent in a shell of its own (`byShell`, T-906), and a host told `inGroup` tells no
/// command, so a live run on Windows that marked every harness so read every look untold
/// (zeewin2, T-946).
fn servers_of(harness: Harness, windows: bool) -> Option<&'static str> {
    match (harness, windows) {
        (Harness::Claude, true) => Some("byShell"),
        (Harness::Claude | Harness::Pi, false) => Some("inGroup"),
        (Harness::Codex | Harness::Muse, false) => Some("atLaunch"),
        _ => None,
    }
}

/// Whether a window held still: its output (`length`, read every `step`) has
/// stopped growing for `hold`, with something printed, before `within` was up
/// and before it `closed`.
fn held_still(
    mut length: impl FnMut() -> usize,
    mut closed: impl FnMut() -> bool,
    within: Duration,
    hold: Duration,
    step: Duration,
) -> bool {
    let started = Instant::now();
    let (mut seen, mut since) = (None, Instant::now());
    while started.elapsed() < within && !closed() {
        let now = length();
        if seen != Some(now) {
            (seen, since) = (Some(now), Instant::now());
        } else if now > 0 && since.elapsed() >= hold {
            return true;
        }
        sleep(step);
    }
    false
}

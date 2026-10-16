//! A turn of the window: what a message asks of it, and how it ends. It speaks
//! the words the other stand-in does: a line `SLEEP <seconds> <words>` is a turn
//! that works that long before it does what the words say, a line `BACKGROUND
//! <seconds> <words>` is a turn that leaves a command running for that long
//! (`command`) and does what the words say, and `Reply with exactly: X` is
//! answered with X, as anything else is acknowledged. What Devin writes of a turn
//! is written in its order: the message in the store and on the wire, the agent's
//! thought, the tool it runs (the command a `BACKGROUND` leaves), and then the
//! answer (the store first, so that a reader that sees the turn end on the wire
//! finds the answer in the store) or, where an Escape stops the turn, the cancel
//! and what was said so far. A command that was left goes on saying what it prints
//! once the turn has ended.
//!
//! A turn takes the time a model takes ([`Pace`]): from the message put to the
//! agent to its first word, in which the turn is begun and its work is not to be
//! seen, and the work itself, which no words but a `SLEEP` set. What a message
//! the window is handed while a turn is at work comes to is the window's
//! (`fake_devin.rs`): it waits, and is written to the store when its turn begins.
//!
//! A line `FILE <name> <words>` is a turn whose tool makes the file `name` once it
//! has worked, and does what the words say. Every call of a tool is asked of the
//! window's permission mode first (`permission`, `calls`): a message that is
//! `/bypass` switches the window to full permission and makes no turn, or makes the
//! turn of the prompt after it. A message that says to go on where the turn stopped
//! (the note of a quit that cut the turn) goes on with the message of the turn the
//! session was cut in, its calls and its answer, its wait done.

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use cf_e2e::pattern;
use regex::Regex;

use crate::calls::Said;
use crate::command;
use crate::permission::{self, Call, Permission, BYPASS, SWITCHED};
use crate::said;
use crate::stop::Stop;
use crate::store::Store;
use crate::wire::Wire;

/// What the note of a quit that cut a turn asks: the engine's own words
/// (`RESUME_WORDS`).
pub const GO_ON: &str = "Go on where you stopped.";

/// The variables that set a window's [`Pace`], in milliseconds: nothing in a
/// person's environment sets them, and a window with neither takes no time at all.
pub const FIRST_WORD_VARIABLE: &str = "FAKE_DEVIN_FIRST_WORD_MS";
pub const WORK_VARIABLE: &str = "FAKE_DEVIN_WORK_MS";

/// How long a turn takes where its message does not say.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pace {
    /// From the message put to the agent to the agent's first word (what Devin
    /// measures as `ttftMs`): the turn is begun, its message is in the store and on
    /// the wire, and nothing of its work is. A look at the window finds it at rest.
    pub first_word: Duration,
    /// How long a turn that asks for no `SLEEP` works before it answers.
    pub work: Duration,
}

impl Pace {
    /// The pace the two variables name: each a number of milliseconds, or none.
    pub fn of(first_word: Option<&str>, work: Option<&str>) -> Self {
        let named = |ms: Option<&str>| {
            ms.and_then(|ms| ms.parse().ok())
                .map_or(Duration::ZERO, Duration::from_millis)
        };
        Self {
            first_word: named(first_word),
            work: named(work),
        }
    }
}

/// What a turn is asked to do.
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    /// How long it works before it answers.
    pub work: Duration,
    /// How many seconds of a command it leaves running as it answers, if it does.
    pub background: Option<u64>,
    /// The file its tool makes once it has worked, if it makes one.
    pub file: Option<String>,
    /// What it answers.
    pub answer: String,
}

impl Plan {
    /// The plan a turn goes on with, after a quit cut the one it was: its calls and
    /// its answer, and the time the pace gives a turn, not its own wait.
    #[must_use]
    pub fn going_on(self, pace: Pace) -> Self {
        Self {
            work: pace.work,
            ..self
        }
    }
}

/// What the message `text` asks of a turn, which works as long as `pace` says
/// where the message says nothing of it.
pub fn plan_of(text: &str, pace: Pace) -> Plan {
    static SLEEP: OnceLock<Regex> = OnceLock::new();
    static BACKGROUND: OnceLock<Regex> = OnceLock::new();
    static FILE: OnceLock<Regex> = OnceLock::new();
    static EXACT: OnceLock<Regex> = OnceLock::new();
    let (work, words) = match pattern::once(&SLEEP, r"(?m)^SLEEP (\d+) (.+)$").captures(text) {
        Some(slow) => (
            Duration::from_secs(slow[1].parse().unwrap_or(0)),
            slow[2].to_owned(),
        ),
        None => (pace.work, text.to_owned()),
    };
    let (background, words) =
        match pattern::once(&BACKGROUND, r"(?m)^BACKGROUND (\d+) (.+)$").captures(&words) {
            Some(left) => (left[1].parse().ok(), left[2].to_owned()),
            None => (None, words),
        };
    let (file, words) = match pattern::once(&FILE, r"(?m)^FILE (\S+) (.+)$").captures(&words) {
        Some(made) => (Some(made[1].to_owned()), made[2].to_owned()),
        None => (None, words),
    };
    let answer = match pattern::once(&EXACT, r"Reply with exactly: (\S+)").captures(&words) {
        Some(exact) => exact[1].to_owned(),
        None => format!("noted: {}", words.split('\n').next().unwrap_or_default()),
    };
    Plan {
        work,
        background,
        file,
        answer,
    }
}

/// The window's turns, one at a time, in the order the messages came.
pub struct Window<W: Write> {
    store: Store,
    wire: Wire,
    stop: Arc<Stop>,
    /// What the window says on its screen.
    screen: W,
    pace: Pace,
    turns: u64,
    /// Where a command a turn leaves is written, where the window has any.
    jobs: Option<Jobs>,
    /// The window's permission mode, which its key reader answers a dialog of.
    permission: Arc<Permission>,
    /// The folder its tools make files in: the one it runs in.
    folder: PathBuf,
    /// How many calls its turns have made.
    calls: u64,
}

/// Where the rig looks for the jobs of a window, which window it is (its process), and
/// how long the agent thinks before it runs its tool: a command a turn leaves is a job.
struct Jobs {
    folder: PathBuf,
    window: u32,
    thinking: Duration,
}

impl<W: Write> Window<W> {
    pub fn new(store: Store, wire: Wire, stop: Arc<Stop>, screen: W, pace: Pace) -> Self {
        Self {
            store,
            wire,
            stop,
            screen,
            pace,
            turns: 0,
            jobs: None,
            permission: Arc::new(Permission::new(true)),
            folder: std::env::current_dir().unwrap_or_default(),
            calls: 0,
        }
    }

    /// The window is in the mode `permission` says, which a dialog of its calls is
    /// answered through, and its tools make files in `folder`.
    #[must_use]
    pub fn in_mode(mut self, permission: Arc<Permission>, folder: PathBuf) -> Self {
        self.permission = permission;
        self.folder = folder;
        self
    }

    /// The window's jobs are written in `folder`, as those of the window `window`; its
    /// agent thinks for `thinking` before it runs a tool.
    #[must_use]
    pub fn leaving_jobs_in(mut self, folder: PathBuf, window: u32, thinking: Duration) -> Self {
        self.jobs = Some(Jobs {
            folder,
            window,
            thinking,
        });
        self
    }

    /// Says `text` on the window's screen.
    fn say(&mut self, text: &str) -> io::Result<()> {
        self.screen.write_all(text.as_bytes())?;
        self.screen.flush()
    }

    /// The window takes the message `text`, pasted: its turn, from the message to the
    /// answer or to the Escape that stops it.
    #[cfg(test)]
    pub fn take(&mut self, text: &str) -> Result<(), String> {
        self.take_message(text, false)
    }

    /// The window takes the message `text`, the first of its launch where
    /// `prompt_file`, as [`Window::take`] does.
    pub fn take_message(&mut self, text: &str, prompt_file: bool) -> Result<(), String> {
        self.stop.begin();
        let done = self.command(text, prompt_file);
        self.stop.end();
        done
    }

    /// A message that is `/bypass` switches the window to full permission, and the
    /// prompt after it, if any, is a turn; any other message is a turn.
    fn command(&mut self, text: &str, prompt_file: bool) -> Result<(), String> {
        let Some(prompt) = permission::command(text) else {
            return self.turn(text, prompt_file);
        };
        if self.permission.to_bypass() {
            self.wire
                .mode_changed(self.permission.mode())
                .map_err(said)?;
        }
        self.say(&format!("{BYPASS}\n{SWITCHED}\n")).map_err(said)?;
        if prompt.is_empty() {
            return Ok(());
        }
        self.turn(prompt, prompt_file)
    }

    /// What the next call of the turn `client` is said with.
    fn next_call<'a>(&'a mut self, client: &'a str) -> Said<'a, W> {
        self.calls += 1;
        Said {
            permission: &self.permission,
            wire: &mut self.wire,
            store: &mut self.store,
            screen: &mut self.screen,
            client,
            call: format!("{client}-call-{}", self.calls),
        }
    }

    fn turn(&mut self, text: &str, prompt_file: bool) -> Result<(), String> {
        self.turns += 1;
        let client = format!("client-{}", self.turns);
        let stream = format!("stream-{}", self.turns);
        let plan = match self.store.take_cut().filter(|_| text.contains(GO_ON)) {
            Some(cut) => plan_of(&cut, self.pace).going_on(self.pace),
            None => plan_of(text, self.pace),
        };
        self.store.user(text, &client)?;
        self.wire.prompt(self.turns + 1, text).map_err(said)?;
        self.say("working (esc twice to interrupt)\n")
            .map_err(said)?;
        // The turn is begun, and its agent has not said a word: a model's time,
        // in which the wire shows none of its work.
        let mut stopped = self.stop.work(self.pace.first_word);
        let mut left = None;
        if !stopped {
            self.wire.thinking(&client).map_err(said)?;
            // The agent has thought, and then runs its tool: the command it leaves running.
            if let (Some(seconds), Some(thinking)) = (
                plan.background,
                self.jobs.as_ref().map(|jobs| jobs.thinking),
            ) {
                std::thread::sleep(thinking);
                let asked = self
                    .next_call(&client)
                    .ask(&format!("sleep {seconds}"), prompt_file)?;
                stopped = asked == Call::Cancelled;
                if let (Call::Runs, Some(jobs)) = (asked, &self.jobs) {
                    left = Some(command::start(
                        seconds,
                        &jobs.folder,
                        jobs.window,
                        &mut self.wire,
                        &client,
                    )?);
                }
            }
            stopped = stopped || self.stop.work(plan.work);
        }
        // Once it has worked, the tool that makes a file.
        if let (false, Some(name)) = (stopped, &plan.file) {
            let folder = self.folder.clone();
            let mut call = self.next_call(&client);
            match call.ask(&format!("write {name}"), prompt_file)? {
                Call::Runs => call.make(&folder, name)?,
                Call::Skipped => {}
                Call::Cancelled => stopped = true,
            }
        }
        if stopped {
            // The TUI asks its agent to cancel, which keeps what it said so far.
            self.wire.cancel().map_err(said)?;
            self.wire
                .said("Interrupted", &stream, &client)
                .map_err(said)?;
            self.store.assistant("Interrupted", false)?;
            self.wire.ended("cancelled", &client).map_err(said)?;
            return self.say("Interrupted\n").map_err(said);
        }
        self.wire
            .said(&plan.answer, &stream, &client)
            .map_err(said)?;
        self.store.assistant(&plan.answer, true)?;
        self.wire.ended("complete", &client).map_err(said)?;
        // The turn is over, and the command goes on saying what it prints.
        if let Some(left) = left {
            left.go_on_reporting(self.wire.another().map_err(said)?);
        }
        self.say(&format!("{}\n", plan.answer)).map_err(said)
    }
}

// The cases of the permission modes, beside the turn's own.
#[cfg(test)]
#[path = "turn_modes.rs"]
mod modes;

#[cfg(test)]
#[path = "turn_tests.rs"]
mod tests;

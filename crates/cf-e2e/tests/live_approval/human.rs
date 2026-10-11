//! The human, played through the calls the app makes: the page's (what the
//! board and the buttons ask of the daemon) and the keys typed into the
//! chief's window, which the pane host takes as it takes the app's. The human
//! never types into a staff window, and never plays the chief with its token:
//! the chief is a real harness that runs `cf` itself.

use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::{cut, text, wait, Log};
use cf_e2e::rig::{Project, Rig};
use cf_e2e::{Error, Result};
use serde_json::{json, Value};

use crate::trace::Trace;

/// How long a chief has to read idle: its window is started with the project,
/// on a harness that may take its time to come up.
const READY: Duration = Duration::from_secs(240);

/// How long a window's screen holds still before it counts as drawn, and the
/// longest a run waits for that.
const STILL: Duration = Duration::from_secs(3);
const SETTLE: Duration = Duration::from_secs(30);

/// How long a chief is given to go to work on what was typed to it before the
/// human presses Enter again, and how many times they do: a pasted prompt may
/// wait in a harness's input box for an Enter of its own.
const TAKES: Duration = Duration::from_secs(5);
const ENTERS: usize = 3;

/// The key Enter sends.
const ENTER: u8 = 13;

/// The human at a project.
pub struct Human<'a> {
    rig: &'a Rig,
    project: Project<'a>,
    log: &'a Log,
}

impl<'a> Human<'a> {
    pub fn new(rig: &'a Rig, project: Project<'a>, log: &'a Log) -> Self {
        Self { rig, project, log }
    }

    /// The chief's lane of the board, as the page draws it.
    pub fn chief(&self) -> Result<Value> {
        let board = self.project.board()?;
        Ok(board["lanes"]
            .as_array()
            .and_then(|lanes| {
                lanes
                    .iter()
                    .find(|lane| lane["participant"]["handle"] == "chief")
            })
            .cloned()
            .unwrap_or(Value::Null))
    }

    /// What the chief's window is doing, as the board says: `idle`, `working`…
    pub fn chief_is(&self) -> Result<String> {
        Ok(text(&self.chief()?["activity"]["state"]))
    }

    /// Waits for the chief's window to read idle and to have drawn its prompt.
    pub fn waits_for_the_chief(&self) -> Result {
        wait(
            self.rig,
            self.log,
            "the chief's window to read idle",
            READY,
            || Ok(self.chief_is()? == "idle"),
        )?;
        self.settles(&self.chief()?["pane"]);
        Ok(())
    }

    /// Types `words` into the chief's window as the human does: a bracketed
    /// paste and Enter, once the chief is idle, and Enter again while the
    /// window keeps the text. How many times it took Enter again.
    pub fn tells(&self, words: &str) -> Result<usize> {
        self.waits_for_the_chief()?;
        self.log.say(format!(
            "the human types into the chief's window: {}",
            cut(words, 200)
        ));
        let mark = Trace::read(self.rig).len();
        self.rig.tell(self.project.id(), words)?;
        let pane = self.chief()?["pane"].clone();
        for again in 0..=ENTERS {
            sleep(TAKES);
            let worked = Trace::read(self.rig).worked_after("chief", mark);
            if worked || self.chief_is()? != "idle" {
                return Ok(again);
            }
            if again == ENTERS {
                break;
            }
            self.log
                .say("the chief's window has not taken the text: the human presses Enter again");
            self.rig.host(
                "pane.input",
                json!({ "id": pane["id"], "generation": pane["generation"], "bytes": [ENTER] }),
            )?;
        }
        Err(Error::Daemon(format!(
            "the chief's window did not take what the human typed, or went to work on it, in \
             {} Enters; its screen:\n{}",
            ENTERS + 1,
            self.screen(&pane, 12).join("\n")
        )))
    }

    /// The message held at the gate now that `matches`, if there is one.
    pub fn gated_now(&self, matches: impl Fn(&Value) -> bool) -> Result<Option<Value>> {
        Ok(self.project.board()?["gated"]
            .as_array()
            .and_then(|held| held.iter().find(|message| matches(message)))
            .cloned())
    }

    /// The message held at the gate that `matches`, waited for.
    pub fn waits_for_gated(
        &self,
        what: &str,
        within: Duration,
        matches: impl Fn(&Value) -> bool,
    ) -> Result<Value> {
        self.waits_for_gated_while(what, within, matches, || Ok(()))
    }

    /// [`Human::waits_for_gated`], looking at how things are getting on at each
    /// look (`each`: it says so, or ends the wait with the error it returns).
    pub fn waits_for_gated_while(
        &self,
        what: &str,
        within: Duration,
        matches: impl Fn(&Value) -> bool,
        mut each: impl FnMut() -> Result,
    ) -> Result<Value> {
        wait(self.rig, self.log, what, within, || {
            each()?;
            Ok(self.gated_now(&matches)?.is_some())
        })?;
        self.gated_now(&matches)?
            .ok_or_else(|| Error::Daemon(format!("{what}: it was held, and is gone")))
    }

    /// Passes a held message on, as the board's Approve does.
    pub fn approves(&self, message: &Value) -> Result<Value> {
        self.log.say(format!(
            "the human approves m-{} ({} to @{}): {}",
            message["id"],
            text(&message["kind"]),
            text(&message["recipient"]),
            cut(&text(&message["body"]), 100)
        ));
        self.decides("message.approve", message)
    }

    /// Declines a held message, as the board's Decline does.
    pub fn declines(&self, message: &Value) -> Result<Value> {
        self.log.say(format!(
            "the human declines m-{} ({} to @{}): {}",
            message["id"],
            text(&message["kind"]),
            text(&message["recipient"]),
            cut(&text(&message["body"]), 100)
        ));
        self.decides("message.decline", message)
    }

    fn decides(&self, op: &str, message: &Value) -> Result<Value> {
        let answer = self.rig.page(op, json!({ "message": message["id"] }))?;
        if answer["ok"] == true {
            Ok(answer["message"].clone())
        } else {
            Err(Error::Daemon(format!("{op} was refused: {answer}")))
        }
    }

    /// The last `lines` lines of a window's screen, as the pane host keeps it:
    /// none where the window is gone.
    pub fn screen(&self, pane: &Value, lines: usize) -> Vec<String> {
        let asked = self.rig.host(
            "pane.snapshot",
            json!({ "id": pane["id"], "generation": pane["generation"], "tail": lines }),
        );
        asked
            .ok()
            .and_then(|answer| answer["tail"].as_array().cloned())
            .map(|shown| shown.iter().map(text).collect())
            .unwrap_or_default()
    }

    /// What a window's tree runs that began after its turn did, as the pane host
    /// says it (the work the daemon holds a task's result back for): a line to
    /// each process, after a mark.
    pub fn background(&self, pane: &Value) -> String {
        let asked = self.rig.host(
            "pane.background",
            json!({ "id": pane["id"], "generation": pane["generation"] }),
        );
        let Ok(answer) = asked else {
            return String::new();
        };
        answer["processes"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|process| {
                format!(
                    "\n    ~ {} {}",
                    text(&process["pid"]),
                    cut(&text(&process["command"]), 140)
                )
            })
            .collect()
    }

    /// Until a window's screen has held still for a few seconds, or a while
    /// has passed: a prompt typed before the harness has drawn its own is lost.
    pub fn settles(&self, pane: &Value) {
        let started = Instant::now();
        let mut seen = self.screen(pane, 12);
        let mut since = Instant::now();
        while started.elapsed() < SETTLE && since.elapsed() < STILL {
            sleep(Duration::from_millis(500));
            let now = self.screen(pane, 12);
            if now != seen {
                seen = now;
                since = Instant::now();
            }
        }
    }
}

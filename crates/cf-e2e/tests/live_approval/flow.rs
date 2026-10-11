//! What the steps have in common: the human has the chief give a task; its
//! brief waits at the gate and no window opens; they approve it and the window
//! opens and is given the brief; the window's result waits at the gate and
//! the chief is told nothing; they approve it and it is typed into the chief
//! once, after its turn; and the chief accepts. Each of these ends in a verdict
//! on what the daemon showed, and each wait says what it waits for.

use std::cell::RefCell;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::trace::Trace;
use cf_e2e::live::{cut, text, wait, Log};
use cf_e2e::rig::{Project, Rig};
use cf_e2e::{Error, Result};
use serde_json::{json, Value};

use crate::human::Human;
use crate::plan::Planned;
use crate::verdict;

/// How long the chief has to put the brief on the board once the human has
/// typed to it: it reads, thinks and runs `cf`.
pub const BRIEF_HELD: Duration = Duration::from_secs(300);

/// How long a brief at the gate is watched to open no window, and how long a
/// result at the gate is watched to be typed nowhere.
pub const QUIET: Duration = Duration::from_secs(8);

/// How long the daemon has to open a window once its brief is passed on: the
/// harness's own start.
pub const OPENS: Duration = Duration::from_secs(180);

/// How long a window has to be given its brief, once it is open.
pub const GIVEN: Duration = Duration::from_secs(120);

/// How long a window has to do a short task and its result to be held.
pub const ANSWERS: Duration = Duration::from_secs(480);

/// How long a result passed on has to be typed into the chief, which may be
/// at the end of a turn of its own.
pub const DELIVERS: Duration = Duration::from_secs(360);

/// How long the chief has to accept a task once its result was typed into it,
/// and how long it is left to do that before the human asks it to.
pub const ACCEPTS: Duration = Duration::from_secs(300);
pub const ASKED_AFTER: Duration = Duration::from_secs(90);

/// How long a window has to close once its task is over.
pub const CLOSES: Duration = Duration::from_secs(120);

/// How often a run that has nothing new to say says it is still waiting.
const HEARTBEAT: Duration = Duration::from_secs(30);

/// The command the human has a chief run to keep it at work while a result is
/// passed on to it: it takes twenty-five seconds, and is on the `PATH` a window
/// has. (Not `sleep`, which Claude Code starts in the background and so ends
/// its turn on.)
const BUSY: &str = "perl -e 'sleep 25'";

/// What the human says to the chief once a task is given and until its result
/// arrives: nothing is done with the result but what they say here.
pub fn accepts(task: &str) -> String {
    format!("When its result arrives, run cf task accept with {task}, then reply with one line.")
}

/// A project of the run, being worked on by the human.
pub struct Ctx<'a> {
    pub rig: &'a Rig,
    pub project: Project<'a>,
    pub human: Human<'a>,
    pub planned: &'a Planned,
    pub folder: &'a Path,
    pub log: &'a Log,
    /// What the run saw that is no failure of the brief: a chief that had to be
    /// asked again, a message that took more than one attempt.
    pub findings: RefCell<Vec<String>>,
    said: RefCell<(String, Instant)>,
}

/// A task given to a seat, as far as its brief is passed on.
#[derive(Debug, Clone)]
pub struct Given {
    pub task: i64,
    /// The member's session the daemon gave the task to: its lane's handle.
    pub session: String,
    /// The window it opens in.
    pub window: String,
    /// The brief, as it was held.
    pub brief: Value,
    /// The windows the member's earlier sessions had, which are not this task's.
    pub before: Vec<String>,
}

impl<'a> Ctx<'a> {
    pub fn new(
        rig: &'a Rig,
        project: Project<'a>,
        planned: &'a Planned,
        folder: &'a Path,
        log: &'a Log,
    ) -> Self {
        Self {
            rig,
            project,
            human: Human::new(rig, project, log),
            planned,
            folder,
            log,
            findings: RefCell::new(Vec::new()),
            said: RefCell::new((String::new(), Instant::now())),
        }
    }

    /// Records something the run saw that is no failure.
    pub fn note(&self, finding: impl Into<String>) {
        let finding = finding.into();
        self.log.say(format!("note: {finding}"));
        self.findings.borrow_mut().push(finding);
    }

    /// An error that says every way what was seen fell short, if it did.
    pub fn check(&self, what: &str, problems: Vec<String>) -> Result {
        if problems.is_empty() {
            return Ok(());
        }
        Err(Error::Daemon(format!(
            "{what}:\n  - {}",
            problems.join("\n  - ")
        )))
    }

    /// What the daemon has written so far.
    pub fn trace(&self) -> Trace {
        Trace::read(self.rig)
    }

    /// The task `number`, with its thread, as the page draws it.
    pub fn task(&self, number: i64) -> Result<Value> {
        self.project.task(number)
    }

    /// The state of the task `number`, on the board.
    pub fn state_of(&self, number: i64) -> Result<String> {
        Ok(text(&self.task(number)?["state"]))
    }

    /// The message `id` of the task `number`, as its thread holds it now.
    pub fn message(&self, number: i64, id: i64) -> Result<Value> {
        self.task(number)?["messages"]
            .as_array()
            .and_then(|thread| thread.iter().find(|message| verdict::id(message) == id))
            .cloned()
            .ok_or_else(|| Error::Daemon(format!("m-{id} is not in the thread of T-{number}")))
    }

    /// The `pane.open` of the window `id`, if the daemon has made it.
    pub fn frame(&self, id: &str) -> Option<Value> {
        self.rig
            .open_frames()
            .into_iter()
            .find(|frame| frame["id"] == id)
    }

    /// The windows the daemon opened for the sessions of the member `agent`.
    pub fn windows_of(&self, agent: &str) -> Vec<Value> {
        let prefix = format!("p{}-{agent}", self.project.id());
        self.rig
            .open_frames()
            .into_iter()
            .filter(|frame| {
                frame["id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with(&prefix))
            })
            .collect()
    }

    /// The ids of the windows of the member `agent` so far.
    fn window_ids_of(&self, agent: &str) -> Vec<String> {
        self.windows_of(agent)
            .iter()
            .map(|frame| text(&frame["id"]))
            .collect()
    }

    /// The windows the daemon opened for the member `agent` since `before`,
    /// the ids it had then: the windows of the task given since.
    pub fn opened_since(&self, agent: &str, before: &[String]) -> Vec<Value> {
        self.windows_of(agent)
            .into_iter()
            .filter(|frame| !before.contains(&text(&frame["id"])))
            .collect()
    }

    /// What the window of the lane `handle` is doing, as the board says.
    pub fn activity_of(&self, handle: &str) -> Result<String> {
        Ok(text(&self.project.lane(handle)?["activity"]["state"]))
    }

    /// Whether the window `id` has ended.
    pub fn ended(&self, id: &str) -> bool {
        self.rig.exits().iter().any(|exit| exit["id"] == id)
    }

    /// One line on how the task and its window are getting on, said when it
    /// changes and now and then when it does not, with the end of the window's
    /// screen: for a wait that is long.
    pub fn progress(&self, task: i64, session: &str) -> Result {
        let state = self.state_of(task)?;
        let activity = self.project.lane(session)?["activity"].clone();
        let window = text(&activity["state"]);
        // Why the board says what it does of the window, when it says why.
        let why = activity["reason"]
            .as_str()
            .map_or_else(String::new, |reason| format!(" ({reason})"));
        let chief = self.human.chief_is()?;
        let line =
            format!("T-{task} is {state}; @{session} is {window}{why}; the chief is {chief}");
        let mut said = self.said.borrow_mut();
        if line != said.0 || said.1.elapsed() >= HEARTBEAT {
            let frame = self.frame(&format!("p{}-{session}", self.project.id()));
            let screen = frame
                .as_ref()
                .map(|frame| self.human.screen(frame, 4))
                .unwrap_or_default()
                .iter()
                .map(|row| format!("\n    | {}", cut(row, 110)))
                .collect::<String>();
            // What the window's tree runs that its turn began, when the board holds it for that.
            let running = match &frame {
                Some(frame) if why.contains("background") => self.human.background(frame),
                _ => String::new(),
            };
            self.log.say(format!("{line}{screen}{running}"));
            *said = (line, Instant::now());
        }
        Ok(())
    }

    /// An error if the task is over without a result: failed, or called off.
    pub fn not_over(&self, task: i64) -> Result {
        let state = self.state_of(task)?;
        if state == "failed" || state == "cancelled" {
            let told: Vec<String> = self
                .project
                .inbox("chief")?
                .iter()
                .map(|message| cut(&text(&message["body"]), 300))
                .filter(|body| body.starts_with(&format!("T-{task} ")))
                .collect();
            return Err(Error::Daemon(format!(
                "T-{task} is {state}; the chief was told: {}",
                if told.is_empty() {
                    "nothing of it".to_owned()
                } else {
                    told.join(" | ")
                }
            )));
        }
        Ok(())
    }

    /// The human has the chief give a task: they type `ask` to it, and the
    /// brief it puts on the board, which holds `fragment` and is for a window
    /// of the member `agent`, waits at the gate with no window opened for it.
    pub fn give(&self, ask: &str, fragment: &str, agent: &str) -> Result<Given> {
        let before = self.window_ids_of(agent);
        let extra_enters = self.human.tells(ask)?;
        if extra_enters > 0 {
            self.note(format!(
                "the chief's window took the human's text only after {extra_enters} more Enter"
            ));
        }
        let brief = self.human.waits_for_gated(
            "the brief the chief puts on the board to wait at the gate",
            BRIEF_HELD,
            |message| {
                message["kind"] == "task"
                    && message["sender"] == "chief"
                    && text(&message["body"]).contains(fragment)
            },
        )?;
        let task = brief["taskNumber"]
            .as_i64()
            .ok_or_else(|| Error::Daemon(format!("the brief is of no task: {brief}")))?;
        let session = text(&brief["recipient"]);
        let window = format!("p{}-{session}", self.project.id());
        self.log.say(format!(
            "T-{task} is on the board for @{session}; its brief m-{} waits at the gate",
            brief["id"]
        ));
        self.check(
            "the brief is not held as the gate holds one",
            verdict::held(&brief, &self.trace()),
        )?;
        if !session.starts_with(agent) {
            return Err(Error::Daemon(format!(
                "T-{task} went to @{session}, not to a window of {agent}"
            )));
        }
        let state = self.state_of(task)?;
        if state != "queued" {
            return Err(Error::Daemon(format!(
                "T-{task} is {state} while its brief waits at the gate, not queued"
            )));
        }
        // The daemon looks at its queues often: a window it meant to open for
        // the brief would be open by now.
        sleep(QUIET);
        if let Some(frame) = self.opened_since(agent, &before).first() {
            return Err(Error::Daemon(format!(
                "a window ({}) was opened for T-{task} while its brief waited for the human's approval",
                text(&frame["id"])
            )));
        }
        self.check(
            "the brief was delivered while it waited",
            verdict::held(&self.message(task, verdict::id(&brief))?, &self.trace()),
        )?;
        Ok(Given {
            task,
            session,
            window,
            brief,
            before,
        })
    }

    /// The human passes the brief on: the window opens, and is given the brief
    /// that was held. The window's `pane.open`.
    pub fn open_window(&self, given: &Given) -> Result<Value> {
        self.human.approves(&given.brief)?;
        wait(
            self.rig,
            self.log,
            &format!("the daemon to open the window of @{}", given.session),
            OPENS,
            || {
                self.not_over(given.task)?;
                Ok(self.frame(&given.window).is_some())
            },
        )?;
        let frame = self.frame(&given.window).ok_or_else(|| {
            Error::Daemon(format!(
                "the window {} was opened, and is gone",
                given.window
            ))
        })?;
        self.log.say(format!(
            "the window {} is open: {}",
            given.window,
            launched(&frame)
        ));
        let id = verdict::id(&given.brief);
        wait(
            self.rig,
            self.log,
            &format!("the brief m-{id} to be delivered to the window"),
            GIVEN,
            || {
                self.not_over(given.task)?;
                self.progress(given.task, &given.session)?;
                Ok(self.message(given.task, id)?["state"] == "delivered")
            },
        )?;
        let delivered = self.message(given.task, id)?;
        let attempts = delivered["attempts"].as_i64().unwrap_or(0);
        if attempts != 1 {
            self.note(format!(
                "the brief m-{id} of T-{} took {attempts} attempts to reach its window",
                given.task
            ));
        }
        Ok(frame)
    }

    /// The task's work is done and its result waits at the gate, waited for up
    /// to `within`: held, saying every one of `needles`, and in nobody's inbox.
    /// The result.
    pub fn result_held(&self, given: &Given, needles: &[&str], within: Duration) -> Result<Value> {
        let task = given.task;
        let result = self.human.waits_for_gated_while(
            "the window's result to wait at the gate",
            within,
            |message| message["kind"] == "result" && message["taskNumber"] == task,
            || {
                self.not_over(task)?;
                self.progress(task, &given.session)
            },
        )?;
        self.log.say(format!(
            "the result m-{} of T-{task} waits at the gate: {}",
            result["id"],
            cut(&text(&result["body"]), 200)
        ));
        sleep(QUIET);
        let held = self.message(task, verdict::id(&result))?;
        self.check(
            "the result is not held at the gate",
            verdict::held(&held, &self.trace()),
        )?;
        self.check(
            "the result is not the task's",
            verdict::says(&text(&result["body"]), needles),
        )?;
        let in_inbox = self
            .project
            .inbox("chief")?
            .iter()
            .any(|message| message["id"] == result["id"]);
        if in_inbox {
            return Err(Error::Daemon(format!(
                "the result m-{} is in the chief's inbox while it waits at the gate",
                result["id"]
            )));
        }
        let state = self.state_of(task)?;
        if state != "done" {
            return Err(Error::Daemon(format!(
                "T-{task} is {state} with its result held at the gate, not done"
            )));
        }
        Ok(result)
    }

    /// What the window recorded of the task, as the page's drawer shows it.
    pub fn record(&self, task: i64) -> Result<Vec<Value>> {
        let read = self.rig.page(
            "task.transcript",
            json!({ "project": self.project.id(), "task": task }),
        )?;
        Ok(read["items"].as_array().cloned().unwrap_or_default())
    }

    /// The human has the chief at work on a command of some seconds, and passes
    /// the result on while it is: the result waits for the end of the chief's
    /// turn, and is typed into it once, then. The message as it is then.
    pub fn deliver_to_a_busy_chief(&self, task: i64, result: &Value) -> Result<Value> {
        self.human.tells(&format!(
            "Run exactly this command in your shell, then reply with one line:\n{BUSY}\n\
             Do not accept T-{task} yet: a ConsensFlow message will bring you its result, and \
             when it has, run cf task accept T-{task} and reply with one line."
        ))?;
        let state = self.human.chief_is()?;
        if state == "working" {
            self.log
                .say("the chief's window is at work: the human passes the result on now");
        } else {
            let screen = self.human.screen(&self.human.chief()?["pane"], 6);
            self.note(format!(
                "the chief's window was {state}, not at work, when the human passed the result on; \
                 its screen: {}",
                cut(&screen.join(" ⏎ "), 400)
            ));
        }
        self.deliver(task, result)
    }

    /// The human passes the result on: it is typed into the chief once, after
    /// its turn. The message as it is then.
    pub fn deliver(&self, task: i64, result: &Value) -> Result<Value> {
        self.human.approves(result)?;
        let id = verdict::id(result);
        wait(
            self.rig,
            self.log,
            &format!("the result m-{id} to be typed into the chief's window"),
            DELIVERS,
            || {
                self.progress_of_chief(task)?;
                let message = self.message(task, id)?;
                // A chief that accepts the task before its result reaches it
                // withdraws the result: the daemon says so, and there is no waiting.
                if message["state"] == "cancelled" {
                    return Err(Error::Daemon(format!(
                        "the result m-{id} was withdrawn before it reached the chief ({}): the chief \
                         accepted T-{task} without it; the task is {}",
                        text(&message["reason"]),
                        self.state_of(task)?
                    )));
                }
                Ok(message["state"] == "delivered")
            },
        )?;
        let delivered = self.message(task, id)?;
        self.check(
            "the result was not typed into the chief as the gate promises",
            verdict::typed_once(&delivered, &self.trace(), "chief"),
        )?;
        self.log.say(format!(
            "the result m-{id} was typed into the chief once, after its turn"
        ));
        Ok(delivered)
    }

    fn progress_of_chief(&self, task: i64) -> Result {
        let line = format!(
            "T-{task} is {}; the chief is {}",
            self.state_of(task)?,
            self.human.chief_is()?
        );
        let mut said = self.said.borrow_mut();
        if line != said.0 || said.1.elapsed() >= HEARTBEAT {
            self.log.say(line.clone());
            *said = (line, Instant::now());
        }
        Ok(())
    }

    /// The chief accepts the task, and the board says so. A chief that has not
    /// by the time it has been idle a while is asked by the human, once.
    pub fn accepted(&self, task: i64) -> Result {
        let started = Instant::now();
        let mut asked = false;
        wait(
            self.rig,
            self.log,
            &format!("the chief to accept T-{task}"),
            ACCEPTS,
            || {
                self.progress_of_chief(task)?;
                if self.state_of(task)? == "accepted" {
                    return Ok(true);
                }
                if !asked && started.elapsed() >= ASKED_AFTER && self.human.chief_is()? == "idle" {
                    asked = true;
                    self.note(format!(
                        "the chief did not accept T-{task} on its own in {} s: the human asked it to",
                        ASKED_AFTER.as_secs()
                    ));
                    self.human.tells(&format!(
                        "Run cf task accept T-{task} in your shell now, then reply with one line."
                    ))?;
                }
                Ok(false)
            },
        )?;
        self.log.say(format!(
            "the chief accepted T-{task}: the board says accepted"
        ));
        Ok(())
    }

    /// The window of a task that is over closes.
    pub fn closes(&self, given: &Given) -> Result {
        wait(
            self.rig,
            self.log,
            &format!("the window of @{} to close", given.session),
            CLOSES,
            || Ok(self.ended(&given.window)),
        )
    }
}

/// How a window was opened, a word at a time, the long ones cut.
pub fn launched(window: &Value) -> String {
    window["argv"]
        .as_array()
        .map(|argv| {
            argv.iter()
                .map(|word| cut(&text(word), 90))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_human_says_about_the_result_names_the_task_the_chief_is_to_accept() {
        assert_eq!(
            accepts("T-3"),
            "When its result arrives, run cf task accept with T-3, then reply with one line."
        );
    }

    #[test]
    fn a_window_is_shown_as_it_was_opened_a_word_at_a_time_and_the_long_ones_cut() {
        let long = "x".repeat(200);
        let window = json!({ "argv": ["/bin/cf", "codex-session", long, "-c", "a\nb"] });
        let shown = launched(&window);
        assert!(shown.starts_with("/bin/cf codex-session xxxx"), "{shown}");
        assert!(shown.ends_with("… -c a b"), "{shown}");
        assert_eq!(launched(&json!({})), "");
    }
}

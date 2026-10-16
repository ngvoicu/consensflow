//! The stage of a long run: the rig (the native daemon and the real pane host, as
//! the app runs them) with the project on it, the look the watch takes of it, and
//! what a disruption does to it from outside ([`Stagehand`]): a window ended, as a
//! crash ends one (by the pane host, which holds its program's handle), and the app
//! quit and started again on the same home, the daemon first (by the handle the rig
//! holds) and then the pane host with every window, as the app's quit orders it. The
//! run signals nothing it did not start, and nothing by its number. A rig that
//! closes, at a restart or at the end, is read to its last word: every exit its host
//! told before it ended, those since the last look among them. Where its output had
//! not ended in time, what was read is taken, and the close is kept as one whose last
//! word was not read whole (a break of the sixth).

use std::path::PathBuf;
use std::time::Duration;

use cf_e2e::rig::{Config, Last, Project, Rig};
use serde_json::{json, Value};

use super::disruptions::{Moment, Worker};
use super::look::{Look, Stranger, Unresolved};
use super::play::Stagehand;
use super::watcher::{lines, Watcher};
use crate::{Error, Result};

/// How long the daemon has to be gone once it is killed, and the pane host and
/// its windows once their input has ended: a window in a command takes its time.
const DAEMON_GONE: Duration = Duration::from_secs(15);
const HOST_GONE: Duration = Duration::from_secs(60);

/// How long the processes of the run are given to end once the app has closed.
const SETTLE: Duration = Duration::from_secs(15);

/// How many lines of a window's screen a disruption's evidence keeps.
const SCREEN: usize = 12;

/// What is left of the stage once the app has closed.
#[derive(Debug)]
pub struct Closed {
    /// Where the rig's files are kept: the home with its ledger, the logs.
    pub root: PathBuf,
    /// The daemon's home in it.
    pub home: PathBuf,
    /// What of the run's processes was left once the app had closed, each left running.
    pub left: Vec<Stranger>,
    /// What of them the system could not be asked of.
    pub unresolved: Vec<Unresolved>,
}

/// The rig of the run, with its project.
pub struct Stage {
    rig: Option<Rig>,
    /// What the rig was started with: the app starts again with it.
    config: Config,
    pub project: i64,
    pub watcher: Watcher,
}

impl Stage {
    /// The stage of `rig`, started with `config`, whose project is `project` and
    /// whose Claude Code keeps its records in `claude`.
    pub fn new(rig: Rig, config: Config, project: i64, claude: PathBuf) -> Self {
        Self {
            rig: Some(rig),
            config,
            project,
            watcher: Watcher::new(claude, project),
        }
    }

    /// The rig.
    pub fn rig(&self) -> Result<&Rig> {
        self.rig
            .as_ref()
            .ok_or_else(|| Error::Refused("the app is not running".to_owned()))
    }

    /// The project's board, as the page reads it.
    pub fn board(&self) -> Result<Value> {
        Ok(Project::new(self.rig()?, self.project).board()?)
    }

    /// What the page asks of the daemon: `op`, with the project in `body`; a
    /// refusal is the error.
    pub fn page(&self, op: &str, mut body: Value) -> Result<Value> {
        if let Some(fields) = body.as_object_mut() {
            fields.insert("project".to_owned(), json!(self.project));
        }
        let answer = self.rig()?.page(op, body)?;
        if answer["ok"] == false {
            return Err(Error::Refused(format!("{op}: {answer}")));
        }
        Ok(answer)
    }

    /// A look at the project, whose board is `board`.
    pub fn look_at(&mut self, board: &Value) -> Result<Look> {
        let rig = self
            .rig
            .as_ref()
            .ok_or_else(|| Error::Refused("the app is not running".to_owned()))?;
        self.watcher.look(rig, board)
    }

    /// The app closed at the end of the run, its home kept: where the ledger is, and
    /// what of the run's processes was left once it had closed (its last exits taken),
    /// each left running, and what the system could not be asked of.
    pub fn close(&mut self) -> Result<Closed> {
        let rig = self
            .rig
            .take()
            .ok_or_else(|| Error::Refused("the app is not running".to_owned()))?;
        let home = rig.home();
        let (last, unread) = last_word(rig)?;
        self.watcher
            .quit(&last, jiff::Timestamp::now().as_millisecond(), unread);
        let left = self.watcher.left_at_the_end(SETTLE);
        Ok(Closed {
            root: last.root,
            home,
            left: left.strangers,
            unresolved: left.unresolved,
        })
    }

    /// The exits of the run's windows, the oldest first.
    pub fn exits(&self) -> &[super::look::Exit] {
        self.watcher.exits()
    }
}

impl Stagehand for Stage {
    fn board(&self) -> Result<Value> {
        Stage::board(self)
    }

    fn page(&self, op: &str, body: Value) -> Result<Value> {
        Stage::page(self, op, body)
    }

    fn state_of(&self, number: i64) -> Result<String> {
        let task = Project::new(self.rig()?, self.project).task(number)?;
        let state = [&task["state"], &task["task"]["state"]]
            .into_iter()
            .find_map(Value::as_str)
            .unwrap_or_default();
        Ok(state.to_owned())
    }

    fn screen(&self, pane: &Value) -> Vec<String> {
        self.rig()
            .ok()
            .and_then(|rig| rig.snapshot(pane, SCREEN))
            .map(|answer| lines(&answer["tail"]))
            .unwrap_or_default()
    }

    fn press(&self, pane: &Value, keys: &[u8]) -> Result {
        self.rig()?.host(
            "pane.input",
            json!({ "id": pane["id"], "generation": pane["generation"], "bytes": keys }),
        )?;
        Ok(())
    }

    /// The window is ended by the pane host, which started its program and holds its
    /// handle, as the app would ask it to (`pane.kill` of its id and generation, which
    /// no other window ever has): the run signals no process it did not start, and none
    /// by its number. The daemon, which asked nothing, sees the window end as a crash
    /// ends one; the pane host ends what the window's tree left running, as it does
    /// for a program that ended by itself.
    fn kill(&mut self, handle: &str) -> Result<(String, i64, String)> {
        let board = self.board()?;
        let pane = lane_of(&board, handle)
            .map(|lane| lane["pane"].clone())
            .filter(|pane| !pane.is_null())
            .ok_or_else(|| Error::Refused(format!("@{handle} has no window to kill")))?;
        let (id, generation) = (
            pane["id"].as_str().unwrap_or_default().to_owned(),
            pane["generation"].as_i64().unwrap_or_default(),
        );
        let answer = self
            .rig()?
            .host("pane.kill", json!({ "id": id, "generation": generation }))?;
        if answer["ok"] != true {
            return Err(Error::Refused(format!(
                "@{handle}'s window {id} was not killed: the pane host said {answer}"
            )));
        }
        self.watcher.killed_by_the_run(&id, generation);
        let said = format!("@{handle}'s window {id} killed (the pane host ended it)");
        Ok((id, generation, said))
    }

    fn exited(&self, pane: &str, generation: i64) -> bool {
        self.rig().is_ok_and(|rig| {
            rig.timed_exits().iter().any(|(body, _)| {
                body["id"] == pane && body["generation"].as_i64() == Some(generation)
            })
        })
    }

    fn opened(&self, pane: &str) -> bool {
        self.rig()
            .is_ok_and(|rig| rig.window_pids().iter().any(|window| window["id"] == pane))
    }

    /// The app quit and started again on the same home: the daemon killed, the pane
    /// host ended with its windows (and read to its last word), and both started
    /// again. The daemon that starts suspends the project, which waits for the human's
    /// Resume.
    fn restart(&mut self) -> Result {
        let mut rig = self
            .rig
            .take()
            .ok_or_else(|| Error::Refused("the app is not running".to_owned()))?;
        rig.kill_daemon();
        rig.wait_for_daemon_exit(DAEMON_GONE)?;
        let (last, unread) = last_word(rig)?;
        self.watcher
            .quit(&last, jiff::Timestamp::now().as_millisecond(), unread);
        self.rig = Some(Rig::start(self.config.clone().existing_root(last.root))?);
        Ok(())
    }

    fn look(&mut self) -> Result<Look> {
        let board = self.board()?;
        self.look_at(&board)
    }
}

/// `rig` closed, read to its last word: what its host told, and why that is not all
/// of it, where its output had not ended in time.
fn last_word(rig: Rig) -> Result<(Last, Option<String>)> {
    match rig.close_with_the_last_exits_within(HOST_GONE) {
        Ok(last) => Ok((last, None)),
        Err(cf_e2e::Error::Unread { said, last }) => Ok((*last, Some(said))),
        Err(failed) => Err(failed.into()),
    }
}

/// The lane of `handle` on `board`.
pub fn lane_of<'b>(board: &'b Value, handle: &str) -> Option<&'b Value> {
    board["lanes"]
        .as_array()?
        .iter()
        .find(|lane| lane["participant"]["handle"] == handle)
}

/// A harness's command name, from the kind the ledger says it by.
pub fn name_of(kind: &str) -> &str {
    if kind == "claude-code" {
        "claude"
    } else {
        kind
    }
}

/// What a look found that a disruption's moment is told by: the chief's state, and
/// each worker's turn and task, and whether its turn has something running.
pub fn moment(board: &Value, look: &Look, elapsed: Duration) -> Moment {
    let mut moment = Moment {
        elapsed,
        ..Moment::default()
    };
    for lane in board["lanes"].as_array().into_iter().flatten() {
        let participant = &lane["participant"];
        let harness = name_of(participant["harness"].as_str().unwrap_or_default()).to_owned();
        let state = lane["activity"]["state"].as_str().unwrap_or_default();
        match participant["role"].as_str() {
            Some("chief") => {
                moment.chief = harness;
                moment.chief_idle = state == "idle";
                moment.chief_compacted = look
                    .ledger
                    .by_handle(participant["handle"].as_str().unwrap_or_default())
                    .and_then(|chief| look.record_of(chief.id))
                    .is_some_and(super::record::Record::ends_in_compaction);
            }
            Some("human") | None => {}
            Some(_) => {
                let handle = participant["handle"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                let task = lane["tasks"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|task| matches!(task["state"].as_str(), Some("working" | "waiting")))
                    .and_then(|task| task["number"].as_i64());
                let tool = look
                    .window_of(&handle)
                    .is_some_and(|window| !window.background.is_empty());
                moment.workers.push(Worker {
                    handle,
                    harness,
                    task,
                    working: state == "working",
                    tool,
                });
            }
        }
    }
    moment
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::long_run::look::{OpenWindow, Process};

    #[test]
    fn a_moment_is_the_chiefs_state_and_each_workers_turn_task_and_running_work() {
        let board = json!({ "lanes": [
            { "participant": { "handle": "chief", "role": "chief", "harness": "claude-code" },
              "activity": { "state": "idle" }, "tasks": [] },
            { "participant": { "handle": "long-devin-1", "role": "worker", "harness": "devin" },
              "activity": { "state": "working" },
              "tasks": [{ "number": 3, "state": "done" }, { "number": 7, "state": "working" }] },
            { "participant": { "handle": "long-claude-reviewer", "role": "reviewer", "harness": "claude-code" },
              "activity": { "state": "idle" }, "tasks": [] },
        ] });
        let mut look = Look::default();
        look.windows.open.push(OpenWindow {
            handle: "long-devin-1".to_owned(),
            pane: "p1-long-devin-1-1".to_owned(),
            generation: 1,
            pid: None,
            running: true,
            hosted: true,
            background: vec![Process {
                pid: 9,
                command: "npm run build".to_owned(),
            }],
            background_total: 1,
            tree_unread: None,
            tail: Vec::new(),
        });
        let moment = moment(&board, &look, Duration::from_secs(60));
        assert_eq!((moment.chief.as_str(), moment.chief_idle), ("claude", true));
        assert_eq!(
            moment.workers,
            [
                Worker {
                    handle: "long-devin-1".to_owned(),
                    harness: "devin".to_owned(),
                    task: Some(7),
                    working: true,
                    tool: true
                },
                Worker {
                    handle: "long-claude-reviewer".to_owned(),
                    harness: "claude".to_owned(),
                    task: None,
                    working: false,
                    tool: false
                }
            ]
        );
        assert_eq!(
            lane_of(&board, "long-devin-1").unwrap()["activity"]["state"],
            "working"
        );
        assert!(lane_of(&board, "nobody").is_none());
        assert!(!moment.chief_compacted, "no record of the chief's");
    }

    #[test]
    fn a_moment_says_whether_the_chiefs_record_ends_in_a_compaction() {
        use crate::long_run::ledger::{Conversation, Participant};
        use crate::long_run::record::Record;
        let board = json!({ "lanes": [
            { "participant": { "handle": "chief", "role": "chief", "harness": "claude-code" },
              "activity": { "state": "idle" }, "tasks": [] },
        ] });
        let mut look = Look::default();
        look.ledger.participants.push(Participant {
            id: 2,
            handle: "chief".to_owned(),
            role: "chief".to_owned(),
            harness: Some("claude-code".to_owned()),
            agent: None,
        });
        look.ledger.conversations.push(Conversation {
            id: 5,
            participant: 2,
            harness: "claude-code".to_owned(),
            session: Some("s".to_owned()),
            ended_at: None,
        });
        look.records.insert(
            5,
            Record::claude(r#"{"type":"system","subtype":"compact_boundary","timestamp":"2026-10-15T10:00:00.000Z"}"#),
        );
        assert!(moment(&board, &look, Duration::ZERO).chief_compacted);
        look.records.insert(5, Record::default());
        assert!(!moment(&board, &look, Duration::ZERO).chief_compacted);
    }
}

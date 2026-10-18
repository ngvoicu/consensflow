//! The look a long run takes of its project every few seconds, live: the ledger
//! from a copy of the file the daemon holds, the board's windows as the pane host
//! answers for each (its process, its screen, what its turn left running), the
//! windows that exited (at the time the host told each, with what its close ended)
//! and who closed each, the strangers, the daemon's trace of each window's state, and
//! each Claude Code window's own record, read as it grows. It steers nothing of the
//! project: it asks the pane host and the daemon what they show, as the page does.
//! It ends nothing: a process the run's windows left behind (a stranger, and what is
//! left at the end) was started by none of the run's own handles, so it is said with
//! what is known of it and left running.

mod exits;

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::tree;
use cf_e2e::process::is_alive;
use cf_e2e::rig::{Last, Rig};
use serde_json::Value;

use super::ledger::Ledger;
use super::look::{
    Activity, Exit, Look, OpenWindow, Process, Stranger, Unread, Unresolved, Windows,
};
use super::mask::Names;
use super::processes;
use super::record::Record;
use super::tracker::{Found, Leftover, Tracker};
use crate::measure::on_copy;
use crate::{clock, Result};

pub use exits::{exit_of, lines, Known};

/// How many lines of a window's screen a look keeps.
const TAIL: usize = 12;

/// A Claude Code record, read as far as its last whole line; and why the last reading
/// of it failed, where it did (a record not read is no record that says nothing).
#[derive(Debug, Default)]
struct Grown {
    file: Option<PathBuf>,
    read: u64,
    record: Record,
    unread: Option<String>,
}

impl Grown {
    /// What the record's file holds past what was read, its whole lines alone, read
    /// with the run's `names` shown where its words are masked; or why it could not be.
    fn grow(&mut self, config: &std::path::Path, session: &str, names: &Names) {
        if self.file.is_none() {
            self.file = cf_e2e::live::claude::transcript_file(config, session);
        }
        let Some(file) = &self.file else {
            self.unread = Some(format!("no record of the session {session} was found"));
            return;
        };
        let mut opened = match File::open(file) {
            Ok(opened) => opened,
            Err(cause) => {
                self.unread = Some(format!("its record could not be opened: {cause}"));
                return;
            }
        };
        let mut more = Vec::new();
        if let Err(cause) = opened
            .seek(SeekFrom::Start(self.read))
            .and_then(|_| opened.read_to_end(&mut more))
        {
            self.unread = Some(format!("its record could not be read: {cause}"));
            return;
        }
        self.unread = None;
        let Some(end) = more.iter().rposition(|byte| *byte == b'\n') else {
            return;
        };
        self.record
            .add_claude(&String::from_utf8_lossy(&more[..=end]), names);
        self.read += u64::try_from(end + 1).unwrap_or_default();
    }
}

/// What the run's looks keep from one to the next.
#[derive(Debug)]
pub struct Watcher {
    tracker: Tracker,
    records: BTreeMap<i64, Grown>,
    /// Whose each window is, by its pane's id, as the board said.
    panes: BTreeMap<String, String>,
    /// How many of the rig's exits were taken.
    exits_taken: usize,
    exits: Vec<Exit>,
    /// The windows the run killed, by pane and generation.
    killed: Vec<(String, i64)>,
    /// The closes of the app whose host's last word was not read to its end.
    unread: Vec<Unread>,
    /// The folder Claude Code keeps its records in for the run's windows.
    claude: PathBuf,
    /// What the id of every window of the project begins with (`p1-`): the rig opens
    /// windows of its own too (Claude's trust of the workspace), which are none of
    /// the project's.
    prefix: String,
}

impl Watcher {
    /// A watcher of the windows of the project `project`, whose Claude Code keeps its
    /// records in `claude`.
    pub fn new(claude: PathBuf, project: i64) -> Self {
        Self {
            tracker: Tracker::default(),
            records: BTreeMap::new(),
            panes: BTreeMap::new(),
            exits_taken: 0,
            exits: Vec::new(),
            killed: Vec::new(),
            unread: Vec::new(),
            claude,
            prefix: format!("p{project}-"),
        }
    }

    /// The app quit at `at`, and `last` is what its rig said last, read to the end of
    /// its host's output once it had closed, or as far as it was read (`unread`, why it
    /// was not read to the end): every exit since the last look is taken (with what
    /// each says its close ended, whose identity no look took), every window went, and
    /// the next rig's exits are counted from its start.
    pub fn quit(&mut self, last: &Last, at: i64, unread: Option<String>) {
        self.take_exits(&last.exits, &last.kills);
        self.tracker.all_gone(at);
        self.exits_taken = 0;
        if let Some(why) = unread {
            self.unread.push(Unread { at, why });
        }
    }

    /// Every close of the app whose host's last word was not read to its end.
    pub fn unread(&self) -> &[Unread] {
        &self.unread
    }

    /// The run had the pane host end the window `pane` of `generation` (a
    /// disruption's crash), and the host said it did: the window's exit, when it is
    /// told, is the run's. A window the run did not end, which closes later, closed by
    /// itself. The exits are taken at a look, never while a disruption plays.
    pub fn killed_by_the_run(&mut self, pane: &str, generation: i64) {
        self.killed.push((pane.to_owned(), generation));
    }

    /// Every exit taken so far, the oldest first.
    pub fn exits(&self) -> &[Exit] {
        &self.exits
    }

    /// A look at the project of `rig`, whose board is `board`.
    pub fn look(&mut self, rig: &Rig, board: &Value) -> Result<Look> {
        let at = jiff::Timestamp::now().as_millisecond();
        let ledger = on_copy(&rig.home().join("consensflow.db"), Ledger::read)?;
        // The trees first, then the exits told since the last look: a tree taken of
        // a window that then went is no word that it still runs.
        let shown_at = jiff::Timestamp::now().as_millisecond();
        let open = self.open_windows(rig, board);
        self.take_exits(&rig.timed_exits(), &rig.kills());
        self.follow(&open, shown_at, processes::identities);
        let due = self.tracker.due(at);
        // What the system cannot be asked of now is asked at the next look.
        let found = if due.is_empty() {
            Found::default()
        } else {
            processes::identities(&due)
                .map(|there| self.tracker.strangers(at, &there))
                .unwrap_or_default()
        };
        let names = Names::of_ledger(&ledger);
        for conversation in ledger
            .conversations
            .iter()
            .filter(|conversation| conversation.harness == "claude-code")
        {
            if let Some(session) = &conversation.session {
                self.records.entry(conversation.id).or_default().grow(
                    &self.claude,
                    session,
                    &names,
                );
            }
        }
        Ok(Look {
            at,
            records: self
                .records
                .iter()
                .filter(|(_, grown)| grown.unread.is_none())
                .map(|(id, grown)| (*id, grown.record.clone()))
                .collect(),
            records_unread: self
                .records
                .iter()
                .filter_map(|(id, grown)| Some((*id, grown.unread.clone()?)))
                .collect(),
            ledger,
            windows: Windows {
                open,
                exits: self.exits.clone(),
                strangers: left_running(found.leftovers),
                unresolved: found.unresolved,
                unread: self.unread.clone(),
                activity: activity(rig),
            },
        })
    }

    /// The processes the `open` windows show, their trees taken at `shown_at`, each
    /// followed as its window's (a window whose exit was told shows nothing); and who
    /// the new ones are, asked of the system once (`ask`) while their windows show
    /// them. One whose identity this asking does not take is never known.
    fn follow(
        &mut self,
        open: &[OpenWindow],
        shown_at: i64,
        ask: impl Fn(&[u32]) -> std::result::Result<BTreeMap<u32, String>, String>,
    ) {
        let mut asked = Vec::new();
        for window in open {
            let owner = (window.pane.clone(), window.generation);
            if self
                .exits
                .iter()
                .any(|exit| exit.pane == owner.0 && exit.generation == owner.1)
            {
                continue;
            }
            let mut seen = window.background.clone();
            if let Some(pid) = window.pid {
                seen.push(Process {
                    pid,
                    command: format!("the program of @{}'s window", window.handle),
                });
            }
            asked.extend(self.tracker.saw(&owner, &seen));
        }
        if asked.is_empty() {
            return;
        }
        let answer = ask(&asked);
        self.tracker
            .identify(shown_at, answer.as_ref().map_err(String::as_str));
    }

    /// What is left of the run's processes once the app has closed: every process
    /// followed that is still there, as it was, after `settle` (the time the run gives
    /// them to end), each said and left running; and what the system could not be
    /// asked of, unresolved. The app's close ended what was a window's own.
    pub fn left_at_the_end(&mut self, settle: Duration) -> Left {
        let at = jiff::Timestamp::now().as_millisecond();
        self.tracker.all_gone(at);
        let begun = Instant::now();
        let mut there = processes::identities(&self.tracker.followed());
        while there.as_ref().map_or(true, |there| !there.is_empty()) && begun.elapsed() < settle {
            sleep(Duration::from_secs(1));
            there = processes::identities(&self.tracker.followed());
        }
        match there {
            Ok(there) => {
                let found = self.tracker.left(at, &there);
                Left {
                    strangers: left_running(found.leftovers),
                    unresolved: found.unresolved,
                }
            }
            Err(why) => Left {
                strangers: Vec::new(),
                unresolved: self.tracker.unresolved_all(at, &why),
            },
        }
    }

    /// The windows the board shows open, each as the pane host answers for it.
    fn open_windows(&mut self, rig: &Rig, board: &Value) -> Vec<OpenWindow> {
        let pids = rig.window_pids();
        let mut open = Vec::new();
        for lane in board["lanes"].as_array().into_iter().flatten() {
            let pane = &lane["pane"];
            let Some(id) = pane["id"].as_str() else {
                continue;
            };
            let handle = lane["participant"]["handle"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            self.panes.insert(id.to_owned(), handle.clone());
            let generation = pane["generation"].as_i64().unwrap_or_default();
            let pid = pids
                .iter()
                .rev()
                .find(|opened| {
                    opened["id"] == id && opened["generation"].as_i64() == Some(generation)
                })
                .and_then(|opened| opened["pid"].as_u64())
                .and_then(|pid| u32::try_from(pid).ok());
            let snapshot = rig.snapshot(pane, TAIL);
            let (background, background_total, tree_unread) = match tree::read(rig, pane) {
                Ok(tree) => {
                    let named: Vec<Process> = tree
                        .processes
                        .into_iter()
                        .filter_map(|process| {
                            Some(Process {
                                pid: u32::try_from(process.pid).ok()?,
                                command: process.command,
                            })
                        })
                        .collect();
                    (named, tree.total, None)
                }
                // Not known, which is no tree that runs nothing.
                Err(why) => (Vec::new(), 0, Some(why)),
            };
            open.push(OpenWindow {
                handle,
                pane: id.to_owned(),
                generation,
                pid,
                running: pid.is_none_or(is_alive),
                hosted: snapshot.is_some(),
                background,
                background_total,
                tree_unread,
                tail: snapshot
                    .map(|answer| exits::lines(&answer["tail"]))
                    .unwrap_or_default(),
            });
        }
        open
    }

    /// The exits of the project's windows the rig told since the last look (`exits`,
    /// each at the time the host told it), with whose window it was and who closed it
    /// (`kills`, the daemon's closes); each window's processes are gone from then, with
    /// what its close ended.
    fn take_exits(&mut self, exits: &[(Value, i64)], kills: &[Value]) {
        let known = Known {
            prefix: &self.prefix,
            panes: &self.panes,
            killed: &self.killed,
            kills,
        };
        let taken: Vec<Exit> = exits
            .iter()
            .skip(self.exits_taken)
            .filter_map(|(body, at)| exit_of(body, *at, &known))
            .collect();
        self.exits_taken = exits.len();
        for exit in taken {
            let owner = (exit.pane.clone(), exit.generation);
            self.tracker.gone(&owner, exit.at, &exit.ended);
            self.exits.push(exit);
        }
    }
}

/// What the run's processes came to once the app had closed.
#[derive(Debug, Default)]
pub struct Left {
    pub strangers: Vec<Stranger>,
    pub unresolved: Vec<Unresolved>,
}

/// Each of `leftovers` as the strangers it is: said, and left running.
fn left_running(leftovers: Vec<Leftover>) -> Vec<Stranger> {
    leftovers
        .into_iter()
        .map(|leftover| Stranger {
            process: leftover.process,
            pane: leftover.pane,
            since: leftover.since,
            began: leftover.began,
        })
        .collect()
}

/// Every change of a window's state the daemon's trace says, oldest first.
fn activity(rig: &Rig) -> Vec<Activity> {
    rig.events()
        .iter()
        .filter(|event| event["kind"] == "window.activity")
        .filter_map(|event| {
            Some(Activity {
                at: clock::millis(event["at"].as_str()?)?,
                handle: event["participant"].as_str()?.to_owned(),
                state: event["state"].as_str()?.to_owned(),
                reason: event["reason"].as_str().map(str::to_owned),
            })
        })
        .collect()
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;

    use cf_e2e::process::{Run, Spawned};
    use serde_json::json;

    use crate::long_run::look::ClosedBy;

    fn sleeping() -> Spawned {
        Run::new("sleep")
            .arg("600")
            .inheriting_env()
            .spawn()
            .unwrap()
    }

    /// The open window `p1-long-worker-1` of generation 1, showing the process `pid`
    /// as its program.
    fn showing(pid: u32) -> OpenWindow {
        OpenWindow {
            handle: "long-worker".to_owned(),
            pane: "p1-long-worker-1".to_owned(),
            generation: 1,
            pid: Some(pid),
            running: true,
            hosted: true,
            background: Vec::new(),
            background_total: 0,
            tree_unread: None,
            tail: Vec::new(),
        }
    }

    /// An asking that fails, as a PowerShell that ran out of its time does.
    fn failing(_: &[u32]) -> std::result::Result<BTreeMap<u32, String>, String> {
        Err("the asking ran out of its time".to_owned())
    }

    /// What a closed rig said last, with no exit.
    fn closed() -> Last {
        Last {
            root: PathBuf::from("/nowhere"),
            exits: Vec::new(),
            kills: Vec::new(),
        }
    }

    #[test]
    fn a_stranger_found_is_said_with_what_is_known_of_it_and_left_running() {
        // The window's process, known by the look that saw it, still runs once the app
        // has closed: the run did not start it, and ends nothing it did not start.
        let mut window = sleeping();
        let pid = window.id();
        let mut watcher = Watcher::new(PathBuf::from("/nowhere"), 1);
        let now = jiff::Timestamp::now().as_millisecond();
        watcher.follow(&[showing(pid)], now + 1_000, processes::identities);
        watcher.quit(&closed(), now, None);
        let left = watcher.left_at_the_end(Duration::ZERO);
        assert_eq!(left.strangers.len(), 1, "{left:?}");
        let stranger = &left.strangers[0];
        assert_eq!(
            (stranger.process.pid, stranger.pane.as_str()),
            (pid, "p1-long-worker-1")
        );
        assert!(!stranger.began.is_empty());
        assert!(
            window.wait(Duration::from_millis(300)).unwrap().is_none(),
            "left running"
        );
        // The test started it, and ends it through the handle it holds.
        window.kill();
    }

    #[test]
    fn a_window_the_run_had_the_pane_host_end_is_the_runs_and_no_other_is() {
        let mut watcher = Watcher::new(PathBuf::from("/nowhere"), 1);
        watcher.killed_by_the_run("p1-long-worker-1", 2);
        let exit = |id: &str, generation: i64| {
            (
                json!({ "id": id, "generation": generation, "signal": "SIGHUP" }),
                1_000,
            )
        };
        watcher.take_exits(
            &[
                exit("p1-long-worker-1", 2),
                exit("p1-long-worker-1", 1),
                exit("p1-long-devin-1", 2),
            ],
            &[],
        );
        let closers: Vec<ClosedBy> = watcher.exits().iter().map(|exit| exit.closed_by).collect();
        assert_eq!(closers, [ClosedBy::Run, ClosedBy::Itself, ClosedBy::Itself]);
    }

    #[test]
    fn an_identity_is_taken_only_by_the_look_that_saw_its_window_show_the_process() {
        // The look that saw the window's process could not ask who it was; the next
        // look, the window showing it still, asks nothing of it: its number may be
        // another's by then. Never known, it is unresolved once its window is gone.
        let mut window = sleeping();
        let pid = window.id();
        let mut watcher = Watcher::new(PathBuf::from("/nowhere"), 1);
        watcher.follow(&[showing(pid)], 1_000, failing);
        let now = jiff::Timestamp::now().as_millisecond();
        watcher.follow(&[showing(pid)], now, processes::identities);
        watcher.quit(&closed(), now, None);
        let left = watcher.left_at_the_end(Duration::ZERO);
        assert!(left.strangers.is_empty(), "{left:?}");
        assert_eq!(left.unresolved.len(), 1, "{left:?}");
        assert!(
            left.unresolved[0].why.contains("could not say who it was"),
            "{}",
            left.unresolved[0].why
        );
        assert!(
            window.wait(Duration::from_millis(300)).unwrap().is_none(),
            "left running"
        );
        window.kill();
    }

    #[test]
    fn what_only_an_exit_names_is_never_known_and_left_running() {
        // The worker's window was closed with the app, and its last `pane.exit` names
        // a server its turn began after the last look: the exit says its number and
        // command, nothing that ties the number to a process. It is followed, said
        // unresolved where its number is there at the end, and left running.
        let mut server = sleeping();
        let last = Last {
            root: PathBuf::from("/nowhere"),
            exits: vec![(
                json!({ "id": "p1-long-worker-1", "generation": 2, "signal": "SIGHUP",
                        "background": [{ "pid": server.id(), "command": "node serve.mjs" }] }),
                1_000,
            )],
            kills: Vec::new(),
        };
        let mut watcher = Watcher::new(PathBuf::from("/nowhere"), 1);
        watcher.quit(
            &last,
            2_000,
            Some("the host's output had not ended".to_owned()),
        );
        assert_eq!(watcher.exits().len(), 1);
        assert_eq!(
            watcher.unread(),
            [Unread {
                at: 2_000,
                why: "the host's output had not ended".to_owned()
            }]
        );
        let left = watcher.left_at_the_end(Duration::ZERO);
        assert!(left.strangers.is_empty(), "{left:?}");
        assert_eq!(left.unresolved.len(), 1, "{left:?}");
        assert_eq!(left.unresolved[0].process.command, "node serve.mjs");
        assert!(left.unresolved[0]
            .why
            .contains("first known from its window's exit"));
        assert!(
            server.wait(Duration::from_millis(300)).unwrap().is_none(),
            "left running"
        );
        server.kill();
    }
}

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
    Activity, Exit, Lane, Look, OpenWindow, Process, Reading, Stranger, Unread, Unresolved, Windows,
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

/// A Claude Code record, read as far as its last whole line; when the last reading that
/// held was taken; and why the latest failed, where it did (a record not read is no
/// record that says nothing). What was read is kept whatever comes of a later reading.
#[derive(Debug, Default)]
struct Grown {
    file: Option<PathBuf>,
    read: u64,
    record: Record,
    through: Option<i64>,
    unread: Option<String>,
}

impl Grown {
    /// What the record's file holds past what was read, its whole lines alone, read
    /// with the run's `names` shown where its words are masked; or why it could not be.
    /// A reading that holds and ends on a whole line is taken as of when it began: all the
    /// record said by then. One that ends in the middle of a line moves it no further.
    fn grow(&mut self, config: &std::path::Path, session: &str, names: &Names) {
        let began = jiff::Timestamp::now().as_millisecond();
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
        // A line still being written when the reading ended is no part of what it read:
        // the record is read through when the reading began only where it read whole lines.
        let whole = more
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |end| end + 1);
        if whole == more.len() {
            self.through = Some(began);
        }
        if whole == 0 {
            return;
        }
        self.record
            .add_claude(&String::from_utf8_lossy(&more[..whole]), names);
        self.read += u64::try_from(whole).unwrap_or_default();
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
    /// How long after the first result of a Claude Code worker every reading of its
    /// record is taken as failed, in milliseconds (the dry run's `--drop-reading-sec`).
    drop: Option<i64>,
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
            drop: None,
        }
    }

    /// From now, every reading of the record of the first Claude Code result's worker,
    /// for `span` after that result, is taken as failed: a gap the readings after it
    /// fill, as a disk that refused them would leave one (the dry run's).
    pub fn drop_readings(&mut self, span: Duration) {
        self.drop = Some(i64::try_from(span.as_millis()).unwrap_or(i64::MAX));
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
        self.read_records(&ledger, at);
        let (records, readings) = kept(&self.records);
        Ok(Look {
            at,
            records,
            readings,
            host: Some(rig.host_pid()),
            ledger,
            windows: Windows {
                open,
                lanes: lanes(board),
                exits: self.exits.clone(),
                strangers: left_running(found.leftovers),
                unresolved: found.unresolved,
                unread: self.unread.clone(),
                activity: activity(rig),
            },
        })
    }

    /// Each Claude Code conversation of `ledger` read on, at `at`, from where its record was
    /// last read: what its file holds past that.
    fn read_records(&mut self, ledger: &Ledger, at: i64) {
        let names = Names::of_ledger(ledger);
        let dropped = self
            .drop
            .map(|span| dropped(ledger, at, span))
            .unwrap_or_default();
        for conversation in ledger
            .conversations
            .iter()
            .filter(|conversation| conversation.harness == "claude-code")
        {
            if let Some(session) = &conversation.session {
                let grown = self.records.entry(conversation.id).or_default();
                if dropped.contains(&conversation.id) {
                    grown.unread = Some(DROPPED.to_owned());
                } else {
                    grown.grow(&self.claude, session, &names);
                }
            }
        }
    }

    /// The records once the app has closed, its ledger read as `ledger`: each read once
    /// more, as the ledger is, so that a row the daemon copied after the last look is held
    /// to a record read past it (invariant 7's summary is exempted only by a record read
    /// through its copy); and how far each was read, or why its reading failed.
    pub fn records_after_close(
        &mut self,
        ledger: &Ledger,
    ) -> (BTreeMap<i64, Record>, BTreeMap<i64, Reading>) {
        self.read_records(ledger, jiff::Timestamp::now().as_millisecond());
        kept(&self.records)
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

/// Why a reading the dry run drops failed.
const DROPPED: &str = "its reading was dropped (--drop-reading-sec)";

/// The conversations of `ledger` whose readings a look at `at` drops: those of the
/// worker of the first result a Claude Code worker gave, for `span` after it.
fn dropped(ledger: &Ledger, at: i64, span: i64) -> Vec<i64> {
    let first = ledger
        .messages
        .iter()
        .filter(|message| message.kind == "result")
        .find_map(|result| {
            let worker = ledger.participant(result.sender?)?;
            (worker.harness.as_deref() == Some("claude-code"))
                .then_some((worker.id, clock::millis(&result.created_at)?))
        });
    match first {
        Some((worker, given)) if given <= at && at <= given + span => ledger
            .conversations_of(worker)
            .iter()
            .map(|conversation| conversation.id)
            .collect(),
        _ => Vec::new(),
    }
}

/// What a look holds of the records `grown`, by their conversation's row: each as far as
/// it was ever read, and how far that was, with why its latest reading failed.
fn kept(grown: &BTreeMap<i64, Grown>) -> (BTreeMap<i64, Record>, BTreeMap<i64, Reading>) {
    let records = grown
        .iter()
        .filter(|(_, grown)| grown.through.is_some())
        .map(|(id, grown)| (*id, grown.record.clone()))
        .collect();
    let readings = grown
        .iter()
        .map(|(id, grown)| {
            (
                *id,
                Reading {
                    through: grown.through,
                    failed: grown.unread.clone(),
                },
            )
        })
        .collect();
    (records, readings)
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

/// Every lane of `board` that reads a state: whose, and the state.
fn lanes(board: &Value) -> Vec<Lane> {
    board["lanes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|lane| {
            Some(Lane {
                handle: lane["participant"]["handle"].as_str()?.to_owned(),
                state: lane["activity"]["state"].as_str()?.to_owned(),
            })
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
mod reading_tests {
    use super::*;

    use std::path::Path;

    #[test]
    fn the_dry_runs_dropped_readings_are_the_first_claude_results_workers_for_its_span() {
        use crate::long_run::ledger::{Conversation, Message, Participant};
        let at = |second: u32| format!("2026-10-15T10:00:{second:02}.000Z");
        let who = |id: i64, harness: &str| Participant {
            id,
            handle: format!("w-{id}"),
            role: "worker".to_owned(),
            harness: Some(harness.to_owned()),
            agent: None,
        };
        let result = |id: i64, sender: i64, second: u32| Message {
            id,
            recipient: 2,
            sender: Some(sender),
            kind: "result".to_owned(),
            task: None,
            body: String::new(),
            state: "delivered".to_owned(),
            created_at: at(second),
        };
        let ledger = Ledger {
            participants: vec![who(3, "devin"), who(4, "claude-code")],
            conversations: vec![Conversation {
                id: 7,
                participant: 4,
                harness: "claude-code".to_owned(),
                session: Some("s".to_owned()),
                started_at: None,
                ended_at: None,
            }],
            messages: vec![result(10, 3, 5), result(11, 4, 10), result(12, 4, 40)],
            ..Ledger::default()
        };
        let looked = |second: u32| dropped(&ledger, clock::millis(&at(second)).unwrap(), 12_000);
        assert_eq!(looked(15), [7]);
        assert!(looked(9).is_empty(), "before the first Claude Code result");
        assert!(looked(23).is_empty(), "after its span");
    }

    #[test]
    fn a_reading_that_ends_in_the_middle_of_a_line_reads_the_record_no_further() {
        // Sol's: the record was read while the line of a job's start was being written.
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let mut grown = Grown {
            file: Some(fixtures.join("record-cut-mid-line.jsonl")),
            ..Grown::default()
        };
        grown.grow(Path::new("/nowhere"), "s-worker", &Names::default());
        assert_eq!(
            grown.through, None,
            "a reading cut mid-line holds nothing past it"
        );
        assert_eq!(
            grown.record.last_said(),
            crate::clock::millis("2026-10-15T10:01:00.000Z")
        );
        // Its readings failing from then on, it is read no further.
        grown.file = Some(PathBuf::from("/nowhere/s-worker.jsonl"));
        grown.grow(Path::new("/nowhere"), "s-worker", &Names::default());
        assert_eq!(grown.through, None);
        // The line whole at the next reading: read through when that reading began.
        grown.file = Some(fixtures.join("record-cut-mid-line-whole.jsonl"));
        let before = jiff::Timestamp::now().as_millisecond();
        grown.grow(Path::new("/nowhere"), "s-worker", &Names::default());
        assert!(grown.through.is_some_and(|read| read >= before));
        assert_eq!(
            grown.record.last_said(),
            crate::clock::millis("2026-10-15T10:01:20.000Z")
        );
    }

    #[test]
    fn a_record_read_once_is_kept_as_far_as_it_was_read_whatever_a_later_reading_says() {
        // A Claude Code record of the product's fixtures.
        let file = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../cf-harness/tests/fixtures/claude/quit-before-a-word.jsonl");
        let said = Record::claude(&std::fs::read_to_string(&file).unwrap()).last_said();
        assert!(said.is_some());
        let mut grown = Grown {
            file: Some(file),
            ..Grown::default()
        };
        let before = jiff::Timestamp::now().as_millisecond();
        grown.grow(Path::new("/nowhere"), "s-worker", &Names::default());
        let read = grown.through.unwrap();
        assert!(read >= before, "{read} {before}");
        // Its file gone (Astra's job B, read before the result): the reading fails, and
        // what was read stays, read as far as it was, not as far as the failed reading.
        sleep(Duration::from_millis(5));
        grown.file = Some(PathBuf::from("/nowhere/s-worker.jsonl"));
        grown.grow(Path::new("/nowhere"), "s-worker", &Names::default());
        let failed = grown.unread.clone().unwrap();
        assert!(
            failed.starts_with("its record could not be opened: "),
            "{failed}"
        );
        let never = Grown {
            unread: Some("no record of the session s-other was found".to_owned()),
            ..Grown::default()
        };
        let (records, readings) = kept(&BTreeMap::from([(7, grown), (8, never)]));
        assert_eq!(records.keys().collect::<Vec<_>>(), [&7]);
        assert_eq!(records[&7].last_said(), said);
        assert_eq!(
            readings[&7],
            Reading {
                through: Some(read),
                failed: Some(failed),
            }
        );
        // A record never read is none, and its reading says why.
        assert_eq!(
            readings[&8],
            Reading {
                through: None,
                failed: Some("no record of the session s-other was found".to_owned()),
            }
        );
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;

    use cf_e2e::process::{Run, Spawned};
    use serde_json::json;

    use crate::long_run::look::ClosedBy;

    #[test]
    fn the_records_are_read_once_more_after_the_close_past_what_the_last_look_read() {
        // Calliope's review of round b5: the last look read the chief's record through
        // Claude Code's flagged summary under a uuid; a retry came under the same uuid after
        // it, and the daemon copied it before the close. Read once more after the close, the
        // record holds both lines, so the summary's uuid is no summary's, and it was read
        // through the close.
        use crate::long_run::ledger::Conversation;
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let ledger = Ledger {
            conversations: vec![Conversation {
                id: 7,
                participant: 2,
                harness: "claude-code".to_owned(),
                session: Some("s-chief".to_owned()),
                started_at: None,
                ended_at: None,
            }],
            ..Ledger::default()
        };
        let mut watcher = Watcher::new(PathBuf::from("/nowhere"), 1);
        watcher.records.insert(
            7,
            Grown {
                file: Some(fixtures.join("record-summary-read.jsonl")),
                ..Grown::default()
            },
        );
        watcher.read_records(&ledger, 0);
        let (records, _) = kept(&watcher.records);
        assert!(records[&7].is_summary("u-b"), "the last look's reading");
        // The file as the close left it: the same line, and the retry after it.
        watcher.records.get_mut(&7).unwrap().file =
            Some(fixtures.join("record-summary-retried.jsonl"));
        let closed = jiff::Timestamp::now().as_millisecond();
        let (records, readings) = watcher.records_after_close(&ledger);
        assert!(!records[&7].is_summary("u-b"), "read past the retry");
        assert!(
            readings[&7]
                .through
                .is_some_and(|through| through >= closed),
            "{:?}",
            readings[&7]
        );
    }

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

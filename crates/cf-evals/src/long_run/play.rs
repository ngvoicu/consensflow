//! A disruption played on the stage, as a person or a crash would make it: the
//! owner pauses a task and resumes it, a window's process dies, the owner types
//! `/compact` into the chief, quits the app and starts it again, switches the chief,
//! closes the project and resumes it. Each ends where the owner would leave it: a task
//! the disruption paused is resumed, a project it suspended is resumed (the tasks its
//! Close paused go on with it by themselves).
//!
//! A disruption is played only when its effect is seen: the paused task paused and on
//! again; the window killed and its task paused, then on again; a compaction boundary
//! in the chief's own record that is new since the keys (one more than before, written
//! after them); the project open again with its chief's window, and for a Devin worker
//! cut mid-tool, that worker's window opened again; the new chief's window open on
//! the agent it was switched to; the tasks a Close paused out of `paused` by themselves
//! once the owner resumed the project, each window back on its own conversation and
//! told once that its turn was cut. An effect not seen within the time a disruption waits is a
//! failure, said with its evidence (the rows of the board, the window's screen, the
//! record); a refusal of what the owner asked is a failure too, unless the task had
//! ended by then, or another had resumed it first. A moment that passed before it could be played (the task ended
//! between the look and the pause, the window went) is missed, and played at the next.
//!
//! The watch goes on while a disruption waits: every wait looks at the project as
//! the run's own loop does ([`Watch`]), so that what breaks in a disruption is found
//! when it breaks. What a disruption does and reads of the stage is a [`Stagehand`]'s:
//! the rig's in a run, a script's in a test.

use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::disruptions::{Disruption, Target};
use super::invariants::{Evidence, CUT};
use super::look::Look;
use super::record::Kind;
use super::stage::lane_of;
use crate::{Error, Result};

/// The keys of `/compact`, then Enter.
const COMPACT: &[u8] = b"/compact";
const ENTER: u8 = 13;

/// The states a task is done in, which a resume of it is refused for.
const ENDED: [&str; 4] = ["done", "accepted", "cancelled", "failed"];

/// Who the ledger says moved a task when the owner did it on the page.
const OWNER: &str = "human";

/// How far the daemon's clock, which times the ledger's events, may read behind the
/// run's, in milliseconds: one machine's, read a moment apart. The owner's pause of an
/// earlier disruption came minutes before.
const CLOCKS: i64 = 1_000;

/// What a disruption does to the stage and reads of it.
pub trait Stagehand {
    /// The project's board, as the page reads it.
    fn board(&self) -> Result<Value>;
    /// What the page asks of the daemon: `op`, with the project in `body`; a refusal
    /// is the error.
    fn page(&self, op: &str, body: Value) -> Result<Value>;
    /// T-`number`'s state now.
    fn state_of(&self, number: i64) -> Result<String>;
    /// The last lines of the window `pane`'s screen.
    fn screen(&self, pane: &Value) -> Vec<String>;
    /// Types `keys` into the window `pane`.
    fn press(&self, pane: &Value, keys: &[u8]) -> Result;
    /// Kills `handle`'s window's process, as a crash does: the window's id, its
    /// generation, and what was killed, in words. A window that cannot be killed (it
    /// went, its process is not the one the run knew) is refused.
    fn kill(&mut self, handle: &str) -> Result<(String, i64, String)>;
    /// Whether the window `pane` of `generation` has exited.
    fn exited(&self, pane: &str, generation: i64) -> bool;
    /// Whether the app, since it last started, opened a window with the id `pane`.
    fn opened(&self, pane: &str) -> bool;
    /// The command line of each window with the id `pane` the app opened since it last
    /// started, the oldest first.
    fn opened_with(&self, pane: &str) -> Vec<Vec<String>>;
    /// The app quits and starts again on the same home.
    fn restart(&mut self) -> Result;
    /// A look at the project.
    fn look(&mut self) -> Result<Look>;
}

/// The run's watch, which goes on while a disruption waits.
pub trait Watch {
    /// A look at the project, if one is due: held to the invariants and told in the
    /// timeline, as the run's own loop takes them.
    fn tick(&mut self, stage: &mut dyn Stagehand);
    /// The last look taken.
    fn last(&self) -> Option<&Look>;
}

/// What a disruption is played with.
#[derive(Debug, Clone)]
pub struct With {
    /// How long a paused task or a suspended project is left so.
    pub hold: Duration,
    /// The agent the chief is switched to.
    pub switch_to: Option<String>,
    /// How long a disruption waits for its effect, and how often it looks.
    pub within: Duration,
    pub every: Duration,
}

/// What came of a disruption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Played, its effect seen: what it did.
    Played(String),
    /// Played, and its effect not seen: what it did, and what shows it.
    Failed { said: String, evidence: Evidence },
    /// Its moment passed before it could be played: why. It waits for the next.
    Missed(String),
}

/// The stage and the watch, which a disruption's waits look through.
struct Hand<'a> {
    stage: &'a mut dyn Stagehand,
    watch: &'a mut dyn Watch,
    within: Duration,
    every: Duration,
}

impl Hand<'_> {
    /// Waits for `done`, the watch looking as it waits: whether it came.
    fn wait(&mut self, done: impl FnMut(&Hand<'_>) -> Result<bool>) -> Result<bool> {
        let (within, every) = (self.within, self.every);
        wait_ticking(
            self,
            within,
            every,
            |hand| hand.watch.tick(hand.stage),
            done,
        )
    }

    /// Leaves things as they are for `hold`, the watch looking.
    fn hold(&mut self, hold: Duration) -> Result {
        let every = self.every;
        wait_ticking(
            self,
            hold,
            every,
            |hand| hand.watch.tick(hand.stage),
            |_| Ok(false),
        )
        .map(|_| ())
    }

    /// What shows a disruption's effect not seen: T-`task`'s row, `handle`'s lane and
    /// screen, and `said` of it.
    fn evidence(&self, task: Option<i64>, handle: &str, said: &[String]) -> Evidence {
        let mut rows: Vec<String> = said.to_vec();
        let board = self.stage.board().unwrap_or(Value::Null);
        if let Some(number) = task {
            rows.push(format!(
                "T-{number} {}",
                self.stage
                    .state_of(number)
                    .unwrap_or_else(|failed| failed.to_string())
            ));
        }
        let lane = lane_of(&board, handle);
        if let Some(lane) = lane {
            rows.push(format!(
                "@{handle}: {} {}, window {}",
                lane["participant"]["agent"], lane["activity"]["state"], lane["pane"]
            ));
        }
        rows.push(format!("project {}", board["project"]["state"]));
        let screen = lane
            .map(|lane| lane["pane"].clone())
            .filter(|pane| !pane.is_null())
            .map(|pane| self.stage.screen(&pane))
            .unwrap_or_default();
        Evidence {
            rows,
            screen,
            record: Vec::new(),
        }
    }

    /// A failure of `said`, shown by T-`task` and `handle`'s window.
    fn failed(&self, said: String, task: Option<i64>, handle: &str) -> Outcome {
        let evidence = self.evidence(task, handle, std::slice::from_ref(&said));
        Outcome::Failed { said, evidence }
    }

    /// Whether the ledger, as the watch last read it, moved T-`number` to paused by `by`
    /// at `since` or after ([`paused_by`]).
    fn paused_by(&self, number: i64, by: Option<&str>, since: i64) -> bool {
        self.watch
            .last()
            .is_some_and(|look| paused_by(look, number, by, since))
    }
}

/// Plays `disruption` at `target` on `stage`, `watch` looking as it waits.
pub fn play(
    stage: &mut dyn Stagehand,
    watch: &mut dyn Watch,
    disruption: Disruption,
    target: &Target,
    with: &With,
) -> Result<Outcome> {
    let mut hand = Hand {
        stage,
        watch,
        within: with.within,
        every: with.every,
    };
    match (disruption, target) {
        (Disruption::Pause, Target::Task(number)) => pause(&mut hand, *number, with.hold),
        (Disruption::Kill, Target::Window { handle, task }) => {
            kill(&mut hand, handle, *task, with.hold)
        }
        (Disruption::Compact, _) => compact(&mut hand),
        (Disruption::Restart, _) => restart(&mut hand, None),
        (Disruption::DevinCut, Target::Window { handle, task }) => {
            restart(&mut hand, Some((handle, *task)))
        }
        (Disruption::Switch, _) => switch(&mut hand, with.switch_to.as_deref()),
        (Disruption::Suspend, _) => suspend(&mut hand, with.hold),
        (disruption, target) => Ok(Outcome::Missed(format!(
            "{} has nothing to do at {target:?}",
            disruption.flag()
        ))),
    }
}

/// Waits up to `within` for `done`, asking it again every `every` and calling `tick`
/// between: whether it came. It asks once more at the end, so that what came as the
/// time ran out is seen.
pub(crate) fn wait_ticking<S: ?Sized>(
    state: &mut S,
    within: Duration,
    every: Duration,
    mut tick: impl FnMut(&mut S),
    mut done: impl FnMut(&S) -> Result<bool>,
) -> Result<bool> {
    let begun = Instant::now();
    loop {
        if done(state)? {
            return Ok(true);
        }
        let elapsed = begun.elapsed();
        if elapsed >= within {
            return Ok(false);
        }
        tick(state);
        sleep(every.min(within.saturating_sub(elapsed)));
    }
}

/// What came of an ask of the owner's: nothing where ConsensFlow took it, its
/// refusal in words where it refused it. A page that could not be asked at all is the
/// run's failure, and is the error.
fn refusal(asked: Result<Value>) -> Result<Option<String>> {
    match asked {
        Ok(_) => Ok(None),
        Err(Error::Refused(why)) => Ok(Some(why)),
        Err(failure) => Err(failure),
    }
}

/// The owner pauses T-`number`, leaves it `hold`, and resumes it: played once its pause
/// moved it to paused and then it went on. The owner's pause is the ledger's, as the watch
/// last read it: the task's move to paused by the human, at or after the ask. The board's
/// word that it is paused does not say whose pause it is, nor does a move by another (the
/// chief may pause it and resume it on its own): neither proves the owner's took.
fn pause(hand: &mut Hand<'_>, number: i64, hold: Duration) -> Result<Outcome> {
    let asked_at = jiff::Timestamp::now().as_millisecond();
    let asked = hand.stage.page("task.pause", json!({ "task": number }));
    if let Some(why) = refusal(asked)? {
        let state = hand.stage.state_of(number)?;
        if state != "working" && state != "waiting" {
            return Ok(Outcome::Missed(format!(
                "T-{number} went {state} before its pause"
            )));
        }
        return Ok(hand.failed(
            format!("the owner's pause of T-{number}, at work, was refused: {why}"),
            Some(number),
            "",
        ));
    }
    if !hand.wait(|hand| Ok(hand.paused_by(number, Some(OWNER), asked_at)))? {
        return Ok(hand.failed(
            format!("T-{number} never moved to paused after the owner's pause"),
            Some(number),
            "",
        ));
    }
    hand.hold(hold)?;
    resume_task(
        hand,
        number,
        &format!(
            "the owner paused T-{number}, and resumed it {} s later",
            hold.as_secs()
        ),
    )
}

/// The owner resumes T-`number`, which a disruption paused: played once it went on,
/// `said` being what was done before. A resume refused because another resumed it first
/// (the chief, the daemon) is played too, the refusal said: it went on.
fn resume_task(hand: &mut Hand<'_>, number: i64, said: &str) -> Result<Outcome> {
    if let Some(why) = refusal(hand.stage.page("task.resume", json!({ "task": number })))? {
        let state = hand.stage.state_of(number)?;
        if ENDED.contains(&state.as_str()) {
            return Ok(Outcome::Played(format!(
                "{said}; T-{number} went {state} in the hold"
            )));
        }
        if state != "paused" {
            let by = hand
                .watch
                .last()
                .and_then(|look| went_on(look, number))
                .map_or_else(
                    || "another".to_owned(),
                    |(event, by)| format!("{by} (event {event})"),
                );
            return Ok(Outcome::Played(format!(
                "{said}; T-{number} went on from paused, to {state}, by {by} first, and the owner's resume was refused: {why}"
            )));
        }
        return Ok(hand.failed(
            format!("{said}; the resume of T-{number}, {state}, was refused: {why}"),
            Some(number),
            "",
        ));
    }
    if !hand.wait(|hand| Ok(hand.stage.state_of(number)? != "paused"))? {
        return Ok(hand.failed(
            format!("{said}; T-{number} stayed paused after its resume"),
            Some(number),
            "",
        ));
    }
    Ok(Outcome::Played(format!(
        "{said}; it went {}",
        hand.stage.state_of(number)?
    )))
}

/// T-`number`'s moves in `look`'s ledger, oldest first.
fn moves_of(look: &Look, number: i64) -> impl Iterator<Item = &crate::long_run::ledger::Event> {
    look.ledger
        .events
        .iter()
        .filter(move |event| event.kind == "task.state" && event.data["task"] == number)
}

/// Whether `look`'s ledger moved T-`number` to paused by `by` (a handle; none for
/// ConsensFlow in its own name) at `since` or after, in milliseconds, give or take
/// [`CLOCKS`].
fn paused_by(look: &Look, number: i64, by: Option<&str>, since: i64) -> bool {
    moves_of(look, number).any(|event| {
        event.data["to"] == "paused"
            && event.data["by"].as_str() == by
            && crate::clock::millis(&event.at).is_some_and(|at| at >= since - CLOCKS)
    })
}

/// What moved T-`number` on from its last move to paused, as `look`'s ledger says: the
/// event, and who (`@handle`, or the daemon in its own name).
fn went_on(look: &Look, number: i64) -> Option<(i64, String)> {
    let moves: Vec<_> = moves_of(look, number).collect();
    let paused = moves
        .iter()
        .rposition(|event| event.data["to"] == "paused")?;
    let on = moves
        .iter()
        .skip(paused + 1)
        .find(|event| event.data["from"] == "paused")?;
    let by = on.data["by"]
        .as_str()
        .map_or_else(|| "the daemon".to_owned(), |by| format!("@{by}"));
    Some((on.id, by))
}

/// `handle`'s window dies: played once its exit is seen and its task paused, which
/// the owner then resumes. Its task's pause is ConsensFlow's, in its own name: read on the
/// board, or in the ledger where another resumed it before the board was read.
fn kill(hand: &mut Hand<'_>, handle: &str, task: Option<i64>, hold: Duration) -> Result<Outcome> {
    let killed_at = jiff::Timestamp::now().as_millisecond();
    // The window may have closed between the look that chose it and now, and its
    // process is killed only while it is the one the run knew.
    let (pane, generation, said) = match hand.stage.kill(handle) {
        Err(Error::Refused(why)) => return Ok(Outcome::Missed(why)),
        killed => killed?,
    };
    if !hand.wait(|hand| Ok(hand.stage.exited(&pane, generation)))? {
        return Ok(hand.failed(format!("{said}; the window never exited"), task, handle));
    }
    let Some(number) = task else {
        return Ok(Outcome::Played(said));
    };
    // A task another resumed to working before any read of it is paused in the ledger: the
    // wait for it to leave working ends there too, not when its time is up.
    hand.wait(|hand| {
        Ok(hand.stage.state_of(number)? != "working" || hand.paused_by(number, None, killed_at))
    })?;
    let state = hand.stage.state_of(number)?;
    if state != "paused" && !hand.wait(|hand| Ok(hand.paused_by(number, None, killed_at)))? {
        return Ok(hand.failed(
            format!("{said}; T-{number} went {state}, not paused"),
            Some(number),
            handle,
        ));
    }
    hand.hold(hold / 2)?;
    resume_task(
        hand,
        number,
        &format!("{said}; T-{number} was paused, and the owner resumed it"),
    )
}

/// The owner types `/compact` into the chief's idle window: played once the chief's
/// own record has a compaction that is new since the keys (one more than it had
/// before them, written after them) and its window has come to rest; failed where
/// either is not seen within the wait.
fn compact(hand: &mut Hand<'_>) -> Result<Outcome> {
    let board = hand.stage.board()?;
    let Some(pane) = lane_of(&board, "chief")
        .map(|lane| lane["pane"].clone())
        .filter(|pane| !pane.is_null())
    else {
        return Ok(Outcome::Missed(
            "the chief has no window to type /compact into".to_owned(),
        ));
    };
    let before = hand.watch.last().map_or(0, compactions);
    let pressed = jiff::Timestamp::now().as_millisecond();
    for keys in [COMPACT, &[ENTER]] {
        hand.stage.press(&pane, keys)?;
        sleep(Duration::from_millis(600).min(hand.every));
    }
    let said = "the owner typed /compact into the idle chief".to_owned();
    let compacted = hand.wait(|hand| {
        Ok(hand
            .watch
            .last()
            .is_some_and(|look| compacted_after(look, before, pressed)))
    })?;
    if !compacted {
        let mut failed = hand.evidence(None, "chief", std::slice::from_ref(&said));
        failed.record = hand
            .watch
            .last()
            .and_then(|look| chief_record(look).map(|record| record.lines_by(look.at, 6)))
            .unwrap_or_default();
        return Ok(Outcome::Failed {
            said: format!("{said}; no new compaction came into its record"),
            evidence: failed,
        });
    }
    let rested = hand.wait(|hand| {
        Ok(lane_of(&hand.stage.board()?, "chief")
            .is_some_and(|lane| lane["activity"]["state"] == "idle"))
    })?;
    if !rested {
        // The Mac's 2-hour run of 2026-10-19: a chief that reads working for good after
        // the owner's /compact stalls the project.
        let state = lane_of(&hand.stage.board()?, "chief")
            .and_then(|lane| lane["activity"]["state"].as_str().map(str::to_owned))
            .unwrap_or_else(|| "nothing".to_owned());
        let said = format!(
            "{said}; its record says it compacted, and it never came to rest within the wait: its window reads {state}"
        );
        let mut failed = hand.evidence(None, "chief", std::slice::from_ref(&said));
        failed.record = hand
            .watch
            .last()
            .and_then(|look| chief_record(look).map(|record| record.lines_by(look.at, 6)))
            .unwrap_or_default();
        return Ok(Outcome::Failed {
            said,
            evidence: failed,
        });
    }
    Ok(Outcome::Played(format!(
        "{said}; its record says it compacted, and it came to rest"
    )))
}

/// The record of the chief's newest conversation the look has.
fn chief_record(look: &Look) -> Option<&super::record::Record> {
    let chief = look
        .ledger
        .participants
        .iter()
        .find(|someone| someone.role == "chief")?;
    look.record_of(chief.id)
}

/// How many compaction entries the chief's record has.
fn compactions(look: &Look) -> usize {
    chief_record(look).map_or(0, |record| {
        record
            .entries
            .iter()
            .filter(|entry| entry.kind == Kind::Compaction)
            .count()
    })
}

/// Whether the chief's record has a compaction that is new since `pressed`: more
/// compaction entries than the `before` it had, one of them written at `pressed` or
/// after.
pub fn compacted_after(look: &Look, before: usize, pressed: i64) -> bool {
    compactions(look) > before
        && chief_record(look).is_some_and(|record| {
            record.entries.iter().any(|entry| {
                entry.kind == Kind::Compaction && entry.at.is_some_and(|at| at >= pressed)
            })
        })
}

/// The app quit and started again; the owner resumes the project: played once it is
/// open with its chief's window, and, for a Devin worker cut mid-tool (`cut`), once
/// that worker's window was opened again.
fn restart(hand: &mut Hand<'_>, cut: Option<(&String, Option<i64>)>) -> Result<Outcome> {
    let who = cut.map_or_else(String::new, |(handle, task)| {
        format!(
            " while @{handle} was at work{}",
            task.map(|number| format!(" on T-{number}"))
                .unwrap_or_default()
        )
    });
    // The worker's window, which is to be opened again.
    let window = match cut {
        None => None,
        Some((handle, _)) => {
            let board = hand.stage.board()?;
            let Some(id) = lane_of(&board, handle)
                .and_then(|lane| lane["pane"]["id"].as_str())
                .map(str::to_owned)
            else {
                return Ok(Outcome::Missed(format!("@{handle} has no window to cut")));
            };
            Some((handle.clone(), id))
        }
    };
    hand.stage.restart()?;
    let said = format!("the app quit{who} and started again; the owner resumed the project");
    if let Some(why) = refusal(hand.stage.page("project.resume", json!({})))? {
        return Ok(hand.failed(
            format!("{said}, and the Resume was refused: {why}"),
            None,
            "chief",
        ));
    }
    if !resumed(hand)? {
        return Ok(hand.failed(
            format!("{said}; it is not open with its chief's window within the wait"),
            None,
            "chief",
        ));
    }
    if let Some((handle, id)) = window {
        if !hand.wait(|hand| Ok(hand.stage.opened(&id)))? {
            let task = cut.and_then(|(_, task)| task);
            return Ok(hand.failed(
                format!("{said}; @{handle}'s window {id} was not opened again"),
                task,
                &handle,
            ));
        }
        return Ok(Outcome::Played(format!(
            "{said}; @{handle}'s window {id} was opened again"
        )));
    }
    Ok(Outcome::Played(said))
}

/// Whether the project came back open with its chief's window, within the wait.
fn resumed(hand: &mut Hand<'_>) -> Result<bool> {
    hand.wait(|hand| {
        let board = hand.stage.board()?;
        Ok(board["project"]["state"] == "open"
            && lane_of(&board, "chief").is_some_and(|lane| !lane["pane"].is_null()))
    })
}

/// The owner switches the chief to the agent `to`, at the end of its turn: played
/// once the chief's window is a new one, on that agent.
fn switch(hand: &mut Hand<'_>, to: Option<&str>) -> Result<Outcome> {
    let Some(agent) = to else {
        return Ok(Outcome::Missed(
            "no agent to switch the chief to".to_owned(),
        ));
    };
    let before =
        lane_of(&hand.stage.board()?, "chief").map(|lane| lane["pane"]["generation"].clone());
    let asked = hand
        .stage
        .page("chief.switch", json!({ "agent": agent, "when": "turn" }));
    let said = format!("the owner switched the chief to {agent}");
    if let Some(why) = refusal(asked)? {
        return Ok(hand.failed(
            format!("{said}; the switch was refused: {why}"),
            None,
            "chief",
        ));
    }
    let switched = hand.wait(|hand| {
        let board = hand.stage.board()?;
        Ok(lane_of(&board, "chief").is_some_and(|lane| {
            let generation = &lane["pane"]["generation"];
            !generation.is_null()
                && Some(generation) != before.as_ref()
                && lane["participant"]["agent"] == agent
        }))
    })?;
    if !switched {
        return Ok(hand.failed(
            format!("{said}; no window of the chief on {agent} opened within the wait"),
            None,
            "chief",
        ));
    }
    Ok(Outcome::Played(said))
}

/// The owner closes the project, leaves it `hold`, and resumes it. A Close pauses the
/// tasks of the windows it closes, and the Resume brings them back by itself: a window
/// that held one task opens again on its own conversation, told once that its turn was
/// cut where the Close cut it; a window that held several (with any other task given
/// it or paused) comes back for none, its tasks left paused for the chief, who is told
/// to resume them one at a time. Played once the project read suspended and came back
/// open with its chief's window, and of each task the Close paused, in a window of its
/// own: it left `paused` by itself, its window was opened again naming the conversation
/// it had at the Close (each harness says it its own way: `--resume`, `resume`,
/// `--session-id`, `--session`; one with no conversation yet, a Codex window before
/// its first turn, opens afresh), and the window was told its turn was cut once, where
/// it was at work at the Close, and never twice; and of each task of a window that held
/// several: it was still paused after the Resume. The tasks the Close paused are those
/// at work or waiting before it that read paused once the Close is over.
fn suspend(hand: &mut Hand<'_>, hold: Duration) -> Result<Outcome> {
    let board = hand.stage.board()?;
    let at_work = tasks_in(&board, &["working", "waiting"]);
    let windows = Window::all(&board);
    let closed_at = jiff::Timestamp::now().as_millisecond();
    if let Some(why) = refusal(hand.stage.page("project.close", json!({})))? {
        return Ok(hand.failed(
            format!("the owner's Close was refused: {why}"),
            None,
            "chief",
        ));
    }
    if !hand.wait(|hand| Ok(hand.stage.board()?["project"]["state"] == "suspended"))? {
        return Ok(hand.failed(
            "the project never read suspended after the Close".to_owned(),
            None,
            "chief",
        ));
    }
    // The Close is over once none of the work it closed reads at work.
    hand.wait(|hand| {
        let board = hand.stage.board()?;
        Ok(!tasks_in(&board, &["working", "waiting"])
            .iter()
            .any(|number| at_work.contains(number)))
    })?;
    let paused: Vec<i64> = tasks_in(&hand.stage.board()?, &["paused"])
        .into_iter()
        .filter(|number| at_work.contains(number))
        .collect();
    hand.hold(hold)?;
    let said = format!(
        "the owner closed the project, and resumed it {} s later",
        hold.as_secs()
    );
    let opened_before: Vec<usize> = windows
        .iter()
        .map(|window| hand.stage.opened_with(&window.pane).len())
        .collect();
    if let Some(why) = refusal(hand.stage.page("project.resume", json!({})))? {
        return Ok(hand.failed(
            format!("{said}; the Resume was refused: {why}"),
            None,
            "chief",
        ));
    }
    if !resumed(hand)? {
        return Ok(hand.failed(
            format!("{said}; it is not open with its chief's window within the wait"),
            None,
            "chief",
        ));
    }
    // A window that held several: the Resume leaves its tasks paused, read before the
    // chief, told of them, resumes one.
    let several: Vec<(&Window, i64)> = paused
        .iter()
        .filter_map(|number| {
            let window = windows
                .iter()
                .find(|window| window.tasks.contains(number))?;
            (window.tasks.len() > 1).then_some((window, *number))
        })
        .collect();
    let mut unresolved = Vec::new();
    for (window, number) in &several {
        match hand.stage.state_of(*number)?.as_str() {
            "paused" => {}
            state => unresolved.push(format!(
                "T-{number} went on, {state}, though @{}'s window held {}",
                window.handle,
                named(&window.tasks)
            )),
        }
    }
    let single: Vec<(usize, i64)> = paused
        .iter()
        .filter_map(|number| {
            let at = windows
                .iter()
                .position(|window| window.tasks == [*number])?;
            Some((at, *number))
        })
        .collect();
    let went_on = hand.wait(|hand| {
        for (_, number) in &single {
            if hand.stage.state_of(*number)? == "paused" {
                return Ok(false);
            }
        }
        Ok(true)
    })?;
    if !went_on {
        for (_, number) in &single {
            if hand.stage.state_of(*number)? == "paused" {
                unresolved.push(format!("T-{number} stayed paused after the Resume"));
            }
        }
    }
    let mut afresh = Vec::new();
    for (at, number) in single.iter().filter(|_| went_on) {
        let (window, before) = (&windows[*at], opened_before[*at]);
        match window.came_back(hand, *number, before, closed_at)? {
            Back::Resumed => {}
            Back::Afresh => afresh.push(format!("@{}", window.handle)),
            Back::Not(why) => unresolved.push(why),
        }
    }
    let tasks = named(&paused);
    if !unresolved.is_empty() {
        return Ok(hand.failed(
            format!(
                "{said}; the close paused {tasks}, and {}",
                unresolved.join("; ")
            ),
            paused.first().copied(),
            "chief",
        ));
    }
    let mut outcome = said;
    if !single.is_empty() {
        let numbers: Vec<i64> = single.iter().map(|(_, number)| *number).collect();
        outcome.push_str(&format!(
            "; the close paused {}, and they went on with the Resume, each window on its own conversation",
            named(&numbers)
        ));
    }
    if !afresh.is_empty() {
        outcome.push_str(&format!(
            " (opened afresh, with no conversation yet: {})",
            afresh.join(", ")
        ));
    }
    if !several.is_empty() {
        let numbers: Vec<i64> = several.iter().map(|(_, number)| *number).collect();
        outcome.push_str(&format!(
            "; {} stayed paused for the chief, their windows holding several tasks",
            named(&numbers)
        ));
    }
    Ok(Outcome::Played(outcome))
}

/// A worker's window as the board read it before a Close: its participant, its
/// window's id, the tasks it held (given it, at work, waiting or paused), and whether
/// a turn of it was at work.
struct Window {
    handle: String,
    pane: String,
    tasks: Vec<i64>,
    working: bool,
}

/// How a window the Resume was to bring back came back.
enum Back {
    /// On the conversation it had at the Close, told once its turn was cut where it was.
    Resumed,
    /// Afresh: it had no conversation to go on with.
    Afresh,
    /// Not as it should: why.
    Not(String),
}

impl Window {
    /// The workers' windows of `board` that held a task.
    fn all(board: &Value) -> Vec<Self> {
        board["lanes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|lane| lane["participant"]["role"] != "chief")
            .filter_map(|lane| {
                let tasks: Vec<i64> = lane["tasks"]
                    .as_array()?
                    .iter()
                    .filter(|task| {
                        task["state"].as_str().is_some_and(|state| {
                            ["queued", "working", "waiting", "paused"].contains(&state)
                        })
                    })
                    .filter_map(|task| task["number"].as_i64())
                    .collect();
                let pane = lane["pane"]["id"].as_str()?;
                (!tasks.is_empty()).then(|| Window {
                    handle: lane["participant"]["handle"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                    pane: pane.to_owned(),
                    tasks,
                    working: lane["activity"]["state"] == "working",
                })
            })
            .collect()
    }

    /// Whether the window, which held T-`number` alone and had been opened `before`
    /// times when the owner resumed the project closed at `closed_at`, came back: opened
    /// again, naming the conversation it had then, and told its turn was cut, once, where
    /// it was at work, and never twice.
    fn came_back(
        &self,
        hand: &mut Hand<'_>,
        number: i64,
        before: usize,
        closed_at: i64,
    ) -> Result<Back> {
        let reopened = hand.wait(|hand| Ok(hand.stage.opened_with(&self.pane).len() > before))?;
        if !reopened {
            return Ok(Back::Not(format!(
                "@{}'s window {} was not opened again",
                self.handle, self.pane
            )));
        }
        let argv = hand
            .stage
            .opened_with(&self.pane)
            .into_iter()
            .nth(before)
            .unwrap_or_default();
        let since = closed_at - CLOCKS;
        let session = hand.watch.last().and_then(|look| {
            let participant = look.ledger.by_handle(&self.handle)?;
            let conversation = look.conversation_open_at(participant.id, since)?;
            look.ledger
                .conversations
                .iter()
                .find(|row| row.id == conversation)?
                .session
                .clone()
        });
        let Some(session) = session else {
            return Ok(Back::Afresh);
        };
        if !argv.contains(&session) {
            return Ok(Back::Not(format!(
                "@{}'s window opened again on {argv:?}, not on its conversation {session}",
                self.handle
            )));
        }
        // The note's sentence ends after the task, or goes on to name the turn (T-814).
        let words = [
            format!("{} T-{number}.", CUT[1]),
            format!("{} T-{number}, in the turn that began ", CUT[1]),
        ];
        let cut_notes = |hand: &Hand<'_>| {
            hand.watch.last().map_or(0, |look| {
                let Some(participant) = look.ledger.by_handle(&self.handle) else {
                    return 0;
                };
                look.ledger
                    .messages
                    .iter()
                    .filter(|message| {
                        message.recipient == participant.id
                            && message.sender.is_none()
                            && message.kind == "note"
                            && words.iter().any(|said| message.body.contains(said))
                            && crate::clock::millis(&message.created_at)
                                .is_some_and(|at| at >= since)
                    })
                    .count()
            })
        };
        if self.working && !hand.wait(|hand| Ok(cut_notes(hand) > 0))? {
            return Ok(Back::Not(format!(
                "@{}'s window, at work on T-{number} at the Close, was never told its turn was cut",
                self.handle
            )));
        }
        let told = cut_notes(hand);
        if told > 1 {
            return Ok(Back::Not(format!(
                "@{}'s window was told {told} times that its turn on T-{number} was cut",
                self.handle
            )));
        }
        Ok(Back::Resumed)
    }
}

/// Tasks as a disruption says them: `T-1, T-2`.
fn named(numbers: &[i64]) -> String {
    let named: Vec<String> = numbers.iter().map(|number| format!("T-{number}")).collect();
    named.join(", ")
}

/// The numbers of the tasks of `board` in one of `states`.
fn tasks_in(board: &Value, states: &[&str]) -> Vec<i64> {
    crate::run::card::tasks_of(board)
        .into_iter()
        .filter(|task| {
            task["state"]
                .as_str()
                .is_some_and(|state| states.contains(&state))
        })
        .filter_map(|task| task["number"].as_i64())
        .collect()
}

#[cfg(test)]
mod tests;

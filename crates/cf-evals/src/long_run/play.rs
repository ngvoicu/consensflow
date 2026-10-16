//! A disruption played on the stage, as a person or a crash would make it: the
//! owner pauses a task and resumes it, a window's process dies, the owner types
//! `/compact` into the chief, quits the app and starts it again, switches the chief,
//! closes the project and resumes it. Each ends where the owner would leave it: a task
//! the disruption paused is resumed, a project it suspended is resumed.
//!
//! A disruption is played only when its effect is seen: the paused task paused and on
//! again; the window killed and its task paused, then on again; a compaction boundary
//! in the chief's own record that is new since the keys (one more than before, written
//! after them); the project open again with its chief's window, and for a Devin worker
//! cut mid-tool, that worker's window opened again; the new chief's window open on
//! the agent it was switched to; the tasks a close paused out of `paused` once the
//! owner resumed them. An effect not seen within the time a disruption waits is a
//! failure, said with its evidence (the rows of the board, the window's screen, the
//! record); a refusal of what the owner asked is a failure too, unless the task had
//! ended by then. A moment that passed before it could be played (the task ended
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
use super::invariants::Evidence;
use super::look::Look;
use super::record::Kind;
use super::stage::lane_of;
use crate::{Error, Result};

/// The keys of `/compact`, then Enter.
const COMPACT: &[u8] = b"/compact";
const ENTER: u8 = 13;

/// The states a task is done in, which a resume of it is refused for.
const ENDED: [&str; 4] = ["done", "accepted", "cancelled", "failed"];

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

/// The owner pauses T-`number`, leaves it `hold`, and resumes it: played once it read
/// paused and then went on.
fn pause(hand: &mut Hand<'_>, number: i64, hold: Duration) -> Result<Outcome> {
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
    if !hand.wait(|hand| Ok(hand.stage.state_of(number)? == "paused"))? {
        return Ok(hand.failed(
            format!("T-{number} never read paused after the owner's pause"),
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
/// `said` being what was done before.
fn resume_task(hand: &mut Hand<'_>, number: i64, said: &str) -> Result<Outcome> {
    if let Some(why) = refusal(hand.stage.page("task.resume", json!({ "task": number })))? {
        let state = hand.stage.state_of(number)?;
        if ENDED.contains(&state.as_str()) {
            return Ok(Outcome::Played(format!(
                "{said}; T-{number} went {state} in the hold"
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

/// `handle`'s window dies: played once its exit is seen and its task paused, which
/// the owner then resumes.
fn kill(hand: &mut Hand<'_>, handle: &str, task: Option<i64>, hold: Duration) -> Result<Outcome> {
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
    hand.wait(|hand| Ok(hand.stage.state_of(number)? != "working"))?;
    let state = hand.stage.state_of(number)?;
    if state != "paused" {
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
/// own record has a compaction that is new since the keys: one more than it had
/// before them, written after them.
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
    Ok(Outcome::Played(format!(
        "{said}; its record says it compacted{}",
        if rested {
            ", and it came to rest"
        } else {
            " (it did not come to rest within the wait)"
        }
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

/// The owner closes the project, leaves it `hold`, and resumes it; then resumes each
/// task the close paused, as a person does at the board (a Close pauses the tasks of
/// the windows it closes, and a task the system paused stays so after the Resume; a
/// quit leaves them at work, and the Resume brings them back). Played once the project
/// read suspended, came back open with its chief's window, and every task the close
/// paused left `paused` (or had ended, its resume refused for that).
fn suspend(hand: &mut Hand<'_>, hold: Duration) -> Result<Outcome> {
    let at_work = tasks_in(&hand.stage.board()?, &["working", "waiting"]);
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
    hand.hold(hold)?;
    let said = format!(
        "the owner closed the project, and resumed it {} s later",
        hold.as_secs()
    );
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
    let paused: Vec<i64> = tasks_in(&hand.stage.board()?, &["paused"])
        .into_iter()
        .filter(|number| at_work.contains(number))
        .collect();
    let mut unresolved = Vec::new();
    let mut asked = Vec::new();
    for number in &paused {
        match refusal(hand.stage.page("task.resume", json!({ "task": number })))? {
            None => asked.push(*number),
            Some(why) => {
                let state = hand.stage.state_of(*number)?;
                if !ENDED.contains(&state.as_str()) {
                    unresolved.push(format!(
                        "the resume of T-{number}, {state}, was refused: {why}"
                    ));
                }
            }
        }
    }
    let went_on = hand.wait(|hand| {
        for number in &asked {
            if hand.stage.state_of(*number)? == "paused" {
                return Ok(false);
            }
        }
        Ok(true)
    })?;
    if !went_on {
        for number in &asked {
            if hand.stage.state_of(*number)? == "paused" {
                unresolved.push(format!("T-{number} stayed paused after the owner's resume"));
            }
        }
    }
    let tasks: Vec<String> = paused.iter().map(|number| format!("T-{number}")).collect();
    if !unresolved.is_empty() {
        return Ok(hand.failed(
            format!(
                "{said}; the close paused {}, and {}",
                tasks.join(", "),
                unresolved.join("; ")
            ),
            paused.first().copied(),
            "chief",
        ));
    }
    let resumed_tasks = if paused.is_empty() {
        String::new()
    } else {
        format!(
            "; the close paused {}, and the owner resumed them",
            tasks.join(", ")
        )
    };
    Ok(Outcome::Played(format!("{said}{resumed_tasks}")))
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

//! What a worker's turn comes to when it ends: its final answer is its task's result,
//! unless its harness says the turn left work running that it wakes its agent for.
//! Turn-end's smallest rule (the chief's decision of 2026-10-21, on the advisor's answer
//! of T-8): what is uncertain never completes a task, reuses a window or ends a process.
//!
//! **The result is the final answer** (the owner's decision of 2026-10-15, after the
//! chief's reports on 3.0.4: T-14 and T-15 were given ten to twenty lines of progress
//! before their reports; dip-agentos #5): what the member wrote after its last tool call
//! or message in its last turn, after the task's latest message pasted into the window
//! ([`answers_to_the_task`]). That message is found by the record's item its arrival was
//! proved by (its receipt), never by words that quote its header. What it wrote before
//! stays in the transcript (`cf task get --transcript`), and so does all it wrote of a
//! turn that ended before the one that answers. Each harness's own record says which turn
//! that is: a harness marks the step that ends its turn complete (Claude Code's
//! `end_turn` and its boundary record, Pi's `stop`, OpenCode's `finish: stop`, Devin's
//! last reply of a request, Codex's `final_answer`), and the progress it writes on the way
//! is no complete answer. What the harness wrote itself is no answer and cuts none: the
//! summary of a compaction, a notification it keeps in its record alone, Claude Code's
//! close of a cut turn. A turn that wrote nothing after its last tool call says so
//! ([`WITHOUT_ANSWER`]).
//!
//! **Pending work keeps the task working** (A2; dip-agentos #1, poker-lab's T-442):
//! Claude Code lets a command run in the background, or a Monitor watch one, and its
//! agent end its turn, and begins a turn by itself for each event and each end, which
//! wakes the agent; it counts the background agents and workflows it wakes the agent for
//! itself. A turn that ended with any of those pending is no result: the task stays
//! working, nothing is ended, and whoever gave the task is told once that it goes on in
//! the background, with the words the turn ended with and how to end the wait
//! ([`Dispatcher::finish_task`]). Only the harness's own word that each is over clears
//! it (`cf-harness`' `claude::record::jobs`): no time that passes releases a task held so,
//! and a look at the window's processes releases one thing alone. A monitor whose notice of
//! its end came (Claude Code's words, which a line of its command's could say too) is
//! taken for over once, on a pass after the one that saw the notice, the window's tree runs
//! no command of its harness's at all (`pane.commands`, [`Dispatcher::monitors_over`]):
//! Claude Code kills a monitor's command as it writes the notice, and a command that
//! printed such a line runs still, or ended its monitor by its exit (T-203). A job that
//! never ends (a dev server) keeps its task working until the chief or the human finishes
//! it.
//!
//! **A turn's end ends nothing.** What a worker's turn left running, its harness's or
//! not, goes on: a free worker's window closes after its result, as any does, and the
//! close ends what the host can tell is the window's, which it lists to whoever gave the
//! task ([`Dispatcher::tell_left_running`]); a window the human keeps open keeps it. What
//! is known of the work the record no longer waits on, and how much of it is not known
//! to have ended, is said in the result itself ([`leftovers`]).
//!
//! **A tell's turn on a paused task** (T-442): a task paused by `cf tell` takes no result,
//! but the complete answer its window's turn ends with, after answering the tell, is not
//! lost: whoever gave the task is told it once, with the words, and Finish takes it as the
//! result ([`paused`]).
//!
//! **A window has one task** (`ownership`): no other task's message goes into a window
//! once one task's has, whatever ended that task; so no later task's turn is woken by
//! what an earlier one left, and no later task is given an earlier task's words.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use cf_base::{js, time};
use cf_harness::contract::Observed;
use cf_harness::records::{End, Item, Monitor, Pending, Reading, Role, Source};
use cf_ledger::{NewNote, ParticipantView, ProjectView, TaskThread};
use serde_json::Value;

use crate::delivery_text::{header_id, is_paste_of};
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::role_change::answers_to_the_task;
use crate::seams::{CommandsSeen, EngineError};

/// What a result says of a turn that wrote nothing after its last tool call.
pub(crate) const WITHOUT_ANSWER: &str = "(the agent ended its turn without a written answer)";

/// What a worker's turn came to, for its task.
pub(crate) enum Turn {
    /// Nothing is over: the turn goes on, or it answers no task, or it failed (and the
    /// task with it).
    Open,
    /// Over, and what it left running is what its harness wakes its agent for: the
    /// task goes on in the background.
    Held,
    /// Over, and its final answer is the task's result, recorded.
    Answered,
}

/// What a worker's task is held for: the task, and how many jobs and agents its turn left
/// running, which its harness wakes the agent for as each ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hold {
    pub task: i64,
    pub jobs: usize,
    pub agents: usize,
}

/// What the engine keeps of a held task: the record's part of the turn's end.
#[derive(Debug, Default)]
pub(crate) struct HeldPart {
    /// What the task is held for, while it is.
    pub(crate) holding: Option<Hold>,
    /// What the task's requester was told it is held for ([`Told`]): told again when that
    /// changes, whatever its turns say on the way.
    told: Option<Told>,
    /// The line of the window's record lost in the task's turn that its requester was told
    /// of, by a note of the hold or of a turn that may have lost its last line: told once.
    lost_said: Option<usize>,
    /// How many monitors that said they ended the last pass found where only they were
    /// pending ([`Dispatcher::watch_monitors`]).
    noticed: usize,
    /// What the last look at the window's commands found, where one was made for them, and
    /// when the next is due, and how long the wait after one that ended nothing is.
    seen: Option<CommandsSeen>,
    next_look_ms: i64,
    wait_ms: i64,
    /// How many looks in a row found a command still running: the first is told nothing
    /// (Calliope on 136a7158: Claude Code's kill can take a second or more).
    running_looks: u32,
    /// The task whose turn first looked finished, and when ([`Dispatcher::looks_finished`]),
    /// while it does.
    quiet: Option<(i64, i64)>,
    /// The task whose requester was told its turn looks finished: told once.
    told_finished: Option<i64>,
    /// The window's rests, which outlive the task's hold: they are the window's, and go
    /// with it, or with the conversation it shows ([`HeldPart::reset`]).
    pub(crate) rests: Rests,
}

/// What the engine keeps of a window's rests (`BackgroundWork::rest`): the latest the host
/// keeps, the one that was the latest when the window was last seen at rest (taken before
/// that look, so before any monitor the look did not show began), whether it was at rest
/// at the last pass, and for each monitor the rest proved before it was first seen.
#[derive(Debug, Default)]
pub(crate) struct Rests {
    rest: Option<u64>,
    proved: Option<u64>,
    at_rest: bool,
    since: HashMap<Arc<str>, Option<u64>>,
}

impl HeldPart {
    /// What a window keeps once its task is no longer one it works on (paused, queued for a
    /// resume): not the hold, nor what the requester was told of it. The look at its monitors
    /// is kept: `watch_monitors` keeps it itself (it clears it at a pass that has no monitor
    /// to decide), and a paused task's turn that ended with a monitor that said it ended is
    /// told only once a look found its command over (T-442, Calliope's P2 on round 2).
    /// Nor whether its turn looked finished, or the note that said so: a resumed task that
    /// sits finished again is told again (the chief's call at 3.0.6-alpha.3's integration B).
    pub(crate) fn drop_hold(&mut self) {
        self.holding = None;
        self.told = None;
        self.quiet = None;
        self.told_finished = None;
    }

    /// What a window keeps of its task's hold once no task of its is working: nothing of
    /// the hold, and its rests, which a window idle between its result and its close does
    /// not take again at every pass (Calliope on 136a7158).
    pub(crate) fn reset(&mut self) {
        let rests = std::mem::take(&mut self.rests);
        *self = HeldPart {
            rests,
            ..HeldPart::default()
        };
    }
}

impl Rests {
    /// A message is pasted into the window at rest: the rest taken then, or before, came
    /// before the turn it begins, and so before every monitor of that turn (Calliope on
    /// 136a7158). A window that took no rest proves none.
    pub(crate) fn pasted(&mut self) {
        if self.rest.is_some() {
            self.proved = self.rest;
        }
    }
}

/// What a held task's requester was told: the task, what was pending, its monitors by
/// their task, source and notice, and what the last look at its window's commands found
/// (Astraeus on 136a7158: a count that fell, or a monitor replaced by another, with the
/// same counts, was not told).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Told {
    task: i64,
    jobs: usize,
    agents: usize,
    noticed: usize,
    monitors: Vec<Monitor>,
    seen: Option<CommandsSeen>,
}

/// How long the engine waits, at most, between two looks at a window's commands that end
/// nothing (Calliope on 472b1d96: a window that runs a dev server, or whose host cannot
/// tell, is not walked each second until Finish).
const LOOKS_APART_MS: i64 = 60_000;

/// What a look takes off the time of a monitor's call for the start of a process to be
/// read against it: a monitor's command begins after its call, and its call's record is
/// written as the call is made, by the wall clock a start is read by too. Measured by the
/// live run (monitor-wait's `call-margin`, three runs on Claude Code 2.1.309, T-301): each
/// message's monitors' commands began 7 to 21 ms after the earliest of their calls' records,
/// none before.
const CALL_MARGIN_MS: i64 = 1_000;

/// How long a turn that looks finished ([`Dispatcher::looks_finished`]) has to look so before
/// its requester is told: the wait between two looks that end nothing ([`LOOKS_APART_MS`]).
/// Claude Code writes a turn's end within a moment of its answer, so a window that waits at
/// its prompt that long with no end written has none to come.
const LOOKS_FINISHED_MS: i64 = LOOKS_APART_MS;

/// What a pass makes of the monitors that said they ended ([`Dispatcher::watch_monitors`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Watching {
    /// Nothing for a look to decide: no such monitor, or something else is pending too.
    Undecided,
    /// A look decides, and none was made yet: the requester is told nothing until one is.
    Waiting,
    /// What the latest look found.
    Seen(CommandsSeen),
}

/// Whether `record`'s window has still to show its launch's first message: owed it until
/// it is up ([`Dispatcher::hand_first`]), or handed it and its record not showing it yet.
/// Such a window has not begun the turn that message begins, and what its record holds
/// after the latest message delivered answers one before it: a window the Resume opened
/// again on its conversation, read at rest before its words to go on are in, ends on the
/// answer written before the Close (T-405's L10; the sweep of T-523, Calliope's P3-4 on
/// T-432). It is no result, at the turn's end nor for Finish.
pub(crate) fn first_on_its_way(record: &Record) -> bool {
    record
        .delivery
        .borrow()
        .delivering
        .as_ref()
        .is_some_and(|delivering| delivering.launch)
}

impl Dispatcher {
    /// A worker's turn, looked at: whether it is over with an answer to its task's latest
    /// message, and what that comes to ([`Turn`]). A turn that failed is its task's
    /// failure, whatever it left running, with why ([`Dispatcher::why_it_failed`]); one
    /// that failed while its task was not working is told whoever gave the task, once
    /// ([`Dispatcher::tell_failed_turn`]). One that ended with its answer and nothing
    /// pending is its result, recorded now, before anything else is asked of anyone: what
    /// follows (a close, its note) can no longer lose it.
    pub(crate) async fn collect(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Rc<Record>,
        observed: &Observed,
        watching: Watching,
    ) -> Result<Turn, EngineError> {
        let task = self
            .seams
            .ledger
            .borrow()
            .active_task(participant.id, false)?;
        let Some(thread) = task.filter(|thread| thread.task.state == "working") else {
            record.held.borrow_mut().drop_hold();
            if observed.failed {
                self.tell_failed_turn(project, participant, record, observed)
                    .await?;
            }
            self.tell_paused_answer(project, participant, observed, watching)?;
            return Ok(Turn::Open);
        };
        if first_on_its_way(record) {
            return Ok(Turn::Open);
        }
        let items = observed.items();
        let Some(start) = self.delivered_at(participant.id, &thread, items)? else {
            return Ok(Turn::Open);
        };
        if observed.failed {
            // A failed turn holds its task for nothing it left (T-532's P3-3).
            record.held.borrow_mut().reset();
            let because = self
                .why_it_failed(participant, record, observed)
                .await?
                .said;
            // The task may have moved on while the window was looked at.
            let still = self
                .seams
                .ledger
                .borrow()
                .active_task(participant.id, false)?
                .is_some_and(|now| now.task.id == thread.task.id && now.task.state == "working");
            if still && !self.forgotten(record) {
                self.fail_task(project, &thread.task, &because)?;
            }
            return Ok(Turn::Open);
        }
        let lost = lost_in_turn(record, observed, Some(start));
        if !observed.settled {
            self.looks_finished(
                project,
                participant,
                record,
                &thread,
                observed,
                &items[start + 1..],
            )?;
            self.tell_lost(project, participant, record, &thread, lost)?;
            return Ok(Turn::Open);
        }
        // Words told to the window before it was up are on their way to it (the brief was
        // in flight when they were told): they go in next, and the answer to them is the
        // result. This answer is neither the result nor a hold, and the requester is told
        // nothing of it (field-306's B3; Calliope's L1 on the merge, T-370).
        let told = self
            .seams
            .ledger
            .borrow()
            .told_waiting(participant.id, thread.task.id)?;
        if told {
            return Ok(Turn::Open);
        }
        let replacements = self.seams.ledger.borrow().role_notes_to(participant.id)?;
        let Some(answers) = answers_to_the_task(&items[start + 1..], &replacements)
            .filter(|answers| answers.last.is_some_and(|last| last.complete))
        else {
            self.tell_lost(project, participant, record, &thread, lost)?;
            return Ok(Turn::Open);
        };
        let pending = pending_after_look(observed, watching);
        // A line lost in the turn is said with its words, held or not: the hold's note is
        // where the requester chooses between waiting and Finish (Calliope's P3 (b), T-624).
        let words = with_lost(words_of(&answers.final_run), &participant.handle, lost);
        if pending.total() > 0 {
            let hold = Hold {
                task: thread.task.number,
                jobs: pending.jobs,
                agents: pending.agents,
            };
            let seen = match watching {
                Watching::Seen(seen) => Some(seen),
                Watching::Undecided | Watching::Waiting => None,
            };
            let now = Told {
                task: hold.task,
                jobs: pending.jobs,
                agents: pending.agents,
                noticed: pending.noticed,
                monitors: observed.monitors(),
                seen,
            };
            let told = {
                let mut held = record.held.borrow_mut();
                held.holding = Some(hold);
                held.told.as_ref() == Some(&now)
            };
            // Where a look decides, the requester is told once it was made, and what it
            // found (Calliope on 472b1d96).
            if !told && watching != Watching::Waiting {
                let said = Held {
                    pending,
                    monitors: observed.monitors(),
                    seen,
                };
                self.tell_it_goes_on(project, participant, &thread, &said, &words)?;
                let mut held = record.held.borrow_mut();
                held.told = Some(now);
                held.lost_said = held.lost_said.or(lost);
            }
            return Ok(Turn::Held);
        }
        record.held.borrow_mut().reset();
        let body = with_leftovers(words, &leftovers(&participant.handle, pending, None, true));
        self.seams
            .ledger
            .borrow_mut()
            .record_result(project.id, thread.task.number, &body)?;
        self.changed();
        Ok(Turn::Answered)
    }

    /// A turn whose record never reads it as over, though it looks finished: the window
    /// opened on a conversation whose last program the host never proved gone
    /// (`WindowPart::unproven`), its harness says it waits at its prompt, nothing it left
    /// runs that wakes its agent, and its answer to the task's latest message is complete.
    /// T-526 (2026-10-25): the program before had queued and taken a notification that no
    /// turn began, which Claude Code's reader, told no relaunch, read as a turn still to
    /// come, and the task stayed working four hours after its answer, nobody told. Its
    /// requester is told once, after it has looked so for [`LOOKS_FINISHED_MS`], how to
    /// take the answer; nothing is recorded and nothing ends: the record's word stands,
    /// and Finish is the requester's. `said` is the record after the task's message.
    fn looks_finished(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Record,
        thread: &TaskThread,
        observed: &Observed,
        said: &[Item],
    ) -> Result<(), EngineError> {
        let number = thread.task.number;
        let looks = record.window.borrow().unproven
            && observed.at_prompt
            && observed.waiting.is_none()
            && observed.pending().total() == 0
            && {
                let replacements = self.seams.ledger.borrow().role_notes_to(participant.id)?;
                answers_to_the_task(said, &replacements)
                    .is_some_and(|answers| answers.last.is_some_and(|last| last.complete))
            };
        let since = {
            let mut held = record.held.borrow_mut();
            if !looks {
                held.quiet = None;
                return Ok(());
            }
            let now = self.now();
            let since = match held.quiet {
                Some((task, since)) if task == number => since,
                _ => {
                    held.quiet = Some((number, now));
                    now
                }
            };
            if now - since < LOOKS_FINISHED_MS || held.told_finished == Some(number) {
                return Ok(());
            }
            held.told_finished = Some(number);
            since
        };
        let way = match thread.task.requester.as_str() {
            "human" => "Finish in the task's drawer takes that answer".to_owned(),
            _ => format!("`cf task finish T-{number}` takes that answer"),
        };
        // The engine knows no zone of the human's: the time is said in UTC, as its notes'
        // times are.
        let at = time::iso(since);
        let body = format!(
            "T-{number} looks finished: its last answer is complete and @{}'s window has been idle since {} UTC, but its turn never read as over. {way}.",
            participant.handle,
            at.get(11..16).unwrap_or(&at),
        );
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: thread.task.requester.clone(),
                task: Some(number),
                body,
            },
        )?;
        self.changed();
        Ok(())
    }

    /// The place among `items`, the window's record, of the latest message of the task
    /// `thread` pasted into the window of the participant: the item its arrival was proved
    /// by (`receipt`). A message whose receipt names no item of the record (one the record
    /// lost, a receipt of an older build) is found by its paste, as its arrival was
    /// (`is_paste_of`: its header begins an item of the user's, in Claude Code's wrapper or
    /// behind the brief a fresh window was given first), the last such, and never by a
    /// quotation of its header in words of the human's or the member's, nor in a
    /// compaction's summary (Astraeus on 0eec91d2: a quotation taken for the delivery let
    /// Finish take an answer to the quotation). None where the task has no message
    /// pasted, or the record holds no proof of where it arrived: the window answers
    /// nothing of the task, and Finish has nothing to take.
    pub(crate) fn delivered_at(
        &self,
        participant: i64,
        thread: &TaskThread,
        items: &[Item],
    ) -> Result<Option<usize>, EngineError> {
        let latest = self
            .seams
            .ledger
            .borrow()
            .last_pasted(participant, thread.task.id)?;
        let Some(latest) = latest else {
            return Ok(None);
        };
        let receipt = latest.receipt.get("item").and_then(Value::as_str);
        if let Some(found) = receipt.and_then(|item| {
            items
                .iter()
                .rposition(|said| said.role == Role::User && &*said.id == item)
        }) {
            return Ok(Some(found));
        }
        let first = self.brief_given_first(&latest)?;
        Ok(items.iter().rposition(|item| {
            item.role == Role::User
                && !item.compaction
                && is_paste_of(&item.text, latest.id, first.as_deref())
        }))
    }

    /// The place among `items` of the latest message about the task that was pasted into
    /// the window: [`Dispatcher::delivered_at`]'s, or a later one ConsensFlow pasted since
    /// (the Resume's word to go on, a question the chief put to the window), which no start is
    /// taken from at a turn's end and is answered by what follows it. The harness's own
    /// notifications, which wake a held task's turns, are none, nor is the note that says
    /// the role (`replacements`, whose word is the role's), nor a compaction summary that
    /// restates a header (the merge with Resume after Close, T-405's L10, and Calliope's P2
    /// on it, T-444). None where `delivered_at` finds none.
    pub(crate) fn latest_message_at(
        &self,
        thread: &TaskThread,
        participant: &ParticipantView,
        items: &[Item],
        replacements: &[i64],
    ) -> Result<Option<usize>, EngineError> {
        let Some(start) = self.delivered_at(participant.id, thread, items)? else {
            return Ok(None);
        };
        Ok(Some(
            items[start + 1..]
                .iter()
                .rposition(|item| {
                    item.role == Role::User
                        && !item.compaction
                        && header_id(&item.text).is_some_and(|id| !replacements.contains(&id))
                })
                .map_or(start, |later| start + 1 + later),
        ))
    }

    /// What the monitors a worker's turn left that said they ended come to, at this pass
    /// ([`Watching`]): a look at the window's commands decides where the turn is settled,
    /// only such monitors are pending and each watches a command, on a pass after the one
    /// that saw the last of their notices (Claude Code kills a monitor's command after it
    /// writes its notice), and no sooner than the wait after a look that ended nothing.
    /// A look that finds no command ends them ([`Dispatcher::collect`]).
    ///
    /// It keeps too the window's rests, for a look to leave out what ran when the window
    /// was last at rest before the monitors began, which is no command of theirs (the
    /// chief's decision on Astraeus's review of 472b1d96: an MCP server that left the
    /// window's group). A rest is taken when the window is first seen at rest after a
    /// turn; it is proved for a monitor where a later look saw the window at rest without
    /// it: that monitor's call was not written then (no call is open at rest), and its
    /// command, which Claude Code starts after the call, began after the rest was taken.
    /// A message pasted into the window at rest proves the rest taken then, or before, for
    /// every monitor of the turn it begins ([`Rests::pasted`]). The rests the monitors that
    /// run are left are kept by the host however many come after (Astraeus on 136a7158).
    ///
    /// Where no rest proves them, as none does in the first turn of a window opened with its
    /// task's message (every worker's), the look names the earliest of their calls' times,
    /// less [`CALL_MARGIN_MS`]: what began before it is no command of theirs (the chief on
    /// T-301, Calliope's timestamp proof on 136a7158). A call whose record says no time
    /// names none, and nothing is left out.
    ///
    /// Limits. The rest proof takes a record read at a look as no older than the rest taken
    /// before it: a record read late by more than a pass would let a monitor's call written
    /// after the rest pass for one written before (Calliope on 136a7158). The time proof
    /// reads a record's time and a process's start by the wall clock, which can step: the
    /// host leaves a process out by it only where it watched the wall clock keep its order
    /// since the window opened, and the process's own two starts agree with what it watched
    /// (Astraeus on 02a6cd4f; cf-panes' `background::clock`, which says the limit left: a
    /// clock set forward and back again between two of its readings, a call stamped in
    /// between). A rest, where one proves the monitors, is named alone.
    pub(crate) async fn watch_monitors(
        &self,
        participant: &ParticipantView,
        record: &Record,
        observed: &Observed,
    ) -> Watching {
        let pane = self.pane(participant.id);
        // A record that could not be read says no monitor, though they run on: their
        // bindings and the rests stay as they were, and nothing is decided (Astraeus on
        // 02a6cd4f: a monitor bound again to a rest its own command ran at).
        if !matches!(observed.reading.as_deref(), Some(Reading::Known(_))) {
            let mut held = record.held.borrow_mut();
            held.noticed = 0;
            held.seen = None;
            held.wait_ms = 0;
            held.next_look_ms = 0;
            held.running_looks = 0;
            return Watching::Undecided;
        }
        let monitors = observed.monitors();
        let take_rest = {
            let mut held = record.held.borrow_mut();
            let rests = &mut held.rests;
            let proved = rests.proved;
            rests
                .since
                .retain(|id, _| monitors.iter().any(|monitor| monitor.id == *id));
            for monitor in &monitors {
                rests.since.entry(Arc::clone(&monitor.id)).or_insert(proved);
            }
            let first = observed.settled && !rests.at_rest;
            if observed.settled {
                rests.proved = rests.rest;
            }
            rests.at_rest = observed.settled;
            // The rests the monitors that run are left, which the host keeps however many
            // come after them (Astraeus on 136a7158).
            let mut keep: Vec<u64> = rests.since.values().flatten().copied().collect();
            keep.sort_unstable();
            keep.dedup();
            first.then_some(keep)
        };
        if let (Some(pane), Some(keep)) = (pane.as_ref(), take_rest) {
            let rest = self.seams.background.rest(pane, &keep).await;
            record.held.borrow_mut().rests.rest = rest;
        }
        let pending = observed.pending();
        let noticed = pending.noticed;
        // A socket's monitor has no command to look for.
        let decides = observed.settled
            && noticed > 0
            && pending.total() == noticed
            && noticed_of(&monitors) == noticed;
        let (seen, since, after) = {
            let mut held = record.held.borrow_mut();
            let seen = held.noticed;
            held.noticed = if decides { noticed } else { 0 };
            if !decides {
                held.seen = None;
                held.wait_ms = 0;
                held.next_look_ms = 0;
                held.running_looks = 0;
            }
            // The oldest rest proved for any of them: none where one has none.
            let since = monitors
                .iter()
                .filter(|monitor| monitor.notice.is_some())
                .map(|monitor| held.rests.since.get(&monitor.id).copied().flatten())
                .try_fold(u64::MAX, |oldest, rest| rest.map(|rest| oldest.min(rest)));
            // Where no rest proves them, the earliest of their calls, less the margin: none
            // where a call's record says no time.
            let after = since
                .is_none()
                .then(|| {
                    monitors
                        .iter()
                        .filter(|monitor| monitor.notice.is_some())
                        .map(|monitor| monitor.called)
                        .try_fold(i64::MAX, |earliest, called| {
                            called.map(|called| earliest.min(called))
                        })
                })
                .flatten()
                .map(|earliest| earliest.saturating_sub(CALL_MARGIN_MS));
            (seen, since, after)
        };
        if !decides {
            return Watching::Undecided;
        }
        if noticed > seen {
            // Another said it ended: the next pass looks, whatever the wait was.
            let mut held = record.held.borrow_mut();
            held.wait_ms = 0;
            held.next_look_ms = 0;
            held.running_looks = 0;
            return watching_of(&held);
        }
        if self.now() < record.held.borrow().next_look_ms {
            return watching_of(&record.held.borrow());
        }
        let Some(pane) = pane else {
            return Watching::Waiting;
        };
        let looked = self.seams.background.commands(&pane, since, after).await;
        let now = self.now();
        let mut held = record.held.borrow_mut();
        held.seen = Some(looked);
        held.running_looks = match looked {
            CommandsSeen::Running(running) if running > 0 => held.running_looks + 1,
            _ => 0,
        };
        held.wait_ms = (held.wait_ms * 2).clamp(1_000, LOOKS_APART_MS);
        held.next_look_ms = now + held.wait_ms;
        watching_of(&held)
    }

    /// What the task of the participant's window is held for, while it is.
    pub fn held_for(&self, participant: i64) -> Option<Hold> {
        self.record(participant)
            .and_then(|record| record.held.borrow().holding)
    }

    /// Whoever gave the task is told it goes on in the background, once for each time it is
    /// held, and again where another of its monitors' notices of their end came
    /// ([`HeldPart`]): what the worker ended its turn with, what it left running that it is
    /// woken for, its monitors and what came of each, and its requester's choices: to wait,
    /// or to take its last complete answer as the result ([`Dispatcher::finish_task`]).
    /// What the engine keeps goes with the window and with the daemon: a task held in a
    /// window opened again, or after a restart, is told again.
    fn tell_it_goes_on(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        thread: &TaskThread,
        held: &Held,
        words: &str,
    ) -> Result<(), EngineError> {
        let number = thread.task.number;
        let what = match held.pending.total() {
            1 => "it",
            _ => "them",
        };
        let way = way_to_finish(thread);
        let handle = &participant.handle;
        let body = format!(
            "T-{number} goes on in the background: @{handle} ended its turn with {}.{} Its result comes when a turn of its ends with none left: to wait for it, do nothing. If its work is done, take what it said last as the result now: {way}, which does not end {what}. What it said:\n\n{words}",
            counted(held.pending, ", which its harness wakes it for as each ends"),
            monitors_said(handle, held),
        );
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: thread.task.requester.clone(),
                task: Some(number),
                body,
            },
        )?;
        self.changed();
        Ok(())
    }

    /// Whoever gave the task is told, once, that a line of the window's record was lost in a
    /// turn that has come to no result (`lost`, where one was): the line may have been the
    /// turn's final answer, or the record that says the turn ended, and then nothing of the
    /// turn ever ends it (Calliope's P3 (a), T-624). Said on the first look that finds it,
    /// while the turn may still be under way: no reading says the window has gone quiet.
    fn tell_lost(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Record,
        thread: &TaskThread,
        lost: Option<usize>,
    ) -> Result<(), EngineError> {
        let Some(line) = lost else {
            return Ok(());
        };
        if record.held.borrow().lost_said == Some(line) {
            return Ok(());
        }
        let number = thread.task.number;
        let handle = &participant.handle;
        let body = format!(
            "T-{number}'s turn may have lost its last line: a line of @{handle}'s record was lost (record {line}) in the turn, and its record has not said the turn ended with an answer. The line may have been its final answer, or the record that ends its turn. To wait for its answer, do nothing. If @{handle} is done, take its last complete answer as the result: {}; where it wrote none, send it a message about the task, or cancel it.",
            way_to_finish(thread)
        );
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: thread.task.requester.clone(),
                task: Some(number),
                body,
            },
        )?;
        record.held.borrow_mut().lost_said = Some(line);
        self.changed();
        Ok(())
    }
}

/// How the requester of `thread`'s task finishes it: the human from the task's drawer; the
/// chief, or a member that gave the task, with `cf` (Calliope on 0eec91d2).
fn way_to_finish(thread: &TaskThread) -> String {
    if thread.task.requester == "human" {
        "Finish in the task's drawer".to_owned()
    } else {
        format!("cf task finish T-{}", thread.task.number)
    }
}

/// What the latest look said, as a pass takes it: none yet is waiting, and so is a first
/// look that found a command still running (one more is made before the requester is
/// told of it).
fn watching_of(held: &HeldPart) -> Watching {
    match held.seen {
        None => Watching::Waiting,
        Some(CommandsSeen::Running(running)) if running > 0 && held.running_looks < 2 => {
            Watching::Waiting
        }
        Some(seen) => Watching::Seen(seen),
    }
}

/// What a held task's requester is told it is held for: what its worker's turn left
/// running that its harness wakes it for, the monitors among it, and what the last look
/// at the window's commands found, where one was made ([`HeldPart::seen`]).
pub(crate) struct Held {
    pending: Pending,
    monitors: Vec<Monitor>,
    seen: Option<CommandsSeen>,
}

/// How many of `monitors` watch a command and said they ended.
fn noticed_of(monitors: &[Monitor]) -> usize {
    monitors
        .iter()
        .filter(|monitor| monitor.source == Source::Command && monitor.notice.is_some())
        .count()
}

/// What a note of a held task says of its monitors, after a sentence of its own: each by
/// its name and what came of it, then what keeps those whose notice came, and where the
/// window's commands were looked at, what was found. Nothing where there is none.
fn monitors_said(handle: &str, held: &Held) -> String {
    if held.monitors.is_empty() {
        return String::new();
    }
    let each: Vec<String> = held
        .monitors
        .iter()
        .map(|monitor| {
            let name = format!("\u{201c}{}\u{201d}", monitor.description);
            let Some(notice) = &monitor.notice else {
                return format!("{name} watches still");
            };
            let at = notice
                .at
                .as_deref()
                .and_then(|at| at.get(11..19))
                .map(|time| format!(" at {time} UTC"))
                .unwrap_or_default();
            // What the record says, which may be a command's line that says the same
            // (Astraeus on 472b1d96): reported, not verified.
            match notice.end {
                End::Expired => format!("{name} said its time ran out{at}"),
                End::Stopped => format!("{name} said it was stopped{at}"),
                End::Closed => format!("{name} said its socket closed{at}"),
            }
        })
        .collect();
    let mut said = format!(" Its monitors: {}.", each.join("; "));
    if noticed_of(&held.monitors) > 0 {
        said.push_str(&format!(
            " A monitor of a command that said it ended is over once no command of @{handle}'s harness runs in its window"
        ));
        said.push_str(&match held.seen {
            Some(CommandsSeen::Running(running)) => format!(
                ", and {running} still {} there.",
                if running == 1 { "runs" } else { "run" }
            ),
            Some(CommandsSeen::Untold) => {
                ", which ConsensFlow cannot see on this machine: it waits for Finish.".to_owned()
            }
            Some(CommandsSeen::Unreadable) => {
                ", and a process of its window runs that ConsensFlow cannot read (another user's): it looks again."
                    .to_owned()
            }
            Some(CommandsSeen::Unsettled) => {
                ", and its window's processes kept changing while ConsensFlow read them: it looks again."
                    .to_owned()
            }
            Some(CommandsSeen::Unanswered) => {
                ", and the pane host did not answer when ConsensFlow looked: it looks again."
                    .to_owned()
            }
            None => ", once nothing else of it is pending.".to_owned(),
        });
    }
    if held
        .monitors
        .iter()
        .any(|monitor| monitor.source == Source::Socket && monitor.notice.is_some())
    {
        said.push_str(" A monitor of a socket that said it ended has no command in the window to look for: it ends by Finish alone.");
    }
    if held
        .monitors
        .iter()
        .any(|monitor| monitor.source == Source::Server && monitor.notice.is_some())
    {
        said.push_str(" A monitor that said it ended and whose call named no command or socket ConsensFlow reads has nothing in the window to look for: it ends by Finish alone.");
    }
    said
}

/// What a reading counts of what a turn left, in the words of a note: what runs, with
/// `woken` after it (what the harness does as each ends), and apart from it the monitors
/// that said they ended (`Pending::noticed`), which may be a command's line that says so.
pub(crate) fn counted(pending: Pending, woken: &str) -> String {
    let runs = Pending {
        jobs: pending.jobs.saturating_sub(pending.noticed),
        noticed: 0,
        ..pending
    };
    let mut said = Vec::new();
    if runs.total() > 0 {
        said.push(format!("{} running{woken}", running(runs)));
    }
    if pending.noticed > 0 {
        said.push(match pending.noticed {
            1 => "1 monitor that said it ended".to_owned(),
            count => format!("{count} monitors that said they ended"),
        });
    }
    said.join(", and ")
}

/// What a turn left running, in the words of a note.
pub(crate) fn running(pending: Pending) -> String {
    let jobs = |count: usize| {
        let (job, jobs) = ("background job", "background jobs");
        format!("{count} {}", if count == 1 { job } else { jobs })
    };
    let agents = |count: usize| {
        let (agent, agents) = (
            "background agent or workflow",
            "background agents or workflows",
        );
        format!("{count} {}", if count == 1 { agent } else { agents })
    };
    match (pending.jobs, pending.agents) {
        (jobs_running, 0) => jobs(jobs_running),
        (0, agents_running) => agents(agents_running),
        (jobs_running, agents_running) => {
            format!("{} and {}", jobs(jobs_running), agents(agents_running))
        }
    }
}

/// What a result says, after the worker's words, of the work its window's harness knew of
/// and no longer waits on (dip-agentos #8: these were notes of their own): what was still
/// running when someone finished the task (`finished_by`), which finishing ends none of;
/// and the jobs an earlier window of the conversation left that no word says ended
/// (`Pending::retired`). Finished in a turn still under way (`settled` false), what it
/// counted is what the record had said so far, and the result says so. Nothing where there
/// is nothing to say. What the host finds left in the window is not here: the window's
/// close says what it ended.
pub(crate) fn leftovers(
    handle: &str,
    pending: Pending,
    finished_by: Option<&str>,
    settled: bool,
) -> String {
    let mut said = Vec::new();
    if let Some(by) = finished_by {
        let who = if by == "human" {
            "the human".to_owned()
        } else {
            format!("@{by}")
        };
        if pending.total() > 0 {
            let what = if pending.total() == 1 { "it" } else { "them" };
            said.push(format!(
                "Finished at {who}'s word while @{handle}'s harness still counted {}, which finishing ends none of: {what} may run on until its window closes, or past it where the host cannot tell what is the window's.",
                counted(pending, "")
            ));
        } else {
            said.push(format!(
                "Finished at {who}'s word: what @{handle} said last is the result."
            ));
        }
        if !settled {
            said.push(format!(
                "@{handle}'s turn was still under way, so that is what its record had said so far: a job its turn starts from now on is not counted, nor are the background agents or workflows it left, which its harness counts only as a turn ends."
            ));
        }
    }
    if pending.retired > 0 {
        let jobs = if pending.retired == 1 {
            "1 background job".to_owned()
        } else {
            format!("{} background jobs", pending.retired)
        };
        said.push(format!(
            "no word came that the {jobs} an earlier window of @{handle}'s conversation started had ended; that window was closed, which ends what the host could tell was the window's, and may not have ended a job its shell detached."
        ));
    }
    said.join(" ")
}

/// `words`, and after them what ConsensFlow says of the leftovers (`said`), where it says
/// anything: a line apart, marked as ConsensFlow's.
pub(crate) fn with_leftovers(words: String, said: &str) -> String {
    if said.is_empty() {
        words
    } else {
        format!("{words}\n\n(ConsensFlow: {said})")
    }
}

/// The first line of the window's record lost in its turn, by the place among
/// the records it stood at. The turn is what follows the paste that began it,
/// the item `pasted`: where the reading says the record each item is of, a line
/// past that paste's own record is the turn's, and nothing is kept, so a
/// restart or a resume loses nothing; a record with no paste in it has no turn
/// to mark. A line lost there may have been the final answer itself, or a word
/// that a job it waits on ended: the result is marked whatever the turn came to.
/// Devin's reading cannot say (its items are its store's, its lost lines its
/// wire log's): the count taken at the last hand-over stands in, and where there
/// is none (the daemon started since) every line counts, so a result is marked
/// rather than taken whole in silence.
pub(crate) fn lost_in_turn(
    record: &Record,
    observed: &Observed,
    pasted: Option<usize>,
) -> Option<usize> {
    let lost = observed.lost();
    let Some(places) = observed.places() else {
        let before = record
            .delivery
            .borrow()
            .lost_before
            .map_or(0, |(_, lost)| lost);
        return lost.get(before).copied();
    };
    let began = *places.get(pasted?)?;
    lost.iter().copied().find(|&line| line > began)
}

/// The item of the last paste the window's record shows: where its latest turn
/// of ConsensFlow's began. A compaction's summary that quotes a header is none, as
/// [`Dispatcher::delivered_at`] reads it (Calliope's P3 (e), T-624).
pub(crate) fn last_paste(observed: &Observed) -> Option<usize> {
    observed.items().iter().rposition(|item| {
        item.role == Role::User
            && !item.compaction
            && header_id(&item.text).is_some_and(|id| is_paste_of(&item.text, id, None))
    })
}

/// `words`, and after them the line that says a line of the record of `handle`'s window
/// was lost in the turn (`lost`, its record): the result may have a hole. Ahead of what
/// ConsensFlow says of the leftovers ([`with_leftovers`]).
pub(crate) fn with_lost(words: String, handle: &str, lost: Option<usize>) -> String {
    match lost {
        Some(lost) => format!(
            "{words}\n\nA line of @{handle}'s record was lost (record {lost}); this result may miss part of the turn."
        ),
        None => words,
    }
}

/// The words of a final answer, in the items it was written in: each trimmed, a blank
/// line between, what a harness marks as its progress left out. Where the member wrote
/// none, what says so.
pub(crate) fn words_of(final_run: &[&Item]) -> String {
    let words: Vec<&str> = final_run
        .iter()
        .filter(|item| !item.commentary)
        .map(|item| js::trim(&item.text))
        .filter(|text| !text.is_empty())
        .collect();
    if words.is_empty() {
        WITHOUT_ANSWER.to_owned()
    } else {
        words.join("\n\n")
    }
}

/// What a worker's turn left pending, as a pass takes it: the monitors that said they ended
/// are over where a look found no command in their window ([`Watching`]). A working task's
/// turn ([`Dispatcher::collect`]) and a paused task's ([`paused`]) are held by the same count.
pub(crate) fn pending_after_look(observed: &Observed, watching: Watching) -> Pending {
    let mut pending = observed.pending();
    if watching == Watching::Seen(CommandsSeen::Running(0)) {
        pending.jobs = pending.jobs.saturating_sub(pending.noticed);
        pending.noticed = 0;
    }
    pending
}

mod finish;
mod paused;

pub use finish::Finished;

#[cfg(test)]
mod tests;

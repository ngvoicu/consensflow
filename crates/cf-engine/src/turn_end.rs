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
//! `end_turn` and its boundary record, Pi's `stop`, Devin's
//! last reply of a request, Codex's `final_answer`), and the progress it writes on the way
//! is no complete answer. What the harness wrote itself is no answer and cuts none: the
//! summary of a compaction, a notification it keeps in its record alone, Claude Code's
//! close of a cut turn. A turn that wrote nothing after its last tool call gives the last
//! words it wrote before it in that turn (the chief's decision on T-262: a result is never
//! empty), and one that wrote no words at all says so ([`WITHOUT_ANSWER`]). A tell's turn is
//! told on the words after its last tool call alone ([`paused`], T-878).
//!
//! **Pending work keeps the task working** (A2; dip-agentos #1, poker-lab's T-442):
//! Claude Code lets a command run in the background, or a Monitor watch one, and its
//! agent end its turn, and begins a turn by itself for each event and each end, which
//! wakes the agent; it counts the background agents and workflows it wakes the agent for
//! itself. A turn that ended with any of those pending is no result: the task stays
//! working, nothing is ended, and whoever gave the task is told once that it is still
//! working in the background, in one sentence that says what runs (by kind and count, and
//! what the reader knows of each command: since when it runs, and that a time limit moved
//! it into the background) and that the result comes when they end; then the one other
//! choice, to take what the worker said last now ([`Dispatcher::finish_task`]); then the
//! worker's words. The hold is shown to the chief, with a typed reminder after an hour in
//! one stretch and every three hours thereafter; a worker's own re-holds do not restart it
//! (CLN-76, T-1598). A hold that turns into one nothing but Finish ends (every job left is a
//! monitor that said it ended and has no command of the window to look for, or the host
//! cannot see the window's commands) is told once more, in the words of that
//! ([`cannot_end_by_itself`]). A job that ends or begins, a monitor's notice, and what a
//! look at the window found write no note of their own: the result is told when it comes, a hold that
//! ends with no result (the window gone, the task failed or paused) in the notes of those,
//! and a task the requester tells in the answer. Only the harness's own word that each is
//! over clears it (`cf-harness`' `claude::record::jobs`): no time that passes releases a task held so,
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
//! **A turn that ended and was not the result** (T-1204; the field board's T-837 and T-1200
//! of T-1219): the result is the final answer of the last turn, so a turn that ended with a
//! complete answer which no result took, because its task waited for an answer to the worker's
//! own question, because its end was held for work that runs, because Claude Code's queue held
//! a notice that began the next turn the moment it ended, or because notes were on their way to
//! the window, lost its words when the next message to the window came. Nothing is completed at
//! that end, and the result, when it is recorded, begins with the words of those turns, each
//! labelled with what it was written before ([`earlier_turns`]); Finish gives the same.
//!
//! **A window has one task** (`ownership`): no other task's message goes into a window
//! once one task's has, whatever ended that task; so no later task's turn is woken by
//! what an earlier one left, and no later task is given an earlier task's words.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use cf_base::{js, time};
use cf_harness::contract::Observed;
use cf_harness::records::{Command, Item, Monitor, Pending, Reading, Role, Source};
use cf_ledger::{NewNote, NewQuestion, ParticipantView, ProjectView, TaskThread};
use cf_proto::ledger::MessageView;
use serde_json::Value;

use crate::delivery_text::{header_id, is_paste_of};
use crate::dispatcher::Dispatcher;
use crate::image_facts::for_task;
use crate::record::Record;
use crate::role_change::{answers_to_the_task, Run};
use crate::seams::{CommandsSeen, EngineError};

use self::asks::asks_the_chief;
use self::earlier_turns::with_earlier;

/// What a result says above the words the never-empty rule took (T-262): its turn wrote none
/// after its last tool call, and these are the last it wrote before it, which may be a line of
/// its progress and not a report (T-1310's review of T-878, T-1311). In the plain voice of the
/// labels of the turns that ended before (`earlier_turns`). cf-evals' long run judges the
/// result with the same line (`long_run::invariants::order`), held to this one by a test.
pub(crate) const LAST_WORDS: &str =
    "Its turn ended with no words after its last command. The last words it wrote:";

/// What a result says of a turn that wrote no words at all, and what a tell's turn says
/// after its last tool call where it wrote none there.
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
    /// Over, and its final answer asks the chief what to do, or says it waits for the
    /// chief's answer: put to the chief as a question, from the worker, and the task waits.
    Asked,
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
    /// What the task's requester was told of its hold ([`Told`]): once for the hold, whatever
    /// its jobs do and its turns say on the way (T-1181); a second time only if the hold turns
    /// into one that cannot end by itself.
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
    /// The task whose requester was told another program shows its window's
    /// conversation ([`Dispatcher::shared`]): told once.
    told_shared: Option<i64>,
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
        self.told_shared = None;
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

/// What a held task's requester was told: the task, and whether the hold was one that
/// cannot end by itself ([`cannot_end_by_itself`]). The count of what is pending, its
/// monitors, their notices and what a look found are not told again (T-1181: T-298 sent its
/// chief 12 notes in 1 h 46 as its jobs began and ended, and kluris' T-104 and T-105 about ten
/// while they waited for Windows runs, each a turn of the chief's with nothing to do).
///
/// A hold that can end by itself is shown, not typed (RS-25), and `aside` is that note.
/// A new worker turn that holds again changes its line and counts as a re-hold, but starts
/// no new hour and writes no note. `exchanges` distinguishes a real message to or from the
/// worker from a harness's own wake-up; the first hold after an exchange starts a stretch.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Told {
    task: i64,
    stuck: bool,
    turn: Option<Arc<str>>,
    aside: Option<i64>,
    exchanges: HashSet<i64>,
}

/// How long a task has held since its first hold after an exchange before the chief is
/// typed a reminder, regardless of how often the worker woke and held again.
pub(crate) const STILL_WORKING_REMINDED_AFTER_MS: i64 = 60 * 60_000;

/// How long after a typed reminder another is due while that stretch continues.
const STILL_WORKING_REMIND_AGAIN_AFTER_MS: i64 = 3 * 60 * 60_000;

/// Messages actually given to the worker, and its messages to its requester that are on
/// their way or received. Shown notes and harness wake-ups are not an exchange. Include
/// every qualifying id so a previously queued message delivered out of order is seen too.
fn exchanges(thread: &TaskThread, worker: &str) -> HashSet<i64> {
    thread
        .messages
        .iter()
        .filter(|message| {
            let received = message.recipient == worker
                && message.sender.is_some()
                && matches!(message.state.as_str(), "delivered" | "read");
            let sent = message.sender.as_deref() == Some(worker)
                && message.recipient == thread.task.requester
                && !matches!(message.state.as_str(), "gated" | "cancelled" | "failed");
            received || sent
        })
        .map(|message| message.id)
        .collect()
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
        // A task that reads working while the words that resumed it wait for the window's
        // rest (`resumed_at_work`) takes no result: the turn at work is the one before them,
        // and the answer to them is the result. It is handled as the queued task it was.
        let waiting = match &task {
            Some(thread) if thread.task.state == "working" => {
                self.resume_words_wait(project.id, thread.task.number)?
            }
            _ => false,
        };
        let Some(thread) = task.filter(|thread| thread.task.state == "working" && !waiting) else {
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
        if observed.shared {
            self.shared(project, participant, record, &thread)?;
            return Ok(Turn::Open);
        }
        if observed.failed {
            // A failed turn holds its task for nothing it left (T-532's P3-3).
            record.held.borrow_mut().reset();
            let why = self.why_it_failed(participant, record, observed).await?;
            // The task may have moved on while the window was looked at.
            let still = self
                .seams
                .ledger
                .borrow()
                .active_task(participant.id, false)?
                .is_some_and(|now| now.task.id == thread.task.id && now.task.state == "working");
            if still && !self.forgotten(record) {
                self.fail_task(project, participant.id, &thread.task, &why.said, &why.key)?;
            }
            return Ok(Turn::Open);
        }
        let lost = lost_in_turn(record, observed, Some(start));
        if !observed.settled {
            self.tell_lost(project, participant, record, &thread, lost)?;
            self.looks_finished(
                project,
                participant,
                record,
                &thread,
                observed,
                &items[start + 1..],
            )?;
            return Ok(Turn::Open);
        }
        // Notes to the window about the task are on their way to it: words told before it
        // was up (the brief was in flight when they were told), or ConsensFlow's own that a
        // start let go of from a paste they rode in (Devin arrival gap 4). They go in next,
        // and the answer to them is the result. This answer is neither the result nor a
        // hold, and the requester is told nothing of it (field-306's B3; Calliope's L1 on
        // the merge, T-370).
        let told = self
            .seams
            .ledger
            .borrow()
            .notes_waiting(participant.id, thread.task.id)?;
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
            let monitors = observed.monitors();
            let stuck = cannot_end_by_itself(pending, &monitors, seen);
            let turn = answers.last.map(|last| Arc::clone(&last.id));
            // Typed at once if the hold cannot end by itself; otherwise shown, with a timed
            // reminder of the whole stretch (not one per worker turn).
            let exchanged = exchanges(&thread, &participant.handle);
            let (to_tell, told) = {
                let mut held = record.held.borrow_mut();
                held.holding = Some(hold);
                let mut told = held.told.clone().filter(|told| told.task == hold.task);
                if let Some(aside) = told.as_ref().and_then(|told| told.aside) {
                    let over = self
                        .seams
                        .ledger
                        .borrow()
                        .message(aside)?
                        .is_some_and(|message| message.receipt.get("over").is_some());
                    if over {
                        told = None;
                    }
                }
                let to_tell = match &told {
                    None => true,
                    Some(told) if stuck => !told.stuck,
                    Some(told) => told.aside.is_none(),
                };
                (to_tell, told)
            };
            let said = Held {
                pending,
                monitors,
                commands: observed.commands(),
                stuck,
            };
            // Where a look decides, the requester is told once it was made (Calliope on
            // 472b1d96).
            if to_tell && watching != Watching::Waiting {
                let aside = self.tell_it_goes_on(project, &thread, &said, &words)?;
                let mut held = record.held.borrow_mut();
                held.told = Some(Told {
                    task: hold.task,
                    stuck,
                    turn,
                    aside,
                    exchanges: exchanged,
                });
                held.lost_said = held.lost_said.or(lost);
            } else if let Some((aside, told)) = told
                .filter(|_| !to_tell && !stuck)
                .and_then(|told| told.aside.map(|aside| (aside, told)))
            {
                // Not told now, and not stuck: the hold was shown (`to_tell` else).
                let new_exchange = exchanged.iter().any(|id| !told.exchanges.contains(id));
                if new_exchange || told.turn != turn {
                    let said_now =
                        format!("in the background: {}", running_said(&said, self.now()));
                    if new_exchange {
                        self.seams
                            .ledger
                            .borrow_mut()
                            .restart_aside(project.id, aside, &said_now)?;
                    } else {
                        self.seams
                            .ledger
                            .borrow_mut()
                            .renew_aside(project.id, aside, &said_now)?;
                    }
                    self.changed();
                    if let Some(told) = record.held.borrow_mut().told.as_mut() {
                        told.turn = turn;
                        told.exchanges = exchanged;
                    }
                }
                self.remind_held(project, &thread, &said, aside)?;
            }
            return Ok(Turn::Held);
        }
        record.held.borrow_mut().reset();
        // Its last words ask the chief, or wait for its answer: they are a question, not
        // the work done. The task waits for the chief's `cf answer`, as for any question
        // its window asks; its window stays (T-1119).
        if asks_the_chief(&words_of(&answers.final_run)) {
            let body = with_finish_line(words, &participant.handle, thread.task.number);
            self.seams.ledger.borrow_mut().ask(
                project.id,
                &NewQuestion {
                    from: Some(participant.handle.clone()),
                    to: "chief".to_owned(),
                    body: Some(body),
                    task: Some(thread.task.number),
                    ..NewQuestion::default()
                },
            )?;
            self.changed();
            return Ok(Turn::Asked);
        }
        // The turns that ended with a question of the worker's out, whose words no result
        // took (T-1204): the result begins with them.
        let earlier = self.earlier_turns(
            &thread,
            participant,
            items,
            &replacements,
            &words_of(&answers.final_run),
        )?;
        let words = for_task(&thread.task, with_earlier(words, &earlier));
        let body = with_leftovers(words, &leftovers(&participant.handle, pending, None, true));
        self.seams
            .ledger
            .borrow_mut()
            .record_result(project.id, thread.task.number, &body)?;
        self.changed();
        Ok(Turn::Answered)
    }

    /// A worker's window whose conversation another program of its harness's has shown
    /// since the window opened (`Observed::shared`): a Claude Code the human started
    /// outside ConsensFlow on it, or the program of an earlier window that the host could
    /// not end. That one's turns are written into the same record, in
    /// records no different from the window's, so the record's answer after the task's
    /// message may be that one's: it is never the task's result. Its requester is told
    /// once how to take the answer they choose to; nothing is recorded and nothing ends.
    fn shared(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Record,
        thread: &TaskThread,
    ) -> Result<(), EngineError> {
        let number = thread.task.number;
        {
            let mut held = record.held.borrow_mut();
            if held.told_shared == Some(number) {
                return Ok(());
            }
            held.told_shared = Some(number);
        }
        let way = match thread.task.requester.as_str() {
            "human" => "Finish in the task's drawer takes the last answer".to_owned(),
            _ => format!("`cf task finish T-{number}` takes the last answer"),
        };
        let body = format!(
            "T-{number}: another Claude than @{}'s window has shown its conversation (one started outside ConsensFlow, or an earlier window's that never ended). Its turns are written into the same record, and nothing tells them from the window's, so no answer there is taken for T-{number}'s result. {way}.",
            participant.handle,
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
    ///
    /// Notes to the window about the task that wait to go in (`notes_waiting`: told words,
    /// or ConsensFlow's own) tell nothing: Finish refuses then (`words-on-their-way`), and
    /// their own path speaks for them (the chief's call on Calliope's P3-6, T-621). A job
    /// still running is no finished turn, though Claude Code says it waits at its prompt
    /// (its `shell` status reads idle): the count of what the turn left running stands
    /// between them (her P3-5).
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
            }
            && !self
                .seams
                .ledger
                .borrow()
                .notes_waiting(participant.id, thread.task.id)?;
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
        // The time the engine first saw the turn look finished (`quiet`), the pass that
        // first found it so: not Claude Code's own time of its status, which the look does
        // not carry. The engine knows no zone of the human's: it is said in UTC, as its
        // notes' times are.
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
        self.paste_of(&latest, items)
    }

    /// The place among `items`, the window's record, of the paste of `message`: the item its
    /// arrival was proved by (`receipt`), or, where the receipt names none of the record's,
    /// the last item that is its paste (see [`Dispatcher::delivered_at`]). None where the
    /// record holds no proof of it.
    pub(crate) fn paste_of(
        &self,
        message: &MessageView,
        items: &[Item],
    ) -> Result<Option<usize>, EngineError> {
        let receipt = message.receipt.get("item").and_then(Value::as_str);
        if let Some(found) = receipt.and_then(|item| {
            items
                .iter()
                .rposition(|said| said.role == Role::User && &*said.id == item)
        }) {
            return Ok(Some(found));
        }
        let first = self.brief_given_first(message)?;
        Ok(items.iter().rposition(|item| {
            item.role == Role::User
                && !item.compaction
                && is_paste_of(&item.text, message.id, first.as_deref())
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
            // Another said it ended: the next pass looks, whatever the wait was, and what the
            // last look found says nothing of its command (a look kept through a pause is of
            // the monitors there were, T-442).
            let mut held = record.held.borrow_mut();
            held.seen = None;
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

    /// Whoever gave the task is told it is still working in the background, once for the
    /// hold, and once more where the hold turns into one that cannot end by itself
    /// ([`HeldPart`], [`cannot_end_by_itself`]): in one sentence what runs, by kind (commands,
    /// agents, monitors), with what is known of each command, and that the result comes
    /// when they end, then the requester's other choice: to take its last complete answer as
    /// the result now ([`Dispatcher::finish_task`]); then the worker's last words. Later
    /// changes of the hold (a job that ends, another that begins, a monitor's notice, what a
    /// look found) are told no more: the result is told when it comes (T-1181). What the
    /// engine keeps goes with the window and with the daemon: a task held in a window opened
    /// again, or after a restart, is told again.
    fn tell_it_goes_on(
        &self,
        project: &ProjectView,
        thread: &TaskThread,
        held: &Held,
        words: &str,
    ) -> Result<Option<i64>, EngineError> {
        let number = thread.task.number;
        let finish = finish_with(thread);
        let running = running_said(held, self.now());
        let ends = if held.pending.total() == 1 {
            "it ends"
        } else {
            "they end"
        };
        let body = if held.stuck {
            format!(
                "T-{number} is still working in the background ({running}); ConsensFlow cannot tell when {ends}, so its result will not come by itself. {finish} takes what it said last.\n\nWhat it said:\n\n{words}"
            )
        } else {
            format!(
                "T-{number} is still working in the background ({running}); its result comes when {ends}. {finish} takes what it said last now.\n\nWhat it said:\n\n{words}"
            )
        };
        let note = NewNote {
            from: None,
            to: thread.task.requester.clone(),
            task: Some(number),
            body,
        };
        // One that cannot end by itself asks for the requester's Finish: typed. Any other
        // asks nothing: shown on the task's line, not typed, to a chief (RS-25).
        let aside = if held.stuck {
            self.seams.ledger.borrow_mut().note(project.id, &note)?;
            None
        } else {
            let shown = self.seams.ledger.borrow_mut().show(
                project.id,
                &note,
                &format!("in the background: {running}"),
            )?;
            Some(shown.id)
        };
        self.changed();
        Ok(aside)
    }

    /// Remind of the stretch after an hour and every three hours after that. The receipt
    /// keeps the start, re-holds and last reminder; another look can never send it twice.
    fn remind_held(
        &self,
        project: &ProjectView,
        thread: &TaskThread,
        held: &Held,
        aside: i64,
    ) -> Result<(), EngineError> {
        let Some(shown) = self.seams.ledger.borrow().message(aside)? else {
            return Ok(());
        };
        let shown_aside = shown
            .receipt
            .get(cf_ledger::SHOWN)
            .is_some_and(|said| said.is_string());
        if !shown_aside || shown.receipt.get("over").is_some() {
            return Ok(());
        }
        // From the first hold after the last exchange, else from the shown note itself.
        let since = shown
            .receipt
            .get("since")
            .and_then(Value::as_str)
            .unwrap_or(&shown.created_at);
        let Some(since) = time::parse(since) else {
            return Ok(());
        };
        let now = self.now();
        let previous = shown.receipt.get("reminded").and_then(Value::as_i64);
        let due_from = if let Some(id) = previous {
            let mut reminded_at = shown
                .receipt
                .get("reminded_at")
                .and_then(Value::as_str)
                .and_then(time::parse);
            if reminded_at.is_none() {
                reminded_at = self
                    .seams
                    .ledger
                    .borrow()
                    .message(id)?
                    .and_then(|message| time::parse(&message.created_at));
            }
            let Some(reminded_at) = reminded_at else {
                return Ok(());
            };
            reminded_at + STILL_WORKING_REMIND_AGAIN_AFTER_MS
        } else {
            since + STILL_WORKING_REMINDED_AFTER_MS
        };
        if now < due_from {
            return Ok(());
        }
        let number = thread.task.number;
        let minutes = (now - since) / 60_000;
        let long = if minutes < 120 {
            format!("{minutes} minutes")
        } else {
            format!("{} h {} min", minutes / 60, minutes % 60)
        };
        let running = running_said(held, now);
        let finish = finish_with(thread);
        let reholds = shown
            .receipt
            .get("reholds")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let times = if reholds == 1 { "time" } else { "times" };
        let body = format!(
            "T-{number} has been working in the background for {long}, holding again {reholds} {times} ({running}). Its result comes when that ends; {finish} takes what it said last now."
        );
        let sent = self.seams.ledger.borrow_mut().remind_of_aside(
            project.id,
            aside,
            shown
                .receipt
                .get("stretch")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            previous,
            &NewNote {
                from: None,
                to: thread.task.requester.clone(),
                task: Some(number),
                body,
            },
        )?;
        if sent.is_some() {
            self.changed();
        }
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
/// running that its harness wakes it for, the monitors and the commands among it as the
/// reading says them, and whether the hold cannot end by itself ([`cannot_end_by_itself`]).
pub(crate) struct Held {
    pending: Pending,
    monitors: Vec<Monitor>,
    commands: Vec<Command>,
    stuck: bool,
}

/// Whether the hold is one that no pass of the engine ends: every job left is a monitor that
/// said it ended, and none of them has a command of the window to look for (a socket's, or one
/// whose call named nothing ConsensFlow reads), or the host cannot see the window's commands
/// on this machine (`CommandsSeen::Untold`). Its requester ends it with Finish, and is told
/// so, the one case where a hold's change is told after its first note (T-1181).
fn cannot_end_by_itself(
    pending: Pending,
    monitors: &[Monitor],
    seen: Option<CommandsSeen>,
) -> bool {
    pending.total() > 0
        && pending.total() == pending.noticed
        && (noticed_of(monitors) < pending.noticed || seen == Some(CommandsSeen::Untold))
}

/// How the requester of `thread`'s task finishes it, as a sentence begins with it: the human
/// from the task's drawer; the chief, or a member that gave the task, with `cf`.
fn finish_with(thread: &TaskThread) -> String {
    if thread.task.requester == "human" {
        "Finish in the task's drawer".to_owned()
    } else {
        format!("`cf task finish T-{}`", thread.task.number)
    }
}

/// What a held task's note says is running, in the words of a sentence: the commands, the
/// agents and the monitors, each by its count and kind, then what is known of the commands
/// (`now` is the time the note is written, to say a day apart from it).
fn running_said(held: &Held, now: i64) -> String {
    let monitors = held.monitors.len().min(held.pending.jobs);
    let commands = held.pending.jobs - monitors;
    let mut parts = Vec::new();
    if commands > 0 {
        parts.push(commands_said(&held.commands, commands, now));
    }
    if held.pending.agents > 0 {
        parts.push(counted_kind(held.pending.agents, "agent", "agents"));
    }
    if monitors > 0 {
        let mut said = counted_kind(monitors, "monitor", "monitors");
        match held.pending.noticed {
            0 => {}
            noticed if noticed >= monitors && monitors == 1 => said.push_str(" that said it ended"),
            noticed if noticed >= monitors => said.push_str(" that said they ended"),
            1 => said.push_str(", 1 of them said it ended"),
            noticed => said.push_str(&format!(", {noticed} of them said they ended")),
        }
        parts.push(said);
    }
    // A part that carries a comma of its own is told apart from the next by a semicolon.
    let apart = if parts.iter().any(|part| part.contains(',')) {
        "; "
    } else {
        ", "
    };
    parts.join(apart)
}

/// "1 command", "3 agents".
fn counted_kind(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// `count` commands, and what the reading knows of them where it knows each (`known`: Claude
/// Code's): since when the one runs, or the oldest, and that a time limit moved it, or how many
/// of them, into the background. A reading that says none of them is a bare count.
fn commands_said(known: &[Command], count: usize, now: i64) -> String {
    let mut said = counted_kind(count, "command", "commands");
    if known.len() != count {
        return said;
    }
    if let Some(since) = known.iter().filter_map(|command| command.since).min() {
        let when = clock(since, now);
        if count == 1 {
            said.push_str(&format!(", running since {when}"));
        } else {
            said.push_str(&format!(", the oldest running since {when}"));
        }
    }
    let moved: Vec<i64> = known
        .iter()
        .filter_map(|command| command.limit_ms)
        .collect();
    match (count, moved.as_slice()) {
        (_, []) => {}
        (1, [limit]) => said.push_str(&format!(
            ", moved to the background when it passed its {} limit",
            limit_said(*limit)
        )),
        (_, [limit]) => said.push_str(&format!(
            ", 1 of them moved to the background when it passed its {} limit",
            limit_said(*limit)
        )),
        (_, limits) => said.push_str(&format!(
            ", {} of them moved to the background when they passed their time limits",
            limits.len()
        )),
    }
    said
}

/// A time limit in a sentence: "10-minute" for whole minutes, else its seconds.
fn limit_said(ms: i64) -> String {
    if ms % 60_000 == 0 {
        format!("{}-minute", ms / 60_000)
    } else {
        format!("{}-second", (ms + 500) / 1_000)
    }
}

/// `at` (milliseconds since the epoch) as the engine's notes say a time: in UTC, its hour and
/// minute, and its date too where that is not the day of `now`.
fn clock(at: i64, now: i64) -> String {
    let (at, now) = (time::iso(at), time::iso(now));
    match (at.get(..10), at.get(11..16)) {
        (Some(day), Some(hour)) if now.get(..10) == Some(day) => format!("{hour} UTC"),
        (Some(day), Some(hour)) => format!("{day} {hour} UTC"),
        _ => at,
    }
}

/// How many of `monitors` watch a command and said they ended.
fn noticed_of(monitors: &[Monitor]) -> usize {
    monitors
        .iter()
        .filter(|monitor| monitor.source == Source::Command && monitor.notice.is_some())
        .count()
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

/// `words`, and after them the line that tells the chief this question was raised by
/// ConsensFlow from the last words of `handle`'s turn, and how to take them as the result
/// if they are (T-1119): `cf task finish` takes a waiting task's last words.
pub(crate) fn with_finish_line(words: String, handle: &str, number: i64) -> String {
    format!(
        "{words}\n\n(ConsensFlow: @{handle}'s turn ended on these words. If they are its result: `cf task finish T-{number}`.)"
    )
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

/// What a final answer is written in: its items, and whether they are its turn's last words,
/// taken where it wrote none after its last tool call (`role_change::Run`).
pub(crate) trait Words {
    fn items(&self) -> &[&Item];
    fn last_words(&self) -> bool {
        false
    }
}

impl Words for [&Item] {
    fn items(&self) -> &[&Item] {
        self
    }
}

impl<const N: usize> Words for [&Item; N] {
    fn items(&self) -> &[&Item] {
        self
    }
}

impl Words for Vec<&Item> {
    fn items(&self) -> &[&Item] {
        self
    }
}

impl Words for Run<'_> {
    fn items(&self) -> &[&Item] {
        self
    }

    fn last_words(&self) -> bool {
        self.last_words
    }
}

/// The words of a final answer, in the items it was written in: each trimmed, a blank
/// line between, what a harness marks as its progress left out, and [`LAST_WORDS`] above
/// them where they are its turn's last words. Where the member wrote none, what says so.
pub(crate) fn words_of(final_run: &(impl Words + ?Sized)) -> String {
    let words: Vec<&str> = final_run
        .items()
        .iter()
        .filter(|item| !item.commentary)
        .map(|item| js::trim(&item.text))
        .filter(|text| !text.is_empty())
        .collect();
    if words.is_empty() {
        WITHOUT_ANSWER.to_owned()
    } else if final_run.last_words() {
        format!("{LAST_WORDS}\n\n{}", words.join("\n\n"))
    } else {
        words.join("\n\n")
    }
}

/// The words a member wrote in reply to the question put to its window, before its first tool
/// call (`Answers::reply`), in the items it was written in: each trimmed, a blank line between.
/// Unlike [`words_of`] it keeps what a harness marks as progress (Codex marks the text it
/// writes beside a tool call so, where its phase says `commentary`), since the question is
/// what that text answers. None where there are none (T-1151).
pub(crate) fn reply_of(reply: &[&Item]) -> Option<String> {
    let words: Vec<&str> = reply
        .iter()
        .map(|item| js::trim(item.text.as_ref()))
        .filter(|text| !text.is_empty())
        .collect();
    (!words.is_empty()).then(|| words.join("\n\n"))
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

mod asks;
mod earlier_turns;
mod finish;
mod paused;

pub use finish::Finished;

#[cfg(test)]
mod tests;

//! The work of the tasks: a task held working by a window whose work was over
//! (1, A1), and a task done while its window's job still ran (2, A2).
//!
//! Whether a window is in a turn or at rest is its harness's own record's word:
//! Claude Code's read whole (an idle compaction is no turn), every other's from
//! the daemon's copy of it, whose last item ends a turn or does not. Claude Code's
//! own record is held to a moment only where it was read through it: a check that
//! needs it at a moment it was not read through is unresolved, whatever else is seen,
//! and what was read before a reading failed stays read. A job a
//! window holds is one its record started and has not heard end (Claude Code's
//! `run_in_background`, which wakes the worker when it ends, or a command a
//! harness moved to the background), and for a harness whose record says nothing
//! of jobs (all but Claude Code), what the pane host says its turn left running.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use cf_e2e::pattern;
use regex::Regex;

use super::{iso, plain, Break, Evidence, Settings, Unjudged};
use crate::clock;
use crate::long_run::ledger::{Item, Message, Participant, Task};
use crate::long_run::look::Look;
use crate::long_run::mask::{cut, Names};
use crate::long_run::record::{jobs_ended_in, jobs_started_in, Record, Stood};

/// ConsensFlow's note that a task held for its jobs was finished at someone's word
/// (the chief's, or the human's): `T-14 is finished at @chief's word: what it said
/// last is its result.`, written to whoever gave it before the result. Its jobs are
/// let go of, and a result taken while they run is what was asked for.
pub const FINISHED: &str =
    r"^T-(\d+) is finished at @[^\s']+'s word: what it said last is its result";

/// Where ConsensFlow's own deliveries begin, which quote what they like.
const DELIVERY: &str = "[ConsensFlow m-";

/// 1: a working task whose window came to rest and has done nothing since, holding
/// no job, for longer than the run lets it. One of a Claude Code window whose own
/// record was not read through the look is unresolved instead ([`idle_unread`]).
pub fn idle(look: &Look, settings: &Settings) -> Vec<Break> {
    idle_found(look, settings)
        .into_iter()
        .filter_map(|(found, unread)| unread.is_none().then_some(found))
        .collect()
}

/// 1, unresolved: a working task of a Claude Code window that the copy of its
/// conversation says came to rest past the idle time holding no job, whose own record
/// (the window's word on its turns and its jobs) was not read through the look.
pub fn idle_unread(look: &Look, settings: &Settings) -> Vec<Unjudged> {
    idle_found(look, settings)
        .into_iter()
        .filter_map(|(found, unread)| {
            let why = unread?;
            Some(Unjudged::new(
                1,
                found.key,
                format!(
                    "{}, as the copy of its conversation says; its own record was not read through the look ({why}): whether it was at rest cannot be judged",
                    found.said
                ),
            ))
        })
        .collect()
}

/// The breaks of the first, each with why its window's own record was not held to it:
/// a Claude Code window's not read through the look, the copy of its conversation read
/// instead.
fn idle_found(look: &Look, settings: &Settings) -> Vec<(Break, Option<String>)> {
    let working = look
        .ledger
        .tasks
        .iter()
        .filter(|task| task.state == "working" && !task.deleted);
    let names = Names::of(look);
    let mut found = Vec::new();
    for task in working {
        let Some(worker) = task.assignee.and_then(|id| look.ledger.participant(id)) else {
            continue;
        };
        let read = Look::is_claude(worker).then(|| record_by(look, worker, look.at, look.at));
        let own = read.as_ref().and_then(|read| read.as_ref().ok().copied());
        let unread = read.and_then(Result::err);
        let Some((rested, record)) = rested_at(look, worker, own, &names) else {
            continue;
        };
        let held = jobs_held(look, worker, look.at, own);
        if !held.is_empty() {
            continue;
        }
        // A task given to a window at rest is waited for from when it was given.
        let since = clock::millis(&task.updated_at).map_or(rested, |given| given.max(rested));
        if look.at - since < settings.idle {
            continue;
        }
        let minutes = (look.at - since) / 60_000;
        let screen = look
            .window_of(&worker.handle)
            .map(|window| window.tail.clone())
            .unwrap_or_default();
        found.push((
            Break::new(
                1,
                format!("1:T-{}:{since}", task.number),
                format!(
                    "T-{} is working, and @{}'s window has been at rest for {minutes} min with no job held",
                    task.number, worker.handle
                ),
            )
            .with(Evidence {
                rows: vec![
                    row_of(task, worker, &names),
                    format!("at rest since {}", iso(rested)),
                ],
                screen,
                record,
            }),
            unread,
        ));
    }
    found
}

/// `worker`'s own record of its conversation at `at`, where the run read it through
/// `until`; else why it did not.
fn record_by<'l>(
    look: &'l Look,
    worker: &Participant,
    at: i64,
    until: i64,
) -> Result<&'l Record, String> {
    let conversation = look
        .conversation_at(worker.id, at)
        .ok_or_else(|| "its record was never read".to_owned())?;
    look.record_through(conversation, until)
}

/// 2: a result taken while a job of the worker's Claude Code window, which its
/// harness wakes the worker for, still ran; unless someone finished the task. A
/// job that ended just before the result has its notification written a moment
/// after it: one written within the settle time after the result says the job had
/// ended, and the check waits that long before it judges. A job the worker's own record
/// alone names is held to it only where the record was read through that time; where it
/// was not, the result is unresolved ([`unread_records`]).
pub fn done_while_job(look: &Look, settings: &Settings) -> Vec<Break> {
    let names = Names::of(look);
    let mut found = Vec::new();
    for result in look
        .ledger
        .messages
        .iter()
        .filter(|message| message.kind == "result")
    {
        let (Some(worker), Some(task), Some(at)) = (
            result.sender.and_then(|id| look.ledger.participant(id)),
            result.task.and_then(|id| look.ledger.task(id)),
            clock::millis(&result.created_at),
        ) else {
            continue;
        };
        if !Look::is_claude(worker) || look.at - at < settings.young {
            continue;
        }
        let own = record_by(look, worker, at, at + settings.settle).ok();
        let held: Vec<(String, String)> = jobs_held_by_record(look, worker, own, at, &names)
            .into_iter()
            .filter(|(id, _)| !ended_by(look, worker, id, at + settings.settle))
            .collect();
        if held.is_empty() || finished_before(look, task, result) {
            continue;
        }
        let mut record: Vec<String> = held.iter().map(|(_, line)| line.clone()).collect();
        record.push(format!(
            "no notification of its end by {}, {} s after the result",
            iso(at + settings.settle),
            settings.settle / 1000
        ));
        found.push(
            Break::new(
                2,
                format!("2:m-{}", result.id),
                format!(
                    "T-{} was done by m-{} while @{}'s background job {} still ran",
                    task.number,
                    result.id,
                    worker.handle,
                    held.iter()
                        .map(|(id, _)| id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )
            .with(Evidence {
                rows: vec![
                    row_of(task, worker, &names),
                    format!(
                        "m-{} result at {}: {}",
                        result.id,
                        result.created_at,
                        cut(&plain(&result.body), 200, &names)
                    ),
                ],
                screen: Vec::new(),
                record,
            }),
        );
    }
    found
}

/// 2, unresolved: a result of a Claude Code worker's, unless someone finished its task,
/// whose worker's own record (where a job it left running would be:
/// `run_in_background`) was not read through its turn and the settle time after it: not
/// read at all, read only as far as before then (its latest reading failing), or
/// holding nothing as far as the turn's prompt. Whatever jobs the copy of its
/// conversation shows, one only the record names may have run on: whether one still
/// ran when the result was written cannot be judged.
pub fn unread_records(look: &Look, settings: &Settings) -> Vec<Unjudged> {
    let mut found = Vec::new();
    for result in look
        .ledger
        .messages
        .iter()
        .filter(|message| message.kind == "result")
    {
        let (Some(worker), Some(task), Some(at)) = (
            result.sender.and_then(|id| look.ledger.participant(id)),
            result.task.and_then(|id| look.ledger.task(id)),
            clock::millis(&result.created_at),
        ) else {
            continue;
        };
        if !Look::is_claude(worker)
            || look.at - at < settings.young
            || finished_before(look, task, result)
        {
            continue;
        }
        let items = look.items_by(worker.id, at);
        // The turn's prompt: the last delivery to the window by the result.
        let prompt = items
            .iter()
            .rev()
            .find(|item| item.role == "user" && item.text.starts_with(DELIVERY))
            .and_then(|item| item.millis())
            .unwrap_or(at);
        let why = match record_by(look, worker, at, at + settings.settle) {
            Err(why) => Some(why),
            Ok(record) => match record.last_said() {
                Some(said) if said >= prompt => None,
                Some(said) => Some(format!(
                    "its record holds nothing after {}, before the turn's prompt at {}",
                    iso(said),
                    iso(prompt)
                )),
                None => Some("its record said nothing yet".to_owned()),
            },
        };
        if let Some(why) = why {
            found.push(Unjudged::new(
                2,
                format!("2:m-{}", result.id),
                format!(
                    "m-{}'s turn is not in @{}'s own record ({why}): whether a background job still ran when it was written cannot be judged",
                    result.id, worker.handle
                ),
            ));
        }
    }
    found
}

/// When `worker`'s window came to rest, if it is at rest now, with the record's
/// last lines; none while a turn of it is at work, or where nothing says. Its own
/// record (`own`, read through the look) says, where it says anything; else the copy.
fn rested_at(
    look: &Look,
    worker: &Participant,
    own: Option<&Record>,
    names: &Names,
) -> Option<(i64, Vec<String>)> {
    if let Some(record) = own {
        if let Some((stood, entry)) = record.stood_at(look.at) {
            return (stood == Stood::AtRest)
                .then(|| (entry.at.unwrap_or(look.at), record.lines_by(look.at, 4)));
        }
    }
    let items = look.items_by(worker.id, look.at);
    let last = items.last()?;
    let at_rest = last.role == "assistant" && last.complete;
    at_rest.then(|| (last.millis().unwrap_or(look.at), lines_of(&items, 4, names)))
}

/// The jobs `worker`'s window holds at `at`: what its record (`own`, its own) and the
/// copy started and have not heard end, the background agents and workflows Claude Code
/// last said its turns left running (the product keeps the task working for them), and
/// for a harness whose record says nothing of jobs, what the pane host lists of its
/// turn's work.
fn jobs_held(look: &Look, worker: &Participant, at: i64, own: Option<&Record>) -> Vec<String> {
    let mut held: Vec<String> = jobs_held_by_record(look, worker, own, at, &Names::default())
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let agents = own.map_or(0, |record| record.agents_pending_at(at));
    if Look::is_claude(worker) && agents > 0 {
        held.push(format!("{agents} background agents or workflows"));
    }
    if !Look::is_claude(worker) {
        if let Some(window) = look.window_of(&worker.handle) {
            held.extend(
                window
                    .background
                    .iter()
                    .map(|process| format!("{} {}", process.pid, process.command)),
            );
        }
    }
    held
}

/// The jobs `worker`'s record started by `at` that it had not heard end by then,
/// each by its id with the line that started it (the run's `names` shown in it):
/// Claude Code's record read whole (`own`, where it was read far enough) and the
/// daemon's copy, whichever names it.
fn jobs_held_by_record(
    look: &Look,
    worker: &Participant,
    own: Option<&Record>,
    at: i64,
    names: &Names,
) -> Vec<(String, String)> {
    let items = look.items_by(worker.id, at);
    let mut started: Vec<(String, String)> = Vec::new();
    let mut ended: BTreeSet<String> = BTreeSet::new();
    for item in &items {
        if item.role == "tool" {
            for id in jobs_started_in(&item.text) {
                started.push((id, item_line(item, names)));
            }
        }
        if item.role == "user" && !item.text.starts_with(DELIVERY) {
            ended.extend(jobs_ended_in(&item.text));
        }
    }
    if let Some(record) = own {
        for entry in record.jobs_pending_at(at) {
            if let crate::long_run::record::Kind::JobStarted(id) = &entry.kind {
                started.push((id.clone(), entry.line()));
            }
        }
        for entry in &record.entries {
            if let crate::long_run::record::Kind::JobEnded(id) = &entry.kind {
                if entry.at.is_none_or(|said| said <= at) {
                    ended.insert(id.clone());
                }
            }
        }
    }
    let mut seen = BTreeSet::new();
    started
        .into_iter()
        .filter(|(id, _)| !ended.contains(id) && seen.insert(id.clone()))
        .collect()
}

/// Whether `worker`'s record or the copy of it says the job `id` ended by `at`.
fn ended_by(look: &Look, worker: &Participant, id: &str, at: i64) -> bool {
    let in_copy = look.items_by(worker.id, at).iter().any(|item| {
        item.role == "user"
            && !item.text.starts_with(DELIVERY)
            && jobs_ended_in(&item.text).iter().any(|ended| ended == id)
    });
    in_copy
        || look.record_of(worker.id).is_some_and(|record| {
            record.entries.iter().any(|entry| {
                entry.kind == crate::long_run::record::Kind::JobEnded(id.to_owned())
                    && entry.at.is_none_or(|said| said <= at)
            })
        })
}

/// Whether ConsensFlow's note that `task` was finished at someone's word came before
/// `result`, in the same attempt: after any result of the task before it.
fn finished_before(look: &Look, task: &Task, result: &Message) -> bool {
    static NOTE: OnceLock<Regex> = OnceLock::new();
    let messages = look.ledger.messages_of(task.id);
    let since = messages
        .iter()
        .filter(|message| message.kind == "result" && message.id < result.id)
        .map(|message| message.id)
        .max()
        .unwrap_or(0);
    messages.iter().any(|message| {
        message.sender.is_none()
            && message.kind == "note"
            && message.id > since
            && message.id < result.id
            && pattern::once(&NOTE, FINISHED)
                .captures(message.body.trim_start())
                .is_some_and(|said| said[1].parse() == Ok(task.number))
    })
}

/// A task's row as evidence, its title masked whole and then cut (the run's `names`
/// shown).
fn row_of(task: &Task, worker: &Participant, names: &Names) -> String {
    format!(
        "T-{} {} @{} ({}) since {}: {}",
        task.number,
        task.state,
        worker.handle,
        worker.harness.as_deref().unwrap_or("?"),
        task.updated_at,
        cut(&task.title, 80, names)
    )
}

/// An item of the copy as evidence, its words masked whole and then cut.
pub(super) fn item_line(item: &Item, names: &Names) -> String {
    format!(
        "{} {}{}: {}",
        item.at.as_deref().unwrap_or(&item.copied_at),
        item.role,
        if item.complete { " (ends a turn)" } else { "" },
        cut(&plain(&item.text), 160, names)
    )
}

/// The last `count` items, as evidence.
pub(super) fn lines_of(items: &[&Item], count: usize, names: &Names) -> Vec<String> {
    items[items.len().saturating_sub(count)..]
        .iter()
        .map(|item| item_line(item, names))
        .collect()
}

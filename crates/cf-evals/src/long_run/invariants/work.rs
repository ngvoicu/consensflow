//! The work of the tasks: a task held working by a window whose work was over
//! (1, A1), and a task done while its window's job still ran (2, A2).
//!
//! Whether a window is in a turn or at rest is its harness's own record's word:
//! Claude Code's read whole (an idle compaction is no turn), every other's from
//! the daemon's copy of it, whose last item ends a turn or does not. A job a
//! window holds is one its record started and has not heard end (Claude Code's
//! `run_in_background`, which wakes the worker when it ends, or a command a
//! harness moved to the background), and for a harness whose record says nothing
//! of jobs (all but Claude Code), what the pane host says its turn left running.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use cf_e2e::pattern;
use regex::Regex;

use super::{iso, plain, Break, Evidence, Settings};
use crate::clock;
use crate::long_run::ledger::{Item, Message, Participant, Task};
use crate::long_run::look::Look;
use crate::long_run::mask::{cut, Names};
use crate::long_run::record::{jobs_ended_in, jobs_started_in, Stood};

/// ConsensFlow's note that a task held for its jobs was finished at someone's word
/// (the chief's, or the human's): `T-14 is finished at @chief's word: what it said
/// last is its result.`, written to whoever gave it before the result. Its jobs are
/// let go of, and a result taken while they run is what was asked for.
pub const FINISHED: &str =
    r"^T-(\d+) is finished at @[^\s']+'s word: what it said last is its result";

/// Where ConsensFlow's own deliveries begin, which quote what they like.
const DELIVERY: &str = "[ConsensFlow m-";

/// 1: a working task whose window came to rest and has done nothing since, holding
/// no job, for longer than the run lets it.
pub fn idle(look: &Look, settings: &Settings) -> Vec<Break> {
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
        let Some((rested, record)) = rested_at(look, worker, &names) else {
            continue;
        };
        let held = jobs_held(look, worker, look.at);
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
        found.push(
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
        );
    }
    found
}

/// 2: a result taken while a job of the worker's Claude Code window, which its
/// harness wakes the worker for, still ran; unless someone finished the task. A
/// job that ended just before the result has its notification written a moment
/// after it: one written within the settle time after the result says the job had
/// ended, and the check waits that long before it judges.
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
        let held: Vec<(String, String)> = jobs_held_by_record(look, worker, at, &names)
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

/// When `worker`'s window came to rest, if it is at rest now, with the record's
/// last lines; none while a turn of it is at work, or where nothing says.
fn rested_at(look: &Look, worker: &Participant, names: &Names) -> Option<(i64, Vec<String>)> {
    if Look::is_claude(worker) {
        if let Some(record) = look.record_of(worker.id) {
            if let Some((stood, entry)) = record.stood_at(look.at) {
                return (stood == Stood::AtRest)
                    .then(|| (entry.at.unwrap_or(look.at), record.lines_by(look.at, 4)));
            }
        }
    }
    let items = look.items_by(worker.id, look.at);
    let last = items.last()?;
    let at_rest = last.role == "assistant" && last.complete;
    at_rest.then(|| (last.millis().unwrap_or(look.at), lines_of(&items, 4, names)))
}

/// The jobs `worker`'s window holds at `at`: what its record started and has not
/// heard end, the background agents and workflows Claude Code last said its turns
/// left running (the product keeps the task working for them), and for a harness
/// whose record says nothing of jobs, what the pane host lists of its turn's work.
fn jobs_held(look: &Look, worker: &Participant, at: i64) -> Vec<String> {
    let mut held: Vec<String> = jobs_held_by_record(look, worker, at, &Names::default())
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let agents = look
        .record_of(worker.id)
        .map_or(0, |record| record.agents_pending_at(at));
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
/// Claude Code's record read whole and the daemon's copy, whichever names it.
fn jobs_held_by_record(
    look: &Look,
    worker: &Participant,
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
    if let Some(record) = look.record_of(worker.id) {
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

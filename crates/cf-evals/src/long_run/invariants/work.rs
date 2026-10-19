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

use super::{iso, plain, Break, Evidence, Gap, Settings, Unjudged};
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
/// no job, for longer than the run lets it. One whose window's own record could not be
/// held to it, or whose window's state nothing tells, is unresolved instead
/// ([`idle_unread`]).
pub fn idle(look: &Look, settings: &Settings) -> Vec<Break> {
    let names = Names::of(look);
    working(look)
        .filter_map(|task| match rest_of(look, settings, &names, task)? {
            Judged::Broke(found, None) => Some(found),
            _ => None,
        })
        .collect()
}

/// 1, unresolved: a working task whose window's rest a look cannot judge. One of a Claude
/// Code window that the copy of its conversation says came to rest past the idle time
/// holding no job, whose own record (the window's word on its turns and its jobs) could
/// not be held to it: not read through the look, or no conversation of the window open.
/// Or one past the idle time since it was given and since its window's conversation
/// opened, holding no job, whose window's state neither its own record nor the copy of
/// that conversation tells.
///
/// It stays unresolved to the run's end, never judged again on a later reading: a moment
/// gone by, judged on what a later look reads, was cleared by what came after it (a turn,
/// a job begun since) or stood alone for the moments after it that went unjudged (the
/// reviews of rounds 13 and 14). One a task, keyed by the task alone: the first look's,
/// whose moment it names, blocks the run's PASS as more would. Its cost is an UNRESOLVED
/// run where a later reading would have told the rest held, never a PASS.
pub fn idle_unread(look: &Look, settings: &Settings) -> Vec<Unjudged> {
    let names = Names::of(look);
    working(look)
        .filter_map(|task| {
            let said = match rest_of(look, settings, &names, task)? {
                Judged::Broke(found, Some(why)) => format!(
                    "{}, as the copy of its conversation says; its own record could not be held to it ({why}): whether it was at rest cannot be judged",
                    found.said
                ),
                Judged::Untold(why) => why,
                Judged::Held | Judged::Broke(_, None) => return None,
            };
            Some(Unjudged::new(1, format!("1:T-{}", task.number), said))
        })
        .collect()
}

/// What a task's rest came to at a look.
enum Judged {
    /// Its window was in a turn, held a job, or rested no longer than the run lets it.
    Held,
    /// Its window rested longer, holding no job: the break, with why its own record could
    /// not be held to it where it could not (judged on the copy).
    Broke(Break, Option<String>),
    /// Neither its window's own record nor the copy of its conversation tells what the
    /// window did: what could not be judged, and why.
    Untold(String),
}

/// The tasks of the look at work.
fn working(look: &Look) -> impl Iterator<Item = &Task> {
    look.ledger
        .tasks
        .iter()
        .filter(|task| task.state == "working" && !task.deleted)
}

/// When `task` was given to its window, as its row says.
fn given(task: &Task) -> Option<i64> {
    clock::millis(&task.updated_at)
}

/// The first, of `task` at `look`: broken where its worker's window came to rest and did
/// nothing since, holding no job, for longer than the run lets it; held where it did not.
/// A Claude Code window is read from its own record of the conversation open at the look
/// (whatever the copy holds: the daemon may still be copying one that ended), read through
/// the look, and from the copy of that conversation where the record says nothing; where
/// the record could not be held to it (not read through the look, or no conversation open,
/// which leaves the task's turns untold), the break comes with why, judged on the copy.
/// Where neither tells what the window did, holding no job past the idle time since the
/// task was given and since that conversation opened, it is untold. None for a task no
/// worker holds.
fn rest_of(look: &Look, settings: &Settings, names: &Names, task: &Task) -> Option<Judged> {
    let worker = task.assignee.and_then(|id| look.ledger.participant(id))?;
    let at = look.at;
    let conversation = if Look::is_claude(worker) {
        look.conversation_open_at(worker.id, at)
    } else {
        None
    };
    let (own, unread) = if Look::is_claude(worker) {
        match conversation {
            Some(conversation) => match look.record_through(conversation, at) {
                Ok(record) => (Some(record), None),
                Err(why) => (None, Some(why)),
            },
            None => (
                None,
                Some(format!(
                    "no conversation of @{}'s was open then, and which holds T-{}'s turns cannot be told",
                    worker.handle, task.number
                )),
            ),
        }
    } else {
        (None, None)
    };
    let seen = rested_at(look, worker, own, conversation, at, names);
    if matches!(seen, Seen::AtWork) || !jobs_held(look, worker, at, own).is_empty() {
        return Some(Judged::Held);
    }
    let given = given(task);
    let Seen::AtRest(rested, record) = seen else {
        // A task given within the idle time was not left waiting longer, whatever its
        // window did; nor did a window rest in a conversation longer than it had been open.
        if given.is_some_and(|given| at - given < settings.idle) {
            return Some(Judged::Held);
        }
        if opened(look, conversation).is_some_and(|opened| at - opened < settings.idle) {
            return Some(Judged::Held);
        }
        let why = match unread {
            Some(why) => format!(
                "its own record could not be held to it ({why}), and the copy of its conversation holds nothing by then"
            ),
            None if Look::is_claude(worker) => {
                "neither its own record nor the copy of its conversation says what it did by then"
                    .to_owned()
            }
            None => "the copy of its conversation holds nothing by then".to_owned(),
        };
        return Some(Judged::Untold(format!(
            "T-{} is working, and whether @{}'s window was at rest cannot be judged: {why}",
            task.number, worker.handle
        )));
    };
    // A task given to a window at rest is waited for from when it was given.
    let since = given.map_or(rested, |given| given.max(rested));
    if at - since < settings.idle {
        return Some(Judged::Held);
    }
    let minutes = (at - since) / 60_000;
    let screen = look
        .window_of(&worker.handle)
        .map(|window| window.tail.clone())
        .unwrap_or_default();
    Some(Judged::Broke(
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
                row_of(task, worker, names),
                format!("at rest since {}", iso(rested)),
            ],
            screen,
            record,
        }),
        unread,
    ))
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
            found.push(
                Unjudged::new(
                    2,
                    format!("2:m-{}", result.id),
                    format!(
                        "m-{}'s turn is not in @{}'s own record ({why}): whether a background job still ran when it was written cannot be judged",
                        result.id, worker.handle
                    ),
                )
                .gap(Gap::Again),
            );
        }
    }
    found
}

/// When `conversation` opened, as its row says.
fn opened(look: &Look, conversation: Option<i64>) -> Option<i64> {
    let conversation = look
        .ledger
        .conversations
        .iter()
        .find(|row| Some(row.id) == conversation)?;
    clock::millis(conversation.started_at.as_deref()?)
}

/// What a window was doing at a moment, as its record or the copy says.
enum Seen {
    /// A turn of it was at work.
    AtWork,
    /// It was at rest, since then, with the record's last lines.
    AtRest(i64, Vec<String>),
    /// Nothing says.
    Nothing,
}

/// What `worker`'s window was doing at `at`. Its own record (`own`, read through `at`)
/// says, where it says anything; else the copy, of the `conversation` open then where
/// one is named.
fn rested_at(
    look: &Look,
    worker: &Participant,
    own: Option<&Record>,
    conversation: Option<i64>,
    at: i64,
    names: &Names,
) -> Seen {
    if let Some(record) = own {
        if let Some((stood, entry)) = record.stood_at(at) {
            return if stood == Stood::AtRest {
                Seen::AtRest(entry.at.unwrap_or(at), record.lines_by(at, 4))
            } else {
                Seen::AtWork
            };
        }
    }
    let mut items = look.items_by(worker.id, at);
    if let Some(conversation) = conversation {
        items.retain(|item| item.conversation == conversation);
    }
    match items.last() {
        None => Seen::Nothing,
        Some(last) if last.role == "assistant" && last.complete => {
            Seen::AtRest(last.millis().unwrap_or(at), lines_of(&items, 4, names))
        }
        Some(_) => Seen::AtWork,
    }
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

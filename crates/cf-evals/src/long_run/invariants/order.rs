//! What reaches the chief of a task, in its order and in its words: the notice
//! that what a worker's turn left running was ended comes with its result or
//! before it, never after (3, A3: a mutation run ended mid-edit, its notice m-132
//! after the result m-131); and a result is its turn's final answer whole, the
//! words after its last tool call, with none of the progress written on the way
//! (4, A4: results that held every progress line). Its turn is the one the delivery
//! attempt of its task that began it began, in that attempt's conversation, and no
//! further than the next delivery of another task: a turn of an earlier attempt (a
//! retry's turn lost, its result the old answer again) or of the next task (a late
//! result carrying another task's answer) is never taken for its own. A delivery is a
//! prompt that opens with the header of a message to the worker, read as B5 reads one
//! (`deliveries`): a header a brief, a job's notification or a tool's output quotes is
//! none, and neither begins nor ends a turn. Both read
//! the ledger alone: the messages, the log of the tasks' states and deliveries, and
//! the daemon's copy of the worker's conversation.
//!
//! A task may be worked more than once: done, sent back, and done again; and a
//! notice is of the attempt of its own result, whenever it is written. Only the log
//! tells an attempt from the next, by the moves of the task: when it was written is
//! no word of whose it is (the notice of an old attempt may come late, a moment
//! before the next attempt's result). A notice no result came before is of a result
//! to come, or of none: it holds. One after a result, the task having begun no
//! attempt since (no move but to `done` or an end), is of that result, and came after
//! it: a break. One after a result and after the task was sent back may be that
//! result's, late, or the next attempt's own, which the log does not say: it is
//! unresolved, said so once in the timeline, never a break and never taken to hold.

use super::deliveries::delivered_to;
use super::work::lines_of;
use super::{plain, Break, Evidence, Settings, Unjudged};
use crate::clock;
use crate::long_run::ledger::{Item, Ledger, Message};
use crate::long_run::look::Look;
use crate::long_run::mask::{cut, Names};

/// What an "ended" notice says, whoever's turn it was: the note of what a turn
/// left running (`T-14's turn left background work running. Ended: …`), and of a
/// task let go once its job was gone.
pub const ENDED: [&str; 2] = ["'s turn left background work running", " is let go: "];

/// The shape of a break of the third whose late notice is the background "ended" note
/// (`ENDED[0]`), and of one whose notice is the note of a task let go (`ENDED[1]`): a
/// list of known breaks tells them apart ([`crate::long_run::known`]).
pub const ENDED_NOTE: &str = "ended-note";
pub const LET_GO_NOTE: &str = "let-go-note";

/// The states a task is in once an attempt has its result: a move to any other
/// after a result begins another attempt.
const AFTER_A_RESULT: [&str; 4] = ["done", "accepted", "cancelled", "failed"];

/// How long a line of progress is before a result holding it counts: a word or
/// two is in any answer.
const PROGRESS: usize = 24;

/// Whose an ended notice is, as far as the log tells.
enum Whose<'l> {
    /// No result came before it: it is of a result to come, or of none.
    NoResultBefore,
    /// Of the result before it: the task began no attempt between them.
    Of(&'l Message),
    /// The task was sent back between the result before it and the notice: that
    /// result's, late, or the next attempt's own.
    Unbound(&'l Message),
}

/// The ended notices of the ledger's tasks: each with its task's row and number.
fn notices(ledger: &Ledger) -> impl Iterator<Item = (&Message, i64, i64)> {
    ledger
        .messages
        .iter()
        .filter(|message| {
            message.sender.is_none()
                && message.kind == "note"
                && ENDED.iter().any(|said| message.body.contains(said))
        })
        .filter_map(|notice| {
            let task = notice.task?;
            let number = ledger.task(task).map_or(0, |task| task.number);
            Some((notice, task, number))
        })
}

/// 3: an ended notice that came after its own result.
pub fn ended_after_result(look: &Look) -> Vec<Break> {
    let names = Names::of(look);
    let mut found = Vec::new();
    for (notice, task, number) in notices(&look.ledger) {
        let Whose::Of(result) = whose(&look.ledger, task, number, notice) else {
            continue;
        };
        found.push(
            Break::new(
                3,
                format!("3:m-{}", notice.id),
                format!(
                    "T-{number}'s ended notice m-{} came after its result m-{}",
                    notice.id, result.id
                ),
            )
            .with_shape(if notice.body.contains(ENDED[0]) {
                ENDED_NOTE
            } else {
                LET_GO_NOTE
            })
            .with(Evidence {
                rows: [result, notice]
                    .iter()
                    .map(|row| message_row(row, &names))
                    .collect(),
                ..Evidence::default()
            }),
        );
    }
    found
}

/// 3, unresolved: an ended notice after a result, written once the task had been sent
/// back since, which the log does not tie to that result or to the next.
pub fn unbound_notices(look: &Look) -> Vec<Unjudged> {
    notices(&look.ledger)
        .filter_map(|(notice, task, number)| match whose(&look.ledger, task, number, notice) {
            Whose::Unbound(result) => Some(Unjudged::new(
                3,
                format!("3:m-{}", notice.id),
                format!(
                    "T-{number}'s ended notice m-{} came after its result m-{} and after T-{number} was sent back: whether it is that result's, late, or the next attempt's own, the log does not say",
                    notice.id, result.id
                ),
            )),
            Whose::NoResultBefore | Whose::Of(_) => None,
        })
        .collect()
}

/// Whose `notice` (of the task whose row is `task`, T-`number`) is: by the last
/// result of the task before it, and whether the log says the task moved, between
/// that result and the notice, to a state of another attempt. A move whose time the
/// log does not order against the two (the same moment, or none read) may be between.
fn whose<'l>(ledger: &'l Ledger, task: i64, number: i64, notice: &Message) -> Whose<'l> {
    let Some(result) = ledger
        .messages_of(task)
        .into_iter()
        .rev()
        .find(|message| message.kind == "result" && message.id < notice.id)
    else {
        return Whose::NoResultBefore;
    };
    let (from, to) = (
        clock::millis(&result.created_at).unwrap_or(i64::MIN),
        clock::millis(&notice.created_at).unwrap_or(i64::MAX),
    );
    let sent_back = ledger.events_of("task.state").iter().any(|event| {
        event.data["task"].as_i64() == Some(number)
            && !event.data["to"]
                .as_str()
                .is_some_and(|state| AFTER_A_RESULT.contains(&state))
            && clock::millis(&event.at).is_none_or(|moved| from <= moved && moved <= to)
    });
    if sent_back {
        Whose::Unbound(result)
    } else {
        Whose::Of(result)
    }
}

/// The results a worker wrote that are old enough to be judged against the copy of
/// its conversation (`settings.young`): each with its worker and when it was written.
fn settled_results<'l>(
    look: &'l Look,
    settings: &Settings,
) -> impl Iterator<Item = (&'l Message, i64, i64)> + 'l {
    let young = settings.young;
    look.ledger
        .messages
        .iter()
        .filter(|message| message.kind == "result")
        .filter_map(move |result| {
            let (worker, at) = (result.sender?, clock::millis(&result.created_at)?);
            (look.at - at >= young).then_some((result, worker, at))
        })
}

/// Why the copy of a worker's conversation holds no turn of a result's.
enum Unheld {
    /// The log shows no delivery to the worker of the result's task before it.
    NoDelivery,
    /// No prompt of the copy holds the header of the message (its id) that the delivery
    /// attempt (its number) began, said once that attempt began.
    NotCopied(i64, i64),
    /// The copy holds no answer of the agent's after its last prompt.
    NoAnswer,
}

/// The delivery attempt that began the turn of `result`, the worker `worker`'s, written
/// at `at`: the last `delivery.begun` by then of a message of the result's task to the
/// worker. Its message's id, its attempt's number, and when it began.
fn attempt_of(look: &Look, result: &Message, worker: i64, at: i64) -> Option<(i64, i64, i64)> {
    look.ledger
        .events_of("delivery.begun")
        .into_iter()
        .rev()
        .find_map(|event| {
            let id = event.data["message"].as_i64()?;
            let message = look
                .ledger
                .messages
                .iter()
                .find(|message| message.id == id)?;
            let began = clock::millis(&event.at)?;
            (message.recipient == worker
                && message.task.is_some()
                && message.task == result.task
                && began <= at)
                .then(|| (id, event.data["attempt"].as_i64().unwrap_or(1), began))
        })
}

/// Whether `item` is a prompt that delivered `worker` a message of another task than
/// `task` ([`delivered_to`]).
fn delivers_another_task(look: &Look, item: &Item, worker: i64, task: Option<i64>) -> bool {
    delivered_to(look, item, worker)
        .iter()
        .any(|message| message.task.is_some() && message.task != task)
}

/// The turn of `result`, the worker `worker`'s, written at `at`, in `items`, the copy of
/// the worker's conversations by then. It is bound to the delivery attempt that began it
/// ([`attempt_of`]): its prompt is the first that delivered that message to the worker
/// once the attempt began, never an earlier attempt's of the same message; and what
/// follows it is the turn's only in that prompt's conversation, and only until a prompt
/// that delivered the worker another task. A delivery is a prompt that opens with a
/// header of a message to the worker ([`delivered_to`]): a header its words, a job's
/// notification or a tool's output quote is none, and begins or ends nothing. Of that,
/// the last turn ([`last_turn`]).
fn turn_of<'a>(
    look: &Look,
    result: &Message,
    worker: i64,
    at: i64,
    items: &'a [&'a Item],
) -> Result<&'a [&'a Item], Unheld> {
    let (message, attempt, began) =
        attempt_of(look, result, worker, at).ok_or(Unheld::NoDelivery)?;
    let prompt = items
        .iter()
        .position(|item| {
            item.millis().is_some_and(|said| said >= began)
                && delivered_to(look, item, worker)
                    .iter()
                    .any(|delivered| delivered.id == message)
        })
        .ok_or(Unheld::NotCopied(message, attempt))?;
    let conversation = items[prompt].conversation;
    let end = items[prompt + 1..]
        .iter()
        .position(|item| {
            item.conversation != conversation
                || delivers_another_task(look, item, worker, result.task)
        })
        .map_or(items.len(), |after| prompt + 1 + after);
    last_turn(&items[prompt..end]).ok_or(Unheld::NoAnswer)
}

/// Whether `worker` works on Codex, whose commentary is no part of its final answer.
fn on_codex(look: &Look, worker: i64) -> bool {
    look.ledger
        .participant(worker)
        .is_some_and(|who| who.harness.as_deref() == Some("codex"))
}

/// 4: a result that holds a line of the progress of its turn the final answer does
/// not hold too, or not the whole of the turn's final answer.
pub fn final_answer(look: &Look, settings: &Settings) -> Vec<Break> {
    let names = Names::of(look);
    let mut found = Vec::new();
    for (result, worker, at) in settled_results(look, settings) {
        let items = look.items_by(worker, at);
        let Ok(turn) = turn_of(look, result, worker, at, &items) else {
            continue;
        };
        let (answer, progress) = answer_and_progress(turn, on_codex(look, worker));
        let answer = plain(&answer);
        if answer.is_empty() {
            continue;
        }
        let body = plain(&result.body);
        let held: Vec<&&Item> = progress
            .iter()
            .filter(|line| {
                let words = plain(&line.text);
                words.chars().count() >= PROGRESS
                    && body.contains(&words)
                    && !answer.contains(&words)
            })
            .collect();
        let said = if !held.is_empty() {
            format!(
                "m-{} holds {} line{} of its turn's progress",
                result.id,
                held.len(),
                if held.len() == 1 { "" } else { "s" }
            )
        } else if !body.contains(&answer) {
            format!("m-{} is not its turn's final answer, whole", result.id)
        } else {
            continue;
        };
        found.push(
            Break::new(4, format!("4:m-{}", result.id), said).with(Evidence {
                rows: vec![message_row(result, &names)],
                screen: Vec::new(),
                record: lines_of(turn, 8, &names),
            }),
        );
    }
    found
}

/// 4, unresolved: a result whose turn the copy of its worker's conversation does not
/// hold (no delivery began it, no prompt holds the delivery that did, or prompts and
/// tools' items alone after it), or holds with nothing said after its last tool call
/// (for Codex, nothing it marked complete): what its final answer was, the result
/// cannot be held to.
pub fn unanswered_results(look: &Look, settings: &Settings) -> Vec<Unjudged> {
    let mut found = Vec::new();
    for (result, worker, at) in settled_results(look, settings) {
        let items = look.items_by(worker, at);
        let codex = on_codex(look, worker);
        let handle = look.ledger.handle(worker);
        let missing = match turn_of(look, result, worker, at, &items) {
            Err(Unheld::NoDelivery) => {
                let task = result
                    .task
                    .and_then(|task| look.ledger.task(task))
                    .map_or_else(|| "its task".to_owned(), |task| format!("T-{}", task.number));
                format!("was begun by no delivery to @{handle} of {task} that the log shows before it")
            }
            Err(Unheld::NotCopied(message, attempt)) => format!(
                "is not in the copy of @{handle}'s conversation, which holds no prompt of m-{message} after its delivery attempt {attempt} began"
            ),
            Err(Unheld::NoAnswer) => format!(
                "is not in the copy of @{handle}'s conversation, which holds no answer of the agent's after its last prompt"
            ),
            Ok(turn) if plain(&answer_and_progress(turn, codex).0).is_empty() && codex => format!(
                "has no final answer after its last tool call in the copy of @{handle}'s conversation, only Codex's commentary, which it never marks complete"
            ),
            Ok(turn) if plain(&answer_and_progress(turn, codex).0).is_empty() => format!(
                "says nothing after its last tool call in the copy of @{handle}'s conversation"
            ),
            Ok(_) => continue,
        };
        found.push(Unjudged::new(
            4,
            format!("4:m-{}", result.id),
            format!(
                "m-{}'s turn {missing}: whether it is the turn's final answer cannot be judged",
                result.id
            ),
        ));
    }
    found
}

/// The items of the last turn in `items`: from the last prompt on (a user's item
/// that is no tool's), the prompt left out.
fn last_turn<'a>(items: &'a [&'a Item]) -> Option<&'a [&'a Item]> {
    let prompt = items.iter().rposition(|item| item.role == "user")?;
    let turn = &items[prompt + 1..];
    turn.iter()
        .any(|item| item.role == "assistant")
        .then_some(turn)
}

/// A turn's final answer, as the product makes the result of it: what it said after
/// its last tool call, every item of it, a blank line between (Codex's commentary
/// left out, whether or not a final answer came: the progress notes it writes there,
/// which its record never marks complete, where its final answer is); and its
/// progress, what it said before.
fn answer_and_progress<'a>(turn: &'a [&'a Item], codex: bool) -> (String, Vec<&'a Item>) {
    let after = turn
        .iter()
        .rposition(|item| item.role == "tool")
        .map_or(0, |tool| tool + 1);
    let said = |items: &'a [&'a Item]| {
        items
            .iter()
            .filter(|item| item.role == "assistant")
            .copied()
            .collect::<Vec<&Item>>()
    };
    let mut answer = said(&turn[after..]);
    let mut progress = said(&turn[..after]);
    if codex {
        let (finals, commentary): (Vec<&Item>, Vec<&Item>) =
            answer.into_iter().partition(|item| item.complete);
        answer = finals;
        progress.extend(commentary);
    }
    let words = answer
        .iter()
        .map(|item| item.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    (words, progress)
}

/// A message's row as evidence: its body masked whole, then cut, the run's `names`
/// shown.
pub(super) fn message_row(message: &Message, names: &Names) -> String {
    format!(
        "m-{} {} {} at {}: {}",
        message.id,
        message.kind,
        message.state,
        message.created_at,
        cut(&plain(&message.body), 200, names)
    )
}

//! Receipt: what makes a message received, and what that does to its task.
//! A message is received when its window's record shows its marker, when
//! the paste that carried it was confirmed, when a door handed an answer to
//! its harness and said so, or when `cf` served its whole body to the one it
//! was for ([`Read::receives`]). A message served whole while its paste is still
//! on its way (`delivering`) is read all the same, but the paste is the window's
//! to settle: the read is kept with the message ([`keep_read`], in its receipt as
//! `readEarly`) and told when the delivery ends ([`settle_when_given_back`],
//! [`settle_when_given_up`]). A role is none of these: each part of it `cf inbox read`
//! prints is logged, whatever state its note is in, and it is read once every part was
//! printed ([`told_if_read`]). Withdrawing a message never resolves what it obliged:
//! only receipt does. A task moves between `queued`, `working` and `waiting`
//! in one place, [`reconcile`], after each receipt.
//!
//! A door is the hook or broker that asked a question for a harness's
//! question tool and waits for its answer. It claims an answer, hands it to
//! its harness, and then says so; until it has, the answer is still the
//! ledger's to deliver as text, and a claim that no one acknowledged is
//! voided at the first sign that its door is gone.

use cf_base::role_read::role_parts;
use cf_proto::cf_word::CfWord;
use cf_proto::ledger::{Claim, MessageView};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

use super::{known_message, message_task, role_of_note};
use crate::conversations::{current_conversation, set_conversation_role_text};
use crate::model::LedgerError;
use crate::store::Store;
use crate::views::{message_view, TaskRow, MESSAGE_SELECT};

/// How `cf` served a message whole: to its recipient's `cf inbox read`, or in
/// the thread `cf task get` printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Read {
    Inbox,
    Task,
}

impl Read {
    fn word(self) -> &'static str {
        match self {
            Read::Inbox => "inbox",
            Read::Task => "task",
        }
    }

    /// Whether a message still queued and served whole this way is received by it:
    /// - by `cf inbox read`, any message but a task's words and a note that
    ///   says a conversation its role. A question, a result, a note or an
    ///   answer read there is the reader's, and is never typed in after it
    ///   (T-260). A task's words send its window on, and only their paste
    ///   proves them; a role is ConsensFlow's word to the conversation, which
    ///   is told it by its paste (`confirm_delivery_telling`), or, where the
    ///   paste was cut, once every part of it was printed and its paste is
    ///   confirmed, in either order ([`told_if_read`]);
    /// - in the thread `cf task get` printed, an answer only: the thread is
    ///   the task's, and the rest of it is not the reader's to receive one by one.
    fn receives(self, message: &MessageView, says_a_role: bool) -> bool {
        match self {
            Read::Inbox => message.kind != "task" && !says_a_role,
            Read::Task => message.kind == "answer",
        }
    }

    /// Whether a message whose paste is still on its way (`delivering`) and served
    /// whole this way keeps the read until its delivery ends: whatever
    /// [`Read::receives`] says of a queued one. A role read by `cf inbox read` never comes
    /// here: its parts printed are logged instead ([`told_if_read`]). A task's words are
    /// none: their paste alone proves them.
    fn keeps(self, message: &MessageView) -> bool {
        match self {
            Read::Inbox => message.kind != "task",
            Read::Task => message.kind == "answer",
        }
    }

    /// The way a kept read was made, from the word its receipt holds.
    fn of_word(word: &str) -> Option<Read> {
        match word {
            "inbox" => Some(Read::Inbox),
            "task" => Some(Read::Task),
            _ => None,
        }
    }
}

/// What a message's receipt holds, while its paste is on its way, of a read made
/// meanwhile ([`keep_read`]): the way it was read, and the conversation of its
/// recipient it was read in, as a receipt names them.
const KEPT: &str = "readEarly";
const KEPT_IN: &str = "readIn";

/// A read made while the paste of a message was on its way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Kept {
    via: Read,
    /// The recipient's conversation when it was read; none where it had none.
    conversation: Option<i64>,
}

/// The read a message on its way was served whole by, if its receipt holds one.
pub(crate) fn kept_read(message: &MessageView) -> Option<Kept> {
    let via = message
        .receipt
        .get(KEPT)
        .and_then(Value::as_str)
        .and_then(Read::of_word)?;
    Some(Kept {
        via,
        conversation: message.receipt.get(KEPT_IN).and_then(Value::as_i64),
    })
}

/// `receipt`, the proof that a paste arrived, with the read a message made on its
/// way ([`kept_read`] of it before) added, so that what the window did and what
/// the harness showed are both in it.
pub(crate) fn with_kept_read(receipt: Value, kept: Option<Kept>) -> Value {
    let Some(kept) = kept else {
        return receipt;
    };
    let mut receipt = match receipt {
        Value::Object(_) => receipt,
        _ => json!({}),
    };
    receipt[KEPT] = json!(kept.via.word());
    if let Some(conversation) = kept.conversation {
        receipt[KEPT_IN] = json!(conversation);
    }
    receipt
}

/// A message served whole to its recipient while its paste is on its way: the read
/// is kept in its receipt, with the conversation it was made in, and is not a
/// receipt of the message yet. The paste is the window's to settle (`delivering`,
/// which `confirm_delivery`, `retry_delivery` and `give_up_delivery` leave), so the
/// message stays as it is; what the read comes to is told when the delivery ends.
/// A read after another keeps the later: it is the conversation that read last
/// that has the words.
fn keep_read(store: &mut Store, message: &MessageView, via: Read) -> Result<(), LedgerError> {
    let conversation = current_conversation(store, message.recipient_id)?.map(|found| found.id);
    let receipt = with_kept_read(json!({}), Some(Kept { via, conversation }));
    store.db.execute(
        "UPDATE message SET receipt = ? WHERE id = ? AND state = 'delivering'",
        params![receipt.to_string(), message.id],
    )?;
    Ok(())
}

/// A delivery was given back to the queue (`retry_delivery`), and the message had been
/// served whole to its recipient while it was on its way: where a read in the inbox
/// would have received it queued ([`Read::receives`]), it is `read` now, and not
/// pasted again after the reader had it (T-260). Whether it was is whether the
/// paste was proved, which it was not. Its task is reconciled, as for any read.
/// A note that says a role is not received by a read: its parts printed stay logged,
/// and tell the role if the paste comes in after all (`confirm_delivery`), as the
/// note's words are the same in every paste of it. Whether the message was
/// settled.
pub(crate) fn settle_when_given_back(
    store: &mut Store,
    message_id: i64,
) -> Result<bool, LedgerError> {
    settle_kept_read(store, message_id, "queued")
}

/// A delivery was given up (`give_up_delivery`), and the message had been served whole
/// to its recipient while it was on its way: where a read would have received it
/// queued, it is `read` and not failed, as it is when the paste is given back
/// ([`settle_when_given_back`]): its recipient has it, and what a failure would tell
/// whoever waits on it (an answer that did not arrive, a question that was not
/// asked) is not so. A note that says a role is failed as any is: a read does not
/// receive it. Whether the message was settled.
pub(crate) fn settle_when_given_up(
    store: &mut Store,
    message_id: i64,
) -> Result<bool, LedgerError> {
    settle_kept_read(store, message_id, "delivering")
}

/// The read kept with message `message_id`, which is in state `now`, is told: the
/// message is received by it where a read of its kind receives one, and its task is
/// reconciled.
fn settle_kept_read(store: &mut Store, message_id: i64, now: &str) -> Result<bool, LedgerError> {
    let message = known_message(store, message_id)?;
    let Some(Kept { via, .. }) = kept_read(&message) else {
        return Ok(false);
    };
    let role = says_a_role(store, message_id)?;
    if message.state != now || !via.receives(&message, role) {
        return Ok(false);
    }
    received_as(store, &message, &json!({ "read": via.word() }))?;
    reconcile_of(store, &message)?;
    Ok(true)
}

/// Whether a task message for the window is still on its way: queued, held
/// for the human, or being pasted.
fn barrier(store: &Store, task: i64, window: i64) -> Result<bool, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT 1 FROM message WHERE task_id = ? AND recipient_id = ? AND kind = 'task'
               AND state IN ('queued', 'gated', 'delivering')",
        )?
        .exists(params![task, window])?)
}

/// Whether the window received a task message about the task: with none
/// still on its way ([`barrier`]), the words it was given are what it has.
fn arrived(store: &Store, task: i64, window: i64) -> Result<bool, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT 1 FROM message WHERE task_id = ? AND recipient_id = ? AND kind = 'task'
               AND state IN ('delivered', 'read')",
        )?
        .exists(params![task, window])?)
}

/// Whether the window asked a question on the task that no received answer
/// resolves. A question the ledger withdrew still obliges once it has an
/// answer, whatever became of that answer: only its transport was taken back,
/// and an answer that failed, was cancelled or declined was not received. One
/// withdrawn with no answer at all (its asker's window ended first, its task
/// was failed) obliges no more: nobody has anything to answer.
fn outstanding(store: &Store, task: i64, window: i64) -> Result<bool, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT 1 FROM message q
             WHERE q.task_id = ?1 AND q.kind = 'question' AND q.sender_id = ?2 AND q.urgent = 0
               AND NOT EXISTS (SELECT 1 FROM message a WHERE a.reply_to = q.id
                               AND a.kind = 'answer' AND a.state IN ('delivered', 'read'))
               AND (q.state != 'cancelled'
                    OR EXISTS (SELECT 1 FROM message a WHERE a.reply_to = q.id
                               AND a.kind = 'answer'))
             LIMIT 1",
        )?
        .exists(params![task, window])?)
}

/// The one place a task moves between `queued`, `working` and `waiting`:
/// - a `queued` task goes on once no task message for its window is still on
///   its way and one was received: to `waiting` if the window has a question
///   nobody received an answer to, else to `working`;
/// - a `working` task waits for such a question;
/// - a `waiting` task works again once none is left;
/// - any other state stays as it is.
pub(crate) fn reconcile(store: &mut Store, task_id: i64) -> Result<(), LedgerError> {
    let task = store
        .db
        .query_row("SELECT * FROM task WHERE id = ?", [task_id], TaskRow::read)?;
    let Some(window) = task.assignee_id else {
        return Ok(());
    };
    let to = match task.state.as_str() {
        "queued" => {
            if barrier(store, task.id, window)? || !arrived(store, task.id, window)? {
                return Ok(());
            }
            if outstanding(store, task.id, window)? {
                "waiting"
            } else {
                "working"
            }
        }
        "working" if outstanding(store, task.id, window)? => "waiting",
        "waiting" if !outstanding(store, task.id, window)? => "working",
        _ => return Ok(()),
    };
    store.move_task(&task, to, json!({}))
}

/// Whether a question its window asks about `task` comes from a turn that
/// is an old one already, and is born with its door shut: the task is
/// paused, or queued with the words that send it on still waiting for their
/// paste, so the window is still on the turn that came before them. Its
/// answer comes as a message, with those words, when the task goes on. A
/// resumed task that reads working because its window is at work on it
/// (`work_while_resumed`), or waiting for the answer to a question it asked meanwhile,
/// is the same: its resume's words are not in yet.
pub(crate) fn door_born_closed(store: &Store, task: &TaskRow) -> Result<bool, LedgerError> {
    let Some(window) = task.assignee_id else {
        return Ok(false);
    };
    Ok(match task.state.as_str() {
        "paused" => true,
        "queued" => store
            .db
            .prepare(
                "SELECT 1 FROM message WHERE task_id = ? AND recipient_id = ? AND kind = 'task'
                   AND state IN ('queued', 'gated')",
            )?
            .exists(params![task.id, window])?,
        // Or at work, or waiting for the answer to a question asked meanwhile, on the turn
        // before the words that resumed it: every door is shut until they are in.
        "working" | "waiting" => crate::tasks::resume_words_not_in(store, task)?.is_some(),
        _ => false,
    })
}

/// A door asks for the answer to its question, as the one who asked it. In
/// one step:
/// - a shut door, or a task paused, is closed;
/// - no answer yet that was not given up on, or one still held for the
///   human, waits;
/// - an answer still `queued` and in no paste is claimed for the door: the
///   paste skips it from now, until the door says it handed it over, or the
///   claim is voided. While the question's project is closed (the human's
///   Close, or the chief's window ending) it waits unclaimed instead, kept for
///   the Resume: a window the Close could not close is handed nothing, its door
///   no more than a paste (both reviews of 2026-10-16, round 5);
/// - an answer the door took already, and its reply was lost, is answered
///   again, writing nothing: a claim is a hand-over begun, the window's
///   whatever came since, as a paste begun is;
/// - an answer that rides in a paste, is being pasted or was pasted is
///   closed: it comes as text.
pub(crate) fn claim_answer(
    store: &mut Store,
    question_id: i64,
    asker: i64,
) -> Result<Claim, LedgerError> {
    store.write(|store| {
        let (sender, task, shut, project): (Option<i64>, Option<i64>, Option<String>, i64) = store
            .db
            .query_row(
                "SELECT sender_id, task_id, door_closed_at, project_id FROM message
                 WHERE id = ? AND kind = 'question'",
                [question_id],
                |row| {
                    Ok((
                        row.get("sender_id")?,
                        row.get("task_id")?,
                        row.get("door_closed_at")?,
                        row.get("project_id")?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| {
                LedgerError::refused_with(
                    "not-a-question",
                    format!("message {question_id} is not a question"),
                    409,
                )
            })?;
        if sender != Some(asker) {
            return Err(LedgerError::refused_with(
                "not-your-question",
                format!("m-{question_id} was asked by someone else"),
                403,
            ));
        }
        let paused = match task {
            Some(task) => {
                let state: String = store.db.query_row(
                    "SELECT state FROM task WHERE id = ?",
                    [task],
                    |row| row.get(0),
                )?;
                state == "paused"
            }
            None => false,
        };
        if shut.is_some() || paused {
            return Ok(Claim::Closed);
        }
        let found: Option<(i64, String, Option<i64>, Option<String>)> = store
            .db
            .query_row(
                "SELECT id, state, carried_by, claimed_at FROM message
                 WHERE reply_to = ? AND kind = 'answer' AND state NOT IN ('gated', 'cancelled', 'failed')
                 ORDER BY id LIMIT 1",
                [question_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let Some((answer, state, carrier, claimed)) = found else {
            return Ok(Claim::Waiting);
        };
        match (state.as_str(), carrier) {
            ("read", _) => Ok(Claim::Answered(Box::new(known_message(store, answer)?))),
            ("queued", None) => {
                if claimed.is_none() {
                    let open = store
                        .db
                        .prepare("SELECT 1 FROM project WHERE id = ? AND state = 'open'")?
                        .exists([project])?;
                    if !open {
                        return Ok(Claim::Waiting);
                    }
                    let at = store.at();
                    store.db.execute(
                        "UPDATE message SET claimed_at = ? WHERE id = ?",
                        params![at, answer],
                    )?;
                    store.log(
                        project,
                        "delivery.claimed",
                        json!({ "message": answer, "question": question_id }),
                    )?;
                }
                Ok(Claim::Answered(Box::new(known_message(store, answer)?)))
            }
            _ => Ok(Claim::Closed),
        }
    })
}

/// A door says whether it handed the answer it claimed to its harness, as
/// the one the answer was for:
/// - handed over, the answer still `queued`, in no paste, its door open: it is
///   `read`, with the door's receipt, and its task is reconciled;
/// - handed over, and read already: nothing more;
/// - handed over, and anything else (the door was shut meanwhile, the answer
///   was pasted as text): refused `door-closed`, and the answer comes as text;
/// - not handed over: the claim is given back, and the answer is the ledger's.
pub(crate) fn settle_claim(
    store: &mut Store,
    answer_id: i64,
    recipient: i64,
    received: bool,
) -> Result<(), LedgerError> {
    store.write(|store| {
        let answer = store
            .message(answer_id)?
            .filter(|answer| answer.kind == "answer" && answer.recipient_id == recipient)
            .ok_or_else(|| {
                LedgerError::refused_with(
                    "unknown-message",
                    format!("no answer m-{answer_id} for you"),
                    404,
                )
            })?;
        if !received {
            store.db.execute(
                "UPDATE message SET claimed_at = NULL WHERE id = ? AND state = 'queued'",
                [answer_id],
            )?;
            return Ok(());
        }
        if answer.state == "read" {
            return Ok(());
        }
        let handed = answer.state == "queued"
            && store
                .db
                .prepare(
                    "SELECT 1 FROM message a JOIN message q ON q.id = a.reply_to
                     WHERE a.id = ? AND a.carried_by IS NULL AND q.door_closed_at IS NULL",
                )?
                .exists([answer_id])?;
        if !handed {
            return Err(LedgerError::refused_with(
                "door-closed",
                format!("m-{answer_id} is not answered here: it comes to you as a message"),
                409,
            ));
        }
        received_as(store, &answer, &json!({ "door": true }))?;
        reconcile_of(store, &answer)
    })
}

/// Messages served whole to the one they were for by `cf`, whichever way:
/// those among `ids` that are still `queued` for `recipient`, a carried or
/// claimed one included (its claim is given up), and that the way they were
/// served receives ([`Read::receives`]), are `read`, each with its receipt,
/// and their tasks reconciled, once each. A note that says `recipient` a role,
/// printed in the inbox, is its part 1 of 1 printed in the window whose `cf` is `cf`
/// ([`receive_read_part`]: refused where that window prints it in more). One whose paste is still on its way
/// (`delivering`) and that the way it was served keeps ([`Read::keeps`]) keeps the
/// read for when its delivery ends ([`keep_read`]); it is not received yet, and not
/// counted. The rest are left as they are. How many were received.
pub(crate) fn receive_read(
    store: &mut Store,
    recipient: i64,
    ids: &[i64],
    via: Read,
    cf: &CfWord,
) -> Result<usize, LedgerError> {
    store.write(|store| {
        let mut received = 0;
        let mut tasks = Vec::new();
        for id in ids {
            let Some(message) = store
                .message(*id)?
                .filter(|message| message.recipient_id == recipient)
            else {
                continue;
            };
            let role = says_a_role(store, message.id)?;
            if via == Read::Inbox && role {
                received += read_part(store, &message, 1, 1, cf)?;
                continue;
            }
            if message.state == "delivering" {
                if via.keeps(&message) {
                    keep_read(store, &message, via)?;
                }
                continue;
            }
            if message.state != "queued" || !via.receives(&message, role) {
                continue;
            }
            received_as(store, &message, &json!({ "read": via.word() }))?;
            received += 1;
            if let Some(task) = message_task(store, message.id)? {
                if !tasks.contains(&task.id) {
                    tasks.push(task.id);
                }
            }
        }
        for task in tasks {
            reconcile(store, task)?;
        }
        Ok(received)
    })
}

/// The kind of event that says `cf inbox read` printed a part of a role to its window.
const PART_READ: &str = "message.part_read";

/// Part `part` of `of` of the note `id`, a role, printed to `recipient` by `cf inbox read` in
/// the window whose `cf` is `cf`: `of` is how many parts `cf` prints it in there
/// (`cf_base::role_read::role_parts` of its words as that window's `cf` reads them), which
/// the ledger counts itself and refuses any other count for. The part is logged as printed
/// ([`part_printed`]), whatever state the note is in, and the role is told if that was the
/// last part to print ([`told_if_read`]). A part printed twice or out of order is no fault.
/// A part there is not, a read by `cf task get`, and a message that is not `recipient`'s or
/// says no role are nothing. How many were received.
pub(crate) fn receive_read_part(
    store: &mut Store,
    recipient: i64,
    id: i64,
    part: usize,
    of: usize,
    via: Read,
    cf: &CfWord,
) -> Result<usize, LedgerError> {
    if part == 0 || part > of || via != Read::Inbox {
        return Ok(0);
    }
    store.write(|store| {
        let Some(message) = store
            .message(id)?
            .filter(|message| message.recipient_id == recipient)
        else {
            return Ok(0);
        };
        if !says_a_role(store, id)? {
            return Ok(0);
        }
        read_part(store, &message, part, of, cf)
    })
}

/// Part `part` of `of` of `message`, a role to its recipient, printed by `cf inbox read` in
/// the window whose `cf` is `cf`: refused where that is not the count of parts `cf` prints
/// it in there, else logged and the role told if it was read ([`told_if_read`]). 1 where it
/// was, else 0.
fn read_part(
    store: &mut Store,
    message: &MessageView,
    part: usize,
    of: usize,
    cf: &CfWord,
) -> Result<usize, LedgerError> {
    let id = message.id;
    let counted = role_parts(&cf.in_text(&message.body)).len();
    if of != counted {
        let parts = if counted == 1 { "part" } else { "parts" };
        return Err(LedgerError::refused_with(
            "invalid-read",
            format!(
                "m-{id} is printed in {counted} {parts} in this window, not {of}: a read says the part `cf inbox read` printed and how many it prints the note in"
            ),
            400,
        ));
    }
    part_printed(store, message, part, of)?;
    Ok(usize::from(told_if_read(store, message)?))
}

/// Part `part` of `of` of `message`, a role, was printed to its recipient by `cf inbox
/// read`: logged ([`PART_READ`]) with the recipient's conversation it was printed in.
fn part_printed(
    store: &mut Store,
    message: &MessageView,
    part: usize,
    of: usize,
) -> Result<(), LedgerError> {
    let conversation = current_conversation(store, message.recipient_id)?.map(|found| found.id);
    store.log(
        message.project_id,
        PART_READ,
        json!({ "message": message.id, "part": part, "of": of, "conversation": conversation }),
    )
}

/// Whether the role `message` says was read: every part of it was printed ([`PART_READ`])
/// in its recipient's current conversation, in the number of parts it was last printed in.
/// The state the note was in at each print is no part of it: a part printed while its paste
/// was on its way, or back in the queue, was read all the same.
fn read_every_part(store: &Store, message: &MessageView) -> Result<bool, LedgerError> {
    let Some(of) = store
        .db
        .query_row(
            "SELECT json_extract(data, '$.of') FROM event
              WHERE project_id = ? AND kind = ? AND json_extract(data, '$.message') = ?
              ORDER BY id DESC LIMIT 1",
            params![message.project_id, PART_READ, message.id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()?
        .flatten()
    else {
        return Ok(false);
    };
    let conversation = current_conversation(store, message.recipient_id)?.map(|found| found.id);
    let printed: i64 = store.db.query_row(
        "SELECT COUNT(DISTINCT json_extract(data, '$.part')) FROM event
          WHERE project_id = ? AND kind = ? AND json_extract(data, '$.message') = ?
            AND json_extract(data, '$.of') = ? AND json_extract(data, '$.conversation') IS ?",
        params![message.project_id, PART_READ, message.id, of, conversation],
        |row| row.get(0),
    )?;
    Ok(printed == of)
}

/// The role a note says is told where it was read ([`read_every_part`]) and its paste is
/// confirmed (`delivered`): asked when a part of it is printed, and when its paste is
/// confirmed (`confirm_delivery`), so whichever comes last tells it ([`read_role`]).
/// Whether it was.
pub(crate) fn told_if_read(store: &mut Store, message: &MessageView) -> Result<bool, LedgerError> {
    let message = known_message(store, message.id)?;
    if message.state != "delivered"
        || !says_a_role(store, message.id)?
        || !read_every_part(store, &message)?
    {
        return Ok(false);
    }
    read_role(store, &message)?;
    Ok(true)
}

/// A note that says its recipient a role, delivered (its paste was cut to its
/// opening and the command that prints the rest), and now printed whole to its
/// recipient by `cf inbox read`: the window has the whole role. The note is
/// `read` (its delivery's time and proof kept), and the recipient's current
/// conversation recorded as told that role, in one write. Not where a later note
/// says it a role, which is what it was told last, nor where its conversation
/// began after the note was delivered: the note went into another.
fn read_role(store: &mut Store, message: &MessageView) -> Result<(), LedgerError> {
    store.db.execute(
        "UPDATE message SET state = 'read' WHERE id = ?",
        [message.id],
    )?;
    store.log(
        message.project_id,
        "message.read",
        json!({ "message": message.id, "receipt": { "read": Read::Inbox.word() } }),
    )?;
    let last = store.db.query_row(
        "SELECT MAX(id) FROM message WHERE recipient_id = ? AND role_text IS NOT NULL",
        [message.recipient_id],
        |row| row.get::<_, Option<i64>>(0),
    )?;
    let Some(conversation) = current_conversation(store, message.recipient_id)? else {
        return Ok(());
    };
    let since = message
        .delivered_at
        .as_deref()
        .is_some_and(|at| at >= conversation.started_at.as_str());
    if let Some(role) =
        role_of_note(store, message.id)?.filter(|_| last == Some(message.id) && since)
    {
        set_conversation_role_text(store, conversation.id, &role)?;
    }
    Ok(())
}

/// Whether a message says its recipient's conversation a role: a note the
/// ledger holds the role text of.
fn says_a_role(store: &Store, message_id: i64) -> Result<bool, LedgerError> {
    Ok(store
        .db
        .prepare("SELECT 1 FROM message WHERE id = ? AND role_text IS NOT NULL")?
        .exists([message_id])?)
}

/// The message is `read`: when, and the proof, its claim given up.
fn received_as(
    store: &mut Store,
    message: &MessageView,
    receipt: &Value,
) -> Result<(), LedgerError> {
    let at = store.at();
    store.db.execute(
        "UPDATE message SET state = 'read', delivered_at = ?, receipt = ?, claimed_at = NULL
         WHERE id = ?",
        params![at, receipt.to_string(), message.id],
    )?;
    crate::tasks::background_exchange(store, message.id, true)?;
    store.log(
        message.project_id,
        "message.read",
        json!({ "message": message.id, "receipt": receipt }),
    )
}

/// The task an answer is about is reconciled.
fn reconcile_of(store: &mut Store, answer: &MessageView) -> Result<(), LedgerError> {
    match message_task(store, answer.id)? {
        Some(task) => reconcile(store, task.id),
        None => Ok(()),
    }
}

/// Every claim on an answer for `participant` that no door has acknowledged
/// is voided, and `because` is said in the log: the answer is the ledger's
/// again, to deliver as text. The first look that finds the window at rest,
/// and its exit, do this: the door that claimed it is as good as gone.
pub(crate) fn release_claims(
    store: &mut Store,
    participant: i64,
    because: &str,
) -> Result<(), LedgerError> {
    release(store, Some(participant), because)
}

/// Every claim of every window is voided, as a daemon that starts again does:
/// no door survives it.
pub(crate) fn release_all_claims(store: &mut Store) -> Result<(), LedgerError> {
    release(store, None, "the daemon started again")
}

fn release(store: &mut Store, participant: Option<i64>, because: &str) -> Result<(), LedgerError> {
    store.write(|store| {
        let claimed: Vec<MessageView> = store
            .db
            .prepare(&format!(
                "{MESSAGE_SELECT} WHERE m.claimed_at IS NOT NULL AND m.state = 'queued'
                   AND (?1 IS NULL OR m.recipient_id = ?1) ORDER BY m.id"
            ))?
            .query_map([participant], message_view)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut tasks = Vec::new();
        for answer in &claimed {
            store.db.execute(
                "UPDATE message SET claimed_at = NULL WHERE id = ?",
                [answer.id],
            )?;
            store.log(
                answer.project_id,
                "delivery.unclaimed",
                json!({ "message": answer.id, "because": because }),
            )?;
            if let Some(task) = message_task(store, answer.id)? {
                if !tasks.contains(&task.id) {
                    tasks.push(task.id);
                }
            }
        }
        for task in tasks {
            reconcile(store, task)?;
        }
        Ok(())
    })
}

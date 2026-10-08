//! What the page reads, each in the one frame the app's bridge carries an
//! answer in: the board, a task with its thread, a participant's messages and
//! what a task's window wrote. What does not fit is cut, marked and counted;
//! `cf` reads the same records whole. Sizes are of the JSON as the page
//! receives it, which serde writes byte for byte as `JSON.stringify` did.

use std::collections::{HashMap, HashSet};

use cf_base::text::utf16_len;
use cf_proto::ledger::{
    Board, Cut, Lane, LatestMessages, LatestTranscript, MessageView, TaskCard, TaskThatFits,
    TaskView, TranscriptItem,
};
use rusqlite::params;
use serde::Serialize;

use crate::conversations::{transcript, TRANSCRIPT_ITEM_MAX};
use crate::model::{cut, title_of, LedgerError};
use crate::projects::project;
use crate::store::Store;
use crate::tasks::task;
use crate::views::{message_view, task_view, MESSAGE_SELECT, TASK_SELECT};

/// How much of a list the page reads at once (the human's notes, what a
/// window wrote), as JSON: half the 1 MiB frame the app's bridge carries
/// each answer in. An item too long for it on its own is cut at
/// [`TRANSCRIPT_ITEM_MAX`] characters; at six bytes a character (a control
/// character, escaped) that is 384 KB, so the newest item always fits, and a
/// refresh that reads the notes stays well clear of the frame.
pub const PAGE_BYTES: usize = 512 * 1024;

/// How much of a message the human's bay on the board carries: two screens
/// of its strip. A message may run to a million characters; its task's
/// thread has it all.
const BAY_EXCERPT: usize = 2_000;

/// The size of `value` as the page receives it.
fn bytes(value: &impl Serialize) -> usize {
    serde_json::to_vec(value).map_or(0, |json| json.len())
}

/// What a list item's long text is, cut to fit a frame.
trait Text: Clone + Serialize {
    fn text(&self) -> &str;
    fn set_text(&mut self, text: String);
}

impl Text for TranscriptItem {
    fn text(&self) -> &str {
        &self.text
    }
    fn set_text(&mut self, text: String) {
        self.text = text;
    }
}

impl Text for MessageView {
    fn text(&self) -> &str {
        &self.body
    }
    fn set_text(&mut self, text: String) {
        self.body = text;
    }
}

/// The newest of `items` (newest first) that fit in [`PAGE_BYTES`] as a
/// JSON list, in that order: what the page reads of a list in one frame.
/// One too long to fit on its own has its text cut at
/// [`TRANSCRIPT_ITEM_MAX`] characters and goes on; the first that does not
/// fit ends the list.
fn newest_that_fit<T: Text>(items: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut fit = Vec::new();
    // A list of n items takes n + 1 bytes of its own: two brackets, n - 1 commas.
    let mut used = 1;
    for mut item in items {
        let mut size = bytes(&item) + 1;
        if 1 + size > PAGE_BYTES {
            let text = cut(item.text(), TRANSCRIPT_ITEM_MAX);
            item.set_text(text);
            size = bytes(&item) + 1;
        }
        if used + size > PAGE_BYTES {
            break;
        }
        used += size;
        fit.push(item);
    }
    fit
}

/// What the window that has a task wrote, as the page reads it in one frame:
/// the last items that fit, no more than `limit` of them, in order, with how
/// many there are and how many came.
pub(crate) fn latest_transcript(
    store: &Store,
    project_id: i64,
    number: i64,
    limit: Option<usize>,
) -> Result<LatestTranscript, LedgerError> {
    let written = transcript(store, project_id, number, limit)?;
    let mut items = newest_that_fit(written.items.into_iter().rev());
    items.reverse();
    Ok(LatestTranscript {
        shown: items.len(),
        items,
        total: written.total,
    })
}

/// The tasks on the board for a member of their tier, as the dispatcher
/// gives them out each pass: these alone, not the whole board with every
/// brief and result an aged project has.
pub(crate) fn open_tasks(store: &Store, project_id: i64) -> Result<Vec<TaskView>, LedgerError> {
    Ok(store
        .db
        .prepare(&format!(
            "{TASK_SELECT} WHERE t.project_id = ? AND t.state = 'open' AND t.assignee_id IS NULL
       ORDER BY t.number"
        ))?
        .query_map([project_id], task_view)?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The board: each task with the first line of its latest result, what its
/// card shows; its brief stays out, as the drawer reads it with the task. A
/// task sits on the lane of whoever has it: its assignee's or, once the
/// session that had it is off the board (the human deleted it), its
/// member's. A task no lane has is among the open ones, whatever its state:
/// one waiting for a member; one paused, called off or failed before any
/// member had it; one of a member who left the staff, finished or not. The
/// page draws each on its requester's row, in the column of its state, a
/// paused one in the backlog. A task the human deleted is on neither
/// (`tests/cancelled.rs` holds the board to Node's on one file).
pub(crate) fn board(store: &Store, project_id: i64) -> Result<Board, LedgerError> {
    let Some(project) = project(store, project_id)? else {
        return Err(LedgerError::refused_with(
            "unknown-project",
            format!("no project {project_id}"),
            404,
        ));
    };
    let mut results: HashMap<i64, String> = HashMap::new();
    let mut statement = store.db.prepare(
        "SELECT task_id, body FROM message WHERE project_id = ? AND kind = 'result' ORDER BY id",
    )?;
    let mut rows = statement.query([project_id])?;
    while let Some(row) = rows.next()? {
        if let Some(task) = row.get::<_, Option<i64>>("task_id")? {
            results.insert(task, row.get("body")?);
        }
    }
    let tasks = store
        .db
        .prepare(&format!(
            "{TASK_SELECT} WHERE t.project_id = ? AND t.deleted_at IS NULL ORDER BY t.number"
        ))?
        .query_map([project_id], |row| {
            let task = task_view(row)?;
            let left: Option<String> = row.get("assignee_left_at")?;
            let member: Option<String> = row.get("assignee_member")?;
            let lane = match member {
                Some(member) if left.is_some() => Some(member),
                _ => task.assignee.clone(),
            };
            Ok((task, lane))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let cards: Vec<(TaskCard, Option<String>)> = tasks
        .into_iter()
        .map(|(task, lane)| {
            let result = results.get(&task.id).map(|body| title_of(body));
            (TaskCard::of(task, result), lane)
        })
        .collect();
    let gated = store
        .db
        .prepare(&format!(
            "{MESSAGE_SELECT} WHERE m.project_id = ? AND m.state = 'gated' ORDER BY m.id"
        ))?
        .query_map([project_id], message_view)?
        .map(|message| {
            message.map(|mut message| {
                message.body = cut(&message.body, BAY_EXCERPT);
                message
            })
        })
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let handles: HashSet<&str> = project
        .participants
        .iter()
        .map(|participant| participant.handle.as_str())
        .collect();
    let has_lane = |lane: &Option<String>| {
        lane.as_deref()
            .is_some_and(|handle| handles.contains(handle))
    };
    Ok(Board {
        // Every task no lane has; one given by name waits in its own lane.
        open: cards
            .iter()
            .filter(|(_, lane)| !has_lane(lane))
            .map(|(card, _)| card.clone())
            .collect(),
        lanes: project
            .participants
            .iter()
            .map(|participant| Lane {
                participant: participant.clone(),
                tasks: cards
                    .iter()
                    .filter(|(_, lane)| lane.as_deref() == Some(participant.handle.as_str()))
                    .map(|(card, _)| card.clone())
                    .collect(),
            })
            .collect(),
        project,
        gated,
    })
}

/// A part of a task's answer, in each form it may take: whole, then cut at
/// [`TRANSCRIPT_ITEM_MAX`] characters, at [`BAY_EXCERPT`], and to the line
/// saying how long it was, each with its size.
fn forms<T: Text>(part: &T) -> Vec<(Cut<T>, usize)> {
    let length = utf16_len(part.text());
    std::iter::once(Cut {
        view: part.clone(),
        body_cut: false,
    })
    .chain(
        [TRANSCRIPT_ITEM_MAX, BAY_EXCERPT, 0]
            .into_iter()
            .filter(|max| length > *max)
            .map(|max| {
                let mut view = part.clone();
                view.set_text(cut(part.text(), max));
                Cut {
                    view,
                    body_cut: true,
                }
            }),
    )
    .map(|form| {
        let size = bytes(&form);
        (form, size)
    })
    .collect()
}

impl Text for TaskView {
    fn text(&self) -> &str {
        &self.body
    }
    fn set_text(&mut self, text: String) {
        self.body = text;
    }
}

/// A task as the page reads it in one frame, none when there is no such
/// task: its brief first, then its thread from the newest message back,
/// each body whole while [`PAGE_BYTES`] allows. A body that does not fit is
/// cut at [`TRANSCRIPT_ITEM_MAX`] characters, at [`BAY_EXCERPT`], or to the
/// line saying how long it was, whichever fits, and marked `bodyCut`; the
/// earliest messages that do not fit even so are left out, and
/// `messagesLeftOut` says how many. A task message (a brief delivered, a
/// resume, a reopen) always stays, cut to its line at least: there are few,
/// and each is a round of the task's story in the drawer.
pub(crate) fn task_that_fits(
    store: &Store,
    project_id: i64,
    number: i64,
) -> Result<Option<TaskThatFits>, LedgerError> {
    let Some(thread) = task(store, project_id, number)? else {
        return Ok(None);
    };
    if bytes(&thread) <= PAGE_BYTES {
        return Ok(Some(TaskThatFits {
            task: Cut {
                view: thread.task,
                body_cut: false,
            },
            messages: thread
                .messages
                .into_iter()
                .map(|view| Cut {
                    view,
                    body_cut: false,
                })
                .collect(),
            messages_left_out: None,
        }));
    }
    let brief = forms(&thread.task);
    let parts: Vec<Vec<(Cut<MessageView>, usize)>> = thread.messages.iter().map(forms).collect();
    let stays = |at: usize| thread.messages[at].kind == "task";
    let least = |options: &[(Cut<MessageView>, usize)]| options.last().map_or(0, |(_, size)| *size);
    let (least_brief, least_brief_size) = brief
        .last()
        .map(|(form, size)| (form.clone(), *size))
        .unwrap_or_else(|| {
            (
                Cut {
                    view: thread.task.clone(),
                    body_cut: false,
                },
                0,
            )
        });
    // The least the answer takes: the brief and every task message cut to
    // its line, and room to say how many messages were left out.
    let mut used = bytes(&TaskThatFits {
        task: least_brief.clone(),
        messages: Vec::new(),
        messages_left_out: Some(thread.messages.len()),
    }) + parts
        .iter()
        .enumerate()
        .map(|(at, options)| if stays(at) { least(options) + 1 } else { 0 })
        .sum::<usize>();
    let shown = first_fit(&brief, &mut used, least_brief_size, 0)
        .map_or(least_brief, |chosen| brief[chosen].0.clone());
    let mut kept: Vec<Option<Cut<MessageView>>> = vec![None; parts.len()];
    let mut leaving = false;
    for at in (0..parts.len()).rev() {
        let options = &parts[at];
        if stays(at) {
            let chosen =
                first_fit(options, &mut used, least(options) + 1, 1).unwrap_or(options.len() - 1);
            kept[at] = options.get(chosen).map(|(form, _)| form.clone());
        } else if !leaving {
            kept[at] = first_fit(options, &mut used, 0, 1).map(|chosen| options[chosen].0.clone());
            leaving = kept[at].is_none();
        }
    }
    let left = kept.iter().filter(|kept| kept.is_none()).count();
    Ok(Some(TaskThatFits {
        task: shown,
        messages: kept.into_iter().flatten().collect(),
        messages_left_out: (left > 0).then_some(left),
    }))
}

/// The first of `options` that fits in the room `used` leaves, past what is
/// counted for it already (`counted`, and the comma before it when `comma`
/// is 1), with `used` grown by it.
fn first_fit<T>(
    options: &[(Cut<T>, usize)],
    used: &mut usize,
    counted: usize,
    comma: usize,
) -> Option<usize> {
    let chosen = options
        .iter()
        .position(|(_, size)| *used + size + comma - counted <= PAGE_BYTES)?;
    *used = *used + options[chosen].1 + comma - counted;
    Some(chosen)
}

/// A participant's messages as the page reads them in one frame, newest
/// first: those that fit, never what still waits for the human, with how
/// many there are and how many came. `unread` keeps to the notes still
/// queued: what For you lists for the human.
pub(crate) fn latest_messages(
    store: &Store,
    participant_id: i64,
    unread: bool,
) -> Result<LatestMessages, LedgerError> {
    let which = if unread {
        "m.kind = 'note' AND m.state = 'queued'"
    } else {
        "m.state != 'gated'"
    };
    let total: i64 = store.db.query_row(
        &format!("SELECT COUNT(*) AS total FROM message m WHERE m.recipient_id = ? AND {which}"),
        params![participant_id],
        |row| row.get("total"),
    )?;
    let mut statement = store.db.prepare(&format!(
        "{MESSAGE_SELECT} WHERE m.recipient_id = ? AND {which} ORDER BY m.id DESC"
    ))?;
    // Read as far as the frame goes, not to the end.
    let mut failed = None;
    let rows = statement
        .query_map([participant_id], message_view)?
        .map_while(|row| row.map_err(|cause| failed = Some(cause)).ok());
    let messages = newest_that_fit(rows);
    if let Some(cause) = failed {
        return Err(cause.into());
    }
    Ok(LatestMessages {
        shown: messages.len(),
        messages,
        total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(text: String) -> TranscriptItem {
        TranscriptItem {
            id: "i".into(),
            conversation: 1,
            role: "user".into(),
            text,
            complete: true,
            at: None,
        }
    }

    #[test]
    fn a_list_fits_to_its_last_byte_and_no_further() {
        let first = item("first".into());
        // A list of two takes its two brackets and a comma besides its items.
        let room = PAGE_BYTES - 3 - bytes(&first) - bytes(&item(String::new()));
        let exactly = item("x".repeat(room));
        assert_eq!(bytes(&[first.clone(), exactly.clone()]), PAGE_BYTES);
        assert_eq!(newest_that_fit([first.clone(), exactly]).len(), 2);
        let over = item("x".repeat(room + 1));
        assert_eq!(newest_that_fit([first, over]).len(), 1);
    }

    #[test]
    fn an_item_too_long_for_a_list_of_its_own_is_cut_and_goes_on() {
        let long = item("x".repeat(PAGE_BYTES));
        let fit = newest_that_fit([long]);
        assert_eq!(
            fit[0].text,
            cut(&"x".repeat(PAGE_BYTES), TRANSCRIPT_ITEM_MAX)
        );
    }
}

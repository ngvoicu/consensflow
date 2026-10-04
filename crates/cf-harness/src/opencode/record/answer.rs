//! What OpenCode's rows and events, as read so far, say (`opencodeAnswer`,
//! `hosts/lib/completion/opencode.js`).
//!
//! The parts of each message are in the order of their events, and the
//! messages too; an item's place in the reading is its event's seq. The
//! latest assistant message decides: its turn closes when it completed with
//! `finish` stop or length, or failed; it is settled once it closed and no
//! tool call is open.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cf_base::js;
use jiff::tz::TimeZone;
use serde_json::Value;

use super::read::{optional, property, text, Read, Stored};
use crate::shared::quota::{exhausted_quota, quota_status};
use crate::shared::record::key::{Key, Keys};
use crate::shared::record::reading::{visible_text, Item, Record, Role, Settlement};
use crate::shared::record::sort::sort;
use crate::shared::record::sqlite::Cell;

/// A part's row, and its data.
type Part<'a> = (&'a Stored, &'a Value);

/// An item and the seq that places it.
struct Placed {
    item: Item,
    seq: f64,
}

/// The reading `read` says. A refusal's reset at a time of day that names
/// no zone is read in `local`.
pub(super) fn answer(read: &Read, local: &TimeZone, keys: &mut Keys) -> Result<Record, String> {
    let parts_of = parts_by_message(read)?;
    let messages = sort(read.messages.values().collect(), |left, right| {
        by_creation(left, right)
    })?;
    let messages = sort(messages, |left, right| {
        by_position(&read.message_positions, left, right, "message")
    })?;
    let mut record = Record::new();
    let mut items = Vec::new();
    let mut open_tools = HashSet::new();
    let mut turn_open = false;
    let mut closed = false;
    for row in messages {
        let message = row.data(&format!("message {}", text(row.cell("id"))))?;
        let parts: &[Part<'_>] = parts_of.get(&row.id).map_or(&[], Vec::as_slice);
        let mut texts = Vec::new();
        for (_, part) in parts {
            if property(part, "type")?.and_then(Value::as_str) == Some("text") {
                if let Some(Value::String(text)) = part.get("text") {
                    texts.push(text.as_str());
                }
            }
        }
        let text = texts.join("\n");
        let message_seq = position(&read.message_positions, row, "message")?;
        let mut text_positions = Vec::new();
        for (part_row, part) in parts {
            if part.get("type").and_then(Value::as_str) == Some("text") {
                text_positions.push(position(&read.part_positions, part_row, "part")?);
            }
        }
        let time = property(message, "time")?;
        let completed_at = optional(time, "completed").filter(|at| !at.is_null());
        let at = completed_at
            .or_else(|| optional(time, "created").filter(|at| !at.is_null()))
            .cloned()
            // OpenCode's every row has a `time_created`.
            .unwrap_or_else(|| row.cell("time_created").map_or(Value::Null, Cell::json));
        let completed = completed_at.is_some();
        let completion_seq = if completed {
            Some(position(
                &read.completion_positions,
                row,
                "message completion",
            )?)
        } else {
            None
        };
        let seq = if text_positions.is_empty() {
            completion_seq.unwrap_or(message_seq)
        } else {
            text_positions.into_iter().fold(message_seq, f64::max)
        };
        match message.get("role").and_then(Value::as_str) {
            Some("user") => {
                open_tools.clear();
                record.failed = false;
                if !js::trim(&text).is_empty() {
                    items.push(Placed {
                        item: item(row, Role::User, text, true, at)?,
                        seq,
                    });
                }
                turn_open = true;
                closed = false;
                continue;
            }
            Some("assistant") => {}
            _ => continue,
        }
        record.failed = false;
        for (part_row, part) in parts {
            if part.get("type").and_then(Value::as_str) != Some("tool") {
                continue;
            }
            let tool = match part.get("callID").filter(|call| !call.is_null()) {
                Some(call) => keys.of(Some(call)),
                None => part_row.id.clone(),
            };
            let done = is_done(part);
            if done {
                open_tools.remove(&tool);
            } else {
                open_tools.insert(tool);
            }
            if part.get("tool").and_then(Value::as_str) == Some("question") {
                record.asking = !done;
            }
        }
        let error = message.get("error");
        let error_name = optional(error, "name");
        let is_failure = completed && js::truthy(error_name);
        let finish = message.get("finish").and_then(Value::as_str);
        let closes_turn = completed && (matches!(finish, Some("stop" | "length")) || is_failure);
        let complete = completed && finish == Some("stop") && !js::truthy(error_name);
        items.push(Placed {
            item: item(row, Role::Assistant, text, complete, at)?,
            seq,
        });
        for (part_row, part) in parts {
            if part.get("type").and_then(Value::as_str) != Some("tool") || !is_done(part) {
                continue;
            }
            let state = part.get("state");
            let ended = optional(optional(state, "time"), "end").filter(|end| !end.is_null());
            let at = ended.cloned().unwrap_or_else(|| {
                part_row
                    .cell("time_updated")
                    .map_or(Value::Null, Cell::json)
            });
            let tool_item = item(part_row, Role::Tool, tool_text(state), true, at)?;
            items.push(Placed {
                item: tool_item,
                seq: position(&read.part_positions, part_row, "part")?,
            });
        }
        if completed {
            record.quota = None;
        }
        if is_failure {
            record.failed = true;
            let data = optional(error, "data");
            let failure = match optional(data, "message").filter(|said| !said.is_null()) {
                Some(said) => js::string(Some(said))?.into_owned(),
                None => visible_text(error),
            };
            if quota_status(js::to_number(optional(data, "statusCode"))?) {
                let at_ms = js::to_number(completed_at)?;
                record.quota = Some(Arc::new(exhausted_quota(&failure, at_ms, local)?));
            }
        }
        closed = closes_turn;
        turn_open = !closes_turn;
    }
    let can_settle = closed && open_tools.is_empty();
    record.in_flight = turn_open || !open_tools.is_empty();
    record.settlement = if can_settle {
        Settlement::Settled
    } else if record.in_flight {
        Settlement::InFlight
    } else {
        Settlement::Unknown
    };
    // In the record's order: an item's seq is its event's.
    let items = sort(items, |left, right| {
        let between = left.seq - right.seq;
        Ok(if between != 0.0 && !between.is_nan() {
            between
        } else {
            ordering(js::locale_compare(&left.item.id, &right.item.id))
        })
    })?;
    record.items = items.into_iter().map(|placed| placed.item).collect();
    Ok(record)
}

/// Each message's parts, read in creation order (each part's data parsed in
/// it), then each message's in the order of their events.
fn parts_by_message(read: &Read) -> Result<HashMap<Key, Vec<Part<'_>>>, String> {
    let parts = sort(read.parts.values().collect(), |left, right| {
        by_creation(left, right)
    })?;
    let mut lists: Vec<(Key, Vec<Part<'_>>)> = Vec::new();
    let mut places: HashMap<Key, usize> = HashMap::new();
    for row in parts {
        let data = row.data(&format!("part {}", text(row.cell("id"))))?;
        match places.get(&row.message) {
            Some(&at) => lists[at].1.push((row, data)),
            None => {
                places.insert(row.message.clone(), lists.len());
                lists.push((row.message.clone(), vec![(row, data)]));
            }
        }
    }
    let mut by_message = HashMap::new();
    for (message, list) in lists {
        let list = sort(list, |(left, _), (right, _)| {
            by_position(&read.part_positions, left, right, "part")
        })?;
        by_message.insert(message, list);
    }
    Ok(by_message)
}

/// `byCreation`: by `time_created`, then by id, as `<` and `>` order them.
fn by_creation(left: &Stored, right: &Stored) -> Result<f64, String> {
    let time = |row: &Stored| row.cell("time_created").map_or(f64::NAN, Cell::number);
    let between = time(left) - time(right);
    if between != 0.0 && !between.is_nan() {
        return Ok(between);
    }
    let less = |left: Option<&Cell>, right: Option<&Cell>| match (left, right) {
        (Some(left), Some(right)) => right.greater(left),
        // `undefined` is no number, and no text: never less nor greater.
        _ => false,
    };
    let (left, right) = (left.cell("id"), right.cell("id"));
    Ok(if less(left, right) {
        -1.0
    } else if less(right, left) {
        1.0
    } else {
        0.0
    })
}

/// By the seq of each row's event, then by id (`localeCompare`).
fn by_position(
    positions: &HashMap<Key, f64>,
    left: &Stored,
    right: &Stored,
    description: &str,
) -> Result<f64, String> {
    let between =
        position(positions, left, description)? - position(positions, right, description)?;
    if between != 0.0 && !between.is_nan() {
        return Ok(between);
    }
    let Some(Cell::Text(id)) = left.cell("id") else {
        return Err(format!(
            "OpenCode's {description} {} has an id that is no text, where localeCompare was asked of it",
            text(left.cell("id"))
        ));
    };
    Ok(ordering(js::locale_compare(id, &text(right.cell("id")))))
}

/// The seq of the event of `row`, or the failure of a row no event named.
fn position(positions: &HashMap<Key, f64>, row: &Stored, description: &str) -> Result<f64, String> {
    positions.get(&row.id).copied().ok_or_else(|| {
        format!(
            "missing OpenCode event for {description} {}",
            text(row.cell("id"))
        )
    })
}

/// Whether a tool part ended: completed, or failed.
fn is_done(part: &Value) -> bool {
    matches!(
        optional(part.get("state"), "status").and_then(Value::as_str),
        Some("completed" | "error")
    )
}

/// `opencodeToolText`: what a tool said, its output else its error.
fn tool_text(state: Option<&Value>) -> String {
    match (optional(state, "output"), optional(state, "error")) {
        (Some(output), _) => visible_text(Some(output)),
        (None, Some(error)) => visible_text(Some(error)),
        (None, None) => String::new(),
    }
}

/// An item of `row`. Kept from Node on purpose: a row whose id is no text
/// (a blob, a number or null, which OpenCode's text column holds only when
/// another writer put it there) fails the look, where Node answered with it:
/// a reading's ids are text.
fn item(row: &Stored, role: Role, text: String, complete: bool, at: Value) -> Result<Item, String> {
    let Some(Cell::Text(id)) = row.cell("id") else {
        return Err(format!(
            "an OpenCode row's id is no text: {}",
            super::read::text(row.cell("id"))
        ));
    };
    Ok(Item {
        id: Arc::from(id.as_str()),
        role,
        text: Arc::from(text),
        complete,
        at,
        commentary: false,
    })
}

/// A comparator's number for an ordering.
fn ordering(order: Ordering) -> f64 {
    match order {
        Ordering::Less => -1.0,
        Ordering::Equal => 0.0,
        Ordering::Greater => 1.0,
    }
}

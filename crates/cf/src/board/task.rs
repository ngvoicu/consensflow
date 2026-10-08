//! `cf task …`: work put on the board, read from it, and moved along it.

use std::io::Read;

use cf_base::js;
use cf_base::text::{utf16_len, utf16_prefix};
use cf_board::Board;
use serde_json::{json, Map, Value};

use super::lines::{a_pool, message_line, numbers, task_head, task_line, tasks, waiting_answers};
use super::usage::{task_usage, ADD_USAGE};
use super::words::{quoted, require_text, split, task_number, task_numbers, Shape, Split};
use super::{help_of, text_of, Failure, Said};

/// How much of one transcript item `cf task get --transcript` shows, in UTF-16 units.
const ITEM_CHARS: usize = 600;

/// The flags of `cf task add`.
const ADD: Shape = Shape {
    switches: &["--self", "--advice", "--review", "--design"],
    valued: &["--tier", "--purpose", "--after", "--needs", "--before"],
    leading: 0,
};

/// The flags of `cf task get`, which stand after its task.
const GET: Shape = Shape {
    switches: &["--transcript"],
    valued: &["--last"],
    leading: 0,
};

/// The commands that move a task, which stands first among their words.
const MOVES: [&str; 6] = ["done", "accept", "cancel", "reopen", "pause", "resume"];

pub fn command(words: &[String], board: &Board, input: &mut dyn Read) -> Result<Said, Failure> {
    let (action, rest) = match words.split_first() {
        Some((action, rest)) => (Some(action.as_str()), rest),
        None => (None, words),
    };
    match action {
        Some("help" | "--help" | "-h") => {
            let usage = task_usage();
            return Ok(Said::new(json!({ "usage": usage }), usage));
        }
        Some("add") => return add(rest, board, input),
        Some("list") | None => return board_list(rest, board),
        Some(_) => {}
    }
    // Asked for before the task is looked for in the words: `cf task get
    // --help` has no task to name.
    if let Some(verb) = action.filter(|action| *action == "get" || MOVES.contains(action)) {
        let shape = Shape {
            leading: 1,
            ..if verb == "get" { GET } else { Shape::TEXT }
        };
        if split(rest, shape).asks_for_help() {
            return Ok(help_of(&["task", verb]));
        }
    }
    let number = task_number(rest.first().map(String::as_str))?;
    match action {
        Some("get") => get(number, rest.get(1..).unwrap_or_default(), board),
        Some(action) if MOVES.contains(&action) => {
            moved(action, number, rest.get(1..).unwrap_or_default(), board, input)
        }
        other => Err(Failure::Usage(format!(
            "unknown task command {}: use add, list, get, done, accept, reopen, cancel, pause or resume",
            quoted(other.unwrap_or_default())
        ))),
    }
}

fn add(rest: &[String], board: &Board, input: &mut dyn Read) -> Result<Said, Failure> {
    let words = split(rest, ADD);
    if words.asks_for_help() {
        return Ok(help_of(&["task", "add"]));
    }
    let tier = words.value("--tier");
    let after = words
        .value("--after")
        .map(|after| task_number(Some(after)))
        .transpose()?;
    let needs = task_numbers(words.value("--needs"))?;
    let before = task_numbers(words.value("--before"))?;
    let mine = words.on("--self");
    // Your own work needs no board while you are at it; on the board it is a
    // wake-up: the brief comes back to this window when what it waits for is accepted.
    if mine && needs.is_none() && before.is_none() {
        return Err(Failure::Usage(
            "you are already at it: do it now, or give it --needs T-3 to be woken when T-3 is accepted"
                .into(),
        ));
    }
    if tier.is_none() && after.is_none() && !mine && !words.on("--design") {
        return Err(Failure::Usage(ADD_USAGE.into()));
    }
    let brief = require_text(text_of(words.text.clone(), input)?, ADD_USAGE)?;

    let mut body = Map::new();
    if mine {
        body.insert("self".into(), true.into());
    } else if let Some(after) = after {
        body.insert("after".into(), after.into());
    } else if words.on("--design") {
        body.insert("design".into(), true.into());
    } else {
        if let Some(tier) = tier {
            body.insert("tier".into(), tier.into());
        }
        if words.on("--advice") {
            body.insert("advice".into(), true.into());
        }
        if words.on("--review") {
            body.insert("review".into(), true.into());
        }
        if let Some(purpose) = words.value("--purpose") {
            body.insert("purpose".into(), purpose.into());
        }
    }
    if let Some(needs) = &needs {
        body.insert("needs".into(), json!(needs));
    }
    if let Some(before) = &before {
        body.insert("before".into(), json!(before));
    }
    body.insert("body".into(), brief.into());

    let created = board.post("/api/tasks", &Value::Object(body))?;
    let task = created.part("task")?;
    let number = js::text(task.get("number"));
    // With human approval required, nothing moves until the human passes it on.
    let gated = if js::truthy(created.value().get("gated")) && !mine {
        " The human approves each message before it moves."
    } else {
        ""
    };
    let blocked = created.list(task.get("blockedBy"), "blockedBy")?;
    let waits = match blocked.len() {
        0 => String::new(),
        1 => format!(" It waits until {} is accepted.", tasks(numbers(blocked))),
        _ => format!(" It waits until {} are accepted.", tasks(numbers(blocked))),
    };
    let holds = match &before {
        None => String::new(),
        Some(before) => {
            let verb = if before.len() == 1 { "waits" } else { "wait" };
            format!(" {} {verb} for it.", tasks(before))
        }
    };
    let text = if mine {
        format!(
            "T-{number} is yours; finish it with: cf task done T-{number} \"what you did\".{waits}"
        )
    } else if let Some(after) = after {
        format!(
            "T-{number} continues in @{}, the window that did T-{after}; its result arrives in your inbox.{gated}{waits}",
            js::text(task.get("assignee"))
        )
    } else {
        let pool = task.get("pool");
        let tier = match task.get("tier") {
            None | Some(Value::Null) => tier.map(Value::from),
            Some(tier) => Some(tier.clone()),
        };
        let nearest = match created.value().get("asked") {
            None => String::new(),
            asked => format!(
                " (no {} {} is on the staff, so the nearest tier)",
                js::text(asked),
                js::text(pool)
            ),
        };
        format!(
            "T-{number} is on the board for {}{nearest}; the first free one gets it, and its result arrives in your inbox.{waits}{holds}{gated}",
            a_pool(pool, tier.as_ref())
        )
    };
    Ok(Said::new(created.into_value(), text))
}

/// Whether a task no lane has waits for a member to take it: a task paused or
/// over before any member had it, or of a member who left the staff, does not.
fn waits_for_a_member(task: &Value) -> bool {
    js::text(task.get("state")) == "open" && matches!(task.get("assignee"), Some(Value::Null))
}

/// `cf task list`: what waits for a member, the other tasks no lane has, then
/// each lane's.
fn board_list(rest: &[String], board: &Board) -> Result<Said, Failure> {
    if split(rest, Shape::TEXT).asks_for_help() {
        return Ok(help_of(&["task", "list"]));
    }
    let answer = board.get("/api/tasks")?;
    let mut lines = Vec::new();
    let (waiting, apart): (Vec<&Value>, Vec<&Value>) = answer
        .list(answer.value().get("open"), "open")?
        .iter()
        .partition(|task| waits_for_a_member(task));
    for (heading, group) in [("Waiting for a member", waiting), ("With no member", apart)] {
        if !group.is_empty() {
            lines.push(heading.to_string());
            lines.extend(group.into_iter().map(task_line));
        }
    }
    for lane in answer.list(answer.value().get("lanes"), "lanes")? {
        let lane_tasks = answer.list(lane.get("tasks"), "tasks")?;
        if !lane_tasks.is_empty() {
            lines.push(format!(
                "@{} ({})",
                js::text(lane.get("handle")),
                js::text(lane.get("role"))
            ));
            lines.extend(lane_tasks.iter().map(task_line));
        }
    }
    let text = if lines.is_empty() {
        "No tasks yet.".to_string()
    } else {
        lines.join("\n")
    };
    Ok(Said::new(answer.into_value(), text))
}

/// `cf task get T-n`: the task with its whole thread, and said of each answer
/// in it still waiting to be pasted that it was written whole; or, with
/// `--transcript`, what its window did, which says of no answer that it was
/// written: the thread is not what was asked for there (its text does not
/// print it, and what its JSON carries of it is along the way). What is wrong
/// with the command is said before the board is asked anything.
fn get(number: u64, rest: &[String], board: &Board) -> Result<Said, Failure> {
    let words = split(rest, GET);
    let last = if words.on("--transcript") {
        Some(items_asked(&words)?)
    } else {
        None
    };
    let path = format!("/api/tasks/{number}");
    let mut answer = board.get(&path)?;
    let task = answer.take("task")?;
    let Some(last) = last else {
        let messages = answer.list(task.get("messages"), "messages")?;
        let thread = messages
            .iter()
            .map(|message| {
                format!(
                    "{}\n{}",
                    message_line(message),
                    js::text(message.get("body"))
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let text = format!("{}\n\n{thread}", task_head(&task));
        let answers = waiting_answers(messages);
        return Ok(Said::new(task, text).having_written("task", answers));
    };
    let transcript = format!("/api/tasks/{number}/transcript?last={last}");
    let copy = board.get(&transcript)?;
    let listed = copy.list(copy.value().get("items"), "items")?;
    let total = copy.value().get("total");
    let text = if total.and_then(Value::as_f64) == Some(0.0) {
        "Its window has written nothing yet.".to_string()
    } else {
        let shown = listed
            .iter()
            .map(|item| {
                let role = match item.get("role").and_then(Value::as_str) {
                    Some("user") => "Sent to the window".into(),
                    Some("assistant") => "The agent".into(),
                    Some("tool") => "Tool output".into(),
                    Some("custom") => "Note".into(),
                    _ => js::text(item.get("role")),
                };
                let writing = if js::truthy(item.get("complete")) {
                    ""
                } else {
                    " · still writing"
                };
                format!(
                    "[{role}{writing}]\n{}",
                    clip(&js::text(item.get("text")), ITEM_CHARS)
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        format!(
            "What its window did, the last {} of {} items:\n\n{shown}",
            listed.len(),
            js::text(total)
        )
    };
    let text = format!("{}\n\n{text}", task_head(&task));
    let mut data = Map::new();
    data.insert("task".into(), task);
    if let Value::Object(copy) = copy.into_value() {
        data.extend(copy);
    }
    Ok(Said::new(Value::Object(data), text))
}

/// How many items of the transcript `--last` asks for: a whole number of one
/// or more, as JavaScript reads it; ten when it is not given.
fn items_asked(words: &Split) -> Result<f64, Failure> {
    let last = words.value("--last").map_or(10.0, js::number);
    if last.fract() != 0.0 || !last.is_finite() || last < 1.0 {
        return Err(Failure::Usage("--last takes a number of items".into()));
    }
    Ok(last)
}

fn moved(
    action: &str,
    number: u64,
    rest: &[String],
    board: &Board,
    input: &mut dyn Read,
) -> Result<Said, Failure> {
    let text = text_of(split(rest, Shape::TEXT).text, input)?;
    if matches!(action, "done" | "reopen" | "resume") && js::trim(&text).is_empty() {
        let what = match action {
            "done" => "your result",
            "resume" => "what to do now",
            _ => "the follow-up",
        };
        return Err(Failure::Usage(format!(
            "cf task {action} T-{number} \"{what}\""
        )));
    }
    let mut body = Map::new();
    if !text.is_empty() {
        body.insert("body".into(), text.into());
    }
    let path = format!("/api/tasks/{number}/{action}");
    let task = board.post(&path, &Value::Object(body))?.take("task")?;
    let said = match action {
        "pause" => format!(
            "T-{number} is paused: its window stops and its work waits. Resume it with: cf task resume T-{number} \"what to do now\""
        ),
        "resume" if task.get("state").and_then(Value::as_str) == Some("open") => format!(
            "T-{number} is back on the board for {}: the window that had it has ended.",
            a_pool(task.get("pool"), task.get("tier"))
        ),
        "resume" => format!("T-{number} resumes in @{} with your words.", js::text(task.get("assignee"))),
        _ => task_line(&task),
    };
    Ok(Said::new(task, said))
}

/// `text` cut to `units` UTF-16 units, with an ellipsis when it was longer.
fn clip(text: &str, units: usize) -> String {
    if utf16_len(text) > units {
        format!("{}…", utf16_prefix(text, units))
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use cf_board::scripted::{reply, scripted};

    use super::*;

    fn card(number: u32, state: &str, assignee: Option<&str>) -> Value {
        json!({
            "number": number, "state": state, "assignee": assignee, "requester": "chief",
            "title": format!("Task {number}"), "blockedBy": [], "pool": "worker", "tier": "standard",
        })
    }

    /// What `cf task list` prints of a board with these open tasks and lanes.
    fn listed(open: &[Value], lanes: &Value) -> String {
        let api = scripted(vec![reply(200, json!({ "open": open, "lanes": lanes }))]);
        board_list(&[], &Board::new(Some(&api.url), "tok"))
            .unwrap()
            .text
    }

    #[test]
    fn what_waits_for_a_member_is_listed_apart_from_the_other_tasks_no_lane_has() {
        let lanes = json!([{ "handle": "zeus", "role": "worker", "tasks": [card(1, "done", Some("zeus"))] }]);
        assert_eq!(
            listed(
                &[
                    card(2, "cancelled", None),
                    card(3, "paused", None),
                    card(4, "failed", None),
                    card(5, "open", None),
                    card(9, "cancelled", Some("athena")),
                    card(13, "open", Some("athena")),
                ],
                &lanes,
            ),
            [
                "Waiting for a member",
                "T-5 [open] for a standard worker ← @chief: Task 5",
                "With no member",
                "T-2 [cancelled] for a standard worker ← @chief: Task 2",
                "T-3 [paused] for a standard worker ← @chief: Task 3",
                "T-4 [failed] for a standard worker ← @chief: Task 4",
                "T-9 [cancelled] @athena ← @chief: Task 9",
                "T-13 [open] @athena ← @chief: Task 13",
                "@zeus (worker)",
                "T-1 [done] @zeus ← @chief: Task 1",
            ]
            .join("\n")
        );
    }

    #[test]
    fn a_heading_with_no_task_under_it_is_not_printed() {
        let lanes = json!([]);
        assert_eq!(
            listed(&[card(5, "open", None)], &lanes),
            "Waiting for a member\nT-5 [open] for a standard worker ← @chief: Task 5"
        );
        assert_eq!(
            listed(&[card(2, "paused", None)], &lanes),
            "With no member\nT-2 [paused] for a standard worker ← @chief: Task 2"
        );
        assert_eq!(listed(&[], &lanes), "No tasks yet.");
    }
}

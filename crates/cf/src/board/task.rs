//! `cf task …`: work put on the board, read from it, and moved along it.

use std::io::Read;

use cf_base::js;
use cf_base::text::{utf16_len, utf16_prefix};
use cf_board::Board;
use serde_json::{json, Map, Value};

use super::lines::{a_pool, message_line, numbers, task_head, task_line, tasks};
use super::usage::{task_usage, ADD_USAGE};
use super::words::{quoted, require_text, split, task_number, task_numbers};
use super::{text_of, Failure, Said};

/// How much of one transcript item `cf task get --transcript` shows, in UTF-16 units.
const ITEM_CHARS: usize = 600;

pub fn command(words: &[String], board: &Board, input: &mut dyn Read) -> Result<Said, Failure> {
    let (action, rest) = match words.split_first() {
        Some((action, rest)) => (Some(action.as_str()), rest),
        None => (None, words),
    };
    match action {
        Some("help" | "--help" | "-h") => {
            let usage = task_usage();
            return Ok(Said {
                data: json!({ "usage": usage }),
                text: usage,
            });
        }
        Some("add") => return add(rest, board, input),
        Some("list") | None => return board_list(board),
        Some(_) => {}
    }
    let number = task_number(rest.first().map(String::as_str))?;
    match action {
        Some("get") => get(number, rest.get(1..).unwrap_or_default(), board),
        Some(action @ ("done" | "accept" | "cancel" | "reopen" | "pause" | "resume")) => {
            moved(action, number, rest.get(1..).unwrap_or_default(), board, input)
        }
        other => Err(Failure::Usage(format!(
            "unknown task command {}: use add, list, get, done, accept, reopen, cancel, pause or resume",
            quoted(other.unwrap_or_default())
        ))),
    }
}

fn add(rest: &[String], board: &Board, input: &mut dyn Read) -> Result<Said, Failure> {
    let words = split(
        rest,
        &["--self", "--advice", "--review", "--design"],
        &["--tier", "--purpose", "--after", "--needs", "--before"],
    );
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
    Ok(Said {
        data: created.into_value(),
        text,
    })
}

fn board_list(board: &Board) -> Result<Said, Failure> {
    let answer = board.get("/api/tasks")?;
    let mut lines = Vec::new();
    let open = answer.list(answer.value().get("open"), "open")?;
    if !open.is_empty() {
        lines.push("Waiting for a member".to_string());
        lines.extend(open.iter().map(task_line));
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
    Ok(Said {
        data: answer.into_value(),
        text,
    })
}

fn get(number: u64, rest: &[String], board: &Board) -> Result<Said, Failure> {
    let words = split(rest, &["--transcript"], &["--last"]);
    let path = format!("/api/tasks/{number}");
    let mut answer = board.get(&path)?;
    let task = answer.take("task")?;
    if !words.on("--transcript") {
        let thread = answer
            .list(task.get("messages"), "messages")?
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
        return Ok(Said { data: task, text });
    }
    let last = words.value("--last").map_or(10.0, js::number);
    if last.fract() != 0.0 || !last.is_finite() || last < 1.0 {
        return Err(Failure::Usage("--last takes a number of items".into()));
    }
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
    Ok(Said {
        data: Value::Object(data),
        text,
    })
}

fn moved(
    action: &str,
    number: u64,
    rest: &[String],
    board: &Board,
    input: &mut dyn Read,
) -> Result<Said, Failure> {
    let text = text_of(rest.join(" "), input)?;
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
    Ok(Said {
        data: task,
        text: said,
    })
}

/// `text` cut to `units` UTF-16 units, with an ellipsis when it was longer.
fn clip(text: &str, units: usize) -> String {
    if utf16_len(text) > units {
        format!("{}…", utf16_prefix(text, units))
    } else {
        text.to_string()
    }
}

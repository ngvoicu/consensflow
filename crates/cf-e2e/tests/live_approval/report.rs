//! What a run says of what came of it: for a step that failed, what the
//! product showed (the board, what waits at the gate, the chief's inbox, each
//! task's thread and what its windows recorded, the windows' screens, the
//! daemon's trace and the warnings of its log, beside the rig's own report);
//! and, for the whole run, the table of what each project's steps came to.

use cf_e2e::live::{cut, text, transcript, warnings};
use serde_json::{json, Value};

use crate::flow::Ctx;

/// How much of a message is shown, and how many lines of a window's screen and
/// of the daemon's trace.
const SHOWN: usize = 300;
const SCREEN: usize = 12;
const TRACE: usize = 40;

/// Says what a failed step left, on the error output.
pub fn failure(ctx: &Ctx) {
    let log = ctx.log;
    let board = ctx.project.board().unwrap_or(Value::Null);
    log.block("the board", &board_lines(&board));
    log.block("held at the gate", &lines(items(&board["gated"])));
    log.block(
        "the chief's inbox",
        &lines(&ctx.project.inbox("chief").unwrap_or_default()),
    );
    for number in task_numbers(&board) {
        match ctx.task(number) {
            Ok(task) => log.block(
                &format!("the thread of T-{number}: {}", text(&task["state"])),
                &lines(items(&task["messages"])),
            ),
            Err(failed) => log.say(format!("T-{number} could not be read: {failed}")),
        }
        log.block(
            &format!("what its windows recorded of T-{number}"),
            &transcript(ctx.rig, ctx.project.id(), number),
        );
    }
    log.block("the windows' screens", &screens(ctx));
    log.block(
        "the daemon's trace, its last events",
        &ctx.trace().tail(TRACE),
    );
    let daemon_log = ctx.rig.log();
    let warned = warnings(&daemon_log);
    log.block(
        "the daemon's log, its warnings and errors",
        &if warned.is_empty() {
            "(none)".to_owned()
        } else {
            warned.join("\n")
        },
    );
}

/// The board in a line to each of its parts: whether the gate is on, the tasks no
/// lane has, and each lane with what its window is doing (and why, where the
/// board says), the window it holds and the tasks it has.
fn board_lines(board: &Value) -> String {
    if board.is_null() {
        return "(could not be read)".to_owned();
    }
    let tasks = |tasks: &Value| -> String {
        items(tasks)
            .iter()
            .map(|task| format!("T-{} {}", text(&task["number"]), text(&task["state"])))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut lines = vec![format!(
        "the gate is {}; no lane has: {}",
        if board["project"]["gate"] == true {
            "on"
        } else {
            "off"
        },
        match tasks(&board["open"]) {
            held if held.is_empty() => "nothing".to_owned(),
            held => held,
        }
    )];
    lines.extend(items(&board["lanes"]).iter().map(|lane| {
        let who = &lane["participant"];
        let reason = lane["activity"]["reason"]
            .as_str()
            .map_or_else(String::new, |reason| format!(" ({reason})"));
        format!(
            "{} ({} on {}): {}{reason}; window {}; tasks: {}",
            text(&who["handle"]),
            text(&who["agent"]),
            text(&who["harness"]),
            text(&lane["activity"]["state"]),
            lane["pane"]["id"]
                .as_str()
                .map_or_else(|| "none".to_owned(), str::to_owned),
            match tasks(&lane["tasks"]) {
                none if none.is_empty() => "none".to_owned(),
                some => some,
            }
        )
    }));
    lines.join("\n")
}

/// The numbers of the tasks of the board, wherever it has them.
fn task_numbers(board: &Value) -> Vec<i64> {
    let mut numbers: Vec<i64> = board["open"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(
            board["lanes"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|lane| lane["tasks"].as_array().into_iter().flatten()),
        )
        .filter_map(|task| task["number"].as_i64())
        .collect();
    numbers.sort_unstable();
    numbers.dedup();
    numbers
}

/// The items of a JSON list, none where it is no list.
fn items(list: &Value) -> &[Value] {
    list.as_array().map_or(&[], Vec::as_slice)
}

/// Messages, a line to each: who it is from and for, what it is, how it stands
/// and how often it was delivered.
fn lines(messages: &[Value]) -> String {
    if messages.is_empty() {
        return "(none)".to_owned();
    }
    messages
        .iter()
        .map(|message| {
            format!(
                "m-{} {} @{} to @{} {}: {} ({} attempts{}) {}",
                text(&message["id"]),
                text(&message["kind"]),
                text(&message["sender"]),
                text(&message["recipient"]),
                message["taskNumber"]
                    .as_i64()
                    .map_or_else(String::new, |task| format!("T-{task}")),
                text(&message["state"]),
                text(&message["attempts"]),
                message["reason"]
                    .as_str()
                    .map_or_else(String::new, |reason| format!(", {}", cut(reason, 80))),
                cut(&text(&message["body"]), SHOWN)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The last lines of each window the daemon opened: the pane host's own while
/// the window is there, and what it said as the window ended once it is not.
fn screens(ctx: &Ctx) -> String {
    let lines_of = |tail: &Value| -> Vec<String> {
        tail.as_array()
            .map(|lines| lines.iter().map(text).collect())
            .unwrap_or_default()
    };
    ctx.rig
        .open_frames()
        .iter()
        .map(|frame| {
            let (id, generation) = (&frame["id"], &frame["generation"]);
            let snapshot = ctx.rig.host(
                "pane.snapshot",
                json!({ "id": id, "generation": generation, "tail": SCREEN }),
            );
            let shown = match snapshot {
                // What the window's turn left running, if anything, after its screen.
                Ok(answer) if answer["ok"] == true => format!(
                    "{}{}",
                    lines_of(&answer["tail"]).join("\n"),
                    ctx.human.background(frame)
                ),
                _ => match ctx.rig.exits().into_iter().find(|exit| exit["id"] == *id) {
                    Some(exit) => format!(
                        "(the window ended: {}, {})\n{}",
                        exit["exitCode"],
                        exit["signal"],
                        lines_of(&exit["tail"]).join("\n")
                    ),
                    None => format!("({id} is neither there nor ended)"),
                },
            };
            format!("--- {}\n{shown}", text(id))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What a step came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Passed,
    Failed(String),
    Skipped(String),
}

/// One line of the table of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub project: String,
    pub step: String,
    pub outcome: Outcome,
    pub seconds: u64,
}

/// The rows as a table: a line to each step, with what came of it and how long
/// it took, and the first line of why for one that did not pass.
pub fn table(rows: &[Row]) -> String {
    let width = rows
        .iter()
        .map(|row| row.project.chars().count())
        .max()
        .unwrap_or(0);
    rows.iter()
        .map(|row| {
            let (word, why) = match &row.outcome {
                Outcome::Passed => ("PASS", String::new()),
                Outcome::Failed(why) => ("FAIL", format!("  {}", first_line(why))),
                Outcome::Skipped(why) => ("skip", format!("  {}", first_line(why))),
            };
            format!(
                "{} {:<width$} {:<9} {:>4} s{why}",
                word, row.project, row.step, row.seconds
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn first_line(why: &str) -> String {
    cut(why.lines().next().unwrap_or_default(), 160)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(project: &str, step: &str, outcome: Outcome, seconds: u64) -> Row {
        Row {
            project: project.to_owned(),
            step: step.to_owned(),
            outcome,
            seconds,
        }
    }

    #[test]
    fn the_table_has_a_line_for_each_step_and_says_why_one_did_not_pass() {
        let rows = [
            row("p1 claude", "worker", Outcome::Passed, 245),
            row(
                "p1 claude",
                "advisor",
                Outcome::Failed("timed out after 360 s waiting for the result\nmore".to_owned()),
                400,
            ),
            row(
                "p1 claude",
                "question",
                Outcome::Skipped("Pi has no question tool".to_owned()),
                0,
            ),
        ];
        assert_eq!(
            table(&rows),
            "PASS p1 claude worker     245 s\n\
             FAIL p1 claude advisor    400 s  timed out after 360 s waiting for the result\n\
             skip p1 claude question     0 s  Pi has no question tool"
        );
        assert_eq!(table(&[]), "");
    }

    #[test]
    fn the_board_is_a_line_for_each_lane_with_what_its_window_does_and_the_tasks_it_has() {
        let board = json!({
            "project": { "gate": true },
            "open": [{ "number": 3, "state": "cancelled" }],
            "lanes": [
                {
                    "participant": { "handle": "chief", "agent": "live-codex", "harness": "codex" },
                    "activity": { "state": "idle" },
                    "pane": { "id": "p1-chief" },
                    "tasks": [],
                },
                {
                    "participant": { "handle": "live-pi-a-b", "agent": "live-pi", "harness": "pi" },
                    "activity": { "state": "working", "reason": "background work running (1 process)" },
                    "pane": null,
                    "tasks": [{ "number": 1, "state": "working" }, { "number": 2, "state": "done" }],
                },
            ],
        });
        assert_eq!(
            board_lines(&board),
            "the gate is on; no lane has: T-3 cancelled\n\
             chief (live-codex on codex): idle; window p1-chief; tasks: none\n\
             live-pi-a-b (live-pi on pi): working (background work running (1 process)); \
             window none; tasks: T-1 working, T-2 done"
        );
        assert_eq!(board_lines(&Value::Null), "(could not be read)");
    }

    #[test]
    fn the_tasks_of_a_board_are_numbered_once_whichever_lane_has_them() {
        let board = json!({
            "open": [{ "number": 3 }],
            "lanes": [
                { "tasks": [{ "number": 1 }, { "number": 3 }] },
                { "tasks": [] },
                { "tasks": [{ "number": 2 }] },
            ],
        });
        assert_eq!(task_numbers(&board), [1, 2, 3]);
        assert_eq!(task_numbers(&json!(null)), Vec::<i64>::new());
    }

    #[test]
    fn a_message_is_a_line_with_who_it_is_for_how_it_stands_and_how_often_it_was_delivered() {
        let held = json!([{
            "id": 7, "kind": "result", "sender": "w-a-b", "recipient": "chief",
            "taskNumber": 1, "state": "gated", "attempts": 0, "reason": null,
            "body": "WORKER_OK\n  done",
        }]);
        assert_eq!(
            lines(items(&held)),
            "m-7 result @w-a-b to @chief T-1: gated (0 attempts) WORKER_OK done"
        );
        assert_eq!(lines(items(&json!(null))), "(none)");
        assert_eq!(lines(&[]), "(none)");
    }
}

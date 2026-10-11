//! What a failed run shows of the product, besides the rig's own report (the
//! programs' error output, the end of the daemon's log, and what each window
//! printed): the board, the chief's inbox, the task and what the designer's
//! window recorded of it, its screen, and the warnings of the daemon's log.

use cf_e2e::live::{pretty, text, transcript as recorded, warnings as warned_in, Log};
use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::waits::designer_window;

pub use cf_e2e::live::cut;

/// How much of a message is shown.
const SHOWN: usize = 400;

/// How many lines of a window's screen are shown.
const LINES: usize = 12;

/// Says what a failed run left, on the error output.
pub fn failure(rig: &Rig, project: &Project, task: Option<i64>, log: &Log) {
    log.block("the board", &pretty(project.board()));
    log.block("the chief's inbox", &inbox(project));
    if let Some(task) = task {
        log.block(&format!("T-{task}"), &pretty(project.task(task)));
        log.block(
            &format!("what the designer's window recorded of T-{task}"),
            &transcript(rig, project, task),
        );
    }
    log.block(
        "the designer's window, as its screen ends",
        &screen(rig, project),
    );
    let daemon_log = rig.log();
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

/// What the designer's window recorded of the task, as the page's drawer shows
/// it: each item by its role, the last few.
pub fn transcript(rig: &Rig, project: &Project, task: i64) -> String {
    recorded(rig, project.id(), task)
}

/// The lines of the daemon's log that are warnings or errors, the first few.
pub fn warnings(log: &str) -> Vec<&str> {
    warned_in(log)
}

/// The chief's inbox, a line to each message.
fn inbox(project: &Project) -> String {
    match project.inbox("chief") {
        Ok(messages) if messages.is_empty() => "(empty)".to_owned(),
        Ok(messages) => messages
            .iter()
            .map(|message| {
                format!(
                    "m-{} {} {}: {}",
                    text(&message["id"]),
                    text(&message["kind"]),
                    text(&message["state"]),
                    cut(&text(&message["body"]), SHOWN)
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Err(failed) => format!("(could not be read: {failed})"),
    }
}

/// The last lines of the designer's screen: the pane host's, while the window
/// is there, and what it said as the window ended once it is not.
pub fn screen(rig: &Rig, project: &Project) -> String {
    let Some(window) = designer_window(rig, project.id()) else {
        return "(no window was opened for the designer)".to_owned();
    };
    let lines = |tail: &Value| -> Vec<String> {
        tail.as_array()
            .map(|lines| lines.iter().map(text).collect())
            .unwrap_or_default()
    };
    let (id, generation) = (&window["id"], &window["generation"]);
    let snapshot = rig.host(
        "pane.snapshot",
        json!({ "id": id, "generation": generation, "tail": LINES }),
    );
    if let Ok(answer) = snapshot {
        if answer["ok"] == true {
            return lines(&answer["tail"]).join("\n");
        }
    }
    match rig.exits().into_iter().find(|exit| exit["id"] == *id) {
        Some(exit) => format!(
            "(the window ended: {}, {})\n{}",
            exit["exitCode"],
            exit["signal"],
            lines(&exit["tail"]).join("\n")
        ),
        None => format!("({id} is neither there nor ended)"),
    }
}

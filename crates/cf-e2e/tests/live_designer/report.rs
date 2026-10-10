//! What a failed run shows of the product, besides the rig's own report (the
//! programs' error output, the end of the daemon's log, and what each window
//! printed): the board, the chief's inbox, the task and what the designer's
//! window recorded of it, its screen, and the warnings of the daemon's log.

use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::waits::designer_window;
use crate::{text, Log};

/// How much of a message or of an item of a transcript is shown.
const SHOWN: usize = 400;

/// How many of a transcript's last items are shown.
const ITEMS: usize = 20;

/// How much of an item's id is shown, which tells two items that read alike apart.
const ID: usize = 14;

/// How many lines of a window's screen are shown.
const LINES: usize = 12;

/// How many lines of the daemon's log that are warnings or errors are shown.
const WARNINGS: usize = 20;

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
    let read = rig.page(
        "task.transcript",
        json!({ "project": project.id(), "task": task }),
    );
    let items = match read {
        Ok(answer) => answer["items"].as_array().cloned().unwrap_or_default(),
        Err(failed) => return format!("(could not be read: {failed})"),
    };
    if items.is_empty() {
        return "(nothing recorded)".to_owned();
    }
    let skipped = items.len().saturating_sub(ITEMS);
    let mut lines = Vec::new();
    if skipped > 0 {
        lines.push(format!("({skipped} earlier items not shown)"));
    }
    lines.extend(items.iter().skip(skipped).map(|item| {
        format!(
            "[{} {}] {}",
            text(&item["role"]),
            cut(&text(&item["id"]), ID),
            cut(&text(&item["text"]), SHOWN)
        )
    }));
    lines.join("\n")
}

/// `text` on one line, and no longer than `most` characters.
pub fn cut(text: &str, most: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > most {
        let head: String = line.chars().take(most).collect();
        format!("{head}…")
    } else {
        line
    }
}

/// A read of the daemon as indented JSON, or why it could not be made.
fn pretty(read: cf_e2e::Result<Value>) -> String {
    match read {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()),
        Err(failed) => format!("(could not be read: {failed})"),
    }
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

/// The lines of the daemon's log that are warnings or errors, the first few.
pub fn warnings(log: &str) -> Vec<&str> {
    log.lines()
        .filter(|line| line.contains(" warn ") || line.contains(" error "))
        .take(WARNINGS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_text_is_one_line_and_cut_where_it_is_too_long() {
        assert_eq!(cut("a\n  b\t c", 10), "a b c");
        assert_eq!(cut("abcdef", 3), "abc…");
        assert_eq!(cut("abc", 3), "abc");
        // Counted in characters, not bytes.
        assert_eq!(cut("ééé", 2), "éé…");
    }

    #[test]
    fn only_the_warnings_and_errors_of_a_log_are_kept_and_at_most_so_many() {
        let log = "t info start\nt warn slow pass\nt error it broke\nt info done\n";
        assert_eq!(warnings(log), ["t warn slow pass", "t error it broke"]);
        assert!(warnings("t info start\n").is_empty());
        let many: String = (0..30).map(|at| format!("t warn {at}\n")).collect();
        assert_eq!(warnings(&many).len(), WARNINGS);
    }

    #[test]
    fn what_cannot_be_read_says_so_and_what_can_is_indented_json() {
        assert_eq!(pretty(Ok(json!({ "a": 1 }))), "{\n  \"a\": 1\n}");
        let failed = pretty(Err(cf_e2e::Error::Daemon("no answer".to_owned())));
        assert_eq!(failed, "(could not be read: no answer)");
    }
}

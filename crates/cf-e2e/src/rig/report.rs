//! What a failed case tells of the rig: the windows' screens, the programs'
//! error output and the daemon's log, for the person who reads the failure; what
//! the stand-in agents wrote of themselves, which is all the daemon reads of a
//! window; and what the daemon says of its lanes and inboxes. And how a
//! window's bytes become the text on its screen.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::{json, Value};

use super::Rig;
use crate::{pattern, process};

/// How much of a window's screen a failure shows.
const SCREEN: usize = 1_500;

/// How many of the last lines of the daemon's log a failure shows.
const LOG: usize = 30;

/// How many of the last records of a conversation a failure shows.
const RECORDS: usize = 4;

/// How many of the newest messages of an inbox a failure shows.
const MESSAGES: usize = 12;

/// How much of what a record or a message says a failure shows.
const SAID: usize = 100;

/// What the bytes a window printed look like on its screen: control sequences
/// stripped. Windows' console host draws a run of blanks as a cursor move and
/// starts a line by jumping to it: those stay the blanks and the line break
/// they stand for, so that text reads the same on every platform.
pub(super) fn plain(bytes: &[u8]) -> String {
    static MOVE: OnceLock<Regex> = OnceLock::new();
    static JUMP: OnceLock<Regex> = OnceLock::new();
    static SEQUENCE: OnceLock<Regex> = OnceLock::new();
    static TITLE: OnceLock<Regex> = OnceLock::new();
    let text = String::from_utf8_lossy(bytes);
    let text =
        pattern::once(&MOVE, r"\x1b\[(\d*)C").replace_all(&text, |found: &regex::Captures| {
            let count: usize = found[1].parse().unwrap_or(0);
            " ".repeat(if count == 0 { 1 } else { count.min(512) })
        });
    let text = pattern::once(&JUMP, r"\x1b\[[0-9;]*[Hf]").replace_all(&text, "\n");
    let text = pattern::once(&SEQUENCE, r"\x1b\[[0-9;?]*[ -/]*[@-~]").replace_all(&text, "");
    let text = pattern::once(&TITLE, r"\x1b\][^\x07]*\x07").replace_all(&text, "");
    text.replace('\r', "")
}

/// `text` as a line, the first `SAID` characters of it.
fn said(text: &str) -> String {
    let line: String = text
        .chars()
        .map(|char| {
            if matches!(char, '\n' | '\r') {
                ' '
            } else {
                char
            }
        })
        .collect();
    if line.chars().count() <= SAID {
        return line;
    }
    let cut: String = line.chars().take(SAID).collect();
    format!("{cut}…")
}

/// A status file of a stand-in agent, as a line: its process and whether that
/// is still there, what the file says it does, and the conversation it shows.
fn status_line(name: &str, text: &str, there: impl Fn(u32) -> bool) -> String {
    let Ok(status) = serde_json::from_str::<Value>(text) else {
        return format!("  sessions/{name}: not JSON: {}\n", said(text));
    };
    let process = match status["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
    {
        Some(pid) if there(pid) => format!("process {pid} is there"),
        Some(pid) => format!("process {pid} is gone"),
        None => "no process".to_owned(),
    };
    format!(
        "  sessions/{name}: {process}; says {} on {}\n",
        status["status"].as_str().unwrap_or("nothing"),
        status["sessionId"].as_str().unwrap_or("no conversation")
    )
}

/// A record of a conversation, as a line: its type, its subtype if it has one,
/// and what it says.
fn record_line(line: &str) -> String {
    let Ok(record) = serde_json::from_str::<Value>(line) else {
        return format!("    not JSON: {}\n", said(line));
    };
    let kind = record["type"].as_str().unwrap_or("?");
    let kind = match record["subtype"].as_str() {
        Some(subtype) => format!("{kind}/{subtype}"),
        None => kind.to_owned(),
    };
    let content = &record["message"]["content"];
    let text = content
        .as_str()
        .or_else(|| content[0]["text"].as_str())
        .unwrap_or_default();
    format!("    {kind}: {}\n", said(text))
}

/// A conversation of a stand-in agent, as lines: how many records it holds
/// and the last of them.
fn conversation_lines(name: &str, text: &str) -> String {
    let records: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let mut shown = format!("  projects/integration/{name}: {} records", records.len());
    shown.push_str(if records.is_empty() {
        "\n"
    } else {
        ", the last:\n"
    });
    for record in &records[records.len().saturating_sub(RECORDS)..] {
        shown.push_str(&record_line(record));
    }
    shown
}

/// The files in `folder` named `*suffix`, by name: each name and what the file
/// holds.
fn files_in(folder: &Path, suffix: &str) -> Vec<(String, String)> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut found: Vec<(String, String)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let held = fs::read(entry.path()).ok()?;
            name.ends_with(suffix)
                .then(|| (name, String::from_utf8_lossy(&held).into_owned()))
        })
        .collect();
    found.sort();
    found
}

/// What the stand-in agents wrote of themselves under the Claude config folder
/// `config`: each status file and the last records of each conversation. It is
/// all the daemon reads of a window.
fn written(config: &Path) -> String {
    let mut text = String::new();
    for (name, held) in files_in(&config.join("sessions"), ".json") {
        text.push_str(&status_line(&name, &held, process::is_alive));
    }
    for (name, held) in files_in(&config.join("projects").join("integration"), ".jsonl") {
        text.push_str(&conversation_lines(&name, &held));
    }
    if text.is_empty() {
        text.push_str("  nothing\n");
    }
    text
}

/// A lane of the board, as a line: who it is, what it is doing and why, and the
/// window it has.
fn lane_line(lane: &Value) -> String {
    let activity = &lane["activity"];
    let because = activity["reason"]
        .as_str()
        .map(|reason| format!(" ({})", said(reason)))
        .unwrap_or_default();
    format!(
        "  @{}: {}{because}; window {}\n",
        lane["participant"]["handle"].as_str().unwrap_or("?"),
        activity["state"].as_str().unwrap_or("?"),
        lane["pane"]["id"].as_str().unwrap_or("none")
    )
}

/// A message of an inbox, as a line: its number, kind, task, state and sender,
/// and the reason it was given for its state.
fn message_line(message: &Value) -> String {
    let task = message["taskNumber"]
        .as_i64()
        .map(|number| format!(" T-{number}"))
        .unwrap_or_default();
    let because = message["reason"]
        .as_str()
        .map(|reason| format!(" ({})", said(reason)))
        .unwrap_or_default();
    format!(
        "    m-{} {}{task} {} from @{}{because}\n",
        message["id"],
        message["kind"].as_str().unwrap_or("?"),
        message["state"].as_str().unwrap_or("?"),
        message["sender"].as_str().unwrap_or("ConsensFlow")
    )
}

/// What the daemon says of its projects, asked as the page asks: each lane, and
/// the newest messages in the inbox of each participant, with the state of each;
/// and what waits at the gate.
fn said_by_the_daemon(rig: &Rig) -> String {
    if !process::is_alive(rig.daemon.id()) {
        return "  the daemon is gone\n".to_owned();
    }
    let Ok(listed) = rig.page("projects.list", json!({})) else {
        return "  projects.list was not answered\n".to_owned();
    };
    let mut text = String::new();
    for project in listed["projects"].as_array().into_iter().flatten() {
        let id = &project["id"];
        text.push_str(&format!(
            "  project {id} ({}):\n",
            project["state"].as_str().unwrap_or("?")
        ));
        let Ok(board) = rig.page("board.get", json!({ "project": id })) else {
            text.push_str("  board.get was not answered\n");
            continue;
        };
        for lane in board["board"]["lanes"].as_array().into_iter().flatten() {
            text.push_str(&lane_line(lane));
            let asked = json!({ "project": id, "participant": lane["participant"]["handle"] });
            let Ok(inbox) = rig.page("inbox.get", asked) else {
                continue;
            };
            let messages = inbox["messages"].as_array().cloned().unwrap_or_default();
            for message in &messages[messages.len().saturating_sub(MESSAGES)..] {
                text.push_str(&message_line(message));
            }
        }
        let gated = board["board"]["gated"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if !gated.is_empty() {
            text.push_str("  at the gate:\n");
        }
        for message in &gated {
            text.push_str(&message_line(message));
        }
    }
    if text.is_empty() {
        text.push_str("  no project\n");
    }
    text
}

/// Says on the standard error what the rig showed: for a case that failed
/// with the rig still up.
pub(super) fn say(rig: &Rig) {
    let mut report = String::from("\n--- the rig, as the failed case left it\n");
    report.push_str(&format!("daemon stderr: {}\n", rig.daemon.errors().trim()));
    report.push_str(&format!("pane host stderr: {}\n", rig.host.errors().trim()));
    let log = rig.log();
    let lines: Vec<&str> = log.lines().collect();
    report.push_str("daemon log, the last lines:\n");
    for line in &lines[lines.len().saturating_sub(LOG)..] {
        report.push_str(&format!("  {line}\n"));
    }
    report.push_str("what the stand-in agents wrote:\n");
    report.push_str(&written(Path::new(
        rig.var("CLAUDE_CONFIG_DIR").unwrap_or_default(),
    )));
    report.push_str("what the daemon says:\n");
    report.push_str(&said_by_the_daemon(rig));
    let (windows, exits, stray) = {
        let seen = rig.shared.seen();
        let mut ids: Vec<&String> = seen.printed.keys().collect();
        ids.sort();
        let windows: Vec<(String, String)> = ids
            .into_iter()
            .map(|id| (id.clone(), plain(&seen.printed[id])))
            .collect();
        (windows, seen.exits.clone(), seen.stray.clone())
    };
    for (id, screen) in windows {
        let tail: String = screen
            .chars()
            .rev()
            .take(SCREEN)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        report.push_str(&format!("--- {id}: {tail}\n"));
    }
    report.push_str(&format!("exits={}\n", serde_json::Value::from(exits)));
    if !stray.is_empty() {
        report.push_str(&format!("lines that are no frame: {stray:?}\n"));
    }
    let _ = std::io::stderr().write_all(told(rig.masker.as_ref(), report).as_bytes());
}

/// What the rig says of itself, `report`, as it is said: through its owner's masker
/// where it was given one, else as it is.
fn told(masker: Option<&super::Masker>, report: String) -> String {
    match masker {
        Some(masker) => masker.shown(&report),
        None => report,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_rig_says_of_itself_goes_through_its_owners_masker_and_as_it_is_without_one() {
        let report = "--- p1-chief: export DB_PASSWORD=hunter2\n".to_owned();
        let masker = crate::rig::Masker::new(|text| text.replace("hunter2", "[masked]"));
        assert_eq!(
            told(Some(&masker), report.clone()),
            "--- p1-chief: export DB_PASSWORD=[masked]\n"
        );
        assert_eq!(told(None, report.clone()), report);
        // Given by the owner on the rig's configuration, and carried with it.
        let config = crate::rig::Config::new("/bin/stand-in").masked_by(masker);
        let carried = config.clone().masker.unwrap();
        assert_eq!(carried.shown("hunter2"), "[masked]");
        assert_eq!(format!("{carried:?}"), "Masker");
    }

    #[test]
    fn control_sequences_are_stripped_and_the_text_between_them_is_left() {
        let bytes = b"\x1b[1;32mgreen\x1b[0m and \x1b[?25lplain\x1b]0;a title\x07!\r\n";
        assert_eq!(plain(bytes), "green and plain!\n");
    }

    #[test]
    fn a_cursor_move_right_is_the_blanks_it_stands_for_and_a_jump_a_line_break() {
        assert_eq!(plain(b"a\x1b[3Cb"), "a   b");
        assert_eq!(plain(b"a\x1b[Cb"), "a b");
        assert_eq!(plain(b"a\x1b[0Cb"), "a b", "a move of none is one blank");
        assert_eq!(plain(b"a\x1b[9999Cb").len(), 2 + 512);
        assert_eq!(
            plain(b"one\x1b[2;1Htwo\x1b[Hthree\x1b[4;5fend"),
            "one\ntwo\nthree\nend"
        );
    }

    #[test]
    fn bytes_that_are_not_text_are_the_replacement_character() {
        assert_eq!(plain(b"caf\xc3\xa9 \xff"), "caf\u{e9} \u{fffd}");
        assert_eq!(plain(b""), "");
    }

    #[test]
    fn what_is_said_is_a_line_and_no_longer_than_a_hundred_characters() {
        assert_eq!(said("one\ntwo\r\nthree"), "one two  three");
        assert_eq!(said(&"é".repeat(100)), "é".repeat(100));
        assert_eq!(said(&"é".repeat(101)), format!("{}…", "é".repeat(100)));
    }

    #[test]
    fn a_status_file_says_its_process_whether_that_is_there_what_it_says_and_the_conversation() {
        let text = r#"{"pid":4242,"sessionId":"sess-1","kind":"interactive","status":"busy"}"#;
        assert_eq!(
            status_line("4242.json", text, |pid| pid == 4242),
            "  sessions/4242.json: process 4242 is there; says busy on sess-1\n"
        );
        assert_eq!(
            status_line("4242.json", text, |_| false),
            "  sessions/4242.json: process 4242 is gone; says busy on sess-1\n"
        );
        assert_eq!(
            status_line("7.json", "{", |_| true),
            "  sessions/7.json: not JSON: {\n"
        );
        assert_eq!(
            status_line("8.json", "{}", |_| true),
            "  sessions/8.json: no process; says nothing on no conversation\n"
        );
    }

    #[test]
    fn a_record_is_its_type_and_what_it_says_in_one_line() {
        let user = r#"{"type":"user","message":{"role":"user","content":"DISPATCH a\nb"}}"#;
        assert_eq!(record_line(user), "    user: DISPATCH a b\n");
        let answer = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"dispatched: T-1"}]}}"#;
        assert_eq!(record_line(answer), "    assistant: dispatched: T-1\n");
        let system = r#"{"type":"system","subtype":"stop_hook_summary"}"#;
        assert_eq!(record_line(system), "    system/stop_hook_summary: \n");
        assert_eq!(record_line("{broken"), "    not JSON: {broken\n");
    }

    #[test]
    fn a_conversation_says_how_many_records_it_holds_and_shows_the_last_four() {
        let record = |n: usize| format!(r#"{{"type":"user","message":{{"content":"turn {n}"}}}}"#);
        let text = (1..=6).map(record).collect::<Vec<_>>().join("\n");
        assert_eq!(
            conversation_lines("s.jsonl", &text),
            "  projects/integration/s.jsonl: 6 records, the last:\n\
             \x20   user: turn 3\n    user: turn 4\n    user: turn 5\n    user: turn 6\n"
        );
        assert_eq!(
            conversation_lines("s.jsonl", "\n"),
            "  projects/integration/s.jsonl: 0 records\n"
        );
    }

    #[test]
    fn what_the_agents_wrote_is_read_from_the_config_folder_by_the_names_of_the_files() {
        let config = tempfile::tempdir().unwrap();
        assert_eq!(written(config.path()), "  nothing\n");
        let at = |relative: &str| config.path().join(relative);
        fs::create_dir_all(at("sessions")).unwrap();
        fs::create_dir_all(at("projects/integration")).unwrap();
        // A process id no 32-bit integer holds is a process that is gone.
        fs::write(
            at("sessions/2.json"),
            r#"{"pid":4000000000,"sessionId":"b","status":"idle"}"#,
        )
        .unwrap();
        fs::write(
            at("sessions/1.json"),
            format!(
                r#"{{"pid":{},"sessionId":"a","status":"busy"}}"#,
                std::process::id()
            ),
        )
        .unwrap();
        fs::write(at("sessions/notes.txt"), "not a status").unwrap();
        fs::write(
            at("projects/integration/a.jsonl"),
            "{\"type\":\"user\",\"message\":{\"content\":\"hello\"}}\n",
        )
        .unwrap();
        assert_eq!(
            written(config.path()),
            format!(
                "  sessions/1.json: process {} is there; says busy on a\n\
                 \x20 sessions/2.json: process 4000000000 is gone; says idle on b\n\
                 \x20 projects/integration/a.jsonl: 1 records, the last:\n\
                 \x20   user: hello\n",
                std::process::id()
            )
        );
    }

    #[test]
    fn a_lane_says_who_it_is_what_it_does_and_why_and_its_window() {
        let lane = json!({
            "participant": { "handle": "chief" },
            "activity": { "state": "working", "reason": "a turn" },
            "pane": { "id": "p1-chief" },
        });
        assert_eq!(
            lane_line(&lane),
            "  @chief: working (a turn); window p1-chief\n"
        );
        let bare = json!({
            "participant": { "handle": "zeus" },
            "activity": { "state": "idle", "reason": null },
        });
        assert_eq!(lane_line(&bare), "  @zeus: idle; window none\n");
    }

    #[test]
    fn a_message_says_its_number_kind_task_state_and_sender_and_what_it_was_told() {
        let result = json!({
            "id": 7, "kind": "result", "taskNumber": 1, "state": "queued",
            "sender": "zeus", "reason": null,
        });
        assert_eq!(
            message_line(&result),
            "    m-7 result T-1 queued from @zeus\n"
        );
        let note = json!({
            "id": 8, "kind": "note", "taskNumber": null, "state": "cancelled",
            "sender": null, "reason": "T-1 was accepted",
        });
        assert_eq!(
            message_line(&note),
            "    m-8 note cancelled from @ConsensFlow (T-1 was accepted)\n"
        );
    }
}

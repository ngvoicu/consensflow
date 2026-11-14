//! What a failed case tells of the rig: the windows' screens, the programs'
//! error output and the daemon's log, for the person who reads the failure; what
//! the stand-in agents wrote of themselves, which is all the daemon reads of a
//! window; and what the daemon says of its lanes and inboxes. It is kept in a file
//! beside the rig's home, on the machine it ran on, and the error output says only
//! where. And how a window's bytes become the text on its screen.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use serde_json::{json, Value};

use super::Rig;
use crate::{files, pattern, process};

/// How much of a window's screen a failure shows.
const SCREEN: usize = 1_500;

/// How much of the end of what a window printed a failure reads, from the start of a
/// line, to show the end of its screen: a window may have printed hours of it (T-139),
/// and it reads only a bounded tail.
const READ: usize = 64 * 1024;

/// How many of the last lines of the daemon's log a failure shows.
const LOG: usize = 30;

/// The kinds of event that say why a delivery failed, was retried or was cancelled: each
/// carries the daemon's own reason.
const FAILURE_KINDS: [&str; 3] = ["delivery.failed", "delivery.retried", "delivery.cancelled"];

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
    let text = pattern::once(&TITLE, r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)").replace_all(&text, "");
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
/// is still there, what the file says it does, and the conversation it shows. What is
/// cut of it is told whole first.
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
/// and what it says, read out of its JSON and told whole before it is cut (told as
/// JSON, an escaped line break in it would hide a secret's shape from the mask).
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
/// `config`: each status file and the last records of each conversation, each text read
/// out of its file and told whole before any of it is cut. It is all the daemon reads
/// of a window.
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
/// and what waits at the gate. Each text of an answer is told whole before any of
/// it is cut. Not asked once a close was tried: the daemon's input is gone then, and
/// where it is there still it answers nothing (ten seconds waited for in each question).
fn said_by_the_daemon(rig: &Rig) -> String {
    if !process::is_alive(rig.daemon.id()) {
        return "  the daemon is gone\n".to_owned();
    }
    if rig.close_failed {
        return "  not asked: the rig's close was tried, and the daemon's input is gone\n"
            .to_owned();
    }
    let page = |method: &str, body: Value| rig.page(method, body);
    let Ok(listed) = page("projects.list", json!({})) else {
        return "  projects.list was not answered\n".to_owned();
    };
    let mut text = String::new();
    for project in listed["projects"].as_array().into_iter().flatten() {
        let id = &project["id"];
        text.push_str(&format!(
            "  project {id} ({}):\n",
            project["state"].as_str().unwrap_or("?")
        ));
        let Ok(board) = page("board.get", json!({ "project": id })) else {
            text.push_str("  board.get was not answered\n");
            continue;
        };
        for lane in board["board"]["lanes"].as_array().into_iter().flatten() {
            text.push_str(&lane_line(lane));
            let asked = json!({ "project": id, "participant": lane["participant"]["handle"] });
            let Ok(inbox) = page("inbox.get", asked) else {
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

/// What a rig dropped unclosed has to say, written while it is up and said when its case
/// is known to have failed (`outcome::leave`).
pub(super) struct Left {
    root: PathBuf,
    case: String,
    report: String,
    folder: Option<PathBuf>,
}

impl Left {
    /// Says on the standard error where the report is kept and which case left it. Through
    /// `eprint!`, which a test's output holds for the case that failed and says nothing of
    /// for one that passed.
    #[allow(clippy::print_stderr)] // Said in the output of the test that failed.
    pub(super) fn say(self) {
        let where_kept = keep(&self.root, &self.case, &self.report, self.folder.as_deref());
        eprint!("{where_kept}");
    }
}

/// What the rig shows of itself, as a failed case left it with the rig still up, told
/// as the rig left it.
pub(super) fn left_by(rig: &Rig) -> Left {
    let report = told(rig);
    Left {
        root: rig.root.clone(),
        case: rig.case.clone(),
        report,
        folder: rig.reports.clone(),
    }
}

/// What the rig showed of a failed case, as its report says it: the windows' screens, the
/// programs' error output, the daemon's failure events and log, what the stand-in agents
/// wrote, and what the daemon says.
pub(super) fn report_of(rig: &Rig) -> String {
    told(rig)
}

/// The report of a failed case (see [`left_by`]).
fn told(rig: &Rig) -> String {
    let mut report = format!(
        "\n--- the rig, as the failed case left it\ncase: {}\n",
        rig.case
    );
    report.push_str(&format!("daemon stderr: {}\n", rig.daemon.errors().trim()));
    report.push_str(&format!("pane host stderr: {}\n", rig.host.errors().trim()));
    report.push_str(
        "the failure events of the run, from its event file (ids, kinds and the daemon's own reasons):\n",
    );
    report.push_str(&failure_lines(&rig.events()));
    report.push_str("daemon log, the last lines:\n");
    report.push_str(&log_lines(&rig.log()));
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
        let windows: Vec<(String, Vec<u8>)> = ids
            .into_iter()
            .map(|id| (id.clone(), end_of(&seen.printed[id].kept, READ).to_vec()))
            .collect();
        (windows, seen.exits.clone(), seen.stray.clone())
    };
    for (id, printed) in windows {
        report.push_str(&window_line(&id, &plain(&printed)));
    }
    report.push_str(&exits_line(&exits));
    if !stray.is_empty() {
        report.push_str(&format!("lines that are no frame: {stray:?}\n"));
    }
    report
}

/// Keeps `report`, what a rig dropped unclosed says of itself, in a file beside its home
/// `root` (the home goes with the rig), or in `folder` where the rig is given one
/// ([`Config::reports_in`]), on the machine it ran on; and what its error output says
/// instead: where the report is and the `case` that left it (a test's name, so that a line
/// of a log points at the case), and none of its words. Whatever runs a rig (an agent's
/// tool, another machine's runner) takes its error output off the machine, and the report
/// holds what its windows showed.
fn keep(root: &Path, case: &str, report: &str, folder: Option<&Path>) -> String {
    let name = root
        .file_name()
        .map_or_else(|| "rig".into(), |name| name.to_string_lossy());
    let file = match folder {
        Some(folder) => folder.join(format!("{name}-left.txt")),
        None => root.with_file_name(format!("{name}-left.txt")),
    };
    match files::write(&file, report) {
        Ok(()) => format!(
            "\n--- the rig, as the failed case left it, is kept in {} (case: {case})\n",
            file.display()
        ),
        Err(unkept) => {
            format!(
                "\n--- the rig, as the failed case left it, could not be kept: {unkept} (case: {case})\n"
            )
        }
    }
}

/// The failure events among `events`, each a line with its place in the file, its kind, its
/// project and its message, and the daemon's own reason, set on one
/// line. No other part of an event is said: a window's screen and a conversation's words are
/// none of it.
fn failure_lines(events: &[Value]) -> String {
    let failures: Vec<&Value> = events
        .iter()
        .filter(|event| {
            event["kind"]
                .as_str()
                .is_some_and(|kind| FAILURE_KINDS.contains(&kind))
        })
        .collect();
    if failures.is_empty() {
        return "  none\n".to_owned();
    }
    let mut text = String::new();
    for event in failures {
        let reason = event["data"]["reason"].as_str().unwrap_or_default();
        text.push_str(&format!(
            "  {} {} project {} message {}: {}\n",
            event["n"],
            event["kind"].as_str().unwrap_or("?"),
            event["project"],
            event["data"]["message"],
            reason.replace('\n', " "),
        ));
    }
    text
}

/// The last [`LOG`] lines of the daemon's log `log`, told whole first, each a line
/// of the report.
fn log_lines(log: &str) -> String {
    let lines: Vec<&str> = log.lines().collect();
    lines[lines.len().saturating_sub(LOG)..]
        .iter()
        .map(|line| format!("  {line}\n"))
        .collect()
}

/// The window `id` with the last [`SCREEN`] characters of its screen `screen`, told
/// as one screen first.
fn window_line(id: &str, screen: &str) -> String {
    let kept: Vec<char> = screen.chars().rev().take(SCREEN).collect();
    let tail: String = kept.into_iter().rev().collect();
    format!("--- {id}: {tail}\n")
}

/// The end of what a window printed, `printed`: its last `most` bytes, from the start of
/// the first line that begins in them where one does (a line's start ends no character
/// and no control sequence), all of it where it is no longer.
fn end_of(printed: &[u8], most: usize) -> &[u8] {
    if printed.len() <= most {
        return printed;
    }
    let tail = &printed[printed.len() - most..];
    tail.iter()
        .position(|byte| *byte == b'\n')
        .map_or(tail, |at| &tail[at + 1..])
}

/// Every exit the host told.
fn exits_line(exits: &[Value]) -> String {
    format!("exits={}\n", Value::from(exits.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT_SECRET: &str = "private report words held in the file";

    #[test]
    #[ignore = "the report output test runs this case in its own process"]
    fn a_nested_dropped_rig_report_is_said() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().join("a-rig");
        Left {
            root,
            case: "a-case".into(),
            report: REPORT_SECRET.into(),
            folder: None,
        }
        .say();
        assert_eq!(
            fs::read_to_string(folder.path().join("a-rig-left.txt")).unwrap(),
            REPORT_SECRET
        );
    }

    #[test]
    fn t1655_a_dropped_rig_prints_only_where_its_report_was_kept() {
        let executable = std::env::current_exe().unwrap();
        let ran = crate::process::case_of(
            executable,
            "rig::report::tests::a_nested_dropped_rig_report_is_said",
            std::time::Duration::from_secs(30),
        )
        .inheriting_env()
        .args(["--ignored", "--nocapture"])
        .run()
        .unwrap();
        assert_eq!(ran.code, Some(0), "{ran}");
        assert!(
            ran.stderr.contains("a-rig-left.txt (case: a-case)"),
            "{ran}"
        );
        assert!(!ran.stderr.contains(REPORT_SECRET), "{ran}");
    }

    #[test]
    fn a_dropped_rigs_report_is_kept_beside_its_home_and_its_error_output_says_only_where() {
        // A screen with a value no masker knows, its name cut off above the window's tail.
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().join("consensflow-integration-AbC123");
        let report = "--- p1-long-claude-1: \"correct horse battery staple\"\n";
        let case = "a_suite::a_case_that_left_it";
        let said = keep(&root, case, report, None);
        let file = folder
            .path()
            .join("consensflow-integration-AbC123-left.txt");
        assert_eq!(fs::read_to_string(&file).unwrap(), report);
        assert_eq!(
            said,
            format!(
                "\n--- the rig, as the failed case left it, is kept in {} (case: {case})\n",
                file.display()
            )
        );
        assert!(!said.contains("horse"), "{said}");
        // Given a folder, the file is in it, and the folder is made.
        let reports = folder.path().join("reports");
        let said = keep(&root, case, report, Some(&reports));
        let file = reports.join("consensflow-integration-AbC123-left.txt");
        assert_eq!(fs::read_to_string(&file).unwrap(), report);
        assert!(said.contains(&file.display().to_string()), "{said}");
        // A file that cannot be written: said so, with none of the report either.
        let said = keep(&file.join("home"), case, report, None);
        assert!(said.contains("could not be kept"), "{said}");
        assert!(said.ends_with(&format!("(case: {case})\n")), "{said}");
        assert!(!said.contains("horse"), "{said}");
    }

    #[test]
    fn a_failure_lists_each_failure_event_by_its_ids_and_the_daemons_reason_and_nothing_else() {
        let events = vec![
            json!({ "n": 1, "at": "t", "kind": "window.activity", "participant": "worker", "state": "idle" }),
            json!({ "n": 2, "at": "t", "kind": "delivery.retried", "project": 1, "data": { "message": 5, "reason": "the window never showed its first message" } }),
            json!({ "n": 3, "at": "t", "kind": "message.sent", "project": 1, "data": { "message": 6, "body": "a transcript line" } }),
            json!({ "n": 4, "at": "t", "kind": "delivery.failed", "project": 1, "data": { "message": 5, "reason": "the window never showed its first message; its last screen lines named a secret, so they are left out" } }),
        ];
        assert_eq!(
            failure_lines(&events),
            concat!(
                "  2 delivery.retried project 1 message 5: the window never showed its first message\n",
                "  4 delivery.failed project 1 message 5: the window never showed its first message; its last screen lines named a secret, so they are left out\n",
            )
        );
        assert_eq!(failure_lines(&events[..1]), "  none\n");
    }

    #[test]
    fn the_end_of_what_a_window_printed_is_read_from_the_start_of_a_line() {
        // T-139: a failure's report made a string of every line a window had printed.
        assert_eq!(end_of(b"one\ntwo\nthree", 64), b"one\ntwo\nthree");
        assert_eq!(end_of(b"one\ntwo\nthree", 9), b"three");
        assert_eq!(
            end_of(b"one\ntwo\nthree", 4),
            b"hree",
            "no line begins in it"
        );
    }

    #[test]
    fn control_sequences_are_stripped_and_the_text_between_them_is_left() {
        let bytes = b"\x1b[1;32mgreen\x1b[0m and \x1b[?25lplain\x1b]0;a title\x07!\r\n";
        assert_eq!(plain(bytes), "green and plain!\n");
    }

    // Claude Code 2.1.312 on Windows titles its window with a string ended by ST (`ESC \`), not by
    // BEL: the title took everything up to the next BEL for itself, and a resumed window's whole
    // screen (2,024 bytes) read as 153 blanks (zeewin2, 2026-10-28: `live resume-cut`).
    #[test]
    fn a_title_ended_by_the_string_terminator_ends_there_and_takes_none_of_what_follows() {
        let bytes = b"\x1b]0;claude\x1b\\hello\x1b[?25l world\x1b]7501;?\x07!";
        assert_eq!(plain(bytes), "hello world!");
        // Either ends a title, and a title with nothing in it is none.
        assert_eq!(
            plain(b"a\x1b]0;one\x07b\x1b]2;two\x1b\\c\x1b]0;\x07d"),
            "abcd"
        );
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

//! What a failed case tells of the rig: the windows' screens, the programs'
//! error output and the daemon's log, for the person who reads the failure; what
//! the stand-in agents wrote of themselves, which is all the daemon reads of a
//! window; and what the daemon says of its lanes and inboxes. It is kept in a file
//! beside the rig's home, on the machine it ran on, and the error output says only
//! where. And how a window's bytes become the text on its screen.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::{json, Value};

use super::Rig;
use crate::{files, pattern, process};

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
/// is still there, what the file says it does, and the conversation it shows. What is
/// cut of it is told whole first.
fn status_line(name: &str, text: &str, there: impl Fn(u32) -> bool, tell: &Tell) -> String {
    let Ok(status) = serde_json::from_str::<Value>(text) else {
        return format!("  sessions/{name}: not JSON: {}\n", said(&tell.text(text)));
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
fn record_line(line: &str, tell: &Tell) -> String {
    let Ok(record) = serde_json::from_str::<Value>(line) else {
        return format!("    not JSON: {}\n", said(&tell.text(line)));
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
    format!("    {kind}: {}\n", said(&tell.text(text)))
}

/// A conversation of a stand-in agent, as lines: how many records it holds
/// and the last of them.
fn conversation_lines(name: &str, text: &str, tell: &Tell) -> String {
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
        shown.push_str(&record_line(record, tell));
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
fn written(config: &Path, tell: &Tell) -> String {
    let mut text = String::new();
    for (name, held) in files_in(&config.join("sessions"), ".json") {
        text.push_str(&status_line(&name, &held, process::is_alive, tell));
    }
    for (name, held) in files_in(&config.join("projects").join("integration"), ".jsonl") {
        text.push_str(&conversation_lines(&name, &held, tell));
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
/// it is cut.
fn said_by_the_daemon(rig: &Rig, tell: &Tell) -> String {
    if !process::is_alive(rig.daemon.id()) {
        return "  the daemon is gone\n".to_owned();
    }
    let page = |method: &str, body: Value| rig.page(method, body).map(|answer| tell.value(&answer));
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

/// Says on the standard error what the rig showed: for a case that failed
/// with the rig still up.
pub(super) fn say(rig: &Rig) {
    let tell = Tell(rig.masker.as_ref());
    let mut report = String::from("\n--- the rig, as the failed case left it\n");
    report.push_str(&format!("daemon stderr: {}\n", rig.daemon.errors().trim()));
    report.push_str(&format!("pane host stderr: {}\n", rig.host.errors().trim()));
    report.push_str("daemon log, the last lines:\n");
    report.push_str(&log_lines(&rig.log(), &tell));
    report.push_str("what the stand-in agents wrote:\n");
    report.push_str(&written(
        Path::new(rig.var("CLAUDE_CONFIG_DIR").unwrap_or_default()),
        &tell,
    ));
    report.push_str("what the daemon says:\n");
    report.push_str(&said_by_the_daemon(rig, &tell));
    // Read under the lock, told after it: an owner's masker reads what the windows
    // printed under the same lock.
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
        report.push_str(&window_line(&id, &screen, &tell));
    }
    report.push_str(&exits_line(&exits, &tell));
    if !stray.is_empty() {
        report.push_str(&format!("lines that are no frame: {stray:?}\n"));
    }
    let where_kept = keep(&rig.root, &tell.text(&report));
    let _ = std::io::stderr().write_all(where_kept.as_bytes());
}

/// Keeps `report`, what a rig dropped unclosed says of itself, in a file beside its home
/// `root` (the home goes with the rig), on the machine it ran on; and what its error
/// output says instead: where the report is, and none of its words. Whatever runs a rig
/// (an agent's tool, another machine's runner) takes its error output off the machine,
/// and the report holds what its windows showed.
fn keep(root: &Path, report: &str) -> String {
    let name = root
        .file_name()
        .map_or_else(|| "rig".into(), |name| name.to_string_lossy());
    let file = root.with_file_name(format!("{name}-left.txt"));
    match files::write(&file, report) {
        Ok(()) => format!(
            "\n--- the rig, as the failed case left it, is kept in {}\n",
            file.display()
        ),
        Err(unkept) => {
            format!("\n--- the rig, as the failed case left it, could not be kept: {unkept}\n")
        }
    }
}

/// The last [`LOG`] lines of the daemon's log `log`, told whole first, each a line
/// of the report.
fn log_lines(log: &str, tell: &Tell) -> String {
    let log = tell.text(log);
    let lines: Vec<&str> = log.lines().collect();
    lines[lines.len().saturating_sub(LOG)..]
        .iter()
        .map(|line| format!("  {line}\n"))
        .collect()
}

/// The window `id` with the last [`SCREEN`] characters of its screen `screen`, told
/// as one screen first.
fn window_line(id: &str, screen: &str, tell: &Tell) -> String {
    let screen = tell.screen_text(screen);
    let kept: Vec<char> = screen.chars().rev().take(SCREEN).collect();
    let tail: String = kept.into_iter().rev().collect();
    format!("--- {id}: {tail}\n")
}

/// Every exit the host told, each told as [`Tell::exit`] says.
fn exits_line(exits: &[Value], tell: &Tell) -> String {
    let told: Vec<Value> = exits.iter().map(|exit| tell.exit(exit)).collect();
    format!("exits={}\n", Value::from(told))
}

/// How the rig says what it shows of itself: through its owner's masker where it was
/// given one, each text whole and each window's screen as one text, before any of it
/// is cut (a secret's name cut off would leave its value unhidden, and a token split
/// across two of a screen's rows, each told apart, both its halves); as it is without
/// one.
struct Tell<'m>(Option<&'m super::Masker>);

impl Tell<'_> {
    /// `text` as it is said.
    fn text(&self, text: &str) -> String {
        self.0
            .map_or_else(|| text.to_owned(), |masker| masker.shown(text))
    }

    /// The rows of a window's screen as they are said, told as one text.
    fn screen(&self, rows: &[String]) -> Vec<String> {
        self.0
            .map_or_else(|| rows.to_vec(), |masker| masker.screen(rows))
    }

    /// A window's screen, `screen`, as it is said: its lines told as one screen's rows.
    fn screen_text(&self, screen: &str) -> String {
        if self.0.is_none() {
            return screen.to_owned();
        }
        let rows: Vec<String> = screen.lines().map(str::to_owned).collect();
        self.screen(&rows).join("\n")
    }

    /// A window's exit as it is said: the last lines of its screen it carries told as
    /// one screen's rows.
    fn exit(&self, exit: &Value) -> Value {
        let mut told = exit.clone();
        let rows: Option<Vec<String>> = exit["tail"].as_array().and_then(|rows| {
            rows.iter()
                .map(|row| row.as_str().map(str::to_owned))
                .collect()
        });
        if let Some(rows) = rows {
            told["tail"] = json!(self.screen(&rows));
        }
        told
    }

    /// An answer of the daemon's as it is said: each of its texts told whole.
    fn value(&self, value: &Value) -> Value {
        match value {
            Value::String(text) => Value::String(self.text(text)),
            Value::Array(items) => items.iter().map(|item| self.value(item)).collect(),
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(name, field)| (name.clone(), self.value(field)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dropped_rigs_report_is_kept_beside_its_home_and_its_error_output_says_only_where() {
        // A screen with a value no masker knows, its name cut off above the window's tail.
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().join("consensflow-integration-AbC123");
        let report = "--- p1-long-claude-1: \"correct horse battery staple\"\n";
        let said = keep(&root, report);
        let file = folder
            .path()
            .join("consensflow-integration-AbC123-left.txt");
        assert_eq!(fs::read_to_string(&file).unwrap(), report);
        assert_eq!(
            said,
            format!(
                "\n--- the rig, as the failed case left it, is kept in {}\n",
                file.display()
            )
        );
        assert!(!said.contains("horse"), "{said}");
        // A file that cannot be written: said so, with none of the report either.
        let said = keep(&file.join("home"), report);
        assert!(said.contains("could not be kept"), "{said}");
        assert!(!said.contains("horse"), "{said}");
    }

    /// A masker as an owner's is, by what is around a secret: the quoted value of
    /// `DB_PASSWORD=` (whatever white space comes between), and `hunter2hunter2`
    /// whole; a screen's rows told as one text, a token the terminal wrapped
    /// (`ghp_` and its letters, run on into the next row) hidden in both.
    fn masker() -> crate::rig::Masker {
        fn text(text: &str) -> String {
            let named = Regex::new(r#"(DB_PASSWORD=\s*)"[^"]*""#).unwrap();
            named
                .replace_all(text, "$1\"[masked]\"")
                .replace("hunter2hunter2", "[masked]")
        }
        crate::rig::Masker::new(text, |rows| {
            let wrapped = Regex::new(r"ghp_[a-z]+\n[a-z0-9]+").unwrap();
            let joined = text(&rows.join("\n"));
            let joined = wrapped.replace_all(&joined, "[masked]\n");
            joined.split('\n').map(str::to_owned).collect()
        })
    }

    #[test]
    fn what_a_rig_says_of_itself_goes_through_its_owners_masker_and_as_it_is_without_one() {
        let masker = masker();
        let tell = Tell(Some(&masker));
        assert_eq!(tell.text("export hunter2hunter2"), "export [masked]");
        let rows = [
            "deploy with ghp_abcdefghij",
            "klmnopqrstuvwxyz0123456789 now",
        ]
        .map(str::to_owned);
        assert_eq!(tell.screen(&rows), ["deploy with [masked]", " now"]);
        let plain = Tell(None);
        assert_eq!(plain.text("export hunter2hunter2"), "export hunter2hunter2");
        assert_eq!(plain.screen(&rows), rows);
        assert_eq!(plain.screen_text("a\nb\n"), "a\nb\n");
        // Given by the owner on the rig's configuration, and carried with it.
        let config = crate::rig::Config::new("/bin/stand-in").masked_by(masker);
        let carried = config.clone().masker.unwrap();
        assert_eq!(carried.shown("hunter2hunter2"), "[masked]");
        assert_eq!(format!("{carried:?}"), "Masker");
    }

    #[test]
    fn a_windows_screen_is_told_whole_before_its_last_characters_are_taken() {
        // Sol's: the last 1,500 characters begin at the value's quote, and its name is
        // before them, at the end of the line above.
        let screen = format!(
            "$ export DB_PASSWORD=\n\"correct horse battery staple\"{}",
            "ok\n".repeat(490)
        );
        let masker = masker();
        let line = window_line("p1-chief", &screen, &Tell(Some(&masker)));
        assert!(!line.contains("horse"), "{line}");
        assert!(line.starts_with("--- p1-chief: "), "{line}");
        // Without a masker, its last 1,500 characters as they are.
        let line = window_line("p1-chief", &screen, &Tell(None));
        assert!(
            line.contains("\"correct horse battery staple\"ok"),
            "{line}"
        );
        assert_eq!(line.chars().count(), "--- p1-chief: \n".len() + SCREEN);
    }

    #[test]
    fn an_exits_last_lines_are_told_as_one_screen_before_they_are_written_as_json() {
        let exits = [json!({
            "id": "p1-chief", "generation": 2, "exitCode": 1,
            "tail": ["deploy with ghp_abcdefghij", "klmnopqrstuvwxyz0123456789 now"],
        })];
        let masker = masker();
        let line = exits_line(&exits, &Tell(Some(&masker)));
        assert!(!line.contains("abcdefghij"), "{line}");
        assert!(!line.contains("klmnop"), "{line}");
        assert!(
            line.contains(r#"["deploy with [masked]"," now"]"#),
            "{line}"
        );
        assert!(line.contains(r#""exitCode":1"#), "{line}");
        assert_eq!(
            exits_line(&exits, &Tell(None)),
            format!("exits={}\n", Value::from(exits.to_vec()))
        );
    }

    #[test]
    fn the_daemons_log_and_what_the_agents_wrote_are_told_whole_before_they_are_cut() {
        // The secret's name is on the 31st line from the end, its value on the 30th.
        let log = format!(
            "DB_PASSWORD=\n\"correct horse battery staple\"{}",
            "\nok".repeat(29)
        );
        let masker = masker();
        let tell = Tell(Some(&masker));
        let lines = log_lines(&log, &tell);
        assert!(!lines.contains("horse"), "{lines}");
        assert!(lines.starts_with("  \"[masked]\"\n  ok\n"), "{lines}");
        assert_eq!(log_lines(&log, &Tell(None)).lines().count(), LOG);
        // A record's words are cut at a hundred characters, after they are told.
        let config = tempfile::tempdir().unwrap();
        let folder = config.path().join("projects").join("integration");
        fs::create_dir_all(&folder).unwrap();
        let words = format!("{}hunter2hunter2", "a".repeat(95));
        fs::write(
            folder.join("a.jsonl"),
            format!("{{\"type\":\"user\",\"message\":{{\"content\":\"{words}\"}}}}\n"),
        )
        .unwrap();
        let wrote = written(config.path(), &tell);
        assert!(!wrote.contains("hunte"), "{wrote}");
        assert!(
            wrote.contains(&format!("user: {}[mask…", "a".repeat(95))),
            "{wrote}"
        );
        assert!(written(config.path(), &Tell(None)).contains("hunte"));
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
        let plain = Tell(None);
        assert_eq!(
            status_line("4242.json", text, |pid| pid == 4242, &plain),
            "  sessions/4242.json: process 4242 is there; says busy on sess-1\n"
        );
        assert_eq!(
            status_line("4242.json", text, |_| false, &plain),
            "  sessions/4242.json: process 4242 is gone; says busy on sess-1\n"
        );
        assert_eq!(
            status_line("7.json", "{", |_| true, &plain),
            "  sessions/7.json: not JSON: {\n"
        );
        assert_eq!(
            status_line("8.json", "{}", |_| true, &plain),
            "  sessions/8.json: no process; says nothing on no conversation\n"
        );
    }

    #[test]
    fn a_record_is_its_type_and_what_it_says_in_one_line() {
        let plain = Tell(None);
        let user = r#"{"type":"user","message":{"role":"user","content":"DISPATCH a\nb"}}"#;
        assert_eq!(record_line(user, &plain), "    user: DISPATCH a b\n");
        let answer = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"dispatched: T-1"}]}}"#;
        assert_eq!(
            record_line(answer, &plain),
            "    assistant: dispatched: T-1\n"
        );
        let system = r#"{"type":"system","subtype":"stop_hook_summary"}"#;
        assert_eq!(
            record_line(system, &plain),
            "    system/stop_hook_summary: \n"
        );
        assert_eq!(record_line("{broken", &plain), "    not JSON: {broken\n");
    }

    #[test]
    fn a_records_words_are_read_out_of_its_json_and_told_before_they_are_cut() {
        // Astra's: told as JSON, the escaped line break between `DB_PASSWORD=` and its
        // value hides the secret's shape from the mask; read out first, it is one.
        let words = format!(
            "{}DB_PASSWORD=\n\"correct horse battery staple\"",
            "x ".repeat(20)
        );
        let line = json!({ "type": "user", "message": { "content": words } }).to_string();
        let masker = masker();
        let said = record_line(&line, &Tell(Some(&masker)));
        assert!(!said.contains("horse"), "{said}");
        assert!(said.contains("DB_PASSWORD= \"[masked]\""), "{said}");
        // And as the rig's report reads it, from the conversation's file.
        let config = tempfile::tempdir().unwrap();
        let folder = config.path().join("projects").join("integration");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("a.jsonl"), format!("{line}\n")).unwrap();
        let wrote = written(config.path(), &Tell(Some(&masker)));
        assert!(!wrote.contains("horse"), "{wrote}");
        assert!(wrote.contains("DB_PASSWORD= \"[masked]\""), "{wrote}");
    }

    #[test]
    fn a_conversation_says_how_many_records_it_holds_and_shows_the_last_four() {
        let plain = Tell(None);
        let record = |n: usize| format!(r#"{{"type":"user","message":{{"content":"turn {n}"}}}}"#);
        let text = (1..=6).map(record).collect::<Vec<_>>().join("\n");
        assert_eq!(
            conversation_lines("s.jsonl", &text, &plain),
            "  projects/integration/s.jsonl: 6 records, the last:\n\
             \x20   user: turn 3\n    user: turn 4\n    user: turn 5\n    user: turn 6\n"
        );
        assert_eq!(
            conversation_lines("s.jsonl", "\n", &plain),
            "  projects/integration/s.jsonl: 0 records\n"
        );
    }

    #[test]
    fn what_the_agents_wrote_is_read_from_the_config_folder_by_the_names_of_the_files() {
        let config = tempfile::tempdir().unwrap();
        assert_eq!(written(config.path(), &Tell(None)), "  nothing\n");
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
            written(config.path(), &Tell(None)),
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

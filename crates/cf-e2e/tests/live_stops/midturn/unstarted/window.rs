//! The windows the daemon opened for the member, said as they are found: how each
//! was opened, how it ended or what its pane host says of it now, and what it
//! printed, as the text of a screen and as it came. A window the daemon gave up on
//! is none of its lane's after, and what it printed is still here.

use cf_e2e::live::members::{plural, squash, Members};
use cf_e2e::live::window::{drawn, raw_end};
use cf_e2e::live::{cut, text};
use serde_json::Value;

use super::super::printed::tail_lines;
use super::super::SCREEN;
use crate::tree::{self, Read, Tree};

/// How many of the last lines a window printed, and how much of each, are said.
const RAW_LINES: usize = 12;
const RAW_WIDTH: usize = 160;

/// How long a word of a command line is said at most.
const WORD: usize = 90;

/// A word of a command line, on one line (the message a window is given with is a word
/// of several): its end where it is long, a `…` before it. A long word is a path, and
/// a path ends in the file it is of.
fn word_of(word: &Value) -> String {
    let word = squash(&text(word), usize::MAX);
    let length = word.chars().count();
    if length <= WORD {
        return word;
    }
    let end: String = word.chars().skip(length - (WORD - 1)).collect();
    format!("…{end}")
}

/// How a window was opened: its command line, each long word cut at its start.
fn launched(open: &Value) -> String {
    open["argv"]
        .as_array()
        .map(|argv| argv.iter().map(word_of).collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
}

/// A screen's lines as one: `a ⏎ b`, or `nothing` where it has none.
fn joined(lines: &[String]) -> String {
    if lines.is_empty() {
        return "nothing".to_owned();
    }
    cut(&lines.join(" ⏎ "), 600)
}

/// How a window ended, from the pane host's `pane.exit` of it, and what its screen
/// showed last.
fn exit_line(exit: &Value) -> String {
    let code = exit["exitCode"].as_u64().map_or_else(
        || "no exit code".to_owned(),
        |code| format!("exit code {code}"),
    );
    let signal = exit["signal"]
        .as_str()
        .map(|signal| format!(", signal {signal}"))
        .unwrap_or_default();
    format!(
        "it ended with {code}{signal}; its screen then ended with: {}",
        joined(&tail_lines(exit))
    )
}

/// What the pane host says of a window it still has: how long it printed nothing,
/// what is on its way in or typed and not sent, and its screen as it draws it.
fn snapshot_lines(snapshot: &Value) -> Vec<String> {
    let printed = match snapshot["outputQuietMs"].as_f64() {
        Some(quiet) => format!("it printed last {quiet:.0} ms ago"),
        None => "it has printed nothing".to_owned(),
    };
    let unsent = snapshot["unsent"]
        .as_str()
        .filter(|unsent| !unsent.is_empty())
        .map_or_else(
            || "nothing typed and unsent".to_owned(),
            |unsent| format!("typed and unsent: {}", cut(unsent, 120)),
        );
    let flag = |name: &str, said: &str| -> String {
        if snapshot[name] == true {
            format!(", {said}")
        } else {
            String::new()
        }
    };
    vec![
        format!(
            "its pane host says: {printed}; {unsent}{}{}",
            flag("pasteInFlight", "a paste is on its way in"),
            flag("inputFailed", "its input failed")
        ),
        format!(
            "its screen as the pane host draws it: {}",
            joined(&tail_lines(snapshot))
        ),
    ]
}

/// What the host's tree has that the window's turn started, as a line: the processes,
/// or why the host could not say.
fn tree_line(read: &Read) -> String {
    format!(
        "its process tree (what began after its turn did): {}",
        read.as_ref().map_or_else(|said| said.clone(), Tree::line)
    )
}

/// What a window printed: how much, as bytes and as the text of a screen (how much
/// of that is no blank), and its last lines as they came.
fn printed_lines(printed: &[u8], cleaned: &str) -> Vec<String> {
    if printed.is_empty() {
        return vec!["it printed nothing: not a byte reached the rig".to_owned()];
    }
    let end = raw_end(printed, RAW_LINES, RAW_WIDTH);
    let mut lines = vec![
        format!(
            "it printed {}; as the text of a screen that is {}, {} of them not blank",
            plural(end.bytes, "byte"),
            plural(cleaned.chars().count(), "character"),
            drawn(cleaned)
        ),
        "its last lines as they came (␛ is Escape, ␍ a carriage return):".to_owned(),
    ];
    lines.extend(end.lines.iter().map(|line| format!("  {line}")));
    lines
}

/// Each window the daemon opened for the member `agent`, said: the first line of
/// a window is its own, and its details follow, two blanks in.
pub fn window_lines(members: &Members, agent: &str) -> Vec<String> {
    let opened = members.windows_opened_for(agent);
    if opened.is_empty() {
        return vec!["the daemon opened no window for it".to_owned()];
    }
    let rig = members.rig();
    let mut lines = Vec::new();
    for open in &opened {
        let id = open["id"].as_str().unwrap_or_default();
        lines.push(format!("its window {id}, opened as: {}", launched(open)));
        let ended = rig
            .exits()
            .into_iter()
            .find(|exit| exit["id"] == open["id"]);
        let mut details = match ended {
            Some(exit) => vec![exit_line(&exit)],
            None => {
                let mut said = rig.snapshot(open, SCREEN).map_or_else(
                    || vec!["its pane host no longer has it".to_owned()],
                    |snapshot| snapshot_lines(&snapshot),
                );
                said.push(tree_line(&tree::read(members, open)));
                said
            }
        };
        details.extend(printed_lines(&rig.printed(id), &rig.screen(id)));
        lines.extend(details.into_iter().map(|line| format!("  {line}")));
    }
    lines
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_window_was_opened_as_its_command_line_each_long_word_cut_at_its_start() {
        let long = format!("C:\\Users\\{}\\prompt.txt", "y".repeat(200));
        let open = json!({ "argv": ["C:\\Users\\x\\devin.EXE", "--prompt-file", long] });
        let said = launched(&open);
        assert!(
            said.starts_with("C:\\Users\\x\\devin.EXE --prompt-file …y"),
            "{said}"
        );
        assert!(said.ends_with("yyy\\prompt.txt"), "{said}");
        assert_eq!(
            said.chars().count(),
            "C:\\Users\\x\\devin.EXE --prompt-file ".len() + WORD
        );
        assert_eq!(launched(&json!({})), "");
        // A word of several lines is one.
        assert_eq!(
            launched(&json!({ "argv": ["claude", "[m-1]\nSLEEP 120  go"] })),
            "claude [m-1] SLEEP 120 go"
        );
    }

    #[test]
    fn a_window_that_ended_says_how_and_what_its_screen_showed_last() {
        let exit =
            json!({ "id": "p1-x", "exitCode": 3, "tail": ["No API key found", "Use /login"] });
        assert_eq!(
            exit_line(&exit),
            "it ended with exit code 3; its screen then ended with: No API key found ⏎ Use /login"
        );
        let killed = json!({ "id": "p1-x", "signal": "SIGKILL", "tail": [] });
        assert_eq!(
            exit_line(&killed),
            "it ended with no exit code, signal SIGKILL; its screen then ended with: nothing"
        );
    }

    #[test]
    fn a_window_the_pane_host_has_is_said_by_how_long_it_printed_nothing_what_is_typed_and_its_screen(
    ) {
        let snapshot = json!({
            "ok": true, "outputQuietMs": 250_000, "unsent": "", "pasteInFlight": false,
            "inputFailed": false, "tail": ["Devin CLI v3000.11.3", "1181", "1182"],
        });
        assert_eq!(
            snapshot_lines(&snapshot),
            [
                "its pane host says: it printed last 250000 ms ago; nothing typed and unsent",
                "its screen as the pane host draws it: Devin CLI v3000.11.3 ⏎ 1181 ⏎ 1182",
            ]
        );
        let stuck = json!({
            "ok": true, "outputQuietMs": null, "unsent": "Write the whole numbers", "pasteInFlight": true,
            "inputFailed": true,
        });
        assert_eq!(
            snapshot_lines(&stuck),
            [
                "its pane host says: it has printed nothing; typed and unsent: Write the whole numbers, a paste is on its way in, its input failed",
                "its screen as the pane host draws it: nothing",
            ]
        );
    }

    #[test]
    fn what_the_hosts_tree_has_of_the_turn_is_a_line_or_why_it_could_not_be_said() {
        let tree = Tree::of(&json!({
            "ok": true, "total": 1, "processes": [{ "pid": 4102, "command": "sleep 119" }],
        }));
        assert_eq!(
            tree_line(&Ok(tree)),
            "its process tree (what began after its turn did): 4102 sleep 119 (1)"
        );
        assert_eq!(
            tree_line(&Ok(Tree::of(
                &json!({ "ok": true, "total": 0, "processes": [] })
            ))),
            "its process tree (what began after its turn did): none"
        );
        assert_eq!(
            tree_line(&Err("(could not be asked: timed out)".to_owned())),
            "its process tree (what began after its turn did): (could not be asked: timed out)"
        );
    }

    #[test]
    fn what_a_window_printed_is_told_as_bytes_and_as_text_and_its_last_lines_as_they_came() {
        assert_eq!(
            printed_lines(b"", ""),
            ["it printed nothing: not a byte reached the rig"]
        );
        // What the console host prints alone: the screen's text is a line break, and no blank is a character.
        let alone = printed_lines(b"\x1b[H\r\n", "\n");
        assert_eq!(
            alone,
            [
                "it printed 5 bytes; as the text of a screen that is 1 character, 0 of them not blank",
                "its last lines as they came (␛ is Escape, ␍ a carriage return):",
                "  ␛[H␍",
            ]
        );
        let drawn = printed_lines(b"\x1b[2;1Hhi\r\n", "\nhi\n");
        assert_eq!(
            drawn[0],
            "it printed 10 bytes; as the text of a screen that is 4 characters, 2 of them not blank"
        );
    }
}

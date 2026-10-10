//! What a failed case tells of the rig: the windows' screens, the programs'
//! error output and the daemon's log, for the person who reads the failure. And
//! how a window's bytes become the text on its screen.

use std::io::Write;
use std::sync::OnceLock;

use regex::Regex;

use super::Rig;
use crate::pattern;

/// How much of a window's screen a failure shows.
const SCREEN: usize = 1_500;

/// How many of the last lines of the daemon's log a failure shows.
const LOG: usize = 30;

/// What the bytes a window printed look like on its screen: control sequences
/// stripped. Windows' console host draws a run of blanks as a cursor move and
/// starts a line by jumping to it: those stay the blanks and the line break
/// they stand for, so that text reads the same on every platform.
fn plain(bytes: &[u8]) -> String {
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
    let _ = std::io::stderr().write_all(report.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

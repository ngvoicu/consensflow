//! What the human types into the TUI's terminal, as Codex's TUI takes it: a line
//! at a time, the text between a bracketed paste's markers whole, and each line
//! a turn. A terminal that is no terminal (a supervisor's test, whose input is
//! nothing) has none to give.

use std::io::Read;
use std::sync::mpsc;
use std::thread;

/// What marks the start and the end of a paste in a terminal that says so.
const PASTE_MARKS: [&str; 2] = ["\u{1b}[200~", "\u{1b}[201~"];

/// What is typed into the terminal, a line at a time, as it comes.
pub fn typed() -> mpsc::Receiver<String> {
    let (lines, typed) = mpsc::channel();
    thread::spawn(move || {
        let mut stdin = std::io::stdin();
        let (mut pending, mut buffer) = (String::new(), [0; 4096]);
        while let Ok(read @ 1..) = stdin.read(&mut buffer) {
            let chunk = String::from_utf8_lossy(&buffer[..read]).into_owned();
            for line in completed(&mut pending, &chunk) {
                if lines.send(line).is_err() {
                    return;
                }
            }
        }
    });
    typed
}

/// The lines `chunk` ends, joined to what `pending` held of the one before it,
/// which then holds what of the next is typed so far. A line is what is typed up
/// to Enter, without the marks of a paste, and one with nothing in it is none.
fn completed(pending: &mut String, chunk: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for typed in chunk.chars() {
        if typed != '\r' && typed != '\n' {
            pending.push(typed);
            continue;
        }
        let line = PASTE_MARKS
            .iter()
            .fold(std::mem::take(pending), |line, mark| line.replace(mark, ""));
        if !line.trim().is_empty() {
            lines.push(line.trim().to_owned());
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_paste_followed_by_enter_is_one_line_without_the_marks_of_the_paste() {
        let mut pending = String::new();
        assert_eq!(
            completed(
                &mut pending,
                "\u{1b}[200~Reply with exactly: X\u{1b}[201~\r"
            ),
            ["Reply with exactly: X"]
        );
        assert_eq!(pending, "");
    }

    #[test]
    fn a_line_typed_in_pieces_is_one_line_once_enter_comes_and_empty_ones_are_none() {
        let mut pending = String::new();
        assert!(completed(&mut pending, "Reply with ").is_empty());
        assert_eq!(pending, "Reply with ");
        assert_eq!(
            completed(&mut pending, "exactly: X\r\n\r\nand then\n"),
            ["Reply with exactly: X", "and then"]
        );
        assert_eq!(pending, "");
    }
}

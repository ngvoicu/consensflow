//! What a window printed, as it came. The text of a screen
//! ([`crate::rig::Rig::screen`]) has the control sequences taken out of it, so a
//! window that printed only what Windows' console host prints of its own accord
//! (a jump of the cursor, a title) reads as blank there, as one that printed
//! nothing does. The bytes tell them apart, with each control character spelled
//! by its picture so that a report can show it.

/// The end of what a window printed, as it came.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEnd {
    /// How many bytes it printed in all.
    pub bytes: usize,
    /// Its last lines, the oldest first. A line ends at a line feed, each control
    /// character in it is its picture (`␛` for Escape, `␍` for a carriage return),
    /// and a line longer than the width keeps its end, after a `…`.
    pub lines: Vec<String>,
}

/// The end of `printed`: its size, and its last `lines` lines, none longer than `width`
/// characters.
pub fn raw_end(printed: &[u8], lines: usize, width: usize) -> RawEnd {
    if printed.is_empty() {
        return RawEnd {
            bytes: 0,
            lines: Vec::new(),
        };
    }
    let text = String::from_utf8_lossy(printed);
    let mut kept: Vec<String> = text
        .split('\n')
        .map(|line| fitted(&spelled(line), width))
        .collect();
    // A last line feed ends the last line, and starts none.
    if text.ends_with('\n') {
        kept.pop();
    }
    kept.drain(..kept.len().saturating_sub(lines));
    RawEnd {
        bytes: printed.len(),
        lines: kept,
    }
}

/// `line` with each control character as its picture (U+2400 and after).
fn spelled(line: &str) -> String {
    line.chars()
        .map(|character| match character {
            '\u{7f}' => '\u{2421}',
            control if control < ' ' => {
                char::from_u32(0x2400 + u32::from(control)).unwrap_or(control)
            }
            other => other,
        })
        .collect()
}

/// `line`, or its last `width` characters after a `…` where it is longer.
fn fitted(line: &str, width: usize) -> String {
    let length = line.chars().count();
    if length <= width {
        return line.to_owned();
    }
    let end: String = line.chars().skip(length - width).collect();
    format!("…{end}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_that_printed_nothing_has_no_bytes_and_no_lines() {
        assert_eq!(
            raw_end(b"", 5, 80),
            RawEnd {
                bytes: 0,
                lines: vec![]
            }
        );
    }

    #[test]
    fn what_the_console_host_prints_alone_is_a_line_of_pictures_where_the_screen_reads_blank() {
        // A jump of the cursor and a title: the cleaned text of this is a line break.
        let printed = b"\x1b[H\x1b]0;devin\x07\r\n";
        let end = raw_end(printed, 5, 80);
        assert_eq!(end.bytes, printed.len());
        assert_eq!(end.lines, ["␛[H␛]0;devin␇␍"]);
    }

    #[test]
    fn the_last_lines_are_kept_oldest_first_and_a_last_line_feed_starts_no_line() {
        let end = raw_end(b"one\ntwo\r\nthree\nfour\n", 3, 80);
        assert_eq!(end.lines, ["two␍", "three", "four"]);
        // Fewer lines than asked for are all there are.
        assert_eq!(raw_end(b"a\nb", 5, 80).lines, ["a", "b"]);
        // A lone line feed is a blank line, and none asked for is none.
        assert_eq!(raw_end(b"\n", 5, 80).lines, [""]);
        assert!(raw_end(b"a\nb", 0, 80).lines.is_empty());
    }

    #[test]
    fn a_long_line_keeps_its_end_and_marks_the_cut() {
        let long = format!("{}end", "x".repeat(100));
        assert_eq!(raw_end(long.as_bytes(), 1, 10).lines, ["…xxxxxxxend"]);
        assert_eq!(raw_end(b"short", 1, 5).lines, ["short"]);
    }

    #[test]
    fn controls_are_pictures_and_what_is_not_text_is_the_replacement_character() {
        let end = raw_end(b"a\x00b\x1b\x7fc \xff\xc3\xa9", 1, 80);
        assert_eq!(end.lines, ["a␀b␛␡c \u{fffd}é"]);
    }
}

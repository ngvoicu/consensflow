//! How much of a role printed by `cf inbox read` the window it was written to
//! reads whole. A role too long for one message goes into its window as its
//! opening and that command (the engine's `role_change`), and counts as told once
//! `cf` printed it whole and said so to the board. The command and the engine
//! read this one bound, so the size of a role the engine counts as read is the
//! size `cf` acknowledges.
//!
//! A role goes by a note only to the windows of two harnesses: a Codex thread
//! that keeps its first instructions and is resumed, and a Muse session that
//! began with none (its `/clear`). Both read a command's output whole well past
//! the 10,000 bytes `cf` proves for every harness (the `cut` of the `cf` crate):
//! probed on 2026-10-31 with one `cat` of a 29,923-byte, 500-line file of the
//! chief's role text, its last line a marker.
//! - **Codex 0.166.0** (`codex exec`): the tool output its rollout records is the
//!   whole file, the last line in it and no "truncated".
//! - **Muse 1.4.14** (`muse exec --json`): its tool result says
//!   `"truncated": false`, `"original_output_tokens": 7481`, and its output is
//!   the file byte for byte.
//!
//! Each model answered the marker. The bound is under what was shown, in bytes
//! and in lines both.
//!
//! A role longer than that is printed in parts ([`role_parts`]), each within the bound with
//! what `cf` prints around it (the message's line above it, the part's own line below:
//! [`part_line`]), so that every part rests on the same proof. The engine counts the parts
//! with the same cut, for the opening it writes to say how many there are, and the ledger
//! takes the role as read once the last part was printed after every part before it.

/// An output of fewer bytes than this is read whole by the window of every
/// harness a role goes to by a note.
pub const ROLE_READ_WHOLE_BYTES: usize = 29_000;

/// An output of fewer lines than this is read whole by the same windows.
pub const ROLE_READ_WHOLE_LINES: usize = 500;

/// Whether what `cf` printed of a role, `printed`, is read whole by the window
/// the role was written to.
pub fn role_read_whole(printed: &str) -> bool {
    printed.len() < ROLE_READ_WHOLE_BYTES && printed.matches('\n').count() < ROLE_READ_WHOLE_LINES
}

/// The bytes and lines a role's words may take where `cf` prints them in one piece: what is
/// left once the message's line (`m-N [delivering] note from ConsensFlow`, a task's number
/// at the most) and the blank line under it are printed with them.
const ONE_ROOM: (usize, usize) = (100, 3);

/// The bytes and lines a part's words may take: what is left once the message's line, the
/// blank lines around the part's words and the part's own line ([`part_line`], with a
/// window's `cf` by its whole path, a long one on Windows) are printed with them.
const PART_ROOM: (usize, usize) = (1_000, 8);

/// A part of a role's words, as `cf inbox read` prints it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Part<'a> {
    /// Its words: the parts of a role, put together, are its words.
    pub words: &'a str,
    /// Whether it ends in the middle of a table (its last line and the next part's first are
    /// rows of one), which its line then says goes on.
    pub table_goes_on: bool,
}

/// The parts `cf inbox read` prints `words` in, the words of a role as it prints them (each
/// command by the window's `cf`): one, the words whole, where they are read whole printed
/// with the message's line, which is all a role printed so ever was; else pieces each of
/// which is read whole printed with the message's line and its own ([`part_line`]). A part
/// ends where a paragraph or a heading begins after a blank line, outside a code fence, and
/// not after a heading or a line that ends in a colon, which introduce what follows; of
/// those the last that leaves the part at least half its room. Where there is none it ends
/// at the last line start of its room outside a fence, which may be between the rows of a
/// table (the part says the table goes on), else at the last line start, and in the middle
/// of a line (never of a character) only where a line is longer than a part. No words are
/// one part, of none.
pub fn role_parts(words: &str) -> Vec<Part<'_>> {
    let fits = |room: (usize, usize), text: &str| {
        text.len() + room.0 < ROLE_READ_WHOLE_BYTES
            && text.matches('\n').count() + room.1 < ROLE_READ_WHOLE_LINES
    };
    if fits(ONE_ROOM, words) {
        return vec![Part {
            words,
            table_goes_on: false,
        }];
    }
    let bytes = ROLE_READ_WHOLE_BYTES - PART_ROOM.0 - 1;
    let lines = ROLE_READ_WHOLE_LINES - PART_ROOM.1 - 1;
    let mut parts = Vec::new();
    let mut start = 0;
    while start < words.len() {
        let rest = &words[start..];
        let end = start + part_end(rest, bytes, lines);
        let before = words[start..end].lines().last().unwrap_or("");
        let after = words[end..].lines().next().unwrap_or("");
        parts.push(Part {
            words: &words[start..end],
            table_goes_on: end < words.len() && before.starts_with('|') && after.starts_with('|'),
        });
        start = end;
    }
    parts
}

/// Where the first part of `text` ends: all of it where it takes at most `bytes` bytes and
/// `lines` line breaks, else the cut [`role_parts`] says.
fn part_end(text: &str, bytes: usize, lines: usize) -> usize {
    // The last line start that begins a block, outside a fence, and any, in the room.
    let (mut block, mut outside, mut any) = (None, None, None);
    let mut fence: Option<(char, usize)> = None;
    let (mut blank_before, mut introduces) = (false, false);
    let (mut at, mut breaks) = (0, 0);
    for line in text.split_inclusive('\n') {
        let bare = line.strip_suffix('\n').unwrap_or(line);
        if at > 0 {
            any = Some(at);
            if fence.is_none() {
                outside = Some(at);
                if blank_before && !introduces && begins_a_block(bare) {
                    block = Some(at);
                }
            }
        }
        let breaks_after = breaks + usize::from(line.ends_with('\n'));
        if at + line.len() > bytes || breaks_after > lines {
            let whole = block.filter(|&line| line >= bytes / 2);
            return match whole.or(outside).or(any) {
                Some(end) => end,
                // One line longer than a part: cut in it, at a character.
                None => {
                    let mut end = bytes.min(line.len());
                    while !line.is_char_boundary(end) {
                        end -= 1;
                    }
                    end.max(line.chars().next().map_or(1, char::len_utf8))
                }
            };
        }
        fence = fenced(fence, bare);
        blank_before = bare.trim().is_empty();
        if !blank_before {
            let text = bare.trim_end();
            introduces = text.ends_with(':') || text.starts_with('#');
        }
        at += line.len();
        breaks = breaks_after;
    }
    text.len()
}

/// Whether a part may end before `line`, a whole line: it has text, is not indented (the
/// rest of an item or an example), is not a list item (`- `, `* `, `+ `, `1. `, `1) `), a
/// table row (`|`), a quotation (`>`) or the opening of a code fence, which are lines that
/// go on with what is before them.
fn begins_a_block(line: &str) -> bool {
    let Some(first) = line.chars().next() else {
        return false;
    };
    if first.is_whitespace() || matches!(first, '|' | '>') || fenced(None, line).is_some() {
        return false;
    }
    let marker =
        |rest: Option<&str>| rest.is_some_and(|rest| rest.is_empty() || rest.starts_with(' '));
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    let item = ['-', '*', '+']
        .into_iter()
        .any(|bullet| marker(line.strip_prefix(bullet)))
        || (digits > 0 && marker(line[digits..].strip_prefix(['.', ')'])));
    !item
}

/// Whether a code fence is open after `line`, given whether one was (`fence`: the character
/// and the length of the run that opened it): a line of up to three spaces and a run of
/// three or more backticks or tildes opens one, and a run of the same character, as long,
/// with nothing but white space after it, closes it.
fn fenced(fence: Option<(char, usize)>, line: &str) -> Option<(char, usize)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return fence;
    }
    let rest = &line[indent..];
    let Some(mark) = rest.chars().next().filter(|mark| matches!(mark, '`' | '~')) else {
        return fence;
    };
    let run = rest
        .chars()
        .take_while(|&character| character == mark)
        .count();
    if run < 3 {
        return fence;
    }
    match fence {
        None => Some((mark, run)),
        Some((open, length)) if mark == open && run >= length && rest[run..].trim().is_empty() => {
            None
        }
        open => open,
    }
}

/// What `cf inbox read` prints of part `part` (the first is 1) of the role `words` of the
/// note `id`, under the message's `line`, in the window whose `cf` is `cf`, and how many
/// parts there are; none where it has no such part. A role of one part is printed as any
/// message is, its line and its words; a part of more, with its own line under it.
pub fn printed_part(
    line: &str,
    words: &str,
    part: usize,
    cf: &str,
    id: i64,
) -> Option<(String, usize)> {
    let parts = role_parts(words);
    let of = parts.len();
    let this = parts.get(part.checked_sub(1)?)?;
    if of == 1 {
        return Some((format!("{line}\n\n{}", this.words), 1));
    }
    let under = part_line(cf, id, part, of, this.table_goes_on);
    Some((
        format!("{line}\n\n{}\n\n{under}", this.words.trim_end_matches('\n')),
        of,
    ))
}

/// The line `cf inbox read` prints under part `part` (the first is 1) of `of` of the note
/// `id`, read in the window whose `cf` is `cf`: which part it is and the command that prints
/// the next, or that it is the last. A part that ends in a table says the table goes on.
pub fn part_line(cf: &str, id: i64, part: usize, of: usize, table_goes_on: bool) -> String {
    if part >= of {
        return format!("(Part {part} of {of} of m-{id}, the last: you have read all of it once you have read every part.)");
    }
    let next = part + 1;
    let table = if table_goes_on {
        format!(" The table above goes on in part {next}.")
    } else {
        String::new()
    };
    format!("(Part {part} of {of} of m-{id}.{table} Print part {next} with: {cf} inbox read m-{id} --part {next})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_role_is_read_whole_only_below_the_bound_in_bytes_and_in_lines() {
        assert!(role_read_whole(&"x".repeat(ROLE_READ_WHOLE_BYTES - 1)));
        assert!(!role_read_whole(&"x".repeat(ROLE_READ_WHOLE_BYTES)));
        assert!(role_read_whole(&"\n".repeat(ROLE_READ_WHOLE_LINES - 1)));
        assert!(!role_read_whole(&"\n".repeat(ROLE_READ_WHOLE_LINES)));
        // Bytes, not characters: 15,000 two-byte characters are 30,000 bytes.
        assert!(!role_read_whole(&"é".repeat(15_000)));
    }

    /// What `cf` prints of part `at` of `parts` of the note 12: the message's line, the
    /// part's words, its line.
    fn printed(parts: &[Part], at: usize, cf: &str) -> String {
        let words: String = parts.iter().map(|part| part.words).collect();
        let line = "m-12 [delivering] note from ConsensFlow";
        let (text, of) = printed_part(line, &words, at + 1, cf, 12).unwrap();
        assert_eq!(of, parts.len());
        format!("{text}\n")
    }

    /// The longest `cf` a window is given: a PowerShell call of a path deep in a long name.
    fn a_long_cf() -> String {
        format!(
            "& 'C:\\Users\\{}\\AppData\\Local\\ConsensFlow\\cli\\bin\\cf.exe'",
            "x".repeat(200)
        )
    }

    /// Paragraphs of `each` bytes, a heading every fifth, `count` of them.
    fn paragraphs(count: usize, each: usize) -> String {
        (0..count)
            .map(|at| {
                let words = format!("Paragraph {at} ").repeat(each / 10 + 1);
                let words = &words[..each];
                if at % 5 == 0 {
                    format!("## Heading {at}\n\n{words}")
                } else {
                    words.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    #[test]
    fn words_read_whole_with_the_messages_line_are_one_part_and_one_byte_more_are_two() {
        for words in [
            "",
            "a role",
            &"x".repeat(ROLE_READ_WHOLE_BYTES - ONE_ROOM.0 - 1),
        ] {
            let parts = role_parts(words);
            assert_eq!(parts.len(), 1);
            assert_eq!(parts[0].words, words);
            assert!(!parts[0].table_goes_on);
            assert!(role_read_whole(&printed(&parts, 0, "cf")));
        }
        assert_eq!(
            role_parts(&"x".repeat(ROLE_READ_WHOLE_BYTES - ONE_ROOM.0)).len(),
            2,
            "in bytes"
        );
        let lines = ROLE_READ_WHOLE_LINES - ONE_ROOM.1 - 1;
        assert_eq!(role_parts(&"\n".repeat(lines)).len(), 1);
        assert_eq!(role_parts(&"\n".repeat(lines + 1)).len(), 2, "in lines");
    }

    #[test]
    fn every_part_is_read_whole_printed_with_its_lines_and_the_parts_are_the_words() {
        let cf = a_long_cf();
        let table: String = (0..900)
            .map(|at| format!("| member-{at} | worker, reviewer | standard |\n"))
            .collect();
        let cases = [
            paragraphs(120, 600),
            "x".repeat(70_000),
            "\n".repeat(1_700),
            format!(
                "{}\n\n{table}\n{}",
                paragraphs(30, 500),
                paragraphs(30, 500)
            ),
            "日".repeat(25_000),
        ];
        for words in &cases {
            let parts = role_parts(words);
            assert!(parts.len() > 1, "{} bytes", words.len());
            assert_eq!(
                parts.iter().map(|part| part.words).collect::<String>(),
                *words,
                "the parts are the words"
            );
            for at in 0..parts.len() {
                let out = printed(&parts, at, &cf);
                assert!(
                    role_read_whole(&out),
                    "part {} of {}: {} bytes, {} lines",
                    at + 1,
                    parts.len(),
                    out.len(),
                    out.matches('\n').count()
                );
            }
        }
    }

    #[test]
    fn a_part_ends_before_a_paragraph_or_a_heading_and_a_table_cut_says_it_goes_on() {
        let words = paragraphs(120, 600);
        let parts = role_parts(&words);
        for part in &parts[..parts.len() - 1] {
            assert!(part.words.ends_with("\n\n"), "at a blank line");
            assert!(!part.table_goes_on);
        }
        for part in &parts[1..] {
            assert!(
                part.words.starts_with("Paragraph") || part.words.starts_with("## Heading"),
                "{:?}",
                &part.words[..40]
            );
        }
        // Not after a heading or a line that introduces what follows, and not in a fence or
        // a list: the cut goes before them.
        // Each block put where a part's room ends in the middle of it.
        let room = ROLE_READ_WHOLE_BYTES - PART_ROOM.0 - 1;
        let filler = paragraphs(40, 600);
        for introduced in [
            "## The heading\n\nIts paragraph.",
            "These follow:\n\nThe paragraph it introduces.",
            "```\nan example\n\nnot a paragraph of the text\n```",
            "- an item\n\n- an item after a blank line",
        ] {
            let pad = room - filler.len() - 4 - introduced.len() / 2;
            let words = format!(
                "{filler}\n\n{}\n\n{introduced}\n\n{filler}",
                "y".repeat(pad)
            );
            let at = words.find(introduced).unwrap();
            assert!(
                at < room && room < at + introduced.len(),
                "the room ends in it"
            );
            let parts = role_parts(&words);
            let first = parts[0].words.len();
            assert!(
                first <= at || first >= at + introduced.len(),
                "a cut inside {introduced:?}: {:?}",
                &words[first.saturating_sub(20)..first + 20]
            );
        }
        // A table longer than a part is cut between its rows, and the part says it goes on.
        let table: String = (0..900)
            .map(|at| format!("| member-{at} | worker, reviewer | standard |\n"))
            .collect();
        let words = format!("The staff:\n\n{table}\nAfter the table.");
        let parts = role_parts(&words);
        assert!(parts.len() > 1);
        assert!(
            parts[0].table_goes_on,
            "{:?}",
            parts[0].words.lines().last()
        );
        assert!(parts[0].words.ends_with("|\n"));
        assert!(parts[1].words.starts_with("| member-"));
        assert!(!parts.last().unwrap().table_goes_on);
        assert!(part_line("cf", 12, 1, 2, true).contains("The table above goes on in part 2."));
    }

    #[test]
    fn a_parts_line_names_it_and_the_command_for_the_next_and_the_last_says_it_is_the_last() {
        assert_eq!(
            part_line("& 'C:/cf.exe'", 12, 1, 3, false),
            "(Part 1 of 3 of m-12. Print part 2 with: & 'C:/cf.exe' inbox read m-12 --part 2)"
        );
        assert_eq!(
            part_line("cf", 12, 2, 3, true),
            "(Part 2 of 3 of m-12. The table above goes on in part 3. Print part 3 with: cf inbox read m-12 --part 3)"
        );
        assert_eq!(
            part_line("cf", 12, 3, 3, false),
            "(Part 3 of 3 of m-12, the last: you have read all of it once you have read every part.)"
        );
    }
}

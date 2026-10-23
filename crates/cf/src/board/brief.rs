//! The words of the commands that give a window work (`cf task add`, `cf task
//! reopen`, `cf task resume`, `cf task edit`, `cf tell`): written out, read from standard input
//! (`-`), or read from a file (`--file <path>`), which no shell cuts. The field
//! report of 2026-10-15 (B6): a chief's heredoc lost its closing word, bash
//! warned, and a brief went to the board cut at about 8.1k characters, which
//! nothing `cf` was given showed. However the words came, the reply says how
//! many characters went and how they end ([`sent`]): a brief cut short shows
//! where.
//!
//! The file is named by its bytes, as the system gave them, never by the text
//! `cf` reads its words as: two names that read the same are two files. It is
//! read only if, opened, it is a file of its own, never a link, a pipe, a
//! device or a folder, and only as much as the board could take (the second
//! review, 2026-10-16). Neither the reply nor a refusal draws what the words or
//! the name say to the terminal: each control character in them is shown
//! escaped.

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use cf_daemon::api::body::MAX_JSON_BYTES;

use super::words::Split;
use super::{text_of, Failure};

/// The flag that names a file to read the words from.
pub const FILE: &str = "--file";

/// How many of the last words the reply says.
const LAST_WORDS: usize = 8;

/// The most of them it says, in characters: a word may be a line of code.
const MOST_SHOWN: usize = 80;

/// The most of a file that is read, in bytes: the board takes no request of
/// more (`MAX_JSON_BYTES`, 2 MiB), and a brief is written in one, so a longer
/// file could never go. Nothing the board would take is refused, and a file
/// that grows as it is read cannot fill the memory.
const MOST_READ: usize = MAX_JSON_BYTES;

/// The words `words` give: the file `--file` names, read whole as UTF-8 (what
/// is no UTF-8 replaced) with one final line ending dropped, the file's and not
/// the text's; or the words written out, standard input for `-` (`text_of`).
/// The file's name is the word of `given` that `words` read as it: `given` is
/// the command's words as the system gave them, of which those `words` split
/// are the last. A `--file` with no name, or one named beside words of their
/// own, `-` among them, is a command written wrong; a file that cannot be read,
/// or is no file of its own, or is longer than any brief, fails the command.
pub fn brief_of(
    words: &Split,
    given: &[OsString],
    input: &mut dyn Read,
) -> Result<String, Failure> {
    if !words.given(FILE) {
        return text_of(words.text.clone(), input);
    }
    let name = words
        .value_back(FILE)
        .and_then(|back| given.len().checked_sub(back))
        .and_then(|at| given.get(at))
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            Failure::Usage(format!(
                "{FILE} takes the file to read the words from: {FILE} brief.md"
            ))
        })?;
    let path = Path::new(name);
    let shown = escaped(&path.to_string_lossy());
    if !words.text.is_empty() {
        return Err(Failure::Usage(format!(
            "give the words or {FILE} {shown}, not both"
        )));
    }
    let bytes =
        read_brief(path).map_err(|why| Failure::Failed(format!("cannot read {shown}{why}")))?;
    let text = String::from_utf8_lossy(&bytes);
    let text = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(&text);
    Ok(text.to_owned())
}

/// The bytes of the file at `path`, read as [`brief_of`] reads one, or why
/// not, as the end of `cannot read <name>`.
fn read_brief(path: &Path) -> Result<Vec<u8>, String> {
    let file = open(path)?;
    if let Some(what) = not_a_file(&file).map_err(|cause| format!(" ({cause})"))? {
        return Err(not_a(what));
    }
    let mut bytes = Vec::new();
    file.take(MOST_READ as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|cause| format!(" ({cause})"))?;
    if bytes.len() > MOST_READ {
        return Err(": it holds more than 2 MiB, more than the board takes at once".to_owned());
    }
    Ok(bytes)
}

/// Why a file that is `what` is not read.
fn not_a(what: &str) -> String {
    format!(": it is {what}, not a file")
}

/// The file at `path`, opened to read, or why not: a link at its end is not
/// followed, and a pipe with no writer is not waited for.
#[cfg(unix)]
fn open(path: &Path) -> Result<File, String> {
    use std::os::unix::fs::OpenOptionsExt;
    File::options()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|cause| match cause.raw_os_error() {
            Some(libc::ELOOP) => not_a("a link"),
            _ => format!(" ({cause})"),
        })
}

/// The file at `path`, opened to read, or why not: a link, or any point that
/// sends the open on elsewhere, is opened as itself.
#[cfg(windows)]
fn open(path: &Path) -> Result<File, String> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
    File::options()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|cause| format!(" ({cause})"))
}

/// What `file` is, by its open handle, when it is no file of its own.
#[cfg(unix)]
fn not_a_file(file: &File) -> io::Result<Option<&'static str>> {
    use std::os::unix::fs::FileTypeExt;
    let kind = file.metadata()?.file_type();
    Ok(if kind.is_file() {
        None
    } else if kind.is_dir() {
        Some("a folder")
    } else if kind.is_fifo() || kind.is_socket() {
        Some("a pipe")
    } else {
        Some("a device")
    })
}

/// What `file` is, by its open handle, when it is no file of its own.
#[cfg(windows)]
fn not_a_file(file: &File) -> io::Result<Option<&'static str>> {
    let handle = winapi_util::file::typ(file)?;
    if handle.is_pipe() {
        return Ok(Some("a pipe"));
    }
    if !handle.is_disk() {
        return Ok(Some("a device"));
    }
    let kind = file.metadata()?.file_type();
    Ok(if kind.is_symlink() {
        Some("a link")
    } else if kind.is_dir() {
        Some("a folder")
    } else {
        None
    })
}

/// What a reply says of the words it sent: how many characters, and the last
/// few words, their spaces and line breaks as one space, after an ellipsis when
/// there were more, each control character among them escaped.
pub fn sent(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let first = words.len().saturating_sub(LAST_WORDS);
    let mut last = words[first..].join(" ");
    let mut cut = first > 0;
    let shown = last.chars().count();
    if shown > MOST_SHOWN {
        last = last.chars().skip(shown - MOST_SHOWN).collect();
        cut = true;
    }
    format!(
        "Sent: {} characters, ending \"{}{}\".",
        grouped(text.chars().count()),
        if cut { "…" } else { "" },
        escaped(&last)
    )
}

/// `text` with each character that would move or redraw what a terminal shows
/// written out instead (`\u{1b}`): the controls, and the marks that turn the
/// direction text is shown in.
fn escaped(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() || turns(character) {
                format!("\\u{{{:x}}}", u32::from(character))
            } else {
                character.to_string()
            }
        })
        .collect()
}

/// Whether `character` turns the direction text is shown in, from where it stands.
fn turns(character: char) -> bool {
    matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// `number` with a comma between each three of its digits: `8,123`.
fn grouped(number: usize) -> String {
    let digits = number.to_string();
    let mut grouped = String::new();
    for (place, digit) in digits.chars().enumerate() {
        if place > 0 && (digits.len() - place).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reply_says_how_many_characters_went_and_how_they_end() {
        assert_eq!(
            sent("Reply with exactly: ONE"),
            "Sent: 23 characters, ending \"Reply with exactly: ONE\"."
        );
        // Lines and spaces are one space; the last eight words, after an ellipsis.
        let brief =
            "Write the parser.\n\nKeep the old tests,\n  and run them all before you answer.";
        assert_eq!(
            sent(brief),
            "Sent: 76 characters, ending \"…tests, and run them all before you answer.\"."
        );
        // A brief that ends inside a fence shows it.
        let cut = format!(
            "{}\n```rust\nfn main() {{\n    let parser = Parser::new(",
            "x ".repeat(4_000)
        );
        assert_eq!(
            sent(&cut),
            "Sent: 8,050 characters, ending \"…```rust fn main() { let parser = Parser::new(\"."
        );
    }

    #[test]
    fn a_character_that_would_redraw_the_terminal_is_shown_escaped() {
        assert_eq!(
            sent("DONE\u{1b}[1A\u{1b}[2K\u{7}\u{9b}2J \u{202e}enod"),
            "Sent: 22 characters, ending \"DONE\\u{1b}[1A\\u{1b}[2K\\u{7}\\u{9b}2J \\u{202e}enod\"."
        );
        assert_eq!(escaped("plain, «quoted» 漢字"), "plain, «quoted» 漢字");
    }

    #[test]
    fn characters_are_counted_as_people_count_them_and_a_long_last_word_is_cut() {
        assert_eq!(sent("漢字 🙂"), "Sent: 4 characters, ending \"漢字 🙂\".");
        let long = format!("see {}", "y".repeat(200));
        assert_eq!(
            sent(&long),
            format!("Sent: 204 characters, ending \"…{}\".", "y".repeat(80))
        );
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1,000");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }
}

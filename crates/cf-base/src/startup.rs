//! The words of a start that failed: what a component says when a program could not be run, with
//! the file it tried **marked** where the code that built the words had the path as data.
//!
//! A note about a failed start (`cf_engine`'s `launch_failed`) must not quote the program's path:
//! it is a folder of the person's, and a name in it can be a credential. The path's extent cannot
//! be recovered from prose (`C:\tools\pi hunter22.cmd` is one file, not a file and a word), so a
//! component that has the path says where it is, with [`a_path_in_words`], and whoever tells the
//! failure replaces exactly that span. Words with a file in them that no one marked are free
//! text, and are not quoted (the joint review T-1322, blocker 4).

use std::path::Path;

/// Opens the file a failed start tried, in words.
pub const PATH_OPENS: char = '\u{2039}';
/// Closes it.
pub const PATH_CLOSES: char = '\u{203A}';

/// `path` marked as a file in the words of a failed start: `‹C:\tools\pi.cmd›`. A marker among
/// the path's own characters is turned into `?`, so that what stands between the marks is the
/// path and nothing more.
pub fn a_path_in_words(path: impl AsRef<Path>) -> String {
    let text: String = path
        .as_ref()
        .to_string_lossy()
        .chars()
        .map(|c| {
            if c == PATH_OPENS || c == PATH_CLOSES {
                '?'
            } else {
                c
            }
        })
        .collect();
    format!("{PATH_OPENS}{text}{PATH_CLOSES}")
}

/// `words` as a person reads them, the marks of the files taken off and the files kept
/// (`‹C:\tools\pi.cmd›` is `C:\tools\pi.cmd`): for a terminal or a page, where the path is
/// the person's own and the words say what to do about it. A note on the board, which others read,
/// replaces the marked files instead.
pub fn unmarked(words: &str) -> String {
    words
        .chars()
        .filter(|c| *c != PATH_OPENS && *c != PATH_CLOSES)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marks_come_off_a_person_s_words_and_the_files_stay() {
        let words = format!(
            "{} is no shim",
            a_path_in_words(r"C:\tools\pi hunter22.cmd")
        );
        assert_eq!(unmarked(&words), r"C:\tools\pi hunter22.cmd is no shim");
        assert_eq!(unmarked("no marks"), "no marks");
    }

    #[test]
    fn a_path_is_marked_whole_with_its_blanks_and_a_marker_among_its_characters_is_none() {
        assert_eq!(
            a_path_in_words(r"C:\tools\pi hunter22.cmd"),
            "\u{2039}C:\\tools\\pi hunter22.cmd\u{203A}"
        );
        assert_eq!(a_path_in_words("/a/b\u{203A}c"), "\u{2039}/a/b?c\u{203A}");
    }
}

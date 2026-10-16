//! The `cf` a member's window runs, for a check that has no daemon for the real
//! one to reach: a stand-in program (`fake-cf`) that writes down each call it is
//! given, a line to each, in a log beside its own file, and says it is a stand-in
//! to the one that asked. What a window's agent does when it runs `cf ask` is
//! then read from the log, where the window's own record says only that the call
//! was made. The program is put in a folder first on the windows' `PATH`
//! ([`install`]), as the bundle's own `cf` is on a member's.

use std::path::{Path, PathBuf};

use crate::{files, Result};

/// The log's name, beside the stand-in program.
pub const LOG: &str = "cf-calls.log";

/// What the stand-in answers a call with: nothing for a hook (the member's question
/// hook, which Devin runs for `cf hook devin` and reads the answer of), and a line
/// that says whose it is for any other call.
pub fn answer(words: &[String]) -> Option<String> {
    if words.first().map(String::as_str) == Some("hook") {
        return None;
    }
    Some(format!("stand-in cf: {}", words.join(" ")))
}

/// A call as the stand-in writes it in the log: the time in milliseconds since
/// the epoch, a space, the words it was given joined by spaces, and a line end.
pub fn line(at_ms: i64, words: &[String]) -> String {
    format!("{at_ms} {}\n", words.join(" "))
}

/// A call the stand-in was given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// When, in milliseconds since the epoch.
    pub at_ms: i64,
    /// What it was given: `ask which colour?`.
    pub words: String,
}

/// The calls of a log's text, oldest first; a line that is none is left out.
fn parse(text: &str) -> Vec<Call> {
    text.lines()
        .filter_map(|line| {
            let (at, words) = line.split_once(' ').unwrap_or((line, ""));
            Some(Call {
                at_ms: at.parse().ok()?,
                words: words.to_owned(),
            })
        })
        .collect()
}

/// The calls the stand-in installed in `folder` was given, oldest first: none
/// while there is no log, which is a stand-in nobody ran.
pub fn calls(folder: &Path) -> Vec<Call> {
    files::read_string(&folder.join(LOG))
        .map(|text| parse(&text))
        .unwrap_or_default()
}

/// Puts the stand-in `program` (the `fake-cf` binary) in `folder`, made if it is
/// not there, under the name a window finds `cf` by, and as a program a shell
/// starts: the path it is at. Its log is beside it.
pub fn install(folder: &Path, program: &Path) -> Result<PathBuf> {
    let installed = folder.join(format!("cf{}", std::env::consts::EXE_SUFFIX));
    files::write_executable(&installed, files::read(program)?)?;
    Ok(installed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn a_call_is_said_back_as_a_stand_in_and_a_hook_is_answered_with_nothing() {
        assert_eq!(
            answer(&words(&["ask", "which colour?"])).as_deref(),
            Some("stand-in cf: ask which colour?")
        );
        assert_eq!(answer(&words(&["hook", "devin"])), None);
        assert_eq!(answer(&[]).as_deref(), Some("stand-in cf: "));
    }

    #[test]
    fn a_call_is_a_line_of_the_time_and_its_words_and_is_read_back_as_that() {
        let said = line(1_790_000_000_123, &words(&["ask", "x"]));
        assert_eq!(said, "1790000000123 ask x\n");
        assert_eq!(
            parse(&said),
            [Call {
                at_ms: 1_790_000_000_123,
                words: "ask x".to_owned()
            }]
        );
        // A call with no words, and a line that is no call.
        let text = format!(
            "{}not a call\n{}",
            line(5, &[]),
            line(6, &words(&["inbox"]))
        );
        let found: Vec<(i64, String)> = parse(&text)
            .into_iter()
            .map(|call| (call.at_ms, call.words))
            .collect();
        assert_eq!(found, [(5, String::new()), (6, "inbox".to_owned())]);
    }

    #[test]
    fn the_stand_in_is_installed_as_cf_and_its_calls_are_read_from_the_log_beside_it() {
        let root = tempfile::tempdir().unwrap();
        let program = root.path().join("fake-cf-program");
        files::write(&program, "the program").unwrap();
        let folder = root.path().join("not").join("yet");
        let installed = install(&folder, &program).unwrap();
        assert_eq!(
            installed,
            folder.join(format!("cf{}", std::env::consts::EXE_SUFFIX))
        );
        assert_eq!(files::read_string(&installed).unwrap(), "the program");
        // A shell starts it: a program, and not a file that holds one.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&installed).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111);
        }
        // No log, no calls: a stand-in nobody ran.
        assert!(calls(&folder).is_empty());
        files::write(&folder.join(LOG), line(9, &words(&["ask", "y"]))).unwrap();
        assert_eq!(
            calls(&folder),
            [Call {
                at_ms: 9,
                words: "ask y".to_owned()
            }]
        );
    }
}

//! A window that says what a script says: the reviewer of the evals' review scenarios
//! (`crates/cf-evals/src/scenarios/review_rounds.rs`), a stand-in whose reviews are fixed
//! text, so that what is measured is the chief's judgment of them and no model's.
//!
//! `CF_TEST_SCRIPTED_RESULTS` names a folder the run made. It holds `.eval-run`, the marker
//! that says it is an eval run's (the folder is removed when the run ends, so a window that
//! is given the name of one that is gone is refused), and the results as `1.md`, `2.md` and
//! so on. Each task brief a window of the stand-in is given takes the next result, counted in
//! the folder (`.taken-1`, `.taken-2`, each made exclusively, so that two windows never take
//! one), since every review is a fresh window and a counter in the process would start again.
//! The last result is given again for any more. A message that is no task brief is
//! acknowledged as any is.

use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
use std::path::Path;
use std::sync::OnceLock;

use cf_e2e::pattern;
use regex::Regex;

/// The variable that names the folder of the results.
pub const FOLDER: &str = "CF_TEST_SCRIPTED_RESULTS";

/// The file that says the folder is an eval run's.
pub const MARKER: &str = ".eval-run";

/// The header of a task brief delivered to a window.
const BRIEF: &str = r"(?m)^\[ConsensFlow m-\d+ · T-\d+ · task from @[^\]]+\]";

/// What the window says to `text`: the next result of the folder if it is a task brief, else
/// none (it is acknowledged as any message is). A folder that is no eval run's, or that holds
/// no result, is refused.
pub fn answer(folder: &Path, text: &str) -> Result<Option<String>, String> {
    static HEADER: OnceLock<Regex> = OnceLock::new();
    if !folder.join(MARKER).is_file() {
        return Err(format!(
            "{} is no eval run's folder of results (there is no {MARKER} in it): the scripted reviewer answers in an eval run and nowhere else",
            folder.display()
        ));
    }
    if !pattern::once(&HEADER, BRIEF).is_match(text) {
        return Ok(None);
    }
    let taken = take(folder)?;
    let file = (1..=taken)
        .rev()
        .map(|number| folder.join(format!("{number}.md")))
        .find(|file| file.is_file())
        .ok_or_else(|| {
            format!(
                "{} holds no result: {taken} was asked for",
                folder.display()
            )
        })?;
    fs::read_to_string(&file)
        .map(Some)
        .map_err(|cause| format!("could not read {}: {cause}", file.display()))
}

/// The number of the brief this is, counted in `folder`: the first `.taken-N` that nobody
/// made yet, made now.
fn take(folder: &Path) -> Result<u32, String> {
    for number in 1.. {
        let file = folder.join(format!(".taken-{number}"));
        match OpenOptions::new().write(true).create_new(true).open(&file) {
            Ok(_) => return Ok(number),
            Err(cause) if cause.kind() == ErrorKind::AlreadyExists => {}
            Err(cause) => return Err(format!("could not make {}: {cause}", file.display())),
        }
    }
    unreachable!("a counter that is never done")
}

#[cfg(test)]
mod tests {
    use super::*;

    const BRIEF_ONE: &str = "[ConsensFlow m-5 · T-2 · task from @chief]\nReview T-1.";
    const BRIEF_TWO: &str = "[ConsensFlow m-9 · T-3 · task from @chief]\nReview T-1 again.";

    /// A folder of a run with the results `results`.
    fn run(results: &[&str]) -> tempfile::TempDir {
        let folder = tempfile::tempdir().unwrap();
        fs::write(folder.path().join(MARKER), "").unwrap();
        for (at, result) in results.iter().enumerate() {
            fs::write(folder.path().join(format!("{}.md", at + 1)), result).unwrap();
        }
        folder
    }

    #[test]
    fn each_task_brief_takes_the_next_result_and_the_last_is_given_again_for_any_more() {
        let folder = run(&["first review", "second review", "third review"]);
        let said: Vec<_> = (0..5)
            .map(|_| answer(folder.path(), BRIEF_ONE).unwrap().unwrap())
            .collect();
        assert_eq!(
            said,
            [
                "first review",
                "second review",
                "third review",
                "third review",
                "third review"
            ]
        );
    }

    #[test]
    fn a_message_that_is_no_task_brief_is_acknowledged_and_takes_no_result() {
        let folder = run(&["first review", "second review"]);
        for text in [
            "[ConsensFlow m-6 · T-2 · answer from @chief]\nalbastru",
            "[ConsensFlow m-7 · T-2 · result from @worker]\nDone.",
            "Please review T-1.",
            "a quote: [ConsensFlow m-5 · T-2 · task from @chief] in the middle of a line",
        ] {
            assert_eq!(answer(folder.path(), text), Ok(None), "{text}");
        }
        // Nothing was taken: the first brief still gets the first result.
        assert_eq!(
            answer(folder.path(), BRIEF_TWO).unwrap().as_deref(),
            Some("first review")
        );
    }

    #[test]
    fn a_brief_after_other_lines_of_the_message_is_still_a_task_brief() {
        let folder = run(&["only review"]);
        let text = format!("\n{BRIEF_ONE}");
        assert_eq!(
            answer(folder.path(), &text).unwrap().as_deref(),
            Some("only review")
        );
    }

    #[test]
    fn a_folder_that_is_no_eval_runs_is_refused_with_the_words_that_say_why() {
        let folder = run(&["review"]);
        fs::remove_file(folder.path().join(MARKER)).unwrap();
        let refused = answer(folder.path(), BRIEF_ONE).unwrap_err();
        assert!(
            refused.contains("is no eval run's folder of results"),
            "{refused}"
        );
        // Refused for any message, and nothing was taken.
        assert!(answer(folder.path(), "hello").is_err());
        assert!(!folder.path().join(".taken-1").exists());
        // A folder that is not there is refused the same way.
        let gone = folder.path().join("gone");
        assert!(answer(&gone, BRIEF_ONE)
            .unwrap_err()
            .contains("is no eval run's folder"));
    }

    #[test]
    fn a_run_with_no_result_refuses_the_brief_and_says_how_many_were_asked_for() {
        let folder = run(&[]);
        let refused = answer(folder.path(), BRIEF_ONE).unwrap_err();
        assert!(
            refused.contains("holds no result: 1 was asked for"),
            "{refused}"
        );
    }

    #[test]
    fn a_counter_made_by_another_window_is_not_taken_twice() {
        let folder = run(&["one", "two"]);
        // Another window took the first.
        fs::write(folder.path().join(".taken-1"), "").unwrap();
        assert_eq!(
            answer(folder.path(), BRIEF_ONE).unwrap().as_deref(),
            Some("two")
        );
    }
}

//! A window that says what a script says: the reviewer and the worker of the evals' review
//! scenarios (`crates/cf-evals/src/scenarios/review_rounds.rs`), stand-ins whose reviews and
//! work are fixed, so that what is measured is the chief's judgment of them and no model's.
//!
//! `CF_TEST_SCRIPTED_RESULTS` names a folder the run made. It holds `.eval-run`, the marker
//! that says it is an eval run's (the folder is removed when the run ends, so a window that
//! is given the name of one that is gone is refused), and the results as `1.md`, `2.md` and
//! so on. Each task brief a window of the stand-in is given takes the next result, counted in
//! the folder (`.taken-1`, `.taken-2`, each made exclusively, so that two windows never take
//! one), since every review is a fresh window and a counter in the process would start again.
//! The last result is given again for any more. A message that is no task brief is
//! acknowledged as any is.
//!
//! A result may have files: the folder `N.files` beside `N.md` holds the files that the work of
//! that answer makes, by their paths in the window's folder (the project's), which the stand-in
//! writes there before it answers, as a worker's tools would have. Every brief taken is written
//! down in `journal.jsonl` of the folder, whole, with the number it was given and the files
//! written, so that a report can say what the script was asked and what it held.

use std::fs::{self};
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

/// The file of the folder that keeps the briefs taken, one JSON object a line.
pub const JOURNAL: &str = "journal.jsonl";

/// What the window says to `text`: the next result of the folder if it is a task brief, else
/// none (it is acknowledged as any message is), with the files of that result written below
/// `into`, the window's folder. A folder that is no eval run's, or that holds no result, is
/// refused.
pub fn answer(folder: &Path, text: &str, into: &Path) -> Result<Option<String>, String> {
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
    let (given, file) = (1..=taken)
        .rev()
        .map(|number| (number, folder.join(format!("{number}.md"))))
        .find(|(_, file)| file.is_file())
        .ok_or_else(|| {
            format!(
                "{} holds no result: {taken} was asked for",
                folder.display()
            )
        })?;
    let result = fs::read_to_string(&file)
        .map_err(|cause| format!("could not read {}: {cause}", file.display()))?;
    let wrote = write_files(&folder.join(format!("{given}.files")), into)?;
    note(folder, taken, given, text, &wrote)?;
    Ok(Some(result))
}

/// The files below `from` (none where it is not there), written below `into` at the same paths:
/// the paths, with a slash between the parts.
fn write_files(from: &Path, into: &Path) -> Result<Vec<String>, String> {
    let mut wrote = Vec::new();
    if from.is_dir() {
        copy_below(from, from, into, &mut wrote)?;
    }
    wrote.sort();
    Ok(wrote)
}

/// The files of `at`, a folder below `root`, written below `into`.
fn copy_below(root: &Path, at: &Path, into: &Path, wrote: &mut Vec<String>) -> Result<(), String> {
    let listing =
        fs::read_dir(at).map_err(|cause| format!("could not list {}: {cause}", at.display()))?;
    for entry in listing {
        let source = entry
            .map_err(|cause| format!("could not list {}: {cause}", at.display()))?
            .path();
        if source.is_dir() {
            copy_below(root, &source, into, wrote)?;
            continue;
        }
        let Ok(relative) = source.strip_prefix(root) else {
            continue;
        };
        let target = into.join(relative);
        if let Some(parent) = target.parent() {
            cf_e2e::door::make_dir(parent)
                .map_err(|cause| format!("could not make {}: {cause}", parent.display()))?;
        }
        cf_e2e::door::copy_keeping_mode(&source, &target)
            .map_err(|cause| format!("could not write {}: {cause}", target.display()))?;
        wrote.push(
            relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/"),
        );
    }
    Ok(())
}

/// Writes down the brief `text`, the `taken`th of the folder, which was given the result `given`
/// and wrote the files `wrote`.
fn note(folder: &Path, taken: u32, given: u32, text: &str, wrote: &[String]) -> Result<(), String> {
    use std::io::Write;
    let line = serde_json::json!({ "taken": taken, "given": given, "brief": text, "wrote": wrote });
    let file = folder.join(JOURNAL);
    let mut journal = cf_e2e::door::append(&file)
        .map_err(std::io::Error::from)
        .map_err(|cause| format!("could not open {}: {cause}", file.display()))?;
    writeln!(journal, "{line}")
        .map_err(|cause| format!("could not write {}: {cause}", file.display()))
}

/// The number of the brief this is, counted in `folder`: the first `.taken-N` that nobody
/// made yet, made now.
fn take(folder: &Path) -> Result<u32, String> {
    for number in 1.. {
        let file = folder.join(format!(".taken-{number}"));
        match cf_e2e::door::create_new(&file).map_err(std::io::Error::from) {
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
            .map(|_| {
                answer(folder.path(), BRIEF_ONE, folder.path())
                    .unwrap()
                    .unwrap()
            })
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
            assert_eq!(
                answer(folder.path(), text, folder.path()),
                Ok(None),
                "{text}"
            );
        }
        // Nothing was taken: the first brief still gets the first result.
        assert_eq!(
            answer(folder.path(), BRIEF_TWO, folder.path())
                .unwrap()
                .as_deref(),
            Some("first review")
        );
    }

    #[test]
    fn a_brief_after_other_lines_of_the_message_is_still_a_task_brief() {
        let folder = run(&["only review"]);
        let text = format!("\n{BRIEF_ONE}");
        assert_eq!(
            answer(folder.path(), &text, folder.path())
                .unwrap()
                .as_deref(),
            Some("only review")
        );
    }

    #[test]
    fn a_folder_that_is_no_eval_runs_is_refused_with_the_words_that_say_why() {
        let folder = run(&["review"]);
        fs::remove_file(folder.path().join(MARKER)).unwrap();
        let refused = answer(folder.path(), BRIEF_ONE, folder.path()).unwrap_err();
        assert!(
            refused.contains("is no eval run's folder of results"),
            "{refused}"
        );
        // Refused for any message, and nothing was taken.
        assert!(answer(folder.path(), "hello", folder.path()).is_err());
        assert!(!folder.path().join(".taken-1").exists());
        // A folder that is not there is refused the same way.
        let gone = folder.path().join("gone");
        assert!(answer(&gone, BRIEF_ONE, &gone)
            .unwrap_err()
            .contains("is no eval run's folder"));
    }

    #[test]
    fn a_run_with_no_result_refuses_the_brief_and_says_how_many_were_asked_for() {
        let folder = run(&[]);
        let refused = answer(folder.path(), BRIEF_ONE, folder.path()).unwrap_err();
        assert!(
            refused.contains("holds no result: 1 was asked for"),
            "{refused}"
        );
    }

    /// A folder of a run with the results `results`, the third of which has files.
    fn work() -> (tempfile::TempDir, tempfile::TempDir) {
        let folder = run(&["page one", "page two", "pages three"]);
        for (number, files) in [
            (1, vec![("site/en/assessment.html", "<p>0 to 100</p>")]),
            (2, vec![("site/en/assessment.html", "<p>1 to 5</p>")]),
            (
                3,
                vec![
                    ("site/en/assessment.html", "<p>1 to 5</p>"),
                    ("site/evaluare.html", "<p>1 la 5</p>"),
                ],
            ),
        ] {
            for (path, text) in files {
                let at = folder.path().join(format!("{number}.files")).join(path);
                fs::create_dir_all(at.parent().unwrap()).unwrap();
                fs::write(at, text).unwrap();
            }
        }
        (folder, tempfile::tempdir().unwrap())
    }

    #[test]
    fn the_files_of_a_result_are_written_below_the_windows_folder_before_it_answers_and_the_last_again(
    ) {
        let (folder, window) = work();
        let page = window.path().join("site/en/assessment.html");
        let ro = window.path().join("site/evaluare.html");
        let ask = || {
            answer(folder.path(), BRIEF_ONE, window.path())
                .unwrap()
                .unwrap()
        };
        assert_eq!(ask(), "page one");
        assert_eq!(fs::read_to_string(&page).unwrap(), "<p>0 to 100</p>");
        assert!(!ro.exists(), "the first answer wrote one file");
        assert_eq!(ask(), "page two");
        assert_eq!(fs::read_to_string(&page).unwrap(), "<p>1 to 5</p>");
        assert!(!ro.exists());
        assert_eq!(ask(), "pages three");
        assert_eq!(fs::read_to_string(&ro).unwrap(), "<p>1 la 5</p>");
        // The last again, with its files: a page made again as it was.
        fs::write(&page, "changed by someone").unwrap();
        assert_eq!(ask(), "pages three");
        assert_eq!(fs::read_to_string(&page).unwrap(), "<p>1 to 5</p>");
    }

    #[test]
    fn a_result_without_files_writes_none_and_a_message_that_is_no_brief_writes_none() {
        let folder = run(&["a review"]);
        let window = tempfile::tempdir().unwrap();
        answer(folder.path(), BRIEF_ONE, window.path()).unwrap();
        answer(folder.path(), "hello", window.path()).unwrap();
        assert_eq!(fs::read_dir(window.path()).unwrap().count(), 0);
    }

    #[test]
    fn every_brief_taken_is_written_down_whole_with_its_number_the_result_it_got_and_the_files_it_wrote(
    ) {
        let (folder, window) = work();
        let long = format!("{BRIEF_ONE}\nCreate site/en/assessment.html.\nAnd more words.");
        answer(folder.path(), &long, window.path()).unwrap();
        answer(folder.path(), BRIEF_TWO, window.path()).unwrap();
        // Not a brief: not written down.
        answer(folder.path(), "hello", window.path()).unwrap();
        let journal: Vec<serde_json::Value> = fs::read_to_string(folder.path().join(JOURNAL))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(journal.len(), 2);
        assert_eq!(
            journal[0],
            serde_json::json!({
                "taken": 1, "given": 1, "brief": long, "wrote": ["site/en/assessment.html"]
            })
        );
        assert_eq!(journal[1]["taken"], 2);
        assert_eq!(journal[1]["brief"], BRIEF_TWO);
    }

    #[test]
    fn a_counter_made_by_another_window_is_not_taken_twice() {
        let folder = run(&["one", "two"]);
        // Another window took the first.
        fs::write(folder.path().join(".taken-1"), "").unwrap();
        assert_eq!(
            answer(folder.path(), BRIEF_ONE, folder.path())
                .unwrap()
                .as_deref(),
            Some("two")
        );
    }
}

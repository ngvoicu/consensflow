//! The command: which words it takes, and what each answers.

use std::ffi::OsString;
use std::fs;

use super::*;

/// What `cf-evals <line>` answers: its status, what it said and what it said on
/// its error output.
fn answer(line: &[&str]) -> (i32, String, String) {
    let words: Vec<OsString> = line.iter().map(OsString::from).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let status = command(&words, &mut out, &mut err);
    (
        status,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn no_command_or_one_it_does_not_have_is_a_mistake_of_the_words_that_says_what_it_takes() {
    let (status, out, err) = answer(&[]);
    assert_eq!((status, out.as_str()), (2, ""));
    assert!(
        err.starts_with("cf-evals: a command is required\nusage: cf-evals <command> [flags]"),
        "{err}"
    );
    let (status, _, err) = answer(&["eval"]);
    assert_eq!(status, 2);
    assert!(
        err.starts_with("cf-evals: unknown command: eval\n"),
        "{err}"
    );
}

#[test]
fn help_is_said_on_the_output_and_asks_nothing_to_run() {
    for word in ["help", "--help", "-h"] {
        let (status, out, err) = answer(&[word]);
        assert_eq!((status, err.as_str()), (0, ""), "{word}");
        for verb in ["  run ", "  long ", "  summary ", "  watch "] {
            assert!(out.contains(verb), "{word}: {out}");
        }
        assert!(out.contains("--scenario NAME") && out.contains("--project NAME-OR-ID"));
        assert!(out.contains("--devin-cut") && out.contains("--hours N"));
    }
}

#[test]
fn a_long_run_whose_words_are_wrong_is_refused_before_anything_is_built_or_spent() {
    for (line, said) in [
        (
            &["long", "--hours", "0"][..],
            "cf-evals: --hours takes a number above 0\n",
        ),
        (
            &["long", "--chief", "kimi"][..],
            "cf-evals: no such eval harness: kimi\n",
        ),
        (
            &["long", "--nope"][..],
            "cf-evals: unknown option --nope: --chief,",
        ),
    ] {
        let (status, out, err) = answer(line);
        assert_eq!((status, out.as_str()), (2, ""), "{line:?}");
        assert!(err.starts_with(said), "{line:?}: {err}");
    }
}

#[test]
fn the_summary_of_a_folder_of_reports_goes_where_the_words_say_and_a_flag_it_does_not_have_is_refused(
) {
    let folder = tempfile::tempdir().unwrap();
    let reports = folder.path().join("reports");
    fs::create_dir(&reports).unwrap();
    let results = folder.path().join("RESULTS.md");
    let line = |reports: &std::path::Path, results: &std::path::Path| {
        answer(&[
            "summary",
            "--reports",
            reports.to_str().unwrap(),
            "--results",
            results.to_str().unwrap(),
        ])
    };
    // Nothing kept yet: it says so, and writes nothing.
    assert_eq!(
        line(&reports, &results),
        (0, "no reports yet\n".to_owned(), String::new())
    );
    assert!(!results.exists());
    fs::write(
        reports.join("2026-10-01T10-00-00-000Z-a.json"),
        r#"{"scenario":"round-trip","chief":"claude","model":"m","seconds":60,"metrics":{"tasks":[],"parallel":0,"advice":0,"reviews":0,"notesToHuman":[],"questionsOnBoard":0}}"#,
    )
    .unwrap();
    let (status, out, err) = line(&reports, &results);
    assert_eq!((status, err.as_str()), (0, ""));
    assert!(
        out.starts_with("2026-10-01T10-00 · round-trip · claude (m) · claude · ? · ? · 0"),
        "{out}"
    );
    assert!(fs::read_to_string(&results)
        .unwrap()
        .contains("| 2026-10-01T10-00 | round-trip |"));
    let (status, _, err) = answer(&["summary", "--nope", "x"]);
    assert_eq!(status, 2);
    assert!(
        err.starts_with("cf-evals: unknown option --nope: --reports, --results\n"),
        "{err}"
    );
}

#[test]
fn a_command_that_fails_for_a_reason_that_is_not_its_words_answers_1_and_says_why() {
    let folder = tempfile::tempdir().unwrap();
    let (status, out, err) = answer(&[
        "watch",
        "--project",
        "nothing",
        "--home",
        folder.path().to_str().unwrap(),
    ]);
    assert_eq!((status, out.as_str()), (1, ""));
    assert!(err.starts_with("cf-evals: could not copy "), "{err}");
}

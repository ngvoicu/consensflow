//! The results file: one row per report, the same numbers the summary prints, and
//! a report from any of the days the evals have been run is read as it was written.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::Path;

use cf_evals::results::{results_table, row, Report, COLUMNS};
use cf_evals::summarize::{read_reports, summarize};
use serde_json::{json, Value};

/// A report of the kind the first evals wrote: no mechanics, no effort, the
/// board's questions as a list.
fn report() -> Value {
    json!({
        "scenario": "six-decisions",
        "chief": "devin",
        "model": "devin's default",
        "staff": ["claude", "pi"],
        "seconds": 338,
        "checks": [{ "ok": true }, { "ok": false }, { "ok": false }],
        "mechanics": [{ "ok": true }, { "ok": true }],
        "terminalAnswers": 2,
        "metrics": {
            "tasks": [{}, {}],
            "parallel": 2,
            "advice": 1,
            "reviews": 1,
            "questionsToHuman": [{}],
            "notesToHuman": [],
            "chiefEdits": null,
            "filesChanged": ["a", "b", "c"],
        },
    })
}

const NAME: &str = "2026-09-26T00-13-20-553Z-six-decisions-chief-devin-staff-claude-1.json";

fn kept(value: Value) -> Report {
    Report {
        name: Some(NAME.to_owned()),
        value,
    }
}

/// `value` with the fields of `changes` set over it (a `null` is the field set to
/// null) and the fields of `removed` taken away.
fn with(mut value: Value, changes: Value, removed: &[&str]) -> Value {
    for (key, change) in changes.as_object().unwrap() {
        value[key] = change.clone();
    }
    for key in removed {
        value.as_object_mut().unwrap().remove(*key);
    }
    value
}

fn cells(text: &[&str]) -> Vec<String> {
    text.iter().map(|cell| (*cell).to_owned()).collect()
}

#[test]
fn turns_a_report_into_one_row_of_the_same_numbers_the_summary_prints() {
    assert_eq!(
        row(&kept(report())),
        cells(&[
            "2026-09-26T00-13",
            "six-decisions",
            "devin (devin's default)",
            "claude+pi",
            "1/3",
            "2/2",
            "2",
            "2",
            "1",
            "1",
            "1",
            "0",
            "?",
            "3",
            "2",
            "?",
            "6",
        ])
    );
    assert_eq!(row(&kept(report())).len(), COLUMNS.len());
    // A run with explicit effort says it for the chief (or that it had none) and the staff.
    let with_effort = with(
        report(),
        json!({ "chief": "codex", "model": "gpt-5.6-sol", "effort": "high", "staffEffort": "medium" }),
        &[],
    );
    assert_eq!(
        row(&kept(with_effort.clone()))[2..4],
        cells(&["codex (gpt-5.6-sol, high)", "claude+pi (medium)"])[..]
    );
    let no_effort = with(with_effort, json!({ "effort": null }), &[]);
    assert_eq!(row(&kept(no_effort))[2], "codex (gpt-5.6-sol, default)");
}

#[test]
fn says_a_question_mark_for_a_column_a_report_predates_and_writes_a_markdown_table() {
    let older = with(report(), json!({}), &["mechanics", "terminalAnswers"]);
    let row = row(&kept(older.clone()));
    assert_eq!((row[5].as_str(), row[14].as_str()), ("?", "?"));
    let table = results_table(&[kept(report()), kept(older)]);
    let lines: Vec<&str> = table.trim_end().split('\n').collect();
    assert_eq!(lines[0], "# Chief eval results");
    assert!(
        lines[lines.len() - 2].starts_with("| 2026-09-26T00-13 | six-decisions | devin"),
        "{}",
        lines[lines.len() - 2]
    );
    assert!(
        lines[lines.len() - 1].ends_with("| ? | 2 | 2 | 1 | 1 | 1 | 0 | ? | 3 | ? | ? | 6 |"),
        "{}",
        lines[lines.len() - 1]
    );
    assert!(table.ends_with(" |\n"), "a file ends with a line end");
}

#[test]
fn names_the_arm_and_what_the_owner_was_asked_on_the_board_and_in_the_terminal() {
    let nocard = with(report(), json!({ "arm": "nocard" }), &[]);
    let mut nocard = nocard;
    nocard["metrics"]["questionsOnBoard"] = json!(0);
    nocard["metrics"]["ownerQuestions"] = json!({ "questions": 4, "turnsAsking": 2, "pickers": 0 });
    let columns = row(&kept(nocard));
    assert_eq!(
        [
            columns[2].as_str(),
            columns[10].as_str(),
            columns[15].as_str()
        ],
        ["devin [nocard] (devin's default)", "0", "4"]
    );
    let mut bare = with(
        report(),
        json!({ "arm": "bare", "staff": null }),
        &["mechanics"],
    );
    bare["metrics"]["tasks"] = json!([]);
    bare["metrics"]["questionsOnBoard"] = json!(0);
    bare["metrics"]["ownerQuestions"] = json!({ "questions": 3, "turnsAsking": 2, "pickers": 2 });
    let columns = row(&kept(bare));
    assert_eq!(
        [
            columns[2].as_str(),
            columns[3].as_str(),
            columns[15].as_str()
        ],
        ["devin [bare] (devin's default)", "none", "3+2p"]
    );
    // A report from when the board took questions reads as it counted them.
    let mut board_days = report();
    board_days["metrics"]["ownerQuestions"] =
        json!({ "board": { "decisions": 1 }, "terminal": { "questions": 4 } });
    let columns = row(&kept(board_days));
    assert_eq!([columns[10].as_str(), columns[15].as_str()], ["1", "4"]);
    let card = with(report(), json!({ "arm": "card" }), &[]);
    assert_eq!(row(&kept(card))[2], "devin (devin's default)");
}

#[test]
fn reads_a_report_the_way_javascript_did_a_missing_field_is_undefined_and_a_null_is_a_default() {
    // No chief: the first evals ran only Claude.
    let no_chief = with(report(), json!({}), &["chief", "staff", "model"]);
    let columns = row(&kept(no_chief));
    assert_eq!(columns[2], "claude (undefined)");
    assert_eq!(columns[3], "claude");
    // A chief of null is no chief either.
    let null_chief = with(report(), json!({ "chief": null, "staffEffort": "" }), &[]);
    assert_eq!(row(&kept(null_chief))[2], "claude (devin's default)");
    // Minutes are rounded as Math.round does: half a minute goes up.
    let at =
        |seconds: i64| row(&kept(with(report(), json!({ "seconds": seconds }), &[])))[16].clone();
    assert_eq!(
        [at(0), at(29), at(30), at(89), at(90), at(338)],
        ["0", "0", "1", "1", "2", "6"].map(str::to_owned)
    );
    // A report with no name has no `when`.
    let unnamed = Report {
        name: None,
        value: report(),
    };
    assert_eq!(row(&unnamed)[0], "");
}

#[test]
fn the_file_begins_as_the_tracked_results_file_does_its_title_what_each_column_means_and_its_header(
) {
    let tracked = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("evals")
            .join("RESULTS.md"),
    )
    .unwrap();
    let header: Vec<&str> = tracked.lines().take(14).collect();
    assert_eq!(results_table(&[]), format!("{}\n", header.join("\n")));
}

#[test]
fn reads_the_reports_of_a_folder_oldest_first_and_only_those_that_are_json() {
    let folder = tempfile::tempdir().unwrap();
    for (name, scenario) in [
        ("2026-10-02T10-00-00-000Z-b.json", "second"),
        ("2026-10-01T10-00-00-000Z-a.json", "first"),
    ] {
        fs::write(
            folder.path().join(name),
            json!({ "scenario": scenario }).to_string(),
        )
        .unwrap();
    }
    fs::write(folder.path().join("notes.md"), "not a report").unwrap();
    fs::create_dir(folder.path().join("windows")).unwrap();
    let reports = read_reports(folder.path()).unwrap();
    let said: Vec<(&str, &str)> = reports
        .iter()
        .map(|report| {
            (
                report.name.as_deref().unwrap(),
                report.value["scenario"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        said,
        [
            ("2026-10-01T10-00-00-000Z-a.json", "first"),
            ("2026-10-02T10-00-00-000Z-b.json", "second"),
        ]
    );
    // A folder that is not there holds none.
    assert!(read_reports(&folder.path().join("missing"))
        .unwrap()
        .is_empty());
}

#[test]
fn a_report_that_is_no_json_is_named_in_the_error() {
    let folder = tempfile::tempdir().unwrap();
    fs::write(
        folder.path().join("2026-10-01T10-00-00-000Z-a.json"),
        "{ nope",
    )
    .unwrap();
    let failed = read_reports(folder.path()).unwrap_err().to_string();
    assert!(
        failed.contains("2026-10-01T10-00-00-000Z-a.json") && failed.contains("no JSON"),
        "{failed}"
    );
}

#[test]
fn the_summary_says_a_line_for_each_report_and_writes_the_results_file_and_none_when_there_is_none()
{
    let folder = tempfile::tempdir().unwrap();
    let (reports, results) = (
        folder.path().join("reports"),
        folder.path().join("RESULTS.md"),
    );
    fs::create_dir(&reports).unwrap();
    let mut said = Vec::new();
    assert_eq!(summarize(&reports, &results, &mut said).unwrap(), 0);
    assert_eq!(String::from_utf8(said).unwrap(), "no reports yet\n");
    assert!(
        !results.exists(),
        "no file is written when there is nothing to say"
    );

    fs::write(reports.join(NAME), report().to_string()).unwrap();
    let mut said = Vec::new();
    assert_eq!(summarize(&reports, &results, &mut said).unwrap(), 1);
    let said = String::from_utf8(said).unwrap();
    let lines: Vec<&str> = said.lines().collect();
    assert_eq!(
        lines[0],
        "2026-09-26T00-13 · six-decisions · devin (devin's default) · claude+pi · 1/3 · 2/2 · 2 · 2 · 1 · 1 · 1 · 0 · ? · 3 · 2 · ? · 6"
    );
    assert!(
        lines[1].starts_with("written: ") && lines[1].ends_with(" (1 rows)"),
        "{said}"
    );
    let written = fs::read_to_string(&results).unwrap();
    assert_eq!(
        written,
        results_table(&[Report {
            name: Some(NAME.to_owned()),
            value: report()
        }])
    );
}

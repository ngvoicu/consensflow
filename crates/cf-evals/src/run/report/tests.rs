//! What a run keeps and says of itself, to the key and to the word.

use super::*;
use crate::measure::{MemberQuestions, OwnerQuestions};
use crate::plan::{chief_environment, staff_for};

fn options(line: &str) -> Options {
    let words: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
    Options::of(&words).unwrap()
}

fn check(name: &str, ok: bool) -> Check {
    Check {
        name: name.to_owned(),
        ok,
    }
}

/// A run of the card arm that did what the scenario asked, in `seconds`.
fn card<'a>(
    options: &'a Options,
    chief: &'a ChiefSetup,
    staff: &'a Staff,
    mechanics: Vec<Check>,
) -> Card<'a> {
    Card {
        options,
        scenario: &options.scenario,
        chief,
        staff,
        index: 2,
        seconds: 338,
        metrics: Metrics {
            chief_turns: 4,
            chief_edits: Some(2),
            owner_questions: OwnerQuestions {
                questions: 2,
                turns_asking: 1,
                pickers: 1,
                texts: Vec::new(),
            },
            member_questions_by: vec![MemberQuestions {
                role: "worker".to_owned(),
                harness: Some("claude-code".to_owned()),
                asked: 1,
                answered: 1,
                delivered: 1,
                longest: 13,
            }],
            ..Metrics::default()
        },
        checks: vec![
            check("one task goes to a worker", true),
            check("the owner is not asked anything", false),
        ],
        mechanics,
        screen: vec!["a".to_owned(), "b".to_owned()],
        said: Said {
            nudges: 1,
            replies: 2,
            dialogs: 3,
            follow_ups: 4,
        },
        refused: vec![Refused {
            message: json!(9),
            error: json!("no"),
        }],
        approvals: 5,
        log: json!([{ "at": 1, "line": "x" }]),
        path: PathBuf::from("evals/reports/r.json"),
    }
}

#[test]
fn a_card_report_has_the_keys_the_first_evals_wrote_in_their_order() {
    let options = options("--scenario round-trip --chief pi --staff claude,codex --gate");
    let chief = chief_environment("pi", None).unwrap();
    let staff = staff_for(&options.staff, &options.staff_models(), Some("medium")).unwrap();
    let run = card(
        &options,
        &chief,
        &staff,
        vec![check("every task brief was delivered (1/1)", true)],
    );
    let report = run.value();
    let keys: Vec<&str> = report
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "scenario",
            "chief",
            "model",
            "effort",
            "staffEffort",
            "staff",
            "switchTo",
            "staffModels",
            "run",
            "seconds",
            "metrics",
            "checks",
            "mechanics",
            "chiefScreen",
            "arm",
            "terminalAnswers",
            "terminalReplies",
            "pickerAnswers",
            "followUpsSent",
            "refusedApprovals",
            "gate",
            "approvals",
            "log",
        ]
    );
    assert_eq!(report["scenario"], "round-trip");
    assert_eq!(report["model"], "openrouter/deepseek/deepseek-v4-pro-0813");
    assert_eq!(report["effort"], "high");
    assert_eq!(report["staff"], json!(["claude", "codex"]));
    assert_eq!(report["switchTo"], Value::Null);
    assert_eq!(report["staffModels"]["eval-codex-reviewer"], "gpt-5.6-luna");
    assert_eq!(report["staffModels"].as_object().unwrap().len(), 8);
    assert_eq!(
        (
            report["terminalAnswers"].clone(),
            report["terminalReplies"].clone(),
            report["pickerAnswers"].clone(),
            report["followUpsSent"].clone()
        ),
        (json!(1), json!(2), json!(3), json!(4))
    );
    assert_eq!(
        report["refusedApprovals"],
        json!([{ "message": 9, "error": "no" }])
    );
    assert_eq!(
        (
            report["gate"].clone(),
            report["approvals"].clone(),
            report["arm"].clone()
        ),
        (json!(true), json!(5), json!("card"))
    );
    assert_eq!(
        report["checks"][1],
        json!({ "name": "the owner is not asked anything", "ok": false })
    );
    assert_eq!(report["metrics"]["chiefTurns"], 4);
}

#[test]
fn a_chief_that_has_no_switch_for_its_effort_says_so_in_its_report_with_a_null() {
    let options = options("--chief devin");
    let chief = chief_environment("devin", None).unwrap();
    let staff = staff_for(&options.staff, &options.staff_models(), None).unwrap();
    let report = card(&options, &chief, &staff, Vec::new()).value();
    assert_eq!(report["effort"], Value::Null);
    assert_eq!(report["model"], "swe-2-medium");
}

#[test]
fn a_report_is_named_by_when_it_was_made_the_scenario_the_chief_its_arm_the_staff_and_the_run() {
    let at = Timestamp::from_millisecond(1_791_711_000_123).unwrap();
    assert_eq!(
        file_name(
            at,
            &options("--scenario six-decisions --staff claude,pi"),
            1
        ),
        "2026-10-11T09-30-00-123Z-six-decisions-chief-claude-staff-claude+pi-1.json"
    );
    assert_eq!(
        file_name(
            at,
            &options("--scenario simple-fix --chief codex --arm nocard --staff pi"),
            3
        ),
        "2026-10-11T09-30-00-123Z-simple-fix-chief-codex-nocard-staff-pi-3.json"
    );
    assert_eq!(
        file_name(at, &options("--scenario simple-fix --arm bare"), 2),
        "2026-10-11T09-30-00-123Z-simple-fix-chief-claude-bare-2.json"
    );
}

#[test]
fn a_run_held_when_every_expectation_and_every_check_of_the_board_did() {
    let options = options("--scenario round-trip");
    let chief = chief_environment("claude", None).unwrap();
    let staff = staff_for(&options.staff, &options.staff_models(), None).unwrap();
    let mut run = card(
        &options,
        &chief,
        &staff,
        vec![check("the board showed every task (2/2)", true)],
    );
    assert!(!run.held(), "an expectation did not hold");
    run.checks = vec![check("one task goes to a worker", true)];
    assert!(run.held());
    run.mechanics
        .push(check("every result reached the chief (1/2)", false));
    assert!(!run.held(), "a check of the board did not");
}

#[test]
fn the_verdict_is_said_on_the_console_as_it_always_was() {
    let options = options("--scenario round-trip --staff claude");
    let chief = chief_environment("claude", None).unwrap();
    let staff = staff_for(&options.staff, &options.staff_models(), Some("medium")).unwrap();
    let run = card(
        &options,
        &chief,
        &staff,
        vec![check("every task brief was delivered (1/1)", true)],
    );
    let mut said = Vec::new();
    run.say(&mut Floor::new(&mut said)).unwrap();
    assert_eq!(
        String::from_utf8(said).unwrap(),
        "\nround-trip · chief claude (claude-opus-5, effort high) · staff effort medium · staff claude · run 2 · 338s\n\
         \x20 PASS one task goes to a worker\n\
         \x20 FAIL the owner is not asked anything\n\
         \x20 the plumbing:\n\
         \x20 PASS every task brief was delivered (1/1)\n\
         \x20 tasks 0 (parallel 0, advice 0, reviews 0) · notes 0 · chief edits 2 in 4 turns · files changed 0 · answered in the terminal 1 (nudges) + 2 (its questions)\n\
         \x20 the owner was asked in the terminal: 2 questions in 1 turns, 1 in its question dialog · on the board 0 · cf ask refused 0\n\
         \x20 members asked the chief: worker/claude-code 1 (answered 1, delivered 1, longest 13)\n\
         \x20 report: evals/reports/r.json\n"
    );
}

#[test]
fn a_chief_that_said_nothing_has_the_end_of_its_screen_said_so_and_an_arm_is_named_in_the_header() {
    let options = options("--scenario round-trip --arm nocard --chief devin");
    let chief = chief_environment("devin", None).unwrap();
    let staff = staff_for(&options.staff, &options.staff_models(), None).unwrap();
    let mut run = card(&options, &chief, &staff, Vec::new());
    run.metrics = Metrics::default();
    run.screen = (1..=12).map(|n| format!("line {n}")).collect();
    let mut said = Vec::new();
    run.say(&mut Floor::new(&mut said)).unwrap();
    let said = String::from_utf8(said).unwrap();
    assert!(
        said.starts_with("\nround-trip · chief devin [nocard] (swe-2-medium, effort its default) · staff effort medium · staff devin · run 2 · 338s\n"),
        "{said}"
    );
    assert!(
        said.contains("  the chief's screen ended with:\n    line 5\n    line 6\n")
            && said.contains("    line 12\n"),
        "{said}"
    );
    assert!(!said.contains("line 4\n"));
    assert!(said.contains("chief edits ? in 0 turns"));
    assert!(said.contains("  members asked the chief: nothing\n"));
}

#[test]
fn a_bare_report_and_its_verdict_are_those_of_a_run_with_no_staff_and_no_board() {
    let options = options("--scenario simple-fix --arm bare --chief codex");
    let chief = chief_environment("codex", None).unwrap();
    let mut metrics = Metrics {
        chief: "chief".to_owned(),
        chief_turns: 3,
        ..Metrics::default()
    };
    metrics.owner_questions.questions = 2;
    metrics.owner_questions.turns_asking = 1;
    let run = Bare {
        options: &options,
        scenario: &options.scenario,
        chief: &chief,
        session: Some("s-1".to_owned()),
        index: 1,
        seconds: 61,
        metrics,
        checks: vec![check("no advice is asked for a one-word fix", true)],
        screen: Vec::new(),
        said: Said::default(),
        log: json!([]),
        path: PathBuf::from("evals/reports/b.json"),
    };
    let report = run.value();
    let keys: Vec<&str> = report
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "scenario",
            "chief",
            "model",
            "effort",
            "arm",
            "session",
            "run",
            "seconds",
            "metrics",
            "checks",
            "chiefScreen",
            "terminalAnswers",
            "terminalReplies",
            "followUpsSent",
            "log",
        ]
    );
    assert_eq!(
        (report["arm"].clone(), report["session"].clone()),
        (json!("bare"), json!("s-1"))
    );
    assert!(report["metrics"].get("plumbing").is_none());
    assert!(run.held());
    let mut said = Vec::new();
    run.say(&mut Floor::new(&mut said)).unwrap();
    assert_eq!(
        String::from_utf8(said).unwrap(),
        "\nsimple-fix · chief codex [bare] (gpt-5.6-luna, effort high) · run 1 · 61s\n\
         \x20 PASS no advice is asked for a one-word fix\n\
         \x20 turns 3 · files changed 0 · answered in the terminal 0 (nudges) + 0 (its questions)\n\
         \x20 the owner was asked in the terminal: 2 questions in 1 turns\n\
         \x20 report: evals/reports/b.json\n"
    );
}

#[test]
fn a_report_is_kept_as_indented_json_in_a_folder_made_for_it() {
    let folder = tempfile::tempdir().unwrap();
    let reports = folder.path().join("not").join("yet");
    let kept = keep(
        &reports,
        "r.json",
        &json!({ "scenario": "x", "checks": [] }),
    )
    .unwrap();
    assert_eq!(kept, reports.join("r.json"));
    assert_eq!(
        std::fs::read_to_string(kept).unwrap(),
        "{\n  \"scenario\": \"x\",\n  \"checks\": []\n}\n"
    );
}

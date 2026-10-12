//! The eval's plan: who is on the staff, how the chief gets its model, how the
//! human answers.

#![allow(clippy::unwrap_used)]

use std::time::Duration;

use cf_evals::plan::{
    chief_environment, dialog_answers, harness, last_lines, staff_for, terminal_answer,
    DialogQuestion, CHIEF_MODEL, CLAUDE_CHIEF_MODEL, HARNESSES,
};
use cf_evals::scenarios::{Answer, Scenario};
use regex::Regex;
use serde_json::json;

/// The owner's answer to the questions a sentence of which matches `pattern`.
fn answer(pattern: &str, text: &str) -> Answer {
    Answer {
        matches: Regex::new(pattern).unwrap(),
        text: text.to_owned(),
    }
}

/// A scenario that has only what the owner's answers need.
fn answering(answers: Vec<Answer>, fallback: &str) -> Scenario {
    Scenario {
        id: "answering",
        title: "",
        fixture: "site",
        prompt: String::new(),
        follow_ups: Vec::new(),
        answers,
        fallback: fallback.to_owned(),
        nudge: None,
        quiet: Duration::from_secs(1),
        expectations: Vec::new(),
    }
}

#[test]
fn gives_each_staff_harness_two_workers_an_advisor_and_a_reviewer_on_its_cheap_model_all_standard_tier(
) {
    let staff = staff_for(&["codex", "pi"], &[("pi", "openrouter/other")], None).unwrap();
    let rows: Vec<(&str, &str, &str)> = staff
        .agents
        .iter()
        .map(|agent| (agent.id.as_str(), agent.kind, agent.model.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            ("eval-codex-worker", "codex", "gpt-5.6-luna"),
            ("eval-codex-worker-2", "codex", "gpt-5.6-luna"),
            ("eval-codex-advisor", "codex", "gpt-5.6-luna"),
            ("eval-codex-reviewer", "codex", "gpt-5.6-luna"),
            ("eval-pi-worker", "pi", "openrouter/other"),
            ("eval-pi-worker-2", "pi", "openrouter/other"),
            ("eval-pi-advisor", "pi", "openrouter/other"),
            ("eval-pi-reviewer", "pi", "openrouter/other"),
        ]
    );
    let members: Vec<(&str, &str)> = staff
        .members
        .iter()
        .map(|member| (member.agent.as_str(), member.role))
        .collect();
    let roles = ["worker", "worker", "advisor", "reviewer"];
    assert_eq!(
        members,
        staff
            .agents
            .iter()
            .enumerate()
            .map(|(at, agent)| (agent.id.as_str(), roles[at % 4]))
            .collect::<Vec<_>>()
    );
    // The rows of the roster say so: every one is standard tier, so the daemon
    // picks among them by its own rule and any harness may get any task.
    assert!(staff
        .agents
        .iter()
        .all(|agent| agent.to_value()["workTier"] == "standard"));
    assert_eq!(
        staff.to_value()[2],
        json!({ "agent": "eval-codex-advisor", "roles": ["advisor"] })
    );
}

#[test]
fn an_effort_reaches_every_member_as_its_harness_names_it_and_devin_has_none() {
    let staff = staff_for(&["claude", "pi", "devin"], &[], Some("medium")).unwrap();
    let claude = staff.agents[0].to_value();
    let pi = staff.agents[4].to_value();
    let devin = staff.agents[8].to_value();
    assert_eq!(claude["effort"], "medium");
    assert_eq!(pi["thinking"], "medium");
    assert!(pi.get("effort").is_none());
    assert!(devin.get("effort").is_none() && devin.get("thinking").is_none());
    let plain = staff_for(&["claude"], &[], None).unwrap().agents[0].to_value();
    assert!(
        plain.get("effort").is_none(),
        "no effort unless one is given"
    );
    // The key order of a roster row, as the JavaScript's was.
    assert_eq!(
        serde_json::to_string(&claude).unwrap(),
        r#"{"id":"eval-claude-worker","kind":"claude-code","model":"claude-haiku-4-5-20251001","workTier":"standard","effort":"medium"}"#
    );
    let refused = staff_for(&["kimi"], &[], None).unwrap_err();
    assert_eq!(refused.to_string(), "no such eval harness: kimi");
    assert_eq!(
        HARNESSES.map(|harness| harness.name),
        ["claude", "codex", "pi", "opencode", "devin"]
    );
    assert_eq!(harness("devin").unwrap().kind, "devin");
}

#[test]
fn sets_the_chiefs_model_through_the_environment_where_the_harness_takes_it_there() {
    let setup = |chief: &str, model: Option<&str>| {
        let setup = chief_environment(chief, model).unwrap();
        (setup.env, setup.model)
    };
    assert_eq!(
        setup("claude", Some("claude-opus-5")),
        (
            vec![("ANTHROPIC_MODEL".to_owned(), "claude-opus-5".to_owned())],
            "claude-opus-5".to_owned()
        )
    );
    assert_eq!(
        setup("opencode", Some("opencode/x")),
        (
            vec![(
                "OPENCODE_CONFIG_CONTENT".to_owned(),
                r#"{"model":"opencode/x"}"#.to_owned()
            )],
            "opencode/x".to_owned()
        )
    );
    assert_eq!(setup("codex", None), (vec![], "gpt-5.6-luna".to_owned()));
    assert_eq!(setup("codex", Some("gpt-x")), (vec![], "gpt-x".to_owned()));
    assert_eq!(
        setup("pi", Some("openrouter/x")),
        (vec![], "openrouter/x".to_owned())
    );
    assert_eq!(setup("pi", None).1, CHIEF_MODEL);
    assert_eq!(setup("claude", None).1, CLAUDE_CHIEF_MODEL);
    assert_eq!(setup("claude", None).1, "claude-opus-5");
    assert_eq!(setup("opencode", None).1, CHIEF_MODEL);
    // A Devin chief runs its staff's model, whatever --model says.
    assert_eq!(
        setup("devin", Some("x")),
        (vec![], "swe-2-medium".to_owned())
    );
    assert_eq!(
        chief_environment("kimi", Some("x"))
            .unwrap_err()
            .to_string(),
        "no such eval harness: kimi"
    );
}

#[test]
fn names_the_folder_claude_keeps_the_workspaces_memory_in_by_the_rigs_rule_which_is_the_one_the_evals_had(
) {
    assert_eq!(
        cf_e2e::rig::project_name("/Users/x/.consensflow-candidate/evals/workspace"),
        "-Users-x--consensflow-candidate-evals-workspace"
    );
}

#[test]
fn keeps_the_last_non_empty_lines_a_window_printed_whatever_the_line_ending() {
    assert_eq!(last_lines("a\r\n\r\nb  \rc\n\n  \nd\n", 3), ["b", "c", "d"]);
    assert!(last_lines("", 40).is_empty());
    assert_eq!(last_lines("one\ntwo", 40), ["one", "two"]);
}

#[test]
fn answers_each_question_the_chief_left_in_its_terminal_once_per_subject_in_order() {
    let scenario = answering(
        vec![
            answer("(?i)document", "Keep it, with a note on top."),
            answer("(?i)publish", "Not yet."),
        ],
        "Yes.",
    );
    let text = [
        "I read the files. Two things:",
        "1. Do we keep the old document in docs/?",
        "2. Should I publish the page today?",
        "Also: the reference document, do we keep it as is?",
    ]
    .join("\n");
    assert_eq!(
        terminal_answer(&scenario, &text),
        "Keep it, with a note on top. Not yet."
    );
}

#[test]
fn falls_back_for_a_question_it_has_no_answer_for() {
    let scenario = answering(vec![answer("(?i)document", "Keep it.")], "Yes.");
    assert_eq!(
        terminal_answer(&scenario, "Shall I start with the footer?"),
        "Yes."
    );
    assert_eq!(
        terminal_answer(&scenario, "Keep the `docs/?` folder? And the footer?"),
        "Yes."
    );
    // Nothing asked, nothing answered.
    assert_eq!(terminal_answer(&scenario, "All done."), "");
}

#[test]
fn answers_a_devin_chiefs_dialog_the_scenarios_words_in_other_past_each_questions_options() {
    let scenario = answering(vec![answer("(?i)file|name", "site/cariere.html")], "");
    let questions = [
        DialogQuestion {
            question: "Which file name?".to_owned(),
            header: Some("File name".to_owned()),
            options: 2,
        },
        DialogQuestion {
            question: "Which language first?".to_owned(),
            header: Some("Language".to_owned()),
            options: 3,
        },
    ];
    let down = vec![27, 91, 66];
    let answers = dialog_answers(&scenario, &questions, &|text| text.to_uppercase());
    assert_eq!(answers.len(), 2);
    assert_eq!(answers[0].question, "Which file name?");
    assert_eq!(answers[0].answer.as_deref(), Some("site/cariere.html"));
    assert_eq!(
        answers[0].keys,
        [
            down.clone(),
            down,
            vec![13],
            b"SITE/CARIERE.HTML".to_vec(),
            vec![13]
        ]
    );
    // No answer on the subject: the first option, with Enter.
    assert_eq!(answers[1].answer, None);
    assert_eq!(answers[1].keys, [vec![13]]);
}

#[test]
fn a_question_of_the_dialog_is_read_as_devins_store_keeps_it() {
    let question = DialogQuestion::of(&json!({
        "question": "Which language?",
        "header": "File",
        "options": [{ "label": "a.html" }, { "label": "b.html" }],
    }));
    assert_eq!(
        question,
        DialogQuestion {
            question: "Which language?".to_owned(),
            header: Some("File".to_owned()),
            options: 2,
        }
    );
    let bare = DialogQuestion::of(&json!({ "question": "Sure?" }));
    assert_eq!((bare.header, bare.options), (None, 0));
}

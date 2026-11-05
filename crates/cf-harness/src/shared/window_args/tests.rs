//! The windows of `tests/goldens/launch/tables.json`, each as Node's window
//! code built it.

use std::fs;
use std::path::Path;

use serde_json::Value;

use super::*;

fn tables() -> Value {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/goldens/launch/tables.json");
    serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap()
}

/// A text of a row: none for null and for JavaScript's undefined.
fn text(value: &Value) -> Option<&str> {
    value.as_str()
}

#[test]
fn every_window_opens_and_resumes_as_node_built_it() {
    let tables = tables();
    let rows = tables["windows"].as_array().unwrap();
    for row in rows {
        let agent = &row["agent"];
        let harness = Harness::from_kind(agent["kind"].as_str().unwrap());
        let fields = Agent {
            model: text(&agent["model"]),
            effort: text(&agent["effort"]),
            thinking: text(&agent["thinking"]),
            designer: false,
        };
        let (session, seed) = (text(&row["sessionId"]), text(&row["seed"]));
        let window = harness.and_then(|harness| match row["call"].as_str().unwrap() {
            "start" => start(harness, fields, session, seed),
            _ => resume(harness, fields, session, seed),
        });
        assert_eq!(
            window.as_ref().map_or(Value::Null, Invocation::written),
            row["window"],
            "{row}"
        );
    }
}

/// A Muse agent on `model` at `effort`.
fn muse(model: Option<&'static str>, effort: Option<&'static str>) -> Agent<'static> {
    Agent {
        model,
        effort,
        thinking: None,
        designer: false,
    }
}

fn words(words: &[&str]) -> Vec<String> {
    words.iter().map(|&word| word.to_owned()).collect()
}

#[test]
fn a_muse_window_opens_in_full_permission_on_its_model_and_effort_with_no_message_on_its_command_line(
) {
    let agent = muse(Some("muse-spark-1.3"), Some("minimal"));
    let opened = start(Harness::Muse, agent, None, Some("The task.")).unwrap();
    assert_eq!(opened.command, "muse");
    assert_eq!(
        opened.args,
        words(&[
            "--model",
            "muse-spark-1.3",
            "--reasoning-effort",
            "minimal",
            "--yolo",
            "--contributor-mode",
            "off",
        ])
    );
    assert_eq!(
        (opened.prompt, opened.drop_env),
        (None, &["META_API_KEY"][..])
    );
    // With no model of its own, it is Muse's default, still out of contributor mode.
    let bare = start(Harness::Muse, muse(None, None), None, None).unwrap();
    assert_eq!(bare.args, words(&["--yolo", "--contributor-mode", "off"]));
}

#[test]
fn a_resumed_muse_window_opens_on_its_session_with_no_message_on_its_command_line() {
    let agent = muse(Some("muse-spark-1.3"), Some("minimal"));
    let session = "01a17bc9-e758-77f3-ac8d-a725edf373ae";
    let resumed = resume(
        Harness::Muse,
        agent,
        Some(session),
        Some("Your turn was cut."),
    )
    .unwrap();
    assert_eq!(
        resumed.args,
        words(&[
            "resume",
            session,
            "--model",
            "muse-spark-1.3",
            "--reasoning-effort",
            "minimal",
            "--yolo",
            "--contributor-mode",
            "off",
        ])
    );
    assert_eq!(resumed.prompt, None);
    assert!(resume(Harness::Muse, agent, None, None).is_none(), "no id");
}

#[test]
fn a_muse_contributor_model_the_human_picked_runs_in_its_contributor_mode() {
    let agent = muse(Some("muse-spark-1.3-contributor"), None);
    let opened = start(Harness::Muse, agent, None, None).unwrap();
    assert_eq!(
        opened.args,
        words(&["--model", "muse-spark-1.3-contributor", "--yolo"])
    );
}

/// The model a Devin window on `model` at `effort` is opened on.
fn devin_model_of(model: &str, effort: Option<&str>) -> String {
    let agent = Agent {
        model: Some(model),
        effort,
        thinking: None,
        designer: false,
    };
    let opened = start(Harness::Devin, agent, None, None).unwrap();
    let at = opened.args.iter().position(|arg| arg == "--model").unwrap();
    opened.args[at + 1].clone()
}

#[test]
fn a_devin_window_names_the_family_then_the_level_then_the_variant_as_devin_writes_its_ids() {
    // T-609: the level goes before a variant's words (claude-opus-5-5-medium-fast), where
    // joining the two made an id Devin has not (claude-opus-5-5-fast-medium).
    let cases = [
        ("claude-opus-5-5", Some("medium"), "claude-opus-5-5-medium"),
        ("swe-2", Some("max"), "swe-2-max"),
        (
            "claude-opus-5-5-fast",
            Some("medium"),
            "claude-opus-5-5-medium-fast",
        ),
        (
            "gpt-6-sol-priority",
            Some("none"),
            "gpt-6-sol-none-priority",
        ),
        ("swe-2-lightning", Some("high"), "swe-2-high-lightning"),
        ("glm-5-2-1m", Some("max"), "glm-5-2-max-1m"),
        // A model with no level is run as it is named, its variant's words with it.
        ("claude-opus-5-5-fast", None, "claude-opus-5-5-fast"),
        (
            "claude-opus-4-6-thinking-1m",
            None,
            "claude-opus-4-6-thinking-1m",
        ),
        ("swe-1-7", None, "swe-1-7"),
        // A level the model names already gives way to the agent's effort.
        ("claude-opus-5-5-high", Some("max"), "claude-opus-5-5-max"),
        (
            "claude-opus-5-5-high-fast",
            Some("low"),
            "claude-opus-5-5-low-fast",
        ),
    ];
    for (model, effort, id) in cases {
        assert_eq!(devin_model_of(model, effort), id, "{model} at {effort:?}");
    }
}

/// What a chief or a member picked on each harness reaches its window as (harness-models):
/// the agent the engine gives the launch (`ChiefRuns`: the effort, Pi's as its thinking,
/// none for Devin, whose model is the exact id it lists for the effort) becomes the flags
/// the harness takes.
#[test]
fn a_model_and_effort_picked_reach_each_harness_s_window_as_it_takes_them() {
    let agent = |model, effort, thinking| Agent {
        model: Some(model),
        effort,
        thinking,
        designer: false,
    };
    for (harness, given, flags) in [
        (
            Harness::Claude,
            agent("claude-opus-5-5", Some("high"), None),
            vec!["--model", "claude-opus-5-5", "--effort", "high"],
        ),
        (
            Harness::Codex,
            agent("gpt-6.1-sol", Some("ultra"), None),
            vec![
                "--model",
                "gpt-6.1-sol",
                "-c",
                "model_reasoning_effort=\"ultra\"",
            ],
        ),
        (
            Harness::Pi,
            agent("anthropic/claude-fable-5", None, Some("max")),
            vec!["--model", "anthropic/claude-fable-5", "--thinking", "max"],
        ),
        (
            Harness::Devin,
            agent("swe-2-max", None, None),
            vec!["--model", "swe-2-max"],
        ),
        (
            Harness::Muse,
            agent("muse-wonder-1.0", Some("low"), None),
            vec!["--model", "muse-wonder-1.0", "--reasoning-effort", "low"],
        ),
    ] {
        let window = start(harness, given, Some("s-1"), Some("hello")).expect("a window");
        let args = window.args.join(" ");
        assert!(args.contains(&flags.join(" ")), "{harness:?}: {args}");
    }
}

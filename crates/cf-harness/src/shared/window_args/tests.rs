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

#[test]
fn every_id_devin_lists_with_a_level_is_what_a_window_on_its_family_and_variant_at_that_level_opens_on(
) {
    // What Devin 3000.11.205 listed on 2026-10-25 (`devin models list --format json`),
    // the catalog's copy: each id with a level is its family and variant at the level.
    let file =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../cf-catalog/tests/catalog/devin-models.json");
    let listed: Value = serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap();
    let levels = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];
    let variants = ["fast", "priority", "lightning", "1m", "thinking"];
    let mut read = 0;
    for family in listed["families"].as_array().unwrap() {
        if family["slug"] == "fusion" {
            continue;
        }
        for id in family["ids"].as_array().unwrap() {
            let id = id.as_str().unwrap();
            let words: Vec<&str> = id.split('-').collect();
            let Some(level) = words
                .iter()
                .rposition(|word| !variants.contains(word))
                .filter(|&at| levels.contains(&words[at]))
            else {
                continue;
            };
            let mut model = words.clone();
            let effort = model.remove(level);
            assert_eq!(devin_model_of(&model.join("-"), Some(effort)), id);
            read += 1;
        }
    }
    assert!(read > 200, "the list has its ids: {read}");
}

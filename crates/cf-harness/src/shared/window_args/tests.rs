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
fn a_muse_window_opens_in_full_permission_on_its_model_and_effort_with_its_first_message_last() {
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
            "The task.",
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
fn a_resumed_muse_window_takes_its_first_message_once_it_is_up_not_on_its_command_line() {
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
    assert_eq!(resumed.prompt.as_deref(), Some("Your turn was cut."));
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

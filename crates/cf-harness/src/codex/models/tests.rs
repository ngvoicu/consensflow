use std::rc::Rc;

use serde_json::json;

use super::*;
use crate::testing::{ChildScript, Driver, Ends, ManualTime, ScriptedProcesses};

/// What Codex 0.165.0's app-server answered `model/list` with on 2026-10-28, hidden models
/// included (`tools/probes/chief-harness/models.py codex`): signed in, the account's list;
/// signed out, the one it ships.
const SIGNED_IN: &str = include_str!("../../../tests/fixtures/models/codex-model-list.json");
const BUNDLED: &str = include_str!("../../../tests/fixtures/models/codex-model-list-bundled.json");

fn page(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

const AT: i64 = 1_793_000_000_000;

#[test]
fn the_account_s_list_has_its_models_efforts_and_default_and_no_hidden_one() {
    let models = parse(&[page(SIGNED_IN)], Some(AT - 60_000), AT).unwrap();
    let ids: Vec<&str> = models.models.iter().map(|model| &*model.id).collect();
    assert_eq!(
        ids,
        [
            "gpt-6.1-sol",
            "gpt-6-astra",
            "gpt-6.1-terra",
            "gpt-6-sol",
            "gpt-6-luna",
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna"
        ],
        "gpt-reserve and codex-auto-review are hidden"
    );
    let sol = &models.models[0];
    assert_eq!(sol.label, "GPT-6.1-Sol");
    assert_eq!(
        sol.efforts,
        ["low", "medium", "high", "xhigh", "max", "ultra"]
    );
    assert!(sol.default_effort.is_some());
    assert!(sol.marked_default);
    assert!(!models.is_stale());
    assert_eq!(
        models.default_model().map(|model| &*model.id),
        Some("gpt-6.1-sol")
    );
}

#[test]
fn a_list_from_no_copy_of_the_account_s_or_an_old_copy_is_stale_and_gives_no_default() {
    let bundled = parse(&[page(BUNDLED)], None, AT).unwrap();
    assert_eq!(
        bundled.freshness,
        cf_proto::models::Freshness::Stale {
            why: "the list Codex ships: it holds no copy of the account's".to_owned()
        }
    );
    assert!(bundled.model("gpt-5.5").is_some(), "what Codex ships");
    assert_eq!(bundled.default_model(), None);

    let old = parse(&[page(SIGNED_IN)], Some(AT - FRESH_MS - 1), AT).unwrap();
    assert!(old.is_stale());
    let fresh = parse(&[page(SIGNED_IN)], Some(AT - FRESH_MS), AT).unwrap();
    assert!(!fresh.is_stale());
}

#[test]
fn the_copy_s_time_is_its_fetched_at() {
    let cache = r#"{"fetched_at":"2026-10-28T19:52:23.955109Z","models":[]}"#;
    let at: jiff::Timestamp = "2026-10-28T19:52:23.955Z".parse().unwrap();
    assert_eq!(fetched_at(cache), Some(at.as_millisecond()));
    assert_eq!(fetched_at("{}"), None);
    assert_eq!(fetched_at("not json"), None);
}

#[test]
fn every_page_is_read_and_a_list_of_none_is_no_list() {
    let first = json!({ "data": [{ "model": "a", "displayName": "A" }], "nextCursor": "x" });
    let second = json!({ "data": [{ "model": "b", "hidden": true }, { "id": "c" }] });
    let models = parse(&[first, second], Some(AT), AT).unwrap();
    let ids: Vec<&str> = models.models.iter().map(|model| &*model.id).collect();
    assert_eq!(ids, ["a", "c"]);
    assert!(parse(&[json!({ "data": [] })], Some(AT), AT).is_err());
}

fn line(value: Value) -> String {
    value.to_string()
}

#[test]
fn a_read_initializes_then_asks_each_page_until_no_cursor() {
    let processes = Rc::new(ScriptedProcesses::default());
    processes.child(
        "codex",
        ChildScript {
            lines: vec![
                line(json!({ "id": 1, "result": {} })),
                line(json!({ "method": "thread/started" })),
                line(json!({ "id": 2, "result": { "data": [{ "model": "a" }], "nextCursor": "n" } })),
                line(json!({ "id": 3, "result": { "data": [{ "model": "b" }], "nextCursor": null } })),
            ],
            ends: Ends::Asked,
        },
    );
    let time = Rc::new(ManualTime::new(0));
    let mut driver = Driver::default();
    let (asked, clock) = (Rc::clone(&processes), Rc::clone(&time));
    driver.begin(0, async move {
        pages(
            &*asked,
            &*clock,
            program(PathBuf::from("codex"), &Env::default()),
        )
        .await
    });
    let (_, answered) = driver.run().pop().expect("read with no wait");
    assert_eq!(answered.unwrap().len(), 2);
    let written: Vec<Value> = processes
        .take_written()
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let methods: Vec<&str> = written
        .iter()
        .map(|line| line["method"].as_str().unwrap())
        .collect();
    assert_eq!(
        methods,
        ["initialize", "initialized", "model/list", "model/list"]
    );
    assert_eq!(written[3]["params"]["cursor"], "n");
    assert_eq!(written[2]["params"]["includeHidden"], false);
}

#[test]
fn a_refusal_is_a_failure_with_codex_s_words() {
    let processes = Rc::new(ScriptedProcesses::default());
    processes.child(
        "codex",
        ChildScript {
            lines: vec![
                line(json!({ "id": 1, "result": {} })),
                line(json!({ "id": 2, "error": { "message": "not now" } })),
            ],
            ends: Ends::Asked,
        },
    );
    let time = ManualTime::new(0);
    let answered = crate::testing::finished(Box::pin(pages(
        &*processes,
        &time,
        program(PathBuf::from("codex"), &Env::default()),
    )));
    assert_eq!(
        answered,
        Err("codex app-server refused model/list: not now".to_owned())
    );
}

#[test]
fn a_model_codex_does_not_have_is_said_in_its_words() {
    // As Codex 0.165.0 said it on CHF-02's probe.
    assert!(gone(
        "The 'gpt-gone-9' model is not supported when using Codex with a ChatGPT account.",
        "gpt-gone-9"
    ));
    assert!(gone(
        r#"{"error":{"code":"model_not_found","message":"The model `gpt-gone-9` does not exist"}}"#,
        "gpt-gone-9"
    ));
    assert!(!gone("stream disconnected before completion", "gpt-gone-9"));
    assert!(!gone(
        "The 'gpt-gone-9' model is not supported when using Codex with a ChatGPT account.",
        "gpt-6-luna"
    ));
}

#[test]
fn a_codex_whose_list_cannot_be_read_is_left_to_its_own_default() {
    let processes = crate::testing::ScriptedProcesses::default();
    let time = crate::testing::ManualTime::new(0);
    let env = Env::from_vars([("PATH", "/nowhere")]);
    assert_eq!(
        crate::testing::finished(Box::pin(default_model(&env, &processes, &time))),
        Ok(None)
    );
}

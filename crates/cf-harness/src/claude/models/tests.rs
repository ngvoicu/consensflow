use std::rc::Rc;

use serde_json::json;

use super::*;
use crate::testing::{ChildScript, Driver, Ends, ManualTime, ScriptedProcesses};

/// When a test's list is read.
const AT: i64 = 1_793_000_000_000;

/// What Claude Code 2.1.312 answered `initialize` with on 2026-10-28, signed out
/// (`tools/probes/chief-harness/models.py claude`), its `account` block dropped.
const RECORDED: &str = include_str!("../../../tests/fixtures/models/claude-initialize.json");

fn recorded() -> Value {
    serde_json::from_str(RECORDED).unwrap()
}

#[test]
fn each_model_is_its_resolved_id_with_its_efforts_and_the_default_pointer_marks_one() {
    let models = parse(&recorded(), AT).unwrap();
    let ids: Vec<&str> = models.models.iter().map(|model| &*model.id).collect();
    assert_eq!(
        ids,
        [
            "claude-opus-5-5",
            "claude-fable-5-1",
            "claude-sonnet-5-5",
            "claude-haiku-5-5"
        ],
        "no `default` row, no alias"
    );
    let opus = &models.models[0];
    assert_eq!(opus.label, "Opus");
    assert_eq!(opus.efforts, ["low", "medium", "high", "xhigh", "max"]);
    assert!(opus.marked_default, "`default` resolves to it");
    assert!(models.models[1..].iter().all(|model| !model.marked_default));
    assert!(!models.is_stale());
    assert_eq!(models.how, HOW);
    assert_eq!(
        models.default_model().map(|model| &*model.id),
        Some("claude-opus-5-5")
    );
}

#[test]
fn a_row_with_no_effort_or_disabled_says_so_and_a_disabled_one_is_never_the_default() {
    let models = parse(
        &json!({ "models": [
        { "value": "default", "resolvedModel": "claude-fable-5-1" },
        { "value": "fable", "resolvedModel": "claude-fable-5-1", "displayName": "Fable",
          "disabled": true, "supportedEffortLevels": ["low"] },
        { "value": "haiku", "resolvedModel": "claude-haiku-5-5", "supportsEffort": false,
          "supportedEffortLevels": ["low"] },
    ] }),
        AT,
    )
    .unwrap();
    assert!(models.models[0].disabled && models.models[0].marked_default);
    assert!(models.models[1].efforts.is_empty());
    assert_eq!(
        models.default_model().map(|model| &*model.id),
        Some("claude-haiku-5-5")
    );
}

#[test]
fn an_answer_with_no_model_is_no_list() {
    assert!(parse(&json!({}), AT).is_err());
    assert!(parse(&json!({ "models": [{ "value": "default" }] }), AT).is_err());
}

#[test]
fn the_list_is_read_in_the_config_folder_it_is_given_never_the_human_s() {
    let program = program(
        PathBuf::from("/bin/claude"),
        &Env::from_vars([("HOME", "/home/someone")]),
        Path::new("/cf/model-lists/claude"),
    );
    assert_eq!(
        program.env.text("CLAUDE_CONFIG_DIR"),
        Some("/cf/model-lists/claude")
    );
    assert_eq!(
        program.cwd.as_deref(),
        Some(Path::new("/cf/model-lists/claude"))
    );
    assert!(program
        .args
        .contains(&"--no-session-persistence".to_owned()));
    assert_eq!(
        program.args[..4],
        ["-p", "--input-format", "stream-json", "--output-format"]
    );
}

/// What a read of a scripted `claude` writing `lines` came to.
fn read_of(lines: &[String]) -> (Result<ModelListing, String>, Vec<String>) {
    let processes = Rc::new(ScriptedProcesses::default());
    processes.child(
        "claude",
        ChildScript {
            lines: lines.to_vec(),
            ends: Ends::Asked,
        },
    );
    let time = Rc::new(ManualTime::new(0));
    let mut driver = Driver::default();
    let (asked, clock) = (Rc::clone(&processes), Rc::clone(&time));
    driver.begin(0, async move {
        let program = program(PathBuf::from("claude"), &Env::default(), Path::new("/c"));
        read(&*asked, &*clock, program).await
    });
    let mut done = driver.run();
    let (_, answer) = done.pop().expect("read with no wait");
    (answer, processes.take_written())
}

#[test]
fn a_read_asks_initialize_and_takes_its_answer_past_the_events_before_it() {
    let answer = json!({
        "type": "control_response",
        "response": { "subtype": "success", "request_id": "models", "response": recorded() },
    });
    let (read, written) = read_of(&[
        json!({ "type": "system", "subtype": "hook" }).to_string(),
        answer.to_string(),
    ]);
    assert_eq!(read.unwrap().models.len(), 4);
    let asked: Value = serde_json::from_str(&written[0]).unwrap();
    assert_eq!(asked["request"], json!({ "subtype": "initialize" }));
    assert_eq!(written.len(), 1, "no user turn: nothing else is written");
}

#[test]
fn a_refused_initialize_is_a_failure_with_claude_s_words() {
    let answer = json!({
        "type": "control_response",
        "response": { "subtype": "error", "request_id": "models", "error": "no" },
    });
    let (read, _) = read_of(&[answer.to_string()]);
    assert_eq!(read, Err("claude -p refused initialize: no".to_owned()));
}

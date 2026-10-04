//! Which `cf` answers what: the board's commands with a window's token, the
//! hooks with or without one, and everything else handed to the CLI's Node
//! sources, here refused because no runtime is named or it cannot start.

mod common;

use std::fs;

use common::cf;

#[test]
fn devins_session_hook_answers_without_a_token_and_with_no_line_break() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    let role = dir.path().join("SKILL.md");
    fs::write(
        &wire,
        "{\"sessionId\":\"native-a\",\"update\":{\"sessionUpdate\":\"config_option_update\",\"configOptions\":[{\"id\":\"mode\"}]}}\n",
    )
    .unwrap();
    fs::write(&role, "# ConsensFlow worker\n").unwrap();
    let ran = cf(
        &["hook", "devin-session"],
        &[
            ("CF_DEVIN_ROLE_FILE", role.to_str().unwrap()),
            ("CHISEL_PURE_ACP_WIRE_LOG", wire.to_str().unwrap()),
        ],
        r#"{"hook_event_name":"SessionStart","session_id":"native-a","source":"startup"}"#,
    );
    assert_eq!(ran.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&ran.stdout),
        r##"{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"# ConsensFlow worker\n"}}"##
    );
}

#[test]
fn a_question_hook_with_no_window_says_nothing_and_succeeds() {
    let event =
        r#"{"tool_name":"AskUserQuestion","tool_input":{"questions":[{"question":"Go?"}]}}"#;
    let ran = cf(
        &["hook", "claude"],
        &[("CONSENSFLOW_URL", "http://127.0.0.1:9")],
        event,
    );
    assert_eq!(
        (ran.status.code(), ran.stdout.len(), ran.stderr.len()),
        (Some(0), 0, 0)
    );
}

#[test]
fn outside_a_window_a_command_needs_the_runtime_the_app_names() {
    for env in [&[][..], &[("CONSENSFLOW_TOKEN", "")][..]] {
        let ran = cf(&["catalog"], env, "");
        assert_eq!(ran.status.code(), Some(1));
        assert!(ran.stdout.is_empty());
        let said = String::from_utf8_lossy(&ran.stderr);
        assert!(
            said.starts_with("cf: CONSENSFLOW_NODE is not set:"),
            "{said}"
        );
        assert!(said.contains("cf.mjs"), "{said}");
    }
}

#[test]
fn a_runtime_that_does_not_start_is_said_and_fails() {
    let ran = cf(
        &["catalog"],
        &[("CONSENSFLOW_NODE", "/nonexistent/node")],
        "",
    );
    assert_eq!(ran.status.code(), Some(1));
    let said = String::from_utf8_lossy(&ran.stderr);
    assert!(
        said.starts_with("cf: /nonexistent/node did not start:"),
        "{said}"
    );
}

//! Which `cf` answers what: the board's commands with a window's token, the
//! hooks with or without one, and the Codex window's supervisor matched on
//! the first word. The standalone verbs are `standalone.rs`'s. A `ui` that
//! reaches the library is held by calling the library: a process never gets
//! one there.

mod common;

use std::ffi::OsString;
use std::{fs, io};

use cf_base::env::Env;
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
fn outside_a_window_an_empty_token_is_none_and_cf_answers_by_itself() {
    let ran = cf(
        &["catalog", "--harness", "pi"],
        &[("CONSENSFLOW_TOKEN", "")],
        "",
    );
    assert_eq!(ran.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&ran.stdout).starts_with("pi:\n"));
}

const BRIDGE: &str =
    r#"{"launchId":"launch-1","port":0,"token":"private-launch-token-1234567890"}"#;

#[test]
fn a_codex_window_is_matched_on_the_first_argument_before_a_window_token_is_looked_for() {
    // With a window's token anything else would be a board command.
    let ran = cf(
        &["codex-session", "/nonexistent/codex"],
        &[
            ("CONSENSFLOW_TOKEN", "window-token"),
            ("CONSENSFLOW_URL", "http://127.0.0.1:9"),
        ],
        "",
    );
    assert_eq!(ran.status.code(), Some(1));
    assert!(ran.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&ran.stderr),
        "ConsensFlow could not open Codex: Invalid Codex broker configuration\n"
    );
}

#[test]
fn a_json_flag_before_it_makes_it_no_codex_window() {
    // It is the standalone verbs' unknown command, as the words came.
    let ran = cf(&["--json", "codex-session", "/nonexistent/codex"], &[], "");
    assert_eq!(
        String::from_utf8_lossy(&ran.stderr),
        "cf: unknown command \"--json\" — run `cf help`\n"
    );
}

#[test]
fn a_codex_window_says_when_codex_cannot_start_and_leaves_no_socket_behind() {
    let home = tempfile::tempdir().unwrap();
    let ran = cf(
        &["codex-session", "/nonexistent/consensflow-codex", "resume"],
        &[
            ("CONSENSFLOW_HOME", home.path().to_str().unwrap()),
            ("CF_CODEX_SESSION_BRIDGE", BRIDGE),
        ],
        "",
    );
    assert_eq!(ran.status.code(), Some(1));
    assert!(ran.stdout.is_empty());
    let said = String::from_utf8_lossy(&ran.stderr);
    assert!(
        said.starts_with(
            "ConsensFlow could not open Codex: Codex server could not start: \
             /nonexistent/consensflow-codex: "
        ) && said.ends_with(")\n"),
        "{said}"
    );
    // The window's socket folder went with it.
    let left = fs::read_dir(home.path().join("tmp")).map_or(0, |entries| entries.count());
    assert_eq!(left, 0);
}

#[test]
fn a_ui_that_reaches_the_library_is_an_unknown_command_and_starts_nothing() {
    // `main` takes `ui` before `cf::run` (`cf::native_ui`), so no process gets
    // one there; a caller of the library that does not has no verb to answer it.
    let home = tempfile::tempdir().unwrap();
    let env = Env::from_vars([("CONSENSFLOW_HOME", home.path())]);
    for words in [&["ui"][..], &["ui", "--json", "--no-open"]] {
        let args: Vec<OsString> = words.iter().map(OsString::from).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = cf::run(&env, &args, &mut io::empty(), &mut out, &mut err).unwrap();
        assert_eq!(code, 1, "{words:?}");
        assert!(out.is_empty(), "{words:?}");
        assert_eq!(
            String::from_utf8(err).unwrap(),
            "cf: unknown command \"ui\" — run `cf help`\n",
            "{words:?}"
        );
    }
    // Nothing was made in the home: no daemon, no log.
    assert_eq!(fs::read_dir(home.path()).unwrap().count(), 0);
}

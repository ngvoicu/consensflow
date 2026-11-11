//! A window Devin 3000.12.43 opens again with no message on a session whose last turn a quit
//! cut, and that it left in full permission, says nothing of its mode, and so names no
//! conversation by its configuration, until it is given a message (T-1417 run 3, T-1432 runs A,
//! 2 and 3: 20 to 65 s silent, five windows). Its log names the conversation all the same, on the
//! lines of its replay. `/bypass` alone writes nothing where the window is in full permission
//! already (T-1432 run A, window b2); `/bypass <message>` writes the configuration as its turn
//! begins and runs the message under it, in either mode (run 2, windows b1 and b2; run 3, b3).

use serde_json::{json, Value};

use super::*;
use crate::testing::AnsweringHost;

/// What the log of a window opened again on `session` holds before anything is typed to it, as
/// T-1432 run 2's window b1 wrote it (the fields recorded, the texts and ids made up): two empty
/// lines, the replay of the turn the quit cut, the titles, the steps, the commands, the turn
/// named with no end, and the start hook. No configuration.
fn silent(session: &str) -> String {
    let lines = [
        json!({}),
        json!({}),
        json!({ "sessionId": session, "update": { "sessionUpdate": "user_message_chunk",
            "content": { "type": "text", "text": "[ConsensFlow m-1 · T-1 · task from @chief]\nRun sleep 60" },
            "_meta": { "cognition.ai/eventId": "3", "cognition.ai/eventType": "user_message",
                "cognition.ai/timestamp": "2026-11-11T07:08:06.000000+00:00" } } }),
        json!({ "sessionId": session, "update": { "sessionUpdate": "session_info_update", "title": "A sleep" } }),
        json!({ "sessionId": session, "steps": [{ "stepId": "s-1", "stepNumber": 1, "kind": "prompt",
            "userMessageId": "u-1", "revertTargetNodeId": 30, "forkTargetNodeId": 32,
            "summary": "[ConsensFlow m-1 · T-1 · task from @chief]" }] }),
        json!({ "sessionId": session, "update": { "sessionUpdate": "available_commands_update", "availableCommands": [] } }),
        json!({ "sessionId": session, "turnClientMessageId": "u-1" }),
        json!({ "sessionId": session, "update": { "sessionUpdate": "tool_call", "toolCallId": "hook-1",
            "status": "completed", "_meta": { "cognition.ai/eventType": "hook_execution",
                "cognition.ai/phase": "session_start", "cognition.ai/durationMs": 3008.0 } } }),
    ];
    lines.iter().map(|line| format!("{line}\n")).collect()
}

/// The configuration Devin writes as `/bypass <message>`'s turn begins: full permission.
fn bypassed(session: &str) -> String {
    let lines = [
        json!({ "sessionId": session, "update": { "sessionUpdate": "config_option_update",
            "configOptions": [{ "id": "mode", "currentValue": "bypass" }] } }),
        json!({ "sessionId": session, "update": { "sessionUpdate": "current_mode_update", "currentModeId": "bypass" } }),
    ];
    lines.iter().map(|line| format!("{line}\n")).collect()
}

/// A pane host whose screen is drawn, and that takes every paste.
fn drawn() -> AnsweringHost<impl Fn(&str) -> Result<Value, crate::contract::HostError>> {
    AnsweringHost::new(|op| {
        Ok(match op {
            "pane.snapshot" => json!({ "ok": true, "outputQuietMs": 0, "screenQuietMs": 2_000.0 }),
            _ => json!({ "ok": true }),
        })
    })
}

fn pane() -> Pane {
    Pane {
        id: "p1-zeus".to_owned(),
        generation: 1,
    }
}

/// The pastes the host was asked for, by their words.
fn pastes(asked: &RefCell<Vec<(String, Value)>>) -> Vec<String> {
    asked
        .borrow()
        .iter()
        .filter(|(op, _)| op == "pane.write_paste")
        .map(|(_, body)| body["body"].as_str().unwrap_or_default().to_owned())
        .collect()
}

fn hold() -> Readiness {
    Readiness::Held(Held::Because(HOLD.to_owned()))
}

const NOTE: &str = "[ConsensFlow m-2 · T-1 · note from @chief]\nGo on.";

#[test]
fn a_window_reopened_silent_on_its_conversation_is_held_a_while_then_given_its_delivery_behind_bypass(
) {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, silent("mild-coin")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window(&fakes, &wire, Some("mild-coin"));
    let host = drawn();
    // A Devin that announces writes its configuration within 2.3 s of its first line (T-1417 and
    // T-1432, nine windows): until the while is over the window is held, and reads unnamed.
    assert_eq!(finished(window.ready(&host, &pane())), Ok(hold()));
    assert!(finished(window.observe()).unwrap().unnamed);
    fakes.time.settle_at(EPOCH_MS + UNANNOUNCED_MS - 1);
    assert_eq!(finished(window.ready(&host, &pane())), Ok(hold()));
    // Past it, the window is on the conversation its log names, and read by its record.
    fakes.time.settle_at(EPOCH_MS + UNANNOUNCED_MS);
    let observed = finished(window.observe()).unwrap();
    assert!(!observed.unnamed && observed.switched.is_none() && observed.waiting.is_none());
    assert_eq!(finished(window.ready(&host, &pane())), Ok(Readiness::Ready));
    // Its delivery goes behind /bypass, which puts it in full permission whatever mode it is in.
    assert!(finished(window.deliver(&host, &pane(), NOTE)).is_ok());
    let note = if Env::default().on_windows() {
        console_text(&window_text(NOTE))
    } else {
        window_text(NOTE)
    };
    assert_eq!(pastes(&host.asked), [format!("{BYPASS} {note}")]);
    // The turn says its configuration: the next delivery goes as any does.
    append(&wire, &bypassed("mild-coin"));
    assert_eq!(finished(window.ready(&host, &pane())), Ok(Readiness::Ready));
    assert!(finished(window.deliver(&host, &pane(), NOTE)).is_ok());
    assert_eq!(pastes(&host.asked).last(), Some(&note));
}

#[test]
fn a_silent_log_that_names_another_conversation_two_or_none_holds_the_window_as_before() {
    let fakes = Fakes::new(&Env::default());
    let dir = tempfile::tempdir().unwrap();
    let cases = [
        ("another", silent("fresh-leaf")),
        (
            "two",
            format!("{}{}", silent("mild-coin"), silent("fresh-leaf")),
        ),
        ("none", "{}\n{}\n".to_owned()),
    ];
    for (case, log) in cases {
        let wire = dir.path().join(format!("{case}.jsonl"));
        fs::write(&wire, log).unwrap();
        let window = window(&fakes, &wire, Some("mild-coin"));
        let host = drawn();
        assert_eq!(finished(window.ready(&host, &pane())), Ok(hold()), "{case}");
        fakes
            .time
            .settle_at(fakes.time.wall_ms() + 10 * UNANNOUNCED_MS);
        assert_eq!(finished(window.ready(&host, &pane())), Ok(hold()), "{case}");
        let observed = finished(window.observe()).unwrap();
        assert!(observed.unnamed, "{case}");
        // Nothing goes into it, behind /bypass or not.
        assert!(finished(window.deliver(&host, &pane(), NOTE)).is_ok());
        assert!(
            pastes(&host.asked).is_empty(),
            "{case}: {:?}",
            pastes(&host.asked)
        );
    }
}

#[test]
fn a_window_that_announces_within_the_while_never_has_a_delivery_put_behind_bypass() {
    // Devin 3000.12.43 opened afresh writes a `steps` line of its session before its
    // configuration (T-1432, every fresh launch), and 3000.11 reopened writes its replay first.
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, silent("mild-coin")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window(&fakes, &wire, Some("mild-coin"));
    let host = drawn();
    assert_eq!(finished(window.ready(&host, &pane())), Ok(hold()));
    fakes.time.settle_at(EPOCH_MS + 2_300);
    append(&wire, &bypassed("mild-coin"));
    fakes.time.settle_at(EPOCH_MS + 10 * UNANNOUNCED_MS);
    assert_eq!(finished(window.ready(&host, &pane())), Ok(Readiness::Ready));
    assert!(finished(window.deliver(&host, &pane(), NOTE)).is_ok());
    assert!(!pastes(&host.asked)[0].starts_with(BYPASS));
}

#[test]
fn a_silent_window_the_human_takes_to_another_conversation_is_held_again() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, silent("mild-coin")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window(&fakes, &wire, Some("mild-coin"));
    let host = drawn();
    assert_eq!(finished(window.ready(&host, &pane())), Ok(hold()));
    fakes.time.settle_at(EPOCH_MS + UNANNOUNCED_MS);
    assert_eq!(finished(window.ready(&host, &pane())), Ok(Readiness::Ready));
    // The human's /resume of another: its replay names it, and nothing goes in.
    append(&wire, &silent("fresh-leaf"));
    fakes.time.settle_at(EPOCH_MS + 3 * UNANNOUNCED_MS);
    assert_eq!(finished(window.ready(&host, &pane())), Ok(hold()));
    assert!(finished(window.deliver(&host, &pane(), NOTE)).is_ok());
    assert!(pastes(&host.asked).is_empty());
}

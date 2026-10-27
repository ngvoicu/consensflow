//! A window of Claude Code whose harness moved its model on its own (MUS-24, T-841): what the
//! window's look says through the adapter, not only what the reader reads. The adapter builds
//! its own `Observed` (it adds Claude's status to the record), and the hold on a move is folded
//! into `Observed::quota` there as in every other harness's (`shared::record_state`): a look
//! that read the move and said no quota would leave the engine pasting into a window on a model
//! nobody chose.
//!
//! The records are the shapes Claude Code 2.1.205 wrote of a fallback (session `3afd6cd4`): the
//! main conversation's assistant record whose one content block is `{type: "fallback", from:
//! {model}, to: {model}}`.

use super::*;
use cf_harness::records::Quota;

const SESSION: &str = "c4d2a8f1-6b37-4e90-8a15-2f9e0b7d3c64";
const BRIEF: &str = "[ConsensFlow m-1 · T-1 · task from @chief]\nWrite the parser";

/// An assistant record of the main conversation on `model`, whose content is `content`.
fn assistant_line(n: u32, model: &str, content: &Value) -> String {
    record(
        SESSION,
        n,
        &json!({
            "type": "assistant",
            "isSidechain": false,
            "message": {
                "id": format!("{SESSION}-message-{n}"),
                "role": "assistant",
                "model": model,
                "content": content,
                "stop_reason": "end_turn",
            },
        }),
    )
}

fn fell_back_line(n: u32, from: &str, to: &str) -> String {
    assistant_line(
        n,
        to,
        &json!([{ "type": "fallback", "from": { "model": from }, "to": { "model": to } }]),
    )
}

fn said_line(n: u32, model: &str) -> String {
    assistant_line(
        n,
        model,
        &json!([{ "type": "text", "text": "Reading the parser." }]),
    )
}

/// A window on the conversation of `lines`, opened when the record held the brief alone: what
/// follows was written by the window's own process.
fn window_on(lines: &[String]) -> (Home, Rc<dyn Window>) {
    window_launched_on("claude-sonnet-5", lines)
}

/// The same, its launch naming the model `model`.
fn window_launched_on(model: &'static str, lines: &[String]) -> (Home, Rc<dyn Window>) {
    let home = Home::new();
    home.transcript(SESSION, &[user_line(SESSION, 1, BRIEF)]);
    let mut request = Request::resumed(SESSION);
    request.agent = Some(Agent {
        model: Some(model),
        effort: Some("high"),
        thinking: None,
        designer: false,
    });
    let window = window(&home.adapter(), &request);
    let mut all = vec![user_line(SESSION, 1, BRIEF)];
    all.extend_from_slice(lines);
    home.transcript(SESSION, &all);
    home.status(SESSION, &json!({ "status": "idle" }), me());
    (home, window)
}

fn look(window: &Rc<dyn Window>) -> cf_harness::contract::Observed {
    finished(window.observe()).unwrap()
}

#[test]
fn a_window_whose_transcript_holds_claude_codes_fallback_is_read_as_a_moved_model_by_its_look() {
    let (_home, window) = window_on(&[
        said_line(2, "claude-fable-5"),
        fell_back_line(3, "claude-fable-5", "claude-opus-4-8"),
    ]);
    let observed = look(&window);
    let Some(Quota::Moved { because, .. }) = observed.quota.as_deref() else {
        panic!("not held for the move: {:?}", observed.quota);
    };
    assert!(
        because.contains("claude-fable-5") && because.contains("claude-opus-4-8"),
        "{because}"
    );
    assert_eq!(observed.model_drift(), None, "told by the hold, not twice");
}

#[test]
fn a_model_that_only_drifts_holds_no_window_and_the_look_says_the_drift() {
    let (_home, window) = window_on(&[
        said_line(2, "claude-opus-5"),
        said_line(3, "claude-opus-5-5"),
    ]);
    let observed = look(&window);
    assert_eq!(
        observed.quota, None,
        "no word of Claude Code's own: no hold"
    );
    let drift = observed.model_drift().expect("a drift to tell");
    assert_eq!(
        (drift.from.as_str(), drift.to.as_str()),
        ("claude-opus-5", "claude-opus-5-5")
    );
}

#[test]
fn an_opusplan_window_flipping_between_opus_and_sonnet_is_told_no_drift_and_another_launch_is() {
    // The launch names the alias, and the look carries it to the reader: plan mode and its
    // flip back are what the human chose (T-841).
    let flips = [
        said_line(2, "claude-opus-5-5"),
        said_line(3, "claude-sonnet-5-5"),
        said_line(4, "claude-opus-5-5"),
    ];
    let (_home, window) = window_launched_on("opusplan", &flips);
    let observed = look(&window);
    assert_eq!(observed.model_drift(), None);
    assert_eq!(observed.quota, None);
    let (_home, window) = window_launched_on("claude-opus-5-5", &flips);
    assert!(look(&window).model_drift().is_some());
}

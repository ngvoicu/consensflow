//! A window in full permission, whatever Devin brought it back in: a resumed
//! window's first message goes behind `/bypass`, and a window whose wire says it is
//! in the normal mode is given `/bypass` once it has drawn and is not ready until
//! the wire says it changed, or is unfit for work once it has had its time (the
//! field report B1 of 2026-10-15, and `cargo xtask live devin-resume`).

use std::cell::Cell;
use std::path::PathBuf;

use serde_json::{json, Value};

use super::*;
use crate::testing::AnsweringHost;

/// The configuration of `session` the window shows, its mode `mode`, as Devin
/// 3000.11.92 writes it at load.
fn configured(session: &str, mode: &str) -> String {
    let record = json!({ "sessionId": session, "update": {
        "sessionUpdate": "config_option_update",
        "configOptions": [
            { "id": "mode", "currentValue": mode },
            { "id": "model", "currentValue": "swe-2-high" },
        ],
    } });
    format!("{record}\n")
}

/// A change of `session`'s mode to `mode`.
fn changed(session: &str, mode: &str) -> String {
    let record = json!({ "sessionId": session,
        "update": { "sessionUpdate": "current_mode_update", "currentModeId": mode } });
    format!("{record}\n")
}

/// A pane host whose window has held still for `quiet` milliseconds, and takes every
/// paste unless `refuse` says it writes nothing.
fn host(
    quiet: Rc<Cell<f64>>,
    refuse: Rc<Cell<bool>>,
) -> AnsweringHost<impl Fn(&str) -> Result<Value, crate::contract::HostError>> {
    AnsweringHost::new(move |op| {
        Ok(match op {
            "pane.snapshot" => json!({ "ok": true, "outputQuietMs": quiet.get() }),
            "pane.write_paste" if refuse.get() => json!({ "ok": false, "admitted": false }),
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

/// What the window is held for while it is switched.
fn switching() -> Readiness {
    Readiness::Held(Held::Because(SWITCHING.to_owned()))
}

#[test]
fn a_resumed_windows_first_message_goes_behind_bypass_and_a_new_windows_does_not() {
    let note = "[ConsensFlow m-2 · T-1 · note from ConsensFlow]\nConsensFlow quit while you were working on T-1. Go on where you stopped.";
    assert_eq!(
        first_message(Some(note), true).as_deref(),
        Some(format!("/bypass {note}").as_str())
    );
    assert_eq!(first_message(Some(note), false).as_deref(), Some(note));
    // As a window can take the text, behind the command as before it.
    let raw = "a\u{7}b";
    assert_eq!(
        first_message(Some(raw), true),
        Some(format!("/bypass {}", window_text(raw)))
    );
    // No message, or none a window can take, is none: no prompt file.
    assert_eq!(first_message(None, true), None);
    assert_eq!(first_message(Some(""), true), None);
}

#[test]
fn a_window_in_the_normal_mode_is_given_bypass_once_drawn_and_is_ready_when_the_wire_says_it_changed(
) {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, configured("mild-coin", "normal")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window(&fakes, &wire, Some("mild-coin"));
    let (quiet, refuse) = (Rc::new(Cell::new(200.0)), Rc::new(Cell::new(false)));
    let host = host(Rc::clone(&quiet), refuse);
    // Its screen is still drawing: it waits, and nothing is pasted yet.
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    assert!(pastes(&host.asked).is_empty());
    // Drawn: /bypass goes in, once, and the window waits for the wire.
    quiet.set(2_000.0);
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    assert_eq!(pastes(&host.asked), ["/bypass"]);
    fakes.time.settle_at(EPOCH_MS + 5_000);
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    assert_eq!(pastes(&host.asked), ["/bypass"], "not again so soon");
    // The wire says the mode changed: ready, and nothing more is pasted.
    append(&wire, &changed("mild-coin", "bypass"));
    assert_eq!(finished(window.ready(&host, &pane())), Ok(Readiness::Ready));
    assert_eq!(pastes(&host.asked), ["/bypass"]);
}

/// The Enters the host was asked to press.
fn enters(asked: &RefCell<Vec<(String, Value)>>) -> usize {
    asked
        .borrow()
        .iter()
        .filter(|(op, body)| op == "pane.input" && body["bytes"] == json!([13]))
        .count()
}

#[test]
fn while_the_wire_says_the_normal_mode_enter_goes_in_again_before_bypass_is_pasted_again_and_a_paste_written_nowhere_is_no_paste(
) {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, configured("mild-coin", "normal")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window(&fakes, &wire, Some("mild-coin"));
    let (quiet, refuse) = (Rc::new(Cell::new(2_000.0)), Rc::new(Cell::new(true)));
    let host = host(quiet, Rc::clone(&refuse));
    // The host wrote nothing: the next look tries again at once.
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    refuse.set(false);
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    assert_eq!(pastes(&host.asked), ["/bypass", "/bypass"]);
    // Taken, and the wire says nothing new for the while a paste is given.
    fakes.time.settle_at(EPOCH_MS + BYPASS_AGAIN_MS - 1);
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    assert_eq!((pastes(&host.asked).len(), enters(&host.asked)), (2, 0));
    // After the while, one more Enter, not a paste: an Enter that came before the window
    // had read the paste left the command in its input, where another would make
    // `/bypass/bypass` of it (Astra and Sol on the B1 fix).
    fakes.time.settle_at(EPOCH_MS + BYPASS_AGAIN_MS);
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    assert_eq!((pastes(&host.asked).len(), enters(&host.asked)), (2, 1));
    fakes.time.settle_at(EPOCH_MS + 2 * BYPASS_AGAIN_MS - 1);
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    assert_eq!((pastes(&host.asked).len(), enters(&host.asked)), (2, 1));
    // That changed nothing either: the paste went nowhere, and goes in once more.
    fakes.time.settle_at(EPOCH_MS + 2 * BYPASS_AGAIN_MS);
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    assert_eq!((pastes(&host.asked).len(), enters(&host.asked)), (3, 1));
    // And the paste is given its Enter again after the while, as the first was.
    fakes.time.settle_at(EPOCH_MS + 3 * BYPASS_AGAIN_MS - 1);
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    assert_eq!((pastes(&host.asked).len(), enters(&host.asked)), (3, 1));
}

#[test]
fn an_enter_the_host_did_not_take_is_pressed_again_at_the_next_look() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, configured("mild-coin", "normal")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window(&fakes, &wire, Some("mild-coin"));
    let taken = Rc::new(Cell::new(false));
    let keys = Rc::clone(&taken);
    let host = AnsweringHost::new(move |op| {
        Ok(match op {
            "pane.snapshot" => json!({ "ok": true, "outputQuietMs": 2_000 }),
            "pane.input" if !keys.get() => json!({ "ok": false, "error": "stale" }),
            _ => json!({ "ok": true }),
        })
    });
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    fakes.time.settle_at(EPOCH_MS + BYPASS_AGAIN_MS);
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    assert_eq!((pastes(&host.asked).len(), enters(&host.asked)), (1, 1));
    // Not taken: the next look presses it, and still pastes nothing.
    taken.set(true);
    fakes.time.settle_at(EPOCH_MS + BYPASS_AGAIN_MS + 1);
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    assert_eq!((pastes(&host.asked).len(), enters(&host.asked)), (1, 2));
    fakes.time.settle_at(EPOCH_MS + BYPASS_AGAIN_MS + 2);
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    assert_eq!(
        (pastes(&host.asked).len(), enters(&host.asked)),
        (1, 2),
        "taken: the while runs from it"
    );
}

/// A host whose window has drawn, and whose look at it takes as long as the wire
/// `wire` takes to gain `meanwhile`: what the human did while the look was on its way.
fn host_while(
    wire: PathBuf,
    meanwhile: String,
) -> AnsweringHost<impl Fn(&str) -> Result<Value, crate::contract::HostError>> {
    AnsweringHost::new(move |op| {
        if op == "pane.snapshot" {
            append(&wire, &meanwhile);
            return Ok(json!({ "ok": true, "outputQuietMs": 2_000 }));
        }
        Ok(json!({ "ok": true }))
    })
}

#[test]
fn what_the_human_did_while_the_screen_was_looked_at_is_what_the_window_is_and_nothing_goes_in_for_what_it_was(
) {
    // The human chose a mode while the host was asked for the screen: ready as it is,
    // and no /bypass over their choice (Astra and Sol on the B1 fix).
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, configured("mild-coin", "normal")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window(&fakes, &wire, Some("mild-coin"));
    let host = host_while(wire.clone(), changed("mild-coin", "accept-edits"));
    assert_eq!(finished(window.ready(&host, &pane())), Ok(Readiness::Ready));
    assert!(pastes(&host.asked).is_empty());

    // They began a turn: it is waited for, and nothing goes into it.
    let wire = dir.path().join("turn.jsonl");
    fs::write(&wire, configured("mild-coin", "normal")).unwrap();
    let window = super::window(&fakes, &wire, Some("mild-coin"));
    let host = host_while(wire.clone(), asks("mild-coin", "look at this first"));
    assert_eq!(
        finished(window.ready(&host, &pane())),
        Ok(Readiness::Held(Held::Unsaid))
    );
    assert_eq!((pastes(&host.asked).len(), enters(&host.asked)), (0, 0));

    // They opened another conversation: the window shows another.
    let wire = dir.path().join("another.jsonl");
    fs::write(&wire, configured("mild-coin", "normal")).unwrap();
    let window = super::window(&fakes, &wire, Some("mild-coin"));
    let host = host_while(wire.clone(), configured("fresh-leaf", "normal"));
    assert_eq!(
        finished(window.ready(&host, &pane())),
        Ok(Readiness::Held(Held::ShowsAnother))
    );
    assert!(pastes(&host.asked).is_empty());
}

#[test]
fn a_turn_begun_after_the_engines_look_found_the_window_at_rest_holds_it_though_its_prompt_was_there_at_the_first_read(
) {
    // The human sent a message after the engine's look and before readiness read the wire:
    // both reads held its prompt, and /bypass went into the turn (Astra on the B1 fix, the
    // second look).
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, configured("mild-coin", "normal")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window(&fakes, &wire, Some("mild-coin"));
    let host = host(Rc::new(Cell::new(2_000.0)), Rc::new(Cell::new(false)));
    finished(window.observe()).unwrap();
    append(&wire, &asks("mild-coin", "look at this first"));
    assert_eq!(
        finished(window.ready(&host, &pane())),
        Ok(Readiness::Held(Held::Unsaid))
    );
    assert_eq!((pastes(&host.asked).len(), enters(&host.asked)), (0, 0));
    // The engine's next look has seen it: readiness goes on as before.
    finished(window.observe()).unwrap();
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    assert_eq!(pastes(&host.asked), ["/bypass"]);
}

#[test]
fn a_conversation_the_window_is_followed_to_has_its_own_time_to_leave_the_normal_mode() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, configured("mild-coin", "normal")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window(&fakes, &wire, Some("mild-coin"));
    let host = host(Rc::new(Cell::new(2_000.0)), Rc::new(Cell::new(false)));
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    // Near the end of its time the human opens another conversation, in the normal mode
    // too, and the window is followed to it (Astra and Sol on the B1 fix).
    let late = EPOCH_MS + BYPASS_WITHIN_MS - 1_000;
    fakes.time.settle_at(late);
    append(&wire, &configured("fresh-leaf", "normal"));
    window.follow("fresh-leaf");
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    assert_eq!(
        pastes(&host.asked),
        ["/bypass", "/bypass"],
        "the new one is given its own paste at once"
    );
    fakes.time.settle_at(late + BYPASS_WITHIN_MS - 1);
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    fakes.time.settle_at(late + BYPASS_WITHIN_MS);
    assert!(matches!(
        finished(window.ready(&host, &pane())),
        Ok(Readiness::Unfit(_))
    ));
    // Followed to the one it is on already, it keeps its time.
    let fakes = Fakes::new(&Env::default());
    let window = super::window(&fakes, &wire, Some("fresh-leaf"));
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    fakes.time.settle_at(EPOCH_MS + BYPASS_WITHIN_MS);
    window.follow("fresh-leaf");
    assert!(matches!(
        finished(window.ready(&host, &pane())),
        Ok(Readiness::Unfit(_))
    ));
}

#[test]
fn a_window_still_in_the_normal_mode_when_its_time_is_up_is_unfit_for_work_and_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, configured("mild-coin", "normal")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window(&fakes, &wire, Some("mild-coin"));
    let host = host(Rc::new(Cell::new(2_000.0)), Rc::new(Cell::new(false)));
    // Its time runs from the first look that found it in that mode.
    fakes.time.settle_at(EPOCH_MS + 1_000);
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    fakes
        .time
        .settle_at(EPOCH_MS + 1_000 + BYPASS_WITHIN_MS - 1);
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    fakes.time.settle_at(EPOCH_MS + 1_000 + BYPASS_WITHIN_MS);
    assert_eq!(
        finished(window.ready(&host, &pane())),
        Ok(Readiness::Unfit(
            "its Devin window stayed in the normal permission mode, which /bypass did not change within 30 s (every tool call of its would wait for a permission nobody gives)"
                .to_owned()
        ))
    );
    assert_eq!(BYPASS_WITHIN_MS, 30_000);
}

#[test]
fn a_window_that_never_draws_is_asked_as_it_draws_and_its_time_runs_all_the_same() {
    // The engine asks a window before it has drawn only where the window says so: a
    // Devin that never drew was never asked, and its time never ran (Astra on the B1 fix).
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, configured("mild-coin", "normal")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window(&fakes, &wire, Some("mild-coin"));
    assert!(window.ready_while_drawing());
    // The host says it has printed nothing.
    let host = AnsweringHost::new(|op| {
        Ok(match op {
            "pane.snapshot" => json!({ "ok": true, "outputQuietMs": null }),
            _ => json!({ "ok": true }),
        })
    });
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    fakes.time.settle_at(EPOCH_MS + BYPASS_WITHIN_MS);
    assert!(matches!(
        finished(window.ready(&host, &pane())),
        Ok(Readiness::Unfit(_))
    ));
    assert!(
        pastes(&host.asked).is_empty(),
        "nothing goes into it undrawn"
    );
}

#[test]
fn a_window_the_wire_says_no_mode_of_or_another_than_the_normal_one_is_ready_as_before() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    // A configuration that names the mode and gives no value of it says none.
    fs::write(&wire, shows("mild-coin")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window(&fakes, &wire, Some("mild-coin"));
    let host = host(Rc::new(Cell::new(2_000.0)), Rc::new(Cell::new(false)));
    assert_eq!(finished(window.ready(&host, &pane())), Ok(Readiness::Ready));
    append(&wire, &configured("mild-coin", "bypass"));
    assert_eq!(finished(window.ready(&host, &pane())), Ok(Readiness::Ready));
    assert!(pastes(&host.asked).is_empty());
}

#[test]
fn what_the_human_typed_and_has_not_sent_holds_the_window_before_the_mode_is_seen_to() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, configured("mild-coin", "normal")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window(&fakes, &wire, Some("mild-coin"));
    let host = AnsweringHost::new(|op| {
        Ok(match op {
            "pane.snapshot" => json!({ "ok": true, "outputQuietMs": 2_000, "unsent": true }),
            _ => json!({ "ok": true }),
        })
    });
    assert_eq!(
        finished(window.ready(&host, &pane())),
        Ok(Readiness::Held(Held::Unsent))
    );
    assert!(
        pastes(&host.asked).is_empty(),
        "nothing goes behind its words"
    );
}

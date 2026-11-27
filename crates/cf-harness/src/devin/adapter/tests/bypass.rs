//! A window in full permission, whatever Devin brought it back in: a resumed
//! window's first message goes behind `/bypass`, and a window whose wire says it is
//! in the normal mode is given `/bypass` once it has drawn and is not ready until
//! the wire says it changed, or is unfit for work once it has had its time (the
//! field report B1 of 2026-10-15, and `cargo xtask live devin-resume`).

use std::cell::{Cell, RefCell};
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

const RULE: &str = "────────────────────────────────────────";

/// What the host says of a Devin window whose screen has held still for `quiet` milliseconds,
/// its box holding `/bypass` where `holds` says so: the look a delivery reads (the box's words,
/// the cursor after them, and the `seen` that names both).
fn looked(quiet: f64, holds: bool) -> Value {
    let (words, plain) = if holds {
        ("❭ /bypass", 7)
    } else {
        ("❭ Ask Devin anything about your code", 0)
    };
    json!({
        "ok": true, "outputQuietMs": 0, "screenQuietMs": quiet as u64, "typed": 0,
        "outputCaughtUp": true,
        "seen": format!("{:016x}", u64::from(holds)),
        "bottom": [format!("{RULE} (normal) ─"), words, RULE, "SWE-2 Medium"],
        "cursor": { "line": 1, "col": 2, "shown": true, "plainAfter": plain },
    })
}

/// A pane host whose window's screen has held still for `quiet` milliseconds while its
/// output never does (what it prints changes nothing shown: `/bypass` waits on the screen,
/// T-1083), and takes every paste unless `refuse` says it writes nothing. A paste it takes sits
/// in the box until an Enter is taken.
fn host(
    quiet: Rc<Cell<f64>>,
    refuse: Rc<Cell<bool>>,
) -> AnsweringHost<impl Fn(&str) -> Result<Value, crate::contract::HostError>> {
    host_holding(quiet, refuse, Rc::new(Cell::new(false)))
}

/// [`host`], its box holding what `holds` says.
fn host_holding(
    quiet: Rc<Cell<f64>>,
    refuse: Rc<Cell<bool>>,
    holds: Rc<Cell<bool>>,
) -> AnsweringHost<impl Fn(&str) -> Result<Value, crate::contract::HostError>> {
    AnsweringHost::new(move |op| {
        Ok(match op {
            "pane.snapshot" => looked(quiet.get(), holds.get()),
            "pane.write_paste" if refuse.get() => json!({ "ok": false, "admitted": false }),
            "pane.write_paste" => {
                holds.set(true);
                json!({ "ok": true })
            }
            "pane.enter" => {
                holds.set(false);
                json!({ "ok": true })
            }
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
    let holds = Rc::new(Cell::new(false));
    let host = host_holding(Rc::clone(&quiet), refuse, Rc::clone(&holds));
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
    // The window took the command, which its box no longer holds.
    holds.set(false);
    assert_eq!(finished(window.ready(&host, &pane())), Ok(Readiness::Ready));
    assert_eq!(pastes(&host.asked), ["/bypass"]);
}

/// The look `ready()` took read the pane: what it said of the human's input is the
/// window's to say, for the trace of a delivery that look held (T-883).
#[test]
fn a_window_says_what_its_last_look_read_of_the_humans_input() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, configured("mild-coin", "bypass")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window(&fakes, &wire, Some("mild-coin"));
    let host = host(Rc::new(Cell::new(2_000.0)), Rc::new(Cell::new(false)));
    assert_eq!(window.human_input(), None, "no look yet");
    assert_eq!(finished(window.ready(&host, &pane())), Ok(Readiness::Ready));
    assert_eq!(
        window.human_input(),
        Some(cf_proto::panes::HumanInput {
            typed: Some(0),
            ..cf_proto::panes::HumanInput::default()
        }),
        "a snapshot that says nothing of the human but its count"
    );
}

/// The Enters the host was asked to press.
fn enters(asked: &RefCell<Vec<(String, Value)>>) -> usize {
    asked
        .borrow()
        .iter()
        .filter(|(op, _)| op == "pane.enter")
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
    let (quiet, refuse) = (Rc::new(Cell::new(3_000.0)), Rc::new(Cell::new(true)));
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
    let box_holds = Cell::new(false);
    let host = AnsweringHost::new(move |op| {
        Ok(match op {
            "pane.snapshot" => looked(3_000.0, box_holds.get()),
            "pane.write_paste" => {
                box_holds.set(true);
                json!({ "ok": true })
            }
            "pane.enter" if !keys.get() => json!({ "ok": false, "error": "stale" }),
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
            return Ok(looked(2_000.0, false));
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
    // The first paste is still in the box (no Enter was pressed for it), and nothing is pasted
    // over it: the paste of the new conversation waits for the Enter's while.
    assert_eq!(
        pastes(&host.asked),
        ["/bypass"],
        "no second paste over the box that holds the first"
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
            "pane.snapshot" => {
                let mut snapshot = looked(0.0, false);
                snapshot["screenQuietMs"] = Value::Null;
                snapshot
            }
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
            "pane.snapshot" => {
                let mut snapshot = looked(2_000.0, true);
                snapshot["unsent"] = json!(true);
                snapshot["typedQuietMs"] = json!(30_000);
                snapshot
            }
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

/// T-2198 finding 4: the first `/bypass` was pasted, its Enter was swallowed, and the person
/// cleared the command from the box: the window is still in its normal mode, its box is empty and
/// quiet, and the Enter is never to go (nothing waits in the box for it). The command is
/// pasted again at the next look, within the launch's budget, where the window would otherwise
/// be given up as unfit with its one paste spent.
#[test]
fn a_box_known_empty_that_lost_its_bypass_while_the_mode_is_normal_gets_the_command_again() {
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    fs::write(&wire, configured("mild-coin", "normal")).unwrap();
    let fakes = Fakes::new(&Env::default());
    let window = window(&fakes, &wire, Some("mild-coin"));
    let holds = Rc::new(Cell::new(false));
    let typed = Rc::new(Cell::new(0u64));
    let (keys, visible) = (Rc::clone(&typed), Rc::clone(&holds));
    let host = AnsweringHost::new(move |op| {
        Ok(match op {
            "pane.snapshot" => {
                let mut snapshot = looked(5_000.0, visible.get());
                snapshot["typed"] = json!(keys.get());
                snapshot["unsent"] = json!(false);
                if keys.get() > 0 {
                    snapshot["typedQuietMs"] = json!(9_000);
                }
                snapshot
            }
            "pane.write_paste" => {
                visible.set(true);
                json!({ "ok": true })
            }
            _ => json!({ "ok": true }),
        })
    });
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    assert_eq!(pastes(&host.asked), ["/bypass"]);
    // The person cleared the command: the box is empty, the mode is still normal.
    holds.set(false);
    typed.set(1);
    fakes.time.settle_at(EPOCH_MS + BYPASS_AGAIN_MS);
    assert_eq!(finished(window.ready(&host, &pane())), Ok(switching()));
    assert_eq!(enters(&host.asked), 0, "no Enter into an empty box");
    assert_eq!(
        pastes(&host.asked),
        ["/bypass", "/bypass"],
        "the command goes again, once"
    );
}

// ---- T-2233: Devin's recovery --------------------------------------------------------------

/// A Devin box that holds `words`, none where the box shows its hint.
fn boxed_words(words: &str) -> Vec<String> {
    let line = if words.is_empty() {
        "❭ Ask Devin anything about your code".to_owned()
    } else {
        format!("❭ {words}")
    };
    vec![
        format!("{RULE} (normal) ─"),
        line,
        RULE.to_owned(),
        "SWE-2 Medium".to_owned(),
    ]
}

/// What the host says of a still, current Devin window whose box holds `words`.
fn shown(words: &str) -> Value {
    json!({
        "ok": true, "outputCaughtUp": true, "outputQuietMs": 0, "screenQuietMs": 5_000,
        "typed": 0, "seen": format!("{:016x}", words.len() + 1),
        "bottom": boxed_words(words),
        "cursor": { "line": 1, "col": 2 + words.chars().count(), "shown": true,
            "plainAfter": if words.is_empty() { 0 } else { words.chars().count() } },
    })
}

/// `snapshot` with `keys` writes of the person's, the last a while ago, and the count above
/// zero, the screens kept as `unsent_at` says.
fn counted(mut snapshot: Value, keys: u64, unsent_at: Value) -> Value {
    snapshot["typed"] = json!(keys);
    snapshot["typedQuietMs"] = json!(5_000);
    snapshot["unsent"] = json!(true);
    snapshot["unsentAt"] = unsent_at;
    snapshot
}

/// What the stand-in host answers each operation with.
type Answers = Box<dyn Fn(&str) -> Result<Value, crate::contract::HostError>>;

/// A window in the normal mode, its host answering `says` to each look, and the clock `fakes`
/// settled `ms` after the epoch before the readiness: what it was.
struct Switching {
    _dir: tempfile::TempDir,
    fakes: Fakes,
    window: Rc<DevinWindow>,
    says: Rc<RefCell<Value>>,
    host: AnsweringHost<Answers>,
}

impl Switching {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let wire = dir.path().join("wire.jsonl");
        fs::write(&wire, configured("mild-coin", "normal")).unwrap();
        let fakes = Fakes::new(&Env::default());
        let window = window(&fakes, &wire, Some("mild-coin"));
        let says = Rc::new(RefCell::new(shown("")));
        let looks = Rc::clone(&says);
        let host: Answers = Box::new(move |op| {
            Ok(match op {
                "pane.snapshot" => looks.borrow().clone(),
                _ => json!({ "ok": true }),
            })
        });
        Self {
            _dir: dir,
            fakes,
            window,
            says,
            host: AnsweringHost::new(host),
        }
    }

    fn ready_at(&self, ms: i64, snapshot: Value) -> Readiness {
        *self.says.borrow_mut() = snapshot;
        self.fakes.time.settle_at(EPOCH_MS + ms);
        finished(self.window.ready(&self.host, &pane())).expect("a readiness")
    }

    fn done(&self) -> (usize, usize) {
        (pastes(&self.host.asked).len(), enters(&self.host.asked))
    }
}

/// T-2230 4: the command is in the box, the person types `X` and erases twice with Ctrl+W: the
/// box reads empty and the host still counts their keys. The screen at the first Ctrl+W held
/// `/bypassX`, so the box was erased: the command is pasted again, and the launch is not given
/// up.
#[test]
fn a_word_erase_of_the_command_lets_it_be_pasted_again() {
    let window = Switching::new();
    assert_eq!(window.ready_at(0, shown("")), switching());
    assert_eq!(window.done(), (1, 0));
    let erased = counted(
        shown(""),
        3,
        json!({ "pressedOver": { "bottom": boxed_words("/bypassX"), "current": true } }),
    );
    assert_eq!(window.ready_at(BYPASS_AGAIN_MS, erased), switching());
    assert_eq!(
        window.done(),
        (2, 0),
        "pasted again, and no Enter into an empty box"
    );
}

/// T-2230 8: the command sits in the box, drawn behind the page's room: the look is behind, so
/// nothing goes, however long. When the room returns the box reads `/bypass` and gets its Enter.
/// One paste.
#[test]
fn the_command_behind_the_room_is_never_pasted_again() {
    let window = Switching::new();
    assert_eq!(window.ready_at(0, shown("")), switching());
    let mut behind = shown("");
    behind["outputCaughtUp"] = json!(false);
    for ms in [10_000, 20_000, 40_000, 100_000] {
        assert!(matches!(
            window.ready_at(ms, behind.clone()),
            Readiness::Held(_)
        ));
    }
    assert_eq!(window.done(), (1, 0), "one paste, nothing more");
    assert_eq!(window.ready_at(110_000, shown("/bypass")), switching());
    assert_eq!(window.done(), (1, 1), "its Enter");
}

/// A person who types in the window does not end its launch, however long: the time stands
/// while the window is held for them, and while there is no look.
#[test]
fn the_launch_is_not_given_up_while_the_person_types() {
    let window = Switching::new();
    assert_eq!(window.ready_at(0, shown("")), switching());
    for ms in [10_000, 20_000, 30_000, 40_000] {
        let typing = counted(shown("/bypassX"), 1, json!({}));
        assert_eq!(
            window.ready_at(ms, typing),
            Readiness::Held(Held::Unsent),
            "{ms}"
        );
    }
    // Erased: they cleared the box, and nothing shows how.
    let cleared = counted(shown(""), 2, json!({}));
    assert_eq!(
        window.ready_at(50_000, cleared),
        Readiness::Held(Held::Unproven)
    );
    assert_eq!(window.ready_at(60_000, shown("")), switching());
    assert_eq!(window.done(), (2, 0), "pasted again once the count is zero");
    assert_eq!(window.ready_at(70_000, shown("")), switching());
}

/// The Enter goes only where the box's words are the command and nothing else: never over the
/// person's words.
#[test]
fn no_enter_goes_over_the_persons_words_behind_the_command() {
    let window = Switching::new();
    assert_eq!(window.ready_at(0, shown("")), switching());
    assert_eq!(
        window.ready_at(10_000, shown("/bypass X")),
        Readiness::Held(Held::Unsent)
    );
    assert_eq!(window.done(), (1, 0));
}

/// No box (a dialog): nothing is done, and the time runs.
#[test]
fn a_dialog_is_given_nothing_and_the_time_runs() {
    let window = Switching::new();
    let mut dialog = shown("");
    dialog["bottom"] = json!(["Do you want to proceed?", "❭ 1 Yes", "2 No"]);
    assert_eq!(window.ready_at(0, dialog.clone()), switching());
    assert!(matches!(
        window.ready_at(BYPASS_WITHIN_MS, dialog),
        Readiness::Unfit(_)
    ));
    assert_eq!(window.done(), (0, 0));
}

/// A swallowed Enter, and a box the person cleared: an empty box read empty at zero is pasted
/// again once the while since the last act is over, and not before.
#[test]
fn one_act_a_look_and_none_before_the_while_is_over() {
    let window = Switching::new();
    assert_eq!(window.ready_at(0, shown("")), switching());
    assert_eq!(window.ready_at(9_999, shown("/bypass")), switching());
    assert_eq!(window.done(), (1, 0));
    assert_eq!(window.ready_at(10_000, shown("/bypass")), switching());
    assert_eq!(window.done(), (1, 1));
    assert_eq!(window.ready_at(19_999, shown("")), switching());
    assert_eq!(window.done(), (1, 1));
    assert_eq!(window.ready_at(20_000, shown("")), switching());
    assert_eq!(window.done(), (2, 1));
}

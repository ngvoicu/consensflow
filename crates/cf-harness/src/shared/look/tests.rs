//! What a look is, and what it licenses (N17): each case is one of the reviews' findings put
//! to the real readers.

use serde_json::{json, Value};

use super::*;
use cf_proto::panes::Evidence;

const RULE: &str = "────────────────────────────────────────";

/// The pane host's answer for a Claude Code box with `typed` after its mark and the person's
/// keys as given: `unsent` by the host's count, `typed_ago` ms since their last key, and a
/// window that printed `printed_ago` ms ago.
fn snapshot(typed: &str, unsent: bool, typed_ago: Option<u64>) -> Value {
    let col = 2 + typed.chars().count();
    json!({
        "ok": true,
        "seen": "00000000000000aa",
        "bottom": [RULE, format!("❯ {typed}"), RULE],
        "cursor": { "line": 1, "col": col, "shown": true, "plainAfter": typed.chars().count() },
        "typed": 4,
        "unsent": unsent,
        "typedQuietMs": typed_ago,
        "outputQuietMs": typed_ago.map(|ago| ago.saturating_sub(10)),
        "screenQuietMs": 60_000,
        "outputRead": 262,
        "outputDrawn": 262,
        "outputCaughtUp": true,
        "pasteInFlight": false,
    })
}

fn look(answer: &Value) -> Look {
    Look::of(answer).expect("a look")
}

const CLEAR: &[u8] = &[3];

#[test]
fn a_snapshot_at_6_over_5_is_no_look_and_one_with_no_seen_or_not_ok_is_none_either() {
    let mut behind = snapshot("", false, None);
    behind["outputRead"] = json!(6);
    behind["outputDrawn"] = json!(5);
    assert_eq!(Look::of(&behind).unwrap_err(), NoLook::Behind);
    let mut old = snapshot("", false, None);
    old.as_object_mut().unwrap().remove("seen");
    assert_eq!(Look::of(&old).unwrap_err(), NoLook::Old);
    let mut no_lines = snapshot("", false, None);
    no_lines.as_object_mut().unwrap().remove("bottom");
    assert_eq!(Look::of(&no_lines).unwrap_err(), NoLook::Old);
    assert_eq!(
        Look::of(&json!({ "ok": false })).unwrap_err(),
        NoLook::NoAnswer
    );
    assert_eq!(Look::of(&json!({})).unwrap_err(), NoLook::NoAnswer);
}

#[test]
fn a_box_with_no_words_and_no_keys_goes_with_the_look_it_was_read_on() {
    let licence = licence_paste(
        &look(&snapshot("", false, None)),
        Harness::Claude,
        BoxState::default(),
        CLEAR,
    );
    assert_eq!(
        licence,
        PasteLicence::Go {
            at: At {
                seen: "00000000000000aa".to_owned(),
                typed: 4
            },
            read_empty: None,
            clear_first: None,
        }
    );
}

/// T-2160 2 (pacing): a box read at 6/5 showing `my unsent draft` is no look, and a later
/// counts-only read at 6/6 may not let the delivery through: the look that read the draft is
/// the one a licence is asked of, and it says wait.
#[test]
fn a_box_that_holds_the_humans_draft_waits_whatever_the_counts_say_after() {
    let held = look(&snapshot("my unsent draft", true, Some(30_000)));
    assert_eq!(
        licence_paste(&held, Harness::Claude, BoxState::default(), CLEAR),
        PasteLicence::Wait(Held::Unsent)
    );
    // The host counts no key and the box holds words all the same: still wait.
    let held = look(&snapshot("my unsent draft", false, None));
    assert_eq!(
        licence_paste(&held, Harness::Claude, BoxState::default(), CLEAR),
        PasteLicence::Wait(Held::Unsent)
    );
}

/// A screen the host kept: `lines` as drawn, current or not.
fn kept(lines: &[&str], current: bool) -> Value {
    json!({ "bottom": lines, "cursor": null, "current": current })
}

/// The box of a Claude Code window holding `typed`.
fn boxed(typed: &str) -> Vec<String> {
    vec![RULE.to_owned(), format!("❯ {typed}"), RULE.to_owned()]
}

/// A dialog of Claude Code's, as drawn in the box's place.
const DIALOG: &[&str] = &[
    "Do you want to proceed?",
    "❯ 1. Yes",
    "  2. No",
    "Esc to cancel",
];

/// A look of an empty box over the person's unsent keys, carrying the screens `unsent_at`.
fn over_count(unsent_at: Value) -> Look {
    let mut answer = snapshot("", true, Some(2_000));
    answer["unsentAt"] = unsent_at;
    look(&answer)
}

fn paste_on(look: &Look) -> PasteLicence {
    licence_paste(look, Harness::Claude, BoxState::default(), CLEAR)
}

fn goes_on(look: &Look) -> Option<Evidence> {
    match paste_on(look) {
        PasteLicence::Go { read_empty, .. } => read_empty,
        PasteLicence::Wait(held) => {
            assert_eq!(held, Held::Unproven, "an empty box over the count");
            None
        }
    }
}

/// T-2230: no length of time and no output sets the count aside: only evidence does. A box read
/// empty with none holds, with the words that say what to press.
#[test]
fn a_box_read_empty_over_the_count_with_no_evidence_holds_however_long_ago_they_typed() {
    for typed in [2_000, 5_156, 60_000, 3_600_000] {
        let mut answer = snapshot("", true, Some(typed));
        answer["outputQuietMs"] = json!(typed.saturating_sub(10));
        assert_eq!(
            paste_on(&look(&answer)),
            PasteLicence::Wait(Held::Unproven),
            "{typed}"
        );
    }
}

/// Erased: at the first key the count cannot follow after their last character, the screen was
/// current and the box held text.
#[test]
fn a_box_read_empty_after_a_key_over_a_box_that_held_text_was_erased() {
    let box_with = |typed: &str, current| {
        let lines = boxed(typed);
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        over_count(json!({ "pressedOver": kept(&lines, current) }))
    };
    assert_eq!(goes_on(&box_with("ab", true)), Some(Evidence::Erased));
    assert_eq!(goes_on(&box_with("ab", false)), None, "behind at the key");
    assert_eq!(goes_on(&box_with("", true)), None, "it held no text");
    assert_eq!(
        goes_on(&over_count(json!({ "pressedOver": kept(DIALOG, true) }))),
        None,
        "no box to hold text"
    );
}

/// The screens kept at the person's characters (`typedAt`), and what stands for the last.
fn typed_at(screens: &[(&[&str], bool)], more: bool, over: Option<(&[&str], bool)>) -> Look {
    let screens: Vec<Value> = screens
        .iter()
        .map(|(lines, current)| kept(lines, *current))
        .collect();
    let mut at = json!({ "typedAt": screens, "typedMore": more });
    if let Some((lines, current)) = over {
        at["pressedOver"] = kept(lines, current);
    }
    over_count(at)
}

/// Typed elsewhere (T-2255): every screen kept at a character shows a dialog and not the box,
/// and the last of them, or the screen kept at the first key after it, was current.
#[test]
fn keys_typed_into_a_dialog_and_nowhere_else_set_the_count_aside() {
    let here = Some(Evidence::Elsewhere);
    assert_eq!(goes_on(&typed_at(&[(DIALOG, true)], false, None)), here);
    assert_eq!(
        goes_on(&typed_at(&[(DIALOG, false), (DIALOG, true)], false, None)),
        here,
        "an earlier screen need not be current"
    );
    assert_eq!(
        goes_on(&typed_at(&[(DIALOG, false)], false, Some((DIALOG, true)))),
        here,
        "the screen at the next key stands for the last"
    );
    assert_eq!(
        goes_on(&typed_at(&[(DIALOG, true), (DIALOG, false)], false, None)),
        None,
        "behind at the last key"
    );
    assert_eq!(
        goes_on(&typed_at(&[(DIALOG, false)], false, Some((DIALOG, false)))),
        None,
        "behind at the next key too"
    );
    assert_eq!(
        goes_on(&typed_at(&[(DIALOG, true)], true, None)),
        None,
        "more keys than the host keeps screens of"
    );
    assert_eq!(goes_on(&typed_at(&[], false, Some((DIALOG, true)))), None);
    let held = boxed("");
    let held: Vec<&str> = held.iter().map(String::as_str).collect();
    assert_eq!(
        goes_on(&typed_at(&[(&held, true)], false, None)),
        None,
        "the box took it"
    );
    assert_eq!(
        goes_on(&typed_at(
            &[(DIALOG, true), (&held, true), (DIALOG, true)],
            false,
            None
        )),
        None,
        "one of the characters went into the box"
    );
    assert_eq!(
        goes_on(&typed_at(&[(DIALOG, false)], false, Some((&held, true)))),
        None,
        "the dialog had ended by the next key"
    );
    // A conversation that quotes a list is drawn over the box, which took the key.
    let mut quoted = DIALOG.to_vec();
    quoted.extend(held.iter().copied());
    assert_eq!(
        goes_on(&typed_at(&[(&quoted, true)], false, None)),
        None,
        "the box is shown"
    );
}

/// A window draws a dialog's end and the keys typed ahead of it in two frames: a paste that
/// sets the count aside waits for a screen that has not changed for a second (T-2255).
#[test]
fn a_paste_over_the_count_waits_for_a_second_of_a_still_screen() {
    let quiet = |ms: Value| {
        let mut answer = snapshot("", true, Some(2_000));
        answer["unsentAt"] = json!({ "typedAt": [kept(DIALOG, true)], "typedMore": false });
        answer["screenQuietMs"] = ms;
        paste_on(&look(&answer))
    };
    assert_eq!(quiet(json!(999)), PasteLicence::Wait(Held::Unproven));
    assert_eq!(quiet(Value::Null), PasteLicence::Wait(Held::Unproven));
    assert!(matches!(
        quiet(json!(1_000)),
        PasteLicence::Go {
            read_empty: Some(Evidence::Elsewhere),
            ..
        }
    ));
    // With nothing unsent no still screen is asked: the box is the window's own word.
    let mut answer = snapshot("", false, Some(2_000));
    answer["screenQuietMs"] = json!(0);
    assert!(matches!(
        paste_on(&look(&answer)),
        PasteLicence::Go {
            read_empty: None,
            ..
        }
    ));
}

#[test]
fn a_box_that_holds_text_or_is_not_read_holds_as_the_persons_words() {
    let lines = boxed("ab");
    let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
    let mut unread = snapshot("", true, Some(2_000));
    unread["bottom"] = json!(["⏺ 4"]);
    for mut answer in [snapshot("ab", true, Some(2_000)), unread] {
        answer["unsentAt"] = json!({ "pressedOver": kept(&lines, true) });
        assert_eq!(paste_on(&look(&answer)), PasteLicence::Wait(Held::Unsent));
    }
}

/// A look exists only where it is current: `outputCaughtUp` is `true`. Absent, no Boolean or
/// false: no look, with equal counts or none.
#[test]
fn a_snapshot_that_does_not_say_it_is_current_is_no_look() {
    for said in [
        None,
        Some(json!("true")),
        Some(json!(null)),
        Some(json!(1)),
        Some(json!(false)),
    ] {
        let mut answer = snapshot("", false, None);
        match &said {
            None => {
                answer.as_object_mut().unwrap().remove("outputCaughtUp");
            }
            Some(said) => answer["outputCaughtUp"] = said.clone(),
        }
        assert_eq!(Look::of(&answer).unwrap_err(), NoLook::Behind, "{said:?}");
    }
}

/// The dialog reader on a kept screen, in the form the host's tail gives it, over every dialog
/// recorded and fixed of Claude Code, Devin and Muse Code: it reads each, or the person's key
/// typed there holds until they press Ctrl+U; and no screen of a window at rest is read as one,
/// which would let a key typed into its box go over.
#[test]
fn the_dialog_reader_over_every_screen_as_drawn() {
    use crate::testing::corpus::{drawn_screens, Source, State};

    let mut unread = Vec::new();
    let mut read_idle = Vec::new();
    let mut dialogs = 0;
    for screen in drawn_screens() {
        let kept = Kept {
            lines: screen.rows.clone(),
            cursor: None,
            current: true,
        };
        let shown = shows_a_dialog(screen.harness, &kept);
        // The corpus cuts Devin's box without the rule over it as the host's tail does; a whole
        // screen as kept has the rule, which is drawn right over the box.
        if screen.name.contains("no rule in the tail") {
            continue;
        }
        match screen.state {
            State::Dialog => {
                dialogs += 1;
                if !shown {
                    unread.push(screen.name.clone());
                }
            }
            _ if shown && screen.source != Source::Rendered => read_idle.push(screen.name.clone()),
            _ => {}
        }
    }
    assert!(dialogs > 10, "{dialogs}");
    assert_eq!(
        read_idle,
        Vec::<String>::new(),
        "a screen at rest reads as a dialog"
    );
    assert_eq!(
        unread, UNREAD_DIALOGS,
        "the dialogs the reader does not read"
    );
}

/// The dialogs the kept screen's reader reads none of: a key typed there holds the window's
/// deliveries until the person presses Ctrl+U in it.
const UNREAD_DIALOGS: [&str; 2] = [
    // Claude Code's effort menu: a slider, with no list of choices and no cursor on one.
    "recorded: claude 08-effort-menu.txt dialog",
    "recorded: claude 08b-effort-right.txt dialog",
];

/// A key a moment ago may not be drawn yet: nothing goes in for a second, count or none.
#[test]
fn a_key_younger_than_a_second_waits_for_the_screen_to_catch_up() {
    for unsent in [true, false] {
        let young = look(&snapshot("", unsent, Some(400)));
        assert!(matches!(
            licence_paste(&young, Harness::Claude, BoxState::default(), CLEAR),
            PasteLicence::Wait(_)
        ));
    }
}

#[test]
fn a_paste_on_its_way_waits() {
    let mut on_its_way = snapshot("", false, None);
    on_its_way["pasteInFlight"] = json!(true);
    assert!(matches!(
        licence_paste(
            &look(&on_its_way),
            Harness::Claude,
            BoxState::default(),
            CLEAR
        ),
        PasteLicence::Wait(Held::Because(_))
    ));
}

/// T-2160 2 (a failed snapshot's clearing key): the key that empties a stale paste is the
/// paste's own `clearFirst`, only where the look reads the box holding something.
#[test]
fn a_stale_paste_is_cleared_first_only_from_a_box_that_holds_it() {
    let stale = BoxState { stale_paste: true };
    let holds = look(&snapshot("/compact", false, None));
    match licence_paste(&holds, Harness::Claude, stale, CLEAR) {
        PasteLicence::Go { clear_first, .. } => assert_eq!(clear_first, Some(CLEAR)),
        other => panic!("{other:?}"),
    }
    let empty = look(&snapshot("", false, None));
    match licence_paste(&empty, Harness::Claude, stale, CLEAR) {
        PasteLicence::Go { clear_first, .. } => assert_eq!(clear_first, None, "an empty box"),
        other => panic!("{other:?}"),
    }
    // A box the reader does not read holds the paste: the key would be blind.
    let mut unread = snapshot("", false, None);
    unread["bottom"] = json!(["⏺ 4"]);
    assert_eq!(
        licence_paste(&look(&unread), Harness::Claude, stale, CLEAR),
        PasteLicence::Wait(Held::Unsent)
    );
}

#[test]
fn an_enter_goes_on_a_look_of_a_still_screen_that_shows_what_it_is_for() {
    let holds = look(&snapshot("a pasted message", false, None));
    let empty = look(&snapshot("", false, None));
    let at = holds.at();
    for (needs, expected) in [
        (EnterFor::PasteNotSent, true),
        (EnterFor::PasteStillThere, true),
        (EnterFor::DevinBypass, true),
        (EnterFor::TurnNeverBegan, true),
    ] {
        assert_eq!(
            licence_enter(&holds, Harness::Claude, needs) == EnterLicence::Go(at.clone()),
            expected,
            "{needs:?}"
        );
    }
    // A box read empty gets no Enter (Claude Code's empty box may show a suggestion); the turn
    // that never began is pressed whatever the box reads, as it was.
    for needs in [
        EnterFor::PasteNotSent,
        EnterFor::PasteStillThere,
        EnterFor::DevinBypass,
    ] {
        assert!(matches!(
            licence_enter(&empty, Harness::Claude, needs),
            EnterLicence::Wait(_)
        ));
    }
    assert!(matches!(
        licence_enter(&empty, Harness::Claude, EnterFor::TurnNeverBegan),
        EnterLicence::Go(_)
    ));
}

#[test]
fn an_enter_waits_for_a_screen_that_changed_in_the_last_three_seconds_and_for_the_humans_text() {
    let mut printing = snapshot("a pasted message", false, None);
    printing["screenQuietMs"] = json!(2_999);
    assert!(matches!(
        licence_enter(&look(&printing), Harness::Claude, EnterFor::PasteNotSent),
        EnterLicence::Wait(_)
    ));
    printing["screenQuietMs"] = json!(3_000);
    assert!(matches!(
        licence_enter(&look(&printing), Harness::Claude, EnterFor::PasteNotSent),
        EnterLicence::Go(_)
    ));
    let unsent = snapshot("a pasted message", true, Some(30_000));
    assert!(matches!(
        licence_enter(&look(&unsent), Harness::Claude, EnterFor::TurnNeverBegan),
        EnterLicence::Wait(_)
    ));
    // A screen that never showed anything is no screen at rest.
    let mut never = snapshot("a pasted message", false, None);
    never["screenQuietMs"] = Value::Null;
    assert!(matches!(
        licence_enter(&look(&never), Harness::Claude, EnterFor::PasteNotSent),
        EnterLicence::Wait(_)
    ));
}

/// T-2160 2 (arrival): the tail said 5/5 and the box read at 6/5 holding the paste: there was no
/// look, so nothing is pasted again; at the next look the box holds it and an Enter is licensed.
#[test]
fn after_a_whole_wait_no_look_pastes_nothing_and_a_box_that_holds_the_paste_gets_an_enter() {
    assert_eq!(
        after_wait(Err(&NoLook::Behind), Harness::Claude),
        AfterWait::LookAgain
    );
    assert_eq!(
        after_wait(Err(&NoLook::NoAnswer), Harness::Claude),
        AfterWait::LookAgain
    );
    let holds = look(&snapshot("a pasted message", false, None));
    assert_eq!(
        after_wait(Ok(&holds), Harness::Claude),
        AfterWait::EnterAgain(holds.at())
    );
    let empty = look(&snapshot("", false, None));
    assert_eq!(
        after_wait(Ok(&empty), Harness::Claude),
        AfterWait::PasteAgain
    );
    let mut printing = snapshot("a pasted message", false, None);
    printing["screenQuietMs"] = json!(1_000);
    assert_eq!(
        after_wait(Ok(&look(&printing)), Harness::Claude),
        AfterWait::Printing
    );
}

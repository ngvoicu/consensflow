//! What a look is, and what it licenses (N17): each case is one of the reviews' findings put
//! to the real readers.

use serde_json::{json, Value};

use super::*;

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
            read_empty: false,
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

#[test]
fn a_box_read_empty_over_the_count_goes_over_it_and_says_so() {
    let empty = look(&snapshot("", true, Some(2_000)));
    match licence_paste(&empty, Harness::Claude, BoxState::default(), CLEAR) {
        PasteLicence::Go { read_empty, at, .. } => {
            assert!(read_empty);
            assert_eq!(at.typed, 4);
        }
        other => panic!("{other:?}"),
    }
}

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
fn a_window_that_answered_no_key_is_believed_only_after_the_undrawn_bound() {
    let mut quiet = snapshot("", true, Some(2_000));
    // It printed nothing since the key, so the key may have been drawn nowhere.
    quiet["outputQuietMs"] = json!(5_000);
    assert!(matches!(
        licence_paste(&look(&quiet), Harness::Claude, BoxState::default(), CLEAR),
        PasteLicence::Wait(Held::Unsent)
    ));
    quiet["typedQuietMs"] = json!(6_000);
    assert!(matches!(
        licence_paste(&look(&quiet), Harness::Claude, BoxState::default(), CLEAR),
        PasteLicence::Go { .. }
    ));
}

#[test]
fn a_paste_on_its_way_waits() {
    let mut on_its_way = snapshot("", false, None);
    on_its_way["pasteInFlight"] = json!(true);
    assert!(matches!(
        licence_paste(&look(&on_its_way), Harness::Claude, BoxState::default(), CLEAR),
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
    assert_eq!(after_wait(Ok(&empty), Harness::Claude), AfterWait::PasteAgain);
    let mut printing = snapshot("a pasted message", false, None);
    printing["screenQuietMs"] = json!(1_000);
    assert_eq!(
        after_wait(Ok(&look(&printing)), Harness::Claude),
        AfterWait::Printing
    );
}

/// T-2198 finding 3: the five-second override counts only with a look that is not behind. The
/// host says whether the window's reader has caught up (`outputCaughtUp`): one held at the
/// page's credit, or not yet back from a read, may leave the echo of the person's key unread, so
/// the box that reads empty says nothing of their unsent character.
#[test]
fn the_override_of_the_humans_unsent_count_needs_a_look_that_is_not_behind() {
    let mut old = snapshot("", true, Some(5_156));
    old["outputQuietMs"] = json!(5_158);
    let current = look(&old);
    assert!(
        matches!(
            licence_paste(&current, Harness::Claude, BoxState::default(), CLEAR),
            PasteLicence::Go { read_empty: true, .. }
        ),
        "a reader that has caught up: the box read empty, the keys long answered"
    );
    old["outputCaughtUp"] = json!(true);
    assert!(look(&old).read_empty_over_count(Harness::Claude));
    old["outputCaughtUp"] = json!(false);
    let behind = look(&old);
    assert!(!behind.read_empty_over_count(Harness::Claude));
    assert_eq!(
        licence_paste(&behind, Harness::Claude, BoxState::default(), CLEAR),
        PasteLicence::Wait(Held::Unsent),
        "the person's unsent typing holds while the reader is behind"
    );
}

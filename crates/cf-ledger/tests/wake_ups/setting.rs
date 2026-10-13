//! The setting: what it is until the human says otherwise, what it reads back
//! as, and when a save is a change.

use cf_ledger::{NewWakeUp, WakeUpSetting, WakeUpUnit, DEFAULT_WAKE_UP_TEXT};
use serde_json::json;

use crate::world::{calls, world, HOUR};

#[test]
fn a_project_has_its_wake_up_calls_off_an_hour_apart_with_the_default_words() {
    let w = world();
    let setting = w.ledger.wake_up(w.project.id).unwrap();
    assert_eq!(
        setting,
        WakeUpSetting {
            on: false,
            every_ms: HOUR,
            text: None
        }
    );
    assert_eq!(setting.words(), DEFAULT_WAKE_UP_TEXT);
    let view = setting.view();
    assert_eq!(
        (view.every, view.unit, view.text.as_str(), view.default_text),
        (1, "hours", DEFAULT_WAKE_UP_TEXT, true)
    );
    // The project the page lists says nothing of it: its answers are as they were.
    let shown = serde_json::to_value(&w.project).unwrap();
    assert_eq!(shown.get("wakeUp"), None, "{shown}");
}

#[test]
fn what_the_human_sets_reads_back_as_set_and_each_change_is_logged_with_what_it_was() {
    let mut w = world();
    let own = NewWakeUp {
        on: true,
        every: 2,
        unit: WakeUpUnit::Days,
        text: Some("Anything left? — “carry on”\nIf not, say so.".to_owned()),
    };
    w.set(&own);
    let read = w.ledger.wake_up(w.project.id).unwrap();
    assert_eq!(
        read,
        WakeUpSetting {
            on: true,
            every_ms: 2 * 24 * HOUR,
            text: own.text.clone()
        }
    );
    let view = read.view();
    assert_eq!(
        (view.every, view.unit, view.default_text),
        (2, "days", false)
    );
    let logged = w
        .ledger
        .events(w.project.id, 0, 500)
        .unwrap()
        .into_iter()
        .filter(|event| event.kind == "project.wake_up_setting")
        .map(|event| event.data)
        .collect::<Vec<_>>();
    assert_eq!(
        logged,
        [json!({
            "from": { "on": false, "everyMs": HOUR, "text": null },
            "to": { "on": true, "everyMs": 2 * 24 * HOUR, "text": own.text },
        })]
    );
}

#[test]
fn a_save_that_changes_nothing_logs_nothing_and_leaves_the_clock_where_it_was() {
    let mut w = world();
    w.turn_on(1, WakeUpUnit::Minutes);
    let before = w.kinds();
    w.advance(30_000);
    w.turn_on(1, WakeUpUnit::Minutes);
    assert_eq!(
        w.kinds(),
        before,
        "no event for a setting that is the one it was"
    );
    // So saving the dialog again at the half minute does not push the first wake-up back.
    w.advance(30_000);
    assert!(
        w.due().is_some(),
        "a minute after it was turned on, not after the last save"
    );
}

#[test]
fn the_default_words_are_kept_as_none_so_they_follow_the_default_and_a_blank_is_refused() {
    let mut w = world();
    w.set(&NewWakeUp {
        on: true,
        every: 1,
        unit: WakeUpUnit::Hours,
        text: Some(DEFAULT_WAKE_UP_TEXT.to_owned()),
    });
    assert_eq!(w.ledger.wake_up(w.project.id).unwrap().text, None);
    let refused = w
        .ledger
        .set_wake_up(
            w.project.id,
            &NewWakeUp {
                text: Some("  ".to_owned()),
                ..calls(true, 1, WakeUpUnit::Hours)
            },
        )
        .unwrap_err();
    assert_eq!(refused.code(), Some("invalid-text"));
}

#[test]
fn an_interval_under_a_minute_is_refused_before_the_project_is_looked_for_and_a_project_that_is_not_there_is_refused(
) {
    let mut w = world();
    let under = w
        .ledger
        .set_wake_up(9_999, &calls(true, 0, WakeUpUnit::Minutes))
        .unwrap_err();
    assert_eq!(
        (under.code(), under.to_string()),
        (
            Some("invalid-interval"),
            "a wake-up comes once a minute at the most, not every 0 minutes".to_owned()
        )
    );
    let gone = w
        .ledger
        .set_wake_up(9_999, &calls(true, 1, WakeUpUnit::Minutes))
        .unwrap_err();
    assert_eq!(
        (gone.code(), gone.to_string()),
        (Some("unknown-project"), "no project 9999".to_owned())
    );
    assert_eq!(
        w.ledger.wake_up(9_999).unwrap_err().code(),
        Some("unknown-project")
    );
    assert_eq!(
        w.ledger.wake_up(w.project.id).unwrap().every_ms,
        HOUR,
        "a refused setting wrote nothing"
    );
    assert!(!w.ledger.wake_up(w.project.id).unwrap().on);
}

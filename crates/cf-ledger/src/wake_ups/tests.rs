use serde_json::json;

use super::*;

fn asked(on: bool, every: i64, unit: WakeUpUnit, text: Option<&str>) -> NewWakeUp {
    NewWakeUp {
        on,
        every,
        unit,
        text: text.map(str::to_owned),
    }
}

fn refused(result: Result<impl std::fmt::Debug, LedgerError>) -> (Option<&'static str>, String) {
    let cause = result.expect_err("a refusal");
    (cause.code(), cause.to_string())
}

#[test]
fn the_default_words_are_the_ones_the_owner_decided_on() {
    assert_eq!(
        DEFAULT_WAKE_UP_TEXT,
        "Do you still have work to do? If tasks are left on the board or in your plan, carry on."
    );
    assert_eq!(WAKE_UP_FLOOR_MS, 60_000);
}

#[test]
fn an_interval_is_said_in_the_largest_unit_that_holds_it_whole() {
    let said = |every_ms: i64| {
        let view = WakeUpSetting {
            on: true,
            every_ms,
            text: None,
        }
        .view();
        (view.every_ms, view.every, view.unit)
    };
    assert_eq!(said(60_000), (60_000, 1, "minutes"));
    assert_eq!(said(5_400_000), (5_400_000, 90, "minutes"));
    assert_eq!(said(3_600_000), (3_600_000, 1, "hours"));
    assert_eq!(said(90_000_000), (90_000_000, 25, "hours"));
    assert_eq!(said(86_400_000), (86_400_000, 1, "days"));
    assert_eq!(said(172_800_000), (172_800_000, 2, "days"));
}

#[test]
fn a_setting_with_no_words_of_its_own_says_the_default_and_that_it_is() {
    let view = WakeUpSetting {
        on: false,
        every_ms: 3_600_000,
        text: None,
    }
    .view();
    assert_eq!(
        (view.text.as_str(), view.default_text),
        (DEFAULT_WAKE_UP_TEXT, true)
    );
    let own = WakeUpSetting {
        on: true,
        every_ms: 3_600_000,
        text: Some("Anything left?".to_owned()),
    };
    assert_eq!(own.words(), "Anything left?");
    let view = own.view();
    assert_eq!(
        (view.text.as_str(), view.default_text),
        ("Anything left?", false)
    );
    assert_eq!(
        serde_json::to_value(&view).expect("a view"),
        json!({
            "on": true,
            "everyMs": 3_600_000,
            "every": 1,
            "unit": "hours",
            "text": "Anything left?",
            "defaultText": false,
        })
    );
}

#[test]
fn the_page_asks_by_a_whole_number_and_a_unit_and_the_words_when_it_has_its_own() {
    let read = |body| NewWakeUp::from_json(&body).expect("a request");
    assert_eq!(
        read(json!({ "on": true, "every": 2, "unit": "hours", "text": "Go on?" })),
        asked(true, 2, WakeUpUnit::Hours, Some("Go on?"))
    );
    assert_eq!(
        read(json!({ "on": false, "every": 90.0, "unit": "minutes", "text": null })),
        asked(false, 90, WakeUpUnit::Minutes, None)
    );
    assert_eq!(
        read(json!({ "on": true, "every": 3, "unit": "days" })),
        asked(true, 3, WakeUpUnit::Days, None)
    );
}

#[test]
fn a_request_that_is_no_on_or_off_no_whole_number_or_no_unit_of_the_three_is_refused_in_words() {
    let refusal = |body| refused(NewWakeUp::from_json(&body));
    assert_eq!(
        refusal(json!({ "every": 1, "unit": "hours" })),
        (
            Some("invalid-wake-up"),
            "wake-up calls are on (true) or off (false)".to_owned()
        )
    );
    assert_eq!(
        refusal(json!({ "on": "yes", "every": 1, "unit": "hours" })).0,
        Some("invalid-wake-up")
    );
    for every in [json!(0), json!(-3), json!(1.5), json!("2"), json!(null)] {
        let said = refusal(json!({ "on": true, "every": every, "unit": "hours" }));
        assert_eq!(said.0, Some("invalid-interval"), "{every}");
        assert!(
            said.1
                .starts_with("a wake-up comes every whole number of minutes, hours or days, not "),
            "{every}: {}",
            said.1
        );
    }
    for unit in [
        json!("seconds"),
        json!("Hours"),
        json!("hour"),
        json!(60),
        json!(null),
    ] {
        let said = refusal(json!({ "on": true, "every": 5, "unit": unit }));
        assert_eq!(said.0, Some("invalid-interval"), "{unit}");
        assert!(said.1.ends_with(&format!(" {unit}")), "{unit}: {}", said.1);
    }
}

#[test]
fn words_that_are_no_text_are_refused() {
    let said = refused(NewWakeUp::from_json(
        &json!({ "on": true, "every": 1, "unit": "hours", "text": 7 }),
    ));
    assert_eq!(
        said,
        (
            Some("invalid-text"),
            "the wake-up text must be text, not 7".to_owned()
        )
    );
}

#[test]
fn an_interval_under_a_minute_or_too_long_to_count_is_refused_and_a_minute_is_the_least_there_is() {
    assert_eq!(
        asked(true, 1, WakeUpUnit::Minutes, None)
            .setting()
            .expect("a minute")
            .every_ms,
        60_000
    );
    assert_eq!(
        refused(asked(true, 0, WakeUpUnit::Minutes, None).setting()),
        (
            Some("invalid-interval"),
            "a wake-up comes once a minute at the most, not every 0 minutes".to_owned()
        )
    );
    assert_eq!(
        refused(asked(true, -5, WakeUpUnit::Hours, None).setting()).0,
        Some("invalid-interval")
    );
    assert_eq!(
        refused(asked(true, i64::MAX, WakeUpUnit::Days, None).setting()),
        (
            Some("invalid-interval"),
            "9223372036854775807 days is longer than a wake-up can wait".to_owned()
        )
    );
}

#[test]
fn units_are_a_minute_an_hour_and_a_day_of_twenty_four_hours() {
    let every_ms = |unit| {
        asked(true, 1, unit, None)
            .setting()
            .expect("a setting")
            .every_ms
    };
    assert_eq!(every_ms(WakeUpUnit::Minutes), 60_000);
    assert_eq!(every_ms(WakeUpUnit::Hours), 60 * 60_000);
    assert_eq!(every_ms(WakeUpUnit::Days), 24 * 60 * 60_000);
}

#[test]
fn words_that_say_nothing_are_refused_and_the_default_words_are_kept_as_none() {
    for blank in ["", "   ", "\n\t"] {
        let said = refused(asked(true, 1, WakeUpUnit::Hours, Some(blank)).setting());
        assert_eq!(said.0, Some("invalid-text"), "{blank:?}");
    }
    let too_long = "x".repeat(MAX_BODY + 1);
    assert_eq!(
        refused(asked(true, 1, WakeUpUnit::Hours, Some(&too_long)).setting()).0,
        Some("invalid-text")
    );
    let own = asked(true, 1, WakeUpUnit::Hours, Some("  Anything left?\n"))
        .setting()
        .expect("a setting");
    assert_eq!(
        own.text.as_deref(),
        Some("  Anything left?\n"),
        "the words are kept as they were written"
    );
    let default = asked(true, 1, WakeUpUnit::Hours, Some(DEFAULT_WAKE_UP_TEXT))
        .setting()
        .expect("a setting");
    assert_eq!(default.text, None, "the default's words follow the default");
}

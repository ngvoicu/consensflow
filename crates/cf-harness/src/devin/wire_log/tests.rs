//! Devin's wire log as the adapter follows it, as Node's Devin adapter suite
//! held it: what a look reads on from where the last one stopped, which
//! conversation the window shows, and Devin's word on its quota.

use std::fs;

use cf_base::time::iso;
use serde_json::json;
use tempfile::TempDir;

use super::*;
use crate::testing::EPOCH_MS;

const MINUTE: i64 = 60_000;

/// A wire log of its own, and a look at it.
struct Log {
    _dir: TempDir,
    wire: WireLog,
}

impl Log {
    fn new() -> Self {
        Self::in_zone(TimeZone::UTC)
    }

    /// A log on a machine in `zone`.
    fn in_zone(zone: TimeZone) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let wire = WireLog::new(&dir.path().join("wire.jsonl").to_string_lossy(), zone);
        Self { _dir: dir, wire }
    }

    fn write(&self, text: &str) {
        fs::write(self.wire.path(), text).unwrap();
    }

    fn append(&self, text: &str) {
        let mut log = fs::read(self.wire.path()).unwrap_or_default();
        log.extend_from_slice(text.as_bytes());
        fs::write(self.wire.path(), log).unwrap();
    }

    /// What the log says at the time every scenario starts at.
    fn said(&self) -> Said {
        self.wire.read(EPOCH_MS).unwrap()
    }

    fn shown(&self) -> Option<String> {
        self.said().shown
    }

    /// Devin's word on its quota now, as the quota it is.
    fn quota(&self) -> Option<Quota> {
        self.said().quota.as_deref().cloned()
    }
}

/// One line of the log.
fn line(event: &Value) -> String {
    format!("{event}\n")
}

/// The line the log gains when its window configures a conversation it opens.
fn shows(session: &str) -> String {
    line(&json!({
        "sessionId": session,
        "update": { "sessionUpdate": "config_option_update", "configOptions": [{ "id": "mode" }] },
    }))
}

/// A prompt's line, which clears what Devin said of its quota before.
fn prompt() -> String {
    line(
        &json!({ "jsonrpc": "2.0", "id": 2, "method": "session/prompt", "params": { "sessionId": "s" } }),
    )
}

/// A piece of the agent's message.
fn says(text: &str) -> String {
    line(&json!({
        "sessionId": "s",
        "update": { "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": text } },
    }))
}

/// A refusal at `at` that names its reset `resets_in` later, if it does.
fn exhausted_at(at: i64, resets_in: Option<i64>) -> Quota {
    Quota::Exhausted {
        at: Some(iso(at)),
        resets_at: resets_in.map(|span| iso(at + span)),
    }
}

#[test]
fn the_log_is_read_on_from_where_the_last_look_stopped() {
    let log = Log::new();
    assert_eq!(log.shown(), None, "a log that is not there says nothing");
    log.write(&shows("native-a"));
    assert_eq!(log.shown().as_deref(), Some("native-a"));
    log.append(&shows("native-b"));
    assert_eq!(log.shown().as_deref(), Some("native-b"));
    assert_eq!(
        log.shown().as_deref(),
        Some("native-b"),
        "nothing new: as it was"
    );
    assert_eq!(
        log.wire.state.borrow().offset,
        fs::metadata(log.wire.path()).unwrap().len()
    );
}

#[test]
fn a_line_still_being_written_is_carried_to_the_look_that_finds_its_end() {
    let log = Log::new();
    let whole = shows("native-a");
    log.write(&whole[..40]);
    assert_eq!(log.shown(), None);
    assert_eq!(log.wire.state.borrow().carry, whole[..40]);
    log.append(&whole[40..]);
    assert_eq!(log.shown().as_deref(), Some("native-a"));
    assert_eq!(log.wire.state.borrow().carry, "");
}

#[test]
fn a_log_that_shrank_was_replaced_and_is_read_from_its_start_with_nothing_carried() {
    let log = Log::new();
    // A line half written when it was read: carried until the rest comes.
    log.write(&format!(
        "{}{}{{\"sessionId\":\"mild",
        shows("another-one"),
        shows("mild-coin")
    ));
    assert_eq!(log.shown().as_deref(), Some("mild-coin"));
    assert_eq!(log.wire.state.borrow().carry, "{\"sessionId\":\"mild");
    log.write(&shows("another-one"));
    assert_eq!(log.shown().as_deref(), Some("another-one"));
    assert_eq!(log.wire.state.borrow().carry, "");
}

#[test]
fn a_log_emptied_keeps_its_offset_so_one_written_again_is_read_on_from_where_the_old_one_ended() {
    let log = Log::new();
    log.write(&shows("native-a"));
    assert_eq!(log.shown().as_deref(), Some("native-a"));
    let offset = log.wire.state.borrow().offset;
    log.write("");
    assert_eq!(log.shown().as_deref(), Some("native-a"));
    assert_eq!(
        log.wire.state.borrow().offset,
        offset,
        "nothing was read, so the offset stays"
    );
    // Longer than the old log by a byte: only that byte is read.
    log.write(&shows("native-bb"));
    assert_eq!(log.shown().as_deref(), Some("native-a"));
}

#[test]
fn a_log_that_cannot_be_opened_says_what_it_said_before() {
    let log = Log::new();
    log.write(&format!(
        "{}{}",
        shows("native-a"),
        says("Usage limit reached")
    ));
    let said = log.said();
    fs::remove_file(log.wire.path()).unwrap();
    let after = log.said();
    assert_eq!(after.shown.as_deref(), Some("native-a"));
    assert!(Arc::ptr_eq(&after.quota.unwrap(), &said.quota.unwrap()));
}

#[cfg(unix)]
#[test]
fn a_folder_where_the_log_should_be_fails_the_look_in_nodes_words() {
    let log = Log::new();
    fs::create_dir(log.wire.path()).unwrap();
    // A folder has a size, which is read as a log's would be.
    assert_eq!(
        log.wire.read(EPOCH_MS).err().as_deref(),
        Some("EISDIR: illegal operation on a directory, read")
    );
}

#[test]
fn lines_that_are_no_json_or_name_no_conversation_are_passed_over() {
    let log = Log::new();
    let configures = |options: Value, session: Value| {
        line(&json!({
            "sessionId": session,
            "update": { "sessionUpdate": "config_option_update", "configOptions": options },
        }))
    };
    log.write(&format!(
        "not json\n\n   \n{{\"sessionId\": \"x\"\n5\n\"text\"\n[]\ntrue\n{}{}{}{}{}",
        line(&json!({ "update": null })),
        line(&json!({ "sessionId": "x", "update": { "sessionUpdate": "config_option_update" } })),
        configures(json!([{ "id": "model" }]), json!("other")),
        configures(json!([{ "id": "mode" }]), json!(7)),
        shows("native-a"),
    ));
    assert_eq!(log.shown().as_deref(), Some("native-a"));
}

#[test]
fn a_line_this_cannot_hold_is_passed_over_as_one_that_is_no_json_is_where_node_read_it() {
    // Kept from Node on purpose: JSON nested past 127 levels, or holding a
    // number past a double's range, is more than a value here can hold.
    let named = |extra: &str| {
        format!(
            "{{\"sessionId\":\"x\",\"update\":{{\"sessionUpdate\":\"config_option_update\",\"configOptions\":[{{\"id\":\"mode\"}}]}},\"extra\":{extra}}}\n"
        )
    };
    let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
    for extra in [deep.as_str(), "1e400"] {
        let log = Log::new();
        log.write(&named(extra));
        assert_eq!(log.shown(), None, "{extra}");
    }
    let log = Log::new();
    log.write(&named("1"));
    assert_eq!(log.shown().as_deref(), Some("x"));
}

#[test]
fn the_last_conversation_a_log_names_is_the_one_it_shows_whatever_ends_its_lines() {
    let log = Log::new();
    log.write(&format!(
        "{}{}",
        shows("native-a").replace('\n', "\r\n"),
        shows("native-b").replace('\n', "\r\n")
    ));
    assert_eq!(log.shown().as_deref(), Some("native-b"));
}

#[test]
fn a_byte_order_mark_opening_the_log_leaves_its_first_line_unread() {
    let log = Log::new();
    log.write(&format!(
        "\u{feff}{}{}",
        shows("native-a"),
        shows("native-b")
    ));
    assert_eq!(log.shown().as_deref(), Some("native-b"));
    let log = Log::new();
    log.write(&format!("\u{feff}{}", shows("native-a")));
    assert_eq!(log.shown(), None);
}

#[test]
fn a_null_line_fails_the_look_after_the_lines_before_it_were_read_and_the_offset_moved_past_it() {
    let log = Log::new();
    log.write(&format!("{}null\n{}", shows("native-a"), shows("native-b")));
    assert_eq!(log.wire.read(EPOCH_MS).err().as_deref(), Some(UNREADABLE));
    // The line after it was consumed with it: the next look finds nothing new.
    assert_eq!(log.shown().as_deref(), Some("native-a"));
}

#[test]
fn a_configuration_that_is_no_list_or_holds_a_null_fails_the_look() {
    for options in [
        json!("mode"),
        json!({}),
        json!(5),
        json!([null]),
        json!([{ "id": "model" }, null]),
    ] {
        let log = Log::new();
        log.write(&line(&json!({
            "sessionId": "x",
            "update": { "sessionUpdate": "config_option_update", "configOptions": options },
        })));
        assert_eq!(
            log.wire.read(EPOCH_MS).err().as_deref(),
            Some(UNREADABLE),
            "{options}"
        );
    }
    // A list that names its mode before the null never reaches it.
    let log = Log::new();
    log.write(&line(&json!({
        "sessionId": "x",
        "update": { "sessionUpdate": "config_option_update", "configOptions": [{ "id": "mode" }, null] },
    })));
    assert_eq!(log.shown().as_deref(), Some("x"));
}

#[test]
fn a_character_cut_between_two_looks_is_read_as_the_two_replacement_characters_node_read() {
    let log = Log::new();
    let text = shows("caf\u{e9}");
    let bytes = text.as_bytes();
    let at = text.find('\u{e9}').unwrap() + 1;
    fs::write(log.wire.path(), &bytes[..at]).unwrap();
    assert_eq!(log.shown(), None);
    fs::write(log.wire.path(), bytes).unwrap();
    assert_eq!(log.shown().as_deref(), Some("caf\u{fffd}\u{fffd}"));
}

#[test]
fn a_byte_that_is_no_utf8_is_read_as_a_replacement_character() {
    let log = Log::new();
    let mut bytes = shows("a-b").into_bytes();
    let at = bytes.iter().position(|&byte| byte == b'-').unwrap();
    bytes[at] = 0xff;
    fs::write(log.wire.path(), bytes).unwrap();
    assert_eq!(log.shown().as_deref(), Some("a\u{fffd}b"));
}

#[test]
fn devins_refusal_in_its_words_is_exhausted_with_the_reset_it_names() {
    let log = Log::new();
    log.write(&format!(
        "{}{}{}",
        shows("s"),
        prompt(),
        says("Reached overall message rate limit. Your limit will reset in 35 minutes.")
    ));
    assert_eq!(log.quota(), Some(exhausted_at(EPOCH_MS, Some(35 * MINUTE))));
}

#[test]
fn every_word_of_devins_refusal_counts_in_any_case_and_no_other_does() {
    for text in [
        "Usage limit reached",
        "QUOTA EXHAUSTED",
        "Your quota has been exhausted.",
        "You hit the Rate Limit",
        "rate limit",
        "xx usage limit xx",
    ] {
        assert!(REFUSAL.is_match(text), "{text}");
    }
    for text in [
        "",
        "rate  limit",
        "ratelimit",
        "the usage was limited",
        "quota",
        "exhausted",
        "quota was exhausted",
        // JavaScript's `/i` folds ASCII alone: the long s and the Kelvin sign are none of its letters.
        "u\u{17f}age limit",
        "rate limit".replace('t', "\u{ff54}").as_str(),
    ] {
        assert!(!REFUSAL.is_match(text), "{text}");
    }
}

#[test]
fn words_that_are_not_devins_refusal_are_not_one_whatever_carries_them() {
    let log = Log::new();
    log.write(&format!(
        "{}{}{}{}{}",
        shows("s"),
        says("the usage was limited"),
        line(&json!({ "update": { "sessionUpdate": "agent_thought_chunk", "content": { "type": "text", "text": "rate limit" } } })),
        line(&json!({ "update": { "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": 7 } } })),
        line(&json!({ "update": { "sessionUpdate": "agent_message_chunk", "content": "rate limit" } })),
    ));
    assert_eq!(log.quota(), None);
}

#[test]
fn a_refusal_is_cleared_by_the_next_prompt_and_by_nothing_else() {
    let log = Log::new();
    log.write(&format!("{}{}", shows("s"), says("Usage limit reached")));
    assert!(log.quota().is_some());
    log.append(&says("On it."));
    log.append(&line(&json!({ "cause": "complete" })));
    assert!(
        log.quota().is_some(),
        "a message that is no refusal clears nothing"
    );
    log.append(&prompt());
    assert_eq!(log.quota(), None);
}

#[test]
fn a_prompt_refused_by_its_code_its_kind_or_its_words_is_exhausted() {
    for (error, resets) in [
        (json!({ "code": -32011 }), None),
        (json!({ "code": -32011.0, "message": 7 }), None),
        (
            json!({ "code": -32011, "message": "Quota exhausted." }),
            None,
        ),
        (
            json!({ "data": { "cognition.ai/errorKind": "resource_exhausted" }, "message": "resets in 1 hour" }),
            Some(60 * MINUTE),
        ),
        (
            json!({ "message": "Rate limit reached, resets in 3 days" }),
            Some(3 * 24 * 60 * MINUTE),
        ),
        (
            json!({ "code": -32011, "message": ["resets in 2 hours"] }),
            Some(120 * MINUTE),
        ),
    ] {
        let log = Log::new();
        log.write(&format!(
            "{}{}",
            shows("s"),
            line(&json!({ "jsonrpc": "2.0", "id": 2, "error": error }))
        ));
        assert_eq!(log.quota(), Some(exhausted_at(EPOCH_MS, resets)), "{error}");
    }
    for error in [
        json!({ "code": -32000, "message": "Something else" }),
        json!({ "code": "-32011", "message": "text" }),
        json!({ "data": { "cognition.ai/errorKind": "other" } }),
        json!("rate limit"),
        json!(null),
        json!({ "message": 7 }),
    ] {
        let log = Log::new();
        log.write(&format!(
            "{}{}",
            shows("s"),
            line(&json!({ "error": error }))
        ));
        assert_eq!(log.quota(), None, "{error}");
    }
}

#[test]
fn a_turn_ended_for_its_quota_is_exhausted_with_the_words_it_gives() {
    let log = Log::new();
    log.write(&format!(
        "{}{}",
        shows("s"),
        line(&json!({
            "cause": "quota_exhausted",
            "errorMessage": "Your daily usage quota has been exhausted. Resets in 4h 30m (trace ID: c27105417b16)",
            "sessionId": "s",
        }))
    ));
    assert_eq!(
        log.quota(),
        Some(exhausted_at(EPOCH_MS, Some(270 * MINUTE)))
    );
    let log = Log::new();
    log.write(&line(
        &json!({ "cause": "quota_exhausted", "errorMessage": null }),
    ));
    assert_eq!(log.quota(), Some(exhausted_at(EPOCH_MS, None)));
}

#[test]
fn an_object_that_has_a_to_string_of_its_own_cannot_be_the_words_of_a_refusal() {
    let log = Log::new();
    log.write(&line(
        &json!({ "error": { "code": -32011, "message": { "toString": 1 } } }),
    ));
    assert_eq!(
        log.wire.read(EPOCH_MS).err().as_deref(),
        Some("an object with a toString of its own cannot be made text")
    );
}

#[test]
fn a_reset_too_far_to_be_a_date_fails_the_look_in_the_words_of_the_error_javascript_threw() {
    let log = Log::new();
    log.write(&says(
        "Usage limit reached. Resets in 99999999999999999 days",
    ));
    assert_eq!(
        log.wire.read(EPOCH_MS).err().as_deref(),
        Some("Invalid time value")
    );
}

#[test]
fn a_reset_named_by_a_time_of_day_is_read_in_its_zone_and_in_none_in_the_machines() {
    let log = Log::new();
    log.write(&says(
        "Usage limit reached. Resets 7:30pm (Europe/Bucharest)",
    ));
    assert_eq!(
        log.quota(),
        Some(Quota::Exhausted {
            at: Some(iso(EPOCH_MS)),
            resets_at: Some("2026-09-19T16:30:00.000Z".to_owned()),
        })
    );
    // As Node read it, in the machine's zone: Bucharest is three hours ahead.
    let log = Log::in_zone(TimeZone::get("Europe/Bucharest").unwrap());
    log.write(&says("Usage limit reached. Resets 7:30pm"));
    assert_eq!(
        log.quota(),
        Some(Quota::Exhausted {
            at: Some(iso(EPOCH_MS)),
            resets_at: Some("2026-09-19T16:30:00.000Z".to_owned()),
        })
    );
}

#[test]
fn a_refusal_is_dated_when_the_look_found_it_and_is_the_same_object_until_the_next_one() {
    let log = Log::new();
    log.write(&shows("s"));
    assert!(log.wire.read(EPOCH_MS).unwrap().quota.is_none());
    log.append(&says("Usage limit reached. Resets in 2 hours."));
    let later = EPOCH_MS + 90_000;
    let found = log.wire.read(later).unwrap().quota.unwrap();
    assert_eq!(*found, exhausted_at(later, Some(120 * MINUTE)));
    let again = log.wire.read(later + MINUTE).unwrap().quota.unwrap();
    assert!(
        Arc::ptr_eq(&found, &again),
        "the daemon tells an old refusal from a new one by it"
    );
    log.append(&says("Usage limit reached. Resets in 2 hours."));
    let newer = log.wire.read(later + 2 * MINUTE).unwrap().quota.unwrap();
    assert!(!Arc::ptr_eq(&found, &newer));
}

#[test]
fn a_refusal_is_kept_by_a_log_that_names_no_conversation_yet() {
    let log = Log::new();
    log.write(&says("Usage limit reached"));
    let said = log.said();
    assert_eq!(said.shown, None);
    assert!(said.quota.is_some());
}

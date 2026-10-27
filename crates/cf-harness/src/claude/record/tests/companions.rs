//! The record Claude Code writes beside a user's message, to say where the images attached to
//! it came from (`turnCompanion`): the harness's own words in the conversation, and no message
//! of the user's. When a pasted line is the path of an image, Claude Code makes an attachment
//! of it, and the transcript has the message, then a record of the companion's with one
//! `[Image: source: <path>]` for each picture (Claude Code 2.1.308, `tests/live_image_paste.rs`;
//! T-215). The reader makes of it an item of the harness's own (`custom`), as it does of what a
//! hook added to a prompt, so the last user's item of a window stays the message it is
//! about (T-265, the second finding of T-215: the cancel of a task found no turn about it, and
//! pressed no Escape).

use std::sync::Arc;

use super::*;
use crate::claude::stopped::{last_user, Pressed};

/// The words of the message in these tests, as Claude Code leaves them behind an attachment.
const MESSAGE: &str = "[Image #1]Warm editorial desk scene.";
pub(super) const SOURCE: &str = "[Image: source: /pictures/desk.png]";

/// The companion of the message `parent`: one source line for the picture, flagged as
/// Claude Code flags it.
pub(super) fn companion(uuid: &str, parent: &str) -> Value {
    having(
        user(uuid, json!(parent), json!([text(json!(SOURCE))])),
        json!({ "isMeta": true, "turnCompanion": true }),
    )
}

/// The message with its companion after it, as the transcript has them.
fn attached() -> Vec<Value> {
    vec![
        user("u1", Value::Null, json!([text(json!(MESSAGE))])),
        companion("c1", "u1"),
    ]
}

#[test]
fn the_companion_of_a_message_is_an_item_of_the_harness_s_own_and_the_turn_is_settled_as_usual() {
    let mut records = attached();
    records.push(answer("a1", "c1"));
    records.push(duration("d1", "a1"));
    let items = format!(
        r#"[["u1","user","{MESSAGE}",true,0],["c1","custom","{SOURCE}",true,1],["m1","assistant","Hi",true,2]]"#
    );
    assert_eq!(once(&records), settled(&items));
}

#[test]
fn a_turn_whose_message_was_followed_by_its_companion_is_in_flight_until_it_is_answered() {
    let items = format!(r#"[["u1","user","{MESSAGE}",true,0],["c1","custom","{SOURCE}",true,1]]"#);
    assert_eq!(once(&attached()), in_flight(&items));
}

#[test]
fn a_flagged_record_that_holds_a_tool_s_result_is_the_result_all_the_same() {
    let call = assistant(
        "a1",
        "u1",
        json!("m1"),
        json!([tool_use(json!("t1"))]),
        Value::Null,
    );
    let done = assistant(
        "a2",
        "r1",
        json!("m1"),
        json!([text(json!("Done"))]),
        json!("end_turn"),
    );
    // Words of its own beside the result, so the words alone would make a companion of it.
    let result = user(
        "r1",
        json!("a1"),
        json!([
            tool_result(json!("t1"), json!("x")),
            text(json!("A note beside the result."))
        ]),
    );
    let flagged = having(
        result.clone(),
        json!({ "isMeta": true, "turnCompanion": true }),
    );
    let read = |result: Value| {
        once(&[
            hello(),
            call.clone(),
            result,
            done.clone(),
            duration("d1", "a2"),
        ])
    };
    let (as_flagged, as_plain) = (read(flagged), read(result));
    assert_eq!(as_flagged, as_plain);
    assert!(as_flagged.contains(r#""tool","x""#), "{as_flagged}");
}

#[test]
fn a_flagged_record_with_no_words_adds_no_item_as_before() {
    let picture = json!([{ "type": "image", "source": { "type": "base64", "data": "" } }]);
    let plain = user("c1", json!("u1"), picture);
    let flagged = having(plain.clone(), json!({ "turnCompanion": true }));
    let read = |record: Value| once(&[hello(), record, answer("a1", "c1"), duration("d1", "a1")]);
    assert_eq!(read(flagged), read(plain));
}

#[test]
fn only_a_record_flagged_as_the_companion_of_a_turn_is_one() {
    // `isMeta` alone is on other records of Claude Code's (the caveat of a command), and is
    // not what makes a companion.
    let meta_alone = having(
        user("c1", json!("u1"), json!([text(json!(SOURCE))])),
        json!({ "isMeta": true }),
    );
    let not_flagged = having(
        user("c1", json!("u1"), json!([text(json!(SOURCE))])),
        json!({ "turnCompanion": false }),
    );
    for record in [meta_alone, not_flagged] {
        let read = once(&[
            user("u1", Value::Null, json!([text(json!(MESSAGE))])),
            record,
        ]);
        assert!(
            read.contains(&format!(r#"["c1","user","{SOURCE}",true,1]"#)),
            "{read}"
        );
    }
}

#[test]
fn the_last_user_message_of_a_look_is_the_message_before_its_companion() {
    let mut stage = Stage::new();
    stage.write(&attached());
    let reading = stage.reader.look(&Options::default(), 0);
    assert_eq!(last_user(&reading).as_deref(), Some("u1"));
    // A press over that message is what stopped the turn, with the companion after it.
    let Reading::Known(record) = &*reading else {
        panic!("the look read nothing");
    };
    assert!(Pressed::new(0, Arc::from("u1")).stopped(record, 5_000));
}

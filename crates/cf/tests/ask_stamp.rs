//! `cf ask` says when its asking began, on the machine's own clock: the board
//! binds the question to the holding its window had then (N2).

mod common;

use cf_base::time::{Clock, SystemClock};
use cf_board::scripted::{reply, scripted};
use common::cf;
use serde_json::json;

#[test]
fn a_question_is_stamped_with_the_moment_its_command_began() {
    let api = scripted(vec![reply(
        201,
        json!({ "message": { "id": 7, "recipient": "chief" } }),
    )]);
    let env = [
        ("CONSENSFLOW_URL", api.url.as_str()),
        ("CONSENSFLOW_TOKEN", "tok"),
        ("HTTP_PROXY", "http://127.0.0.1:9"),
        ("ALL_PROXY", "http://127.0.0.1:9"),
    ];
    let before = SystemClock.now_ms();
    let output = cf(&["ask".to_owned(), "Which dialect?".to_owned()], &env, "");
    let after = SystemClock.now_ms();
    assert_eq!(output.status.code(), Some(0));
    let posted = api.received();
    let body = posted[0].json().expect("a JSON body");
    assert_eq!(body["body"], "Which dialect?");
    let at = body["askedAt"].as_i64().expect("a stamp");
    assert!(
        (before..=after).contains(&at),
        "{before} <= {at} <= {after}"
    );
}

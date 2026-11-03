//! A turn Codex ended for its usage limit (`task_complete` whose error is
//! `usage_limit_exceeded`, as Codex 0.163 names it beside the
//! `server_overloaded` of `errored-task-complete.jsonl`): a refusal for quota,
//! said by the turn itself, whatever the rate limits said last (dip-agentos,
//! 2026-10-19: two tasks held out of quota ended failed at the reset).

use super::*;
use crate::records::ModelMoved;
use crate::shared::quota::Level;

/// The rate limits as a `token_count` reports them: the primary window `used`
/// percent spent, resetting at `resets_at` (seconds), not reached.
fn limits(used: f64, resets_at: i64) -> Value {
    event(json!({
        "type": "token_count",
        "rate_limits": {
            "primary": { "used_percent": used, "window_minutes": 300, "resets_at": resets_at },
            "rate_limit_reached_type": null,
        },
    }))
}

/// A turn's end with `error` (`codex_error_info` named `info`), at `at`.
fn failed(turn: &str, info: &str, at: &str) -> Value {
    json!({
        "timestamp": at,
        "type": "event_msg",
        "payload": {
            "type": "task_complete",
            "turn_id": turn,
            "last_agent_message": null,
            "error": { "message": "You've hit your usage limit.", "codex_error_info": info },
        },
    })
}

const AT: &str = "2026-10-19T03:00:00.000Z";

/// 09:10 in Bucharest that day, in seconds.
const RESETS: i64 = 1_792_390_200;

#[test]
fn a_turn_ended_for_its_usage_limit_is_a_refusal_for_quota_at_its_time_with_the_reset_the_limits_named(
) {
    let read = read(&[
        limits(98.0, RESETS),
        started(json!("t1")),
        failed("t1", "usage_limit_exceeded", AT),
    ])
    .unwrap();
    assert!(read.failed);
    assert_eq!(
        read.quota.as_deref(),
        Some(&Quota::Exhausted {
            at: Some(AT.to_owned()),
            resets_at: Some(cf_base::time::iso(RESETS * 1000)),
        })
    );
}

#[test]
fn a_reset_the_limits_named_before_the_refusal_is_none_of_its_own() {
    let read = read(&[
        limits(98.0, 1_792_300_000),
        started(json!("t1")),
        failed("t1", "usage_limit_exceeded", AT),
    ])
    .unwrap();
    assert_eq!(
        read.quota.as_deref(),
        Some(&Quota::Exhausted {
            at: Some(AT.to_owned()),
            resets_at: None,
        })
    );
}

#[test]
fn another_error_fails_its_turn_and_says_nothing_of_quota() {
    for info in ["context_window_exceeded", "bad_request", "sandbox_error"] {
        let read = read(&[
            limits(40.0, RESETS),
            started(json!("t1")),
            failed("t1", info, AT),
        ])
        .unwrap();
        assert!(read.failed, "{info}");
        assert_eq!(
            read.quota.as_deref().map(Quota::level),
            Some(Level::Ok),
            "{info}"
        );
    }
}

/// A turn's end with `error` saying `message`, its `codex_error_info` `info`, at `at`.
fn errored(info: Value, message: &str) -> Value {
    json!({
        "timestamp": AT,
        "type": "event_msg",
        "payload": {
            "type": "task_complete",
            "turn_id": "t1",
            "last_agent_message": null,
            "error": { "message": message, "codex_error_info": info },
        },
    })
}

#[test]
fn a_turn_its_server_refused_for_a_moment_is_busy_and_no_quota() {
    // T-1112: the words and kind `errored-task-complete.jsonl` recorded (codex-cli 0.153.4),
    // a server's own error, a request Codex gave up on with a server's status, and words
    // only.
    let at_capacity = "Selected model is at capacity. Please try a different model.";
    for (info, message) in [
        (json!("server_overloaded"), at_capacity),
        (json!("internal_server_error"), "Internal server error"),
        (
            json!("policy_capacity_unavailable"),
            "Policy-restricted capacity is unavailable. Please try again later.",
        ),
        (
            json!({ "response_too_many_failed_attempts": { "http_status_code": 503 } }),
            "exceeded retry limit, last status: 503 Service Unavailable",
        ),
        (
            json!({ "http_connection_failed": { "http_status_code": 529 } }),
            "unexpected status 529",
        ),
        (json!("other"), "The server is overloaded, try again later"),
    ] {
        let read = read(&[
            limits(40.0, RESETS),
            started(json!("t1")),
            errored(info.clone(), message),
        ])
        .unwrap();
        assert!(read.failed, "{info}");
        assert_eq!(
            read.quota.as_deref(),
            Some(&Quota::Busy {
                at: Some(AT.to_owned()),
                because: message.to_owned(),
            }),
            "{info}"
        );
    }
}

#[test]
fn a_request_codex_gave_up_on_for_no_server_error_is_no_refusal() {
    let read = read(&[
        limits(40.0, RESETS),
        started(json!("t1")),
        errored(
            json!({ "response_stream_disconnected": { "http_status_code": 400 } }),
            "stream disconnected",
        ),
    ])
    .unwrap();
    assert!(read.failed);
    assert_eq!(read.quota.as_deref().map(Quota::level), Some(Level::Ok));
}

#[test]
fn a_turn_ended_for_a_login_codex_refused_is_the_account_unavailable_and_no_quota_that_resets() {
    // MUS-23: `unauthorized` (an expired or revoked login) has no reset, as a usage limit has.
    let read = read(&[
        limits(40.0, RESETS),
        started(json!("t1")),
        failed("t1", "unauthorized", AT),
    ])
    .unwrap();
    assert!(read.failed);
    assert_eq!(
        read.quota.as_deref(),
        Some(&Quota::Unavailable {
            at: Some(AT.to_owned()),
            because: "its login was refused: You've hit your usage limit.".to_owned(),
        })
    );
}

#[test]
fn the_next_turn_takes_the_refusal_back_and_the_limits_say_again() {
    let read = read(&[
        started(json!("t1")),
        failed("t1", "usage_limit_exceeded", AT),
        started(json!("t2")),
        limits(3.0, RESETS + 18_000),
    ])
    .unwrap();
    assert!(!read.failed);
    assert_eq!(read.quota.as_deref().map(Quota::level), Some(Level::Ok));
}

#[test]
fn the_refusal_is_the_same_quota_look_after_look() {
    let mut rollout = Rollout::new(Arc::from("s"));
    for (index, record) in [
        started(json!("t1")),
        failed("t1", "usage_limit_exceeded", AT),
    ]
    .into_iter()
    .enumerate()
    {
        rollout.visit(record, index).unwrap();
    }
    let (first, second) = (rollout.result().unwrap(), rollout.result().unwrap());
    assert!(Arc::ptr_eq(
        first.quota.as_ref().unwrap(),
        second.quota.as_ref().unwrap()
    ));
}

#[test]
fn a_turn_ended_for_a_rate_limit_codex_gave_up_on_is_a_refusal_for_quota_too() {
    // The chief's rule (T-95): wait on any limit, as Claude's reader does with `rate_limit`.
    let read = read(&[
        started(json!("t1")),
        failed("t1", "rate_limit_exceeded", AT),
    ])
    .unwrap();
    assert!(read.failed);
    assert_eq!(
        read.quota.as_deref(),
        Some(&Quota::Exhausted {
            at: Some(AT.to_owned()),
            resets_at: None,
        })
    );
}

#[test]
fn a_reset_the_limits_name_after_the_refusal_is_its_reset() {
    // Codex may report its rate limits after it ends the turn its limit cut.
    let later = read(&[
        started(json!("t1")),
        failed("t1", "usage_limit_exceeded", AT),
        limits(100.0, RESETS),
    ])
    .unwrap();
    assert_eq!(
        later.quota.as_deref(),
        Some(&Quota::Exhausted {
            at: Some(AT.to_owned()),
            resets_at: Some(cf_base::time::iso(RESETS * 1000)),
        })
    );
    // One that names a reset before the refusal leaves it with none.
    let earlier = read(&[
        started(json!("t1")),
        failed("t1", "usage_limit_exceeded", AT),
        limits(100.0, 1_792_300_000),
    ])
    .unwrap();
    assert_eq!(earlier.quota.as_deref().and_then(Quota::resets_at), None);
}

#[test]
fn a_limit_whose_window_is_far_from_spent_borrows_no_reset_from_it() {
    // Codex names its weekly window in every snapshot, days from its reset and
    // mostly unspent: a rate limit Codex gave up on is no wait of a week (T-96).
    for info in ["rate_limit_exceeded", "usage_limit_exceeded"] {
        let read = read(&[
            limits(30.0, RESETS),
            started(json!("t1")),
            failed("t1", info, AT),
        ])
        .unwrap();
        assert_eq!(
            read.quota.as_deref(),
            Some(&Quota::Exhausted {
                at: Some(AT.to_owned()),
                resets_at: None,
            }),
            "{info}"
        );
        let after_it = read_after_limits(info);
        assert_eq!(
            after_it.quota.as_deref().and_then(Quota::resets_at),
            None,
            "{info}"
        );
    }
}

/// A turn ended for `info`, its rate limits reported after it at 30 percent.
fn read_after_limits(info: &str) -> Record {
    read(&[
        started(json!("t1")),
        failed("t1", info, AT),
        limits(30.0, RESETS),
    ])
    .unwrap()
}

#[test]
fn a_failed_turn_keeps_the_words_of_its_error_and_the_next_turn_drops_them() {
    let mut end = failed("t1", "server_overloaded", AT);
    end["payload"]["error"]["message"] = json!("The model is busy, try another.");
    let read_once = read(&[started(json!("t1")), end.clone()]).unwrap();
    assert!(read_once.failed);
    assert_eq!(
        read_once.failure.as_deref(),
        Some("The model is busy, try another.")
    );

    // An error Codex wrote as text alone is its words too.
    end["payload"]["error"] = json!("stream disconnected before completion");
    let plain = read(&[started(json!("t1")), end.clone()]).unwrap();
    assert_eq!(
        plain.failure.as_deref(),
        Some("stream disconnected before completion")
    );

    let after = read(&[started(json!("t1")), end, started(json!("t2"))]).unwrap();
    assert!(!after.failed);
    assert_eq!(after.failure, None);
}

#[test]
fn a_model_codex_rerouted_a_turn_to_is_a_move_nobody_chose() {
    // MUS-24. Provisional: Codex 0.164 names `model_reroute` (its `EventMsg::ModelReroute`, and
    // `exec` prints "model rerouted: <from> -> <to> (<reason>)"); none was ever recorded on this
    // machine, so the fields are the protocol's: `from_model`, `to_model` and `reason`.
    let rerouted = json!({
        "timestamp": AT,
        "type": "event_msg",
        "payload": {
            "type": "model_reroute",
            "from_model": "gpt-6-astra",
            "to_model": "gpt-6-safe",
            "reason": "high_risk_cyber_activity",
        },
    });
    let read = read(&[started(json!("t1")), rerouted.clone()]).unwrap();
    assert_eq!(
        read.model_moved.as_deref(),
        Some(&ModelMoved {
            from: "gpt-6-astra".to_owned(),
            to: "gpt-6-safe".to_owned(),
            because: "Codex rerouted it: high_risk_cyber_activity".to_owned(),
            at: Some(AT.to_owned()),
        })
    );
    // One that names the same model, or none, moved nothing.
    for (from, to) in [("gpt-6-astra", "gpt-6-astra"), ("", "gpt-6-safe")] {
        let mut same = rerouted.clone();
        same["payload"]["from_model"] = json!(from);
        same["payload"]["to_model"] = json!(to);
        assert_eq!(
            read_ok(&[started(json!("t1")), same]).model_moved,
            None,
            "{from:?} -> {to:?}"
        );
    }
    assert_eq!(read_ok(&[started(json!("t1"))]).model_moved, None);
}

#[test]
fn a_turn_that_begins_after_a_reroute_ends_it_and_one_inside_the_turn_does_not() {
    // T-816 P2-1: a reroute is a turn's. Held for, it is over where the human's Try again
    // starts another turn; read for good, the window was held again at every look.
    let rerouted = |from: &str, to: &str| {
        json!({
            "timestamp": AT,
            "type": "event_msg",
            "payload": { "type": "model_reroute", "from_model": from, "to_model": to,
                         "reason": "high_risk_cyber_activity" },
        })
    };
    let inside = read_ok(&[started(json!("t1")), rerouted("gpt-6-astra", "gpt-6-safe")]);
    assert!(inside.model_moved.is_some());
    let after = read_ok(&[
        started(json!("t1")),
        rerouted("gpt-6-astra", "gpt-6-safe"),
        started(json!("t2")),
    ]);
    assert_eq!(after.model_moved, None, "the next turn began");
    // And one that is rerouted again says so again.
    let again = read_ok(&[
        started(json!("t1")),
        rerouted("gpt-6-astra", "gpt-6-safe"),
        started(json!("t2")),
        rerouted("gpt-6-astra", "gpt-6-safe"),
    ]);
    assert!(again.model_moved.is_some());
}

fn read_ok(records: &[Value]) -> Record {
    read(records).unwrap()
}

/// What a turn ended for a cybersecurity flag recorded, word for word (kluris T-158 and T-163,
/// 2026-10-27, Codex 0.164.0): its `message`, beside `codex_error_info` `cyber_policy`.
const CYBER_WORDS: &str = "This content was flagged for possible cybersecurity risk. If this seems wrong, try rephrasing your request. If you\u{2019}re doing authorized security work that requires more cyber permissive safeguards, apply for Daybreak access via https://platform.openai.com/settings/organization/status-and-access before retrying.";

#[test]
fn a_turn_codex_refused_for_a_safety_filter_names_the_kind_in_plain_words_and_keeps_its_own() {
    // T-823: the board said only "harness reported a failure", and the chief could not tell
    // a refusal for content from a crash or a conversation grown too long.
    let read = read(&[
        limits(40.0, RESETS),
        started(json!("t1")),
        errored(json!("cyber_policy"), CYBER_WORDS),
    ])
    .unwrap();
    assert!(read.failed);
    assert_eq!(read.failure.as_deref(), Some(CYBER_WORDS));
    assert_eq!(
        read.failure_kind.as_deref(),
        Some("Codex's safety filter refused the turn as a cybersecurity risk (cyber_policy): reopen it with the work rephrased, or give security work to another harness")
    );
    // No refusal for a moment, and no quota: the engine does not hold the turn to try it again.
    assert_eq!(read.quota.as_deref().map(Quota::level), Some(Level::Ok));
}

#[test]
fn every_failure_kind_codex_names_that_is_no_limit_has_a_plain_name_with_its_own_after_it() {
    // The kinds of `CodexErrorInfo` in the binary of Codex 0.166.0, less the limits
    // (`usage_limit_exceeded`, `rate_limit_exceeded`), the moments a server refused for
    // (`server_overloaded`, `internal_server_error`, `policy_capacity_unavailable`), the
    // login (`unauthorized`) and `other`: those the reader holds, or leaves to the words.
    let named = [
        "cyber_policy",
        "bio_policy",
        "misalignment_policy_violation",
        "context_window_exceeded",
        "session_budget_exceeded",
        "model_not_found",
        "invalid_prompt",
        "bad_request",
        "sandbox_error",
        "active_turn_not_steerable",
        "thread_rollback_failed",
        "http_connection_failed",
        "response_stream_connection_failed",
        "response_stream_disconnected",
        "response_too_many_failed_attempts",
    ];
    for info in named {
        // A kind is a name, or an object that holds one.
        for kind in [json!(info), json!({ info: { "http_status_code": 400 } })] {
            let read = read(&[started(json!("t1")), errored(kind.clone(), "Words.")]).unwrap();
            let said = read.failure_kind.unwrap_or_default();
            assert!(said.contains(&format!("({info})")), "{kind}: {said:?}");
        }
    }
    for info in [
        "usage_limit_exceeded",
        "rate_limit_exceeded",
        "server_overloaded",
        "other",
        "a_kind_of_a_later_codex",
    ] {
        let read = read(&[started(json!("t1")), failed("t1", info, AT)]).unwrap();
        assert_eq!(read.failure_kind, None, "{info}");
    }
    // A turn with no kind at all, or one that did not fail, names none.
    let none = read(&[started(json!("t1")), errored(Value::Null, "Words.")]).unwrap();
    assert_eq!(none.failure_kind, None);
}

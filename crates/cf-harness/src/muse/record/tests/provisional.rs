//! **Provisional (schema only, MUS-02):** the shapes no Muse turn on this Mac ever
//! wrote (its login runs no model turn), made here from what Muse 1.4.13's schema and
//! its own session reader name. Apart from the recorded cases, as the live fixtures of
//! MUS-02 will replace them.

use serde_json::json;

use super::{intent, record_of, run};
use crate::records::{Quota, Role, Settlement};

#[test]
fn tool_calls_hold_the_turn_open_until_each_is_answered_and_each_result_is_a_tools_item() {
    let mut records = vec![
        intent(1, "t1", "List the files."),
        run(2, "t1", json!({ "kind": "started" })),
        run(
            3,
            "t1",
            json!({ "kind": "assistant_tool_calls_committed", "tool_calls": [
                { "name": "shell", "args": "{\"cmd\":\"ls\"}", "call_id": "c1" },
                { "name": "read_file", "args": { "path": "a" }, "id": "c2" },
            ]}),
        ),
        run(
            4,
            "t1",
            json!({ "kind": "terminal", "terminal": "completed" }),
        ),
    ];
    let open = record_of(&records);
    assert_eq!(
        open.settlement,
        Settlement::InFlight,
        "two calls unanswered"
    );
    assert!(open.in_flight);
    records.insert(
        3,
        run(
            5,
            "t1",
            json!({ "kind": "tool_result_batch_committed", "results": [
                { "tool_call_id": "c1", "text": "a\nb" },
                { "call_id": "c2", "output": { "bytes": 1 } },
            ]}),
        ),
    );
    let answered = record_of(&records);
    assert_eq!(answered.settlement, Settlement::Settled);
    let tools: Vec<_> = answered
        .items
        .iter()
        .filter(|item| item.role == Role::Tool)
        .map(|item| (&*item.id, &*item.text))
        .collect();
    assert_eq!(tools, [("c1", "a\nb"), ("c2", "{\"bytes\":1}")]);
    // A result of one tool alone, with no list.
    let single = record_of(&[
        run(2, "t1", json!({ "kind": "started" })),
        run(
            3,
            "t1",
            json!({ "kind": "assistant_tool_calls_committed", "tool_calls": [{ "call_id": "c1" }] }),
        ),
        run(
            4,
            "t1",
            json!({ "kind": "tool_result", "call_id": "c1", "result": "done" }),
        ),
        run(
            5,
            "t1",
            json!({ "kind": "terminal", "terminal": "completed" }),
        ),
    ]);
    assert_eq!(single.settlement, Settlement::Settled);
    assert_eq!(single.items.last().map(|item| &*item.text), Some("done"));
}

#[test]
fn a_question_or_an_approval_open_is_asking_and_holds_the_turn_until_settled() {
    for (asked, settled) in [
        ("user_input_prompt_requested", "user_input_prompt_settled"),
        ("approval_requested", "approval_resolved"),
    ] {
        let mut records = vec![
            run(2, "t1", json!({ "kind": "started" })),
            run(
                3,
                "t1",
                json!({ "kind": "assistant_tool_calls_committed", "tool_calls": [] }),
            ),
            run(
                4,
                "t1",
                json!({ "kind": asked, "user_input_id": "q1", "approval_id": "q1" }),
            ),
        ];
        let waiting = record_of(&records);
        assert!(waiting.asking, "{asked}");
        assert_eq!(waiting.settlement, Settlement::InFlight, "{asked}");
        records.push(run(
            5,
            "t1",
            json!({ "kind": settled, "user_input_id": "q1", "approval_id": "q1" }),
        ));
        records.push(run(
            6,
            "t1",
            json!({ "kind": "terminal", "terminal": "completed" }),
        ));
        let answered = record_of(&records);
        assert!(!answered.asking, "{settled}");
        assert_eq!(answered.settlement, Settlement::Settled, "{settled}");
    }
}

#[test]
fn a_compactions_summary_is_the_harnesss_own_item_marked_a_compaction() {
    let record = record_of(&[
        intent(1, "t1", "Go on."),
        run(2, "t1", json!({ "kind": "started" })),
        run(
            3,
            "t1",
            json!({ "kind": "context_compaction_installed", "summary": "So far: a, b." }),
        ),
        run(4, "t1", json!({ "kind": "context_compaction_installed" })),
    ]);
    let summary = record.items.last().unwrap();
    assert_eq!(
        (summary.role, &*summary.text),
        (Role::Custom, "So far: a, b.")
    );
    assert!(summary.compaction);
    assert_eq!(
        record.items.len(),
        2,
        "a compaction with no summary says nothing"
    );
}

#[test]
fn a_task_put_in_the_background_is_pending_until_its_task_ends() {
    let task = |sequence: u64, kind: &str| {
        json!({
            "schema_version": 1, "id": format!("r{sequence}"), "sequence": sequence,
            "stream": { "kind": "session", "id": "s1" }, "payload_type": "runtime.session",
            "payload": { "kind": "task", "run_id": "t1", "task_id": "k1",
                         "event": { "kind": kind, "task_id": "k1" } },
        })
    };
    let mut records = vec![
        run(2, "t1", json!({ "kind": "started" })),
        task(3, "started"),
        run(
            4,
            "t1",
            json!({ "kind": "task_backgrounded", "task_id": "k1" }),
        ),
        run(
            5,
            "t1",
            json!({ "kind": "terminal", "terminal": "completed" }),
        ),
    ];
    let running = record_of(&records);
    assert_eq!(running.settlement, Settlement::Settled);
    assert_eq!(running.pending.jobs, 1);
    records.push(task(6, "completed"));
    assert_eq!(record_of(&records).pending.jobs, 0);
    // Muse's own tasks (each turn's reminder children) are no background work.
    let own = record_of(&[
        run(2, "t1", json!({ "kind": "started" })),
        task(3, "started"),
    ]);
    assert_eq!(own.pending.jobs, 0);
}

#[test]
fn a_delivery_that_is_not_the_users_is_the_harnesss_and_a_steer_is_the_users_intent() {
    let record = record_of(&[
        run(2, "t1", json!({ "kind": "started" })),
        run(
            3,
            "t1",
            json!({ "kind": "inbox_item_queued", "source": { "source": "user_steer" }, "body": "steer" }),
        ),
        run(
            4,
            "t1",
            json!({ "kind": "inbox_item_queued", "source": { "source": "background_terminal" }, "body": "exit 0" }),
        ),
    ]);
    let said: Vec<_> = record
        .items
        .iter()
        .map(|item| (item.role, &*item.text))
        .collect();
    assert_eq!(said, [(Role::Custom, "exit 0")]);
}

#[test]
fn a_turn_refused_for_the_accounts_limits_says_the_quota_spent_at_its_time() {
    let refused = |reason: &str| {
        record_of(&[
            run(2, "t1", json!({ "kind": "started" })),
            run(
                3,
                "t1",
                json!({ "kind": "terminal", "terminal": "failed", "reason": reason }),
            ),
        ])
    };
    for reason in [
        "API error 429: rate limited",
        "Usage limit reached. Check /usage.",
    ] {
        let quota = refused(reason).quota;
        let Some(quota) = quota.as_deref() else {
            panic!("{reason}: no quota");
        };
        assert!(
            matches!(
                quota,
                Quota::Exhausted {
                    resets_at: None,
                    ..
                }
            ),
            "{reason}: {quota:?}"
        );
        assert!(quota.at().is_some_and(|at| at.starts_with("2026-10-26T")));
    }
    for reason in ["API error 500: upstream", "tool failed"] {
        assert_eq!(refused(reason).quota, None, "{reason}");
    }
    // A later turn that got through says no refusal.
    let after = record_of(&[
        run(2, "t1", json!({ "kind": "started" })),
        run(
            3,
            "t1",
            json!({ "kind": "terminal", "terminal": "failed", "reason": "API error 429" }),
        ),
        run(4, "t2", json!({ "kind": "started" })),
        run(
            5,
            "t2",
            json!({ "kind": "terminal", "terminal": "completed" }),
        ),
    ]);
    assert_eq!(after.quota, None);
}

#[test]
fn a_402_is_no_quota_and_an_account_whose_login_is_refused_is_unavailable_too() {
    // MUS-23: Muse's billing refusal has no reset, in whatever words the status is
    // said: unavailable, and a failed turn in plain words (the shape seen live is
    // `billing-spark.jsonl`'s; these are the variants the schema allows).
    let refused = |reason: &str| {
        record_of(&[
            run(2, "t1", json!({ "kind": "started" })),
            run(
                3,
                "t1",
                json!({ "kind": "terminal", "terminal": "failed", "reason": reason }),
            ),
        ])
    };
    for (reason, plain) in [
        (
            "run ended with Failed: API error 402 [request_id=x]: Billing verification failed. (billing_error)",
            "Muse refused: the account has no allowance; set up billing at accountscenter.meta.com/muse_code",
        ),
        (
            "API error 402: payment required",
            "Muse refused: the account has no allowance; set up billing at accountscenter.meta.com/muse_code",
        ),
        (
            "No Meta credentials were found. Run `muse login`.",
            "Muse refused: its login is not valid; run `muse login`",
        ),
        (
            "API error 401 [request_id=x]: invalid api key (authentication_error)",
            "Muse refused: its login is not valid; run `muse login`",
        ),
    ] {
        let record = refused(reason);
        assert!(record.failed, "{reason}");
        assert_eq!(record.failure.as_deref(), Some(plain), "{reason}");
        let quota = record.quota;
        let Some(Quota::Unavailable { because, .. }) = quota.as_deref() else {
            panic!("{reason}: {quota:?}");
        };
        assert_eq!(because, plain, "{reason}");
    }
}

#[test]
fn a_refusal_that_names_its_limit_and_reset_is_exhausted_until_then_and_the_second_wording_is_one()
{
    // Provisional (MUS-14): the wire's `error.quota` (`QuotaDetail`: `{limitKind, resetsAtMs}`)
    // is never seen in the log of a run; where a terminal carries it, its reset is the reset.
    let refused = |event: serde_json::Value| {
        record_of(&[
            run(2, "t1", json!({ "kind": "started" })),
            run(3, "t1", event),
        ])
    };
    for carried in [
        json!({ "kind": "terminal", "terminal": "failed", "reason": "stopped",
                "quota": { "limitKind": "tierLimit", "resetsAtMs": 1_793_051_801_000_u64 } }),
        json!({ "kind": "terminal", "terminal": "failed", "reason": "stopped",
                "error": { "kind": "modelError", "quota": { "limitKind": "tierLimit", "resetsAtMs": 1_793_051_801_000_u64 } } }),
    ] {
        let quota = refused(carried).quota;
        let Some(quota) = quota.as_deref() else {
            panic!("no quota");
        };
        assert_eq!(quota.resets_at(), Some("2026-10-26T21:56:41.000Z"));
        assert!(matches!(quota, Quota::Exhausted { .. }));
    }
    // Muse's own words for a limit, both of the sentences its screen says.
    for reason in [
        "This turn stopped because your usage limit was reached.",
        "Usage limit reached",
    ] {
        let quota =
            refused(json!({ "kind": "terminal", "terminal": "failed", "reason": reason })).quota;
        assert!(
            matches!(
                quota.as_deref(),
                Some(Quota::Exhausted {
                    resets_at: None,
                    ..
                })
            ),
            "{reason}"
        );
    }
}

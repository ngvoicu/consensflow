//! Records made here of a tool's calls and results in each shape Muse's binary names
//! (the live one, `tool_result_batch_committed`, and three no live log has written),
//! and, **provisional (the binary's strings only)**, a refusal for quota (MUS-14 and
//! MUS-23, T-770's), apart from the recorded cases.

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
    // The one refusal seen (T-700): the Mac's billing, a 402.
    let billing = refused(
        "run ended with Failed: API error 402 [request_id=x]: Billing verification failed. (billing_error)",
    );
    assert!(billing.failed);
    let Some(quota) = billing.quota.as_deref() else {
        panic!("no quota");
    };
    assert!(matches!(
        quota,
        Quota::Exhausted {
            resets_at: None,
            ..
        }
    ));
    assert!(quota.at().is_some_and(|at| at.starts_with("2026-10-26T")));
    for reason in [
        "API error 429: rate limited",
        "Usage limit reached. Check /usage.",
    ] {
        assert!(refused(reason).quota.is_some(), "{reason}");
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

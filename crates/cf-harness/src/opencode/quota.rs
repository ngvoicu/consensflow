//! OpenCode waiting to retry a refused request: the one quota its window
//! reports of itself, where the store never sees a refused request. A retry is
//! a spent quota when its action names one (`free_tier_limit`), or when the
//! words say a limit and the retry is due only at a reset a minute or more
//! away. A retry due in seconds is backoff on a rate limit or an overloaded
//! provider: the window is working, and taking its task away would waste what
//! it did. Anything else is not a quota.

use std::sync::LazyLock;

use cf_base::js;
use regex::Regex;
use serde_json::Value;

use crate::records::Quota;
use crate::shared::pattern::compile;
use crate::shared::quota::date;

/// `/limit|usage|quota|too many requests/i`: OpenCode's words for a limit it
/// waits out, in a retry's message or reason.
static LIMIT: LazyLock<Regex> =
    LazyLock::new(|| compile(r"(?i-u:limit|usage|quota|too many requests)"));

/// `/free_tier_limit|quota|usage/i`: OpenCode's own name for a spent free
/// tier, in a retry's action.
static SPENT: LazyLock<Regex> = LazyLock::new(|| compile(r"(?i-u:free_tier_limit|quota|usage)"));

/// A retry sooner than this is backoff, not the limit's reset.
const BACKOFF_MS: f64 = 60_000.0;

/// The quota OpenCode's live `status` says at `now_ms` (`{type: 'retry',
/// message, action, next}` while it waits to retry, as the plugin reports it;
/// null where it says nothing). Fails where JavaScript threw: on a status
/// whose words are an object with a `toString` of its own, and on a reset no
/// date can hold.
pub(super) fn retry_quota(status: &Value, now_ms: i64) -> Result<Option<Quota>, String> {
    if status.get("type").and_then(Value::as_str) != Some("retry") {
        return Ok(None);
    }
    let next = js::to_number(status.get("next"))?;
    #[allow(clippy::cast_precision_loss)] // A time of day is within 2^53 milliseconds.
    let soon = next.is_finite() && next - (now_ms as f64) < BACKOFF_MS;
    let reason = words(status.get("action").and_then(|action| action.get("reason")))?;
    let spent = SPENT.is_match(&reason);
    let message = words(status.get("message"))?;
    let limit = LIMIT.is_match(&format!("{message} {reason}"));
    if !spent && (!limit || soon) {
        return Ok(None);
    }
    let resets_at = if next.is_finite() && !soon {
        Some(date(next)?)
    } else {
        None
    };
    Ok(Some(Quota::Exhausted {
        at: None,
        resets_at,
    }))
}

/// A field's words as a template writes them where `?? ''` stands for none.
fn words(field: Option<&Value>) -> Result<String, String> {
    match field {
        None | Some(Value::Null) => Ok(String::new()),
        some => js::string(some).map(std::borrow::Cow::into_owned),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The retry `status` says at `now_ms`, as the quota's own JSON.
    fn said(status: &Value, now_ms: i64) -> Value {
        match retry_quota(status, now_ms).unwrap() {
            Some(quota) => serde_json::to_value(quota).unwrap(),
            None => Value::Null,
        }
    }

    #[test]
    fn every_status_of_the_table_is_read_as_node_read_it_at_each_time() {
        let tables: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/goldens/records/tables.json"
        )))
        .unwrap();
        let rows = tables["quota"]["opencode"].as_array().unwrap();
        for row in rows {
            // `{undefined: true}` is no status: `undefined` or `null`.
            let status = match &row["status"] {
                status if *status == json!({ "undefined": true }) => Value::Null,
                status => status.clone(),
            };
            let now_ms = row["nowMs"].as_i64().unwrap();
            assert_eq!(
                js::stringify(&said(&status, now_ms)),
                js::stringify(&row["quota"]),
                "{status} at {now_ms}"
            );
        }
        assert_eq!(rows.len(), 24);
    }

    #[test]
    fn a_reset_a_minute_away_is_a_limit_and_one_sooner_is_backoff() {
        let status = |next: i64| json!({ "type": "retry", "message": "Rate limit", "next": next });
        assert_eq!(said(&status(59_999), 0), Value::Null);
        assert_eq!(
            said(&status(60_000), 0),
            json!({ "state": "exhausted", "at": null, "resetsAt": "1970-01-01T00:01:00.000Z" })
        );
        assert_eq!(
            said(&status(1_000_000 + 59_999), 1_000_000),
            Value::Null,
            "a minute from now, not from the epoch"
        );
    }

    #[test]
    fn the_words_of_a_limit_are_found_in_any_case_and_in_the_actions_reason_too() {
        let retry = |fields: Value| {
            let mut status = json!({ "type": "retry", "next": 3_600_000 });
            status
                .as_object_mut()
                .unwrap()
                .extend(fields.as_object().unwrap().clone());
            said(&status, 0)
        };
        let waiting =
            json!({ "state": "exhausted", "at": null, "resetsAt": "1970-01-01T01:00:00.000Z" });
        assert_eq!(retry(json!({ "message": "TOO MANY REQUESTS" })), waiting);
        assert_eq!(
            retry(json!({ "message": "x", "action": { "reason": "your usage" } })),
            waiting
        );
        assert_eq!(retry(json!({ "message": "Overloaded" })), Value::Null);
        assert_eq!(
            retry(json!({ "message": ["a limit"] })),
            waiting,
            "a list is its text"
        );
        assert_eq!(retry(json!({})), Value::Null, "no words, no limit");
    }

    #[test]
    fn a_time_that_is_no_finite_number_is_no_backoff_and_names_no_reset() {
        let waiting = |next: Value| {
            let status = json!({ "type": "retry", "message": "Rate limit", "next": next });
            said(&status, 0)
        };
        let no_reset = json!({ "state": "exhausted", "at": null, "resetsAt": null });
        // Node: Number("-Infinity") and the rest are no number to wait out.
        for next in ["-Infinity", "Infinity", "NaN", "soon"] {
            assert_eq!(waiting(json!(next)), no_reset, "{next}");
        }
        assert_eq!(waiting(json!({})), no_reset, "an object is NaN");
        // And these are numbers: 0, 0 and 1, a retry due at once, which is backoff.
        for next in [Value::Null, json!(""), json!([]), json!(true)] {
            assert_eq!(waiting(next.clone()), Value::Null, "{next}");
        }
    }

    #[test]
    fn a_status_that_is_no_retry_says_no_quota() {
        for status in [
            Value::Null,
            json!("retry"),
            json!(["retry"]),
            json!({ "type": "idle" }),
            json!({ "type": ["retry"] }),
            json!({ "message": "usage limit", "next": 9_000_000 }),
        ] {
            assert_eq!(said(&status, 0), Value::Null, "{status}");
        }
    }

    #[test]
    fn the_failures_javascript_threw_are_failures_here() {
        let retry = |fields: Value| {
            let mut status = json!({ "type": "retry", "message": "limit", "next": 3_600_000 });
            status
                .as_object_mut()
                .unwrap()
                .extend(fields.as_object().unwrap().clone());
            retry_quota(&status, 0)
        };
        assert_eq!(
            retry(json!({ "next": 1e300 })).unwrap_err(),
            "Invalid time value"
        );
        // An object with a `toString` of its own has no text, and so no number.
        for field in [
            json!({ "next": { "toString": 1 } }),
            json!({ "message": { "toString": 1 } }),
            json!({ "action": { "reason": [{ "toString": 1 }] } }),
        ] {
            assert!(retry(field.clone()).is_err(), "{field}");
        }
        // Any other object is `[object Object]`, and its number NaN.
        assert!(retry(json!({ "next": { "a": 1 }, "message": { "a": 1 } })).is_ok());
    }
}

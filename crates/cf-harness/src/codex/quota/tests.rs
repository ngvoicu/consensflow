use super::*;
use cf_base::js;
use serde_json::json;

/// What `codexQuota` answered for each case of `tables.json`, as text.
#[test]
fn every_rate_limit_of_the_table_is_read_as_node_read_it() {
    let tables: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/goldens/records/tables.json"
    )))
    .unwrap();
    let cases = tables["quota"]["codex"].as_array().unwrap();
    for case in cases {
        // `{undefined: true}` is no limits at all: `undefined` or `null`.
        let limits = match &case["limits"] {
            limits if *limits == json!({ "undefined": true }) => Value::Null,
            limits => limits.clone(),
        };
        let quota = serde_json::to_value(codex_quota(&limits).unwrap()).unwrap();
        assert_eq!(
            js::stringify(&quota),
            js::stringify(&case["quota"]),
            "{limits}"
        );
    }
    assert_eq!(cases.len(), 13);
}

#[test]
fn a_reset_is_the_instant_toward_zero_and_one_past_a_dates_range_fails() {
    let reset = |seconds: Value| {
        codex_quota(&json!({ "primary": { "used_percent": 1, "resets_at": seconds } }))
            .map(|quota| serde_json::to_value(quota).unwrap()["resetsAt"].clone())
    };
    // Node: new Date(seconds * 1000).toISOString().
    assert_eq!(
        reset(json!(1_790_423_393.000_5)).unwrap(),
        "2026-09-26T11:49:53.000Z"
    );
    assert_eq!(reset(json!(-0.000_5)).unwrap(), "1970-01-01T00:00:00.000Z");
    assert_eq!(
        reset(json!(8.64e12)).unwrap(),
        "+275760-09-13T00:00:00.000Z"
    );
    assert_eq!(
        reset(json!(-8.64e12)).unwrap(),
        "-271821-04-20T00:00:00.000Z"
    );
    assert!(reset(json!(8.640_000_000_001e12)).is_err());
    assert!(reset(json!(1e300)).is_err());
}

#[test]
fn the_fuller_window_decides_and_the_primary_wins_a_tie() {
    let quota = |limits: Value| serde_json::to_value(codex_quota(&limits).unwrap()).unwrap();
    let both = |primary: f64, secondary: f64| {
        quota(json!({
            "primary": { "used_percent": primary, "resets_at": 1 },
            "secondary": { "used_percent": secondary, "resets_at": 2 },
        }))
    };
    assert_eq!(both(10.0, 20.0)["resetsAt"], "1970-01-01T00:00:02.000Z");
    assert_eq!(both(20.0, 10.0)["resetsAt"], "1970-01-01T00:00:01.000Z");
    assert_eq!(both(20.0, 20.0)["resetsAt"], "1970-01-01T00:00:01.000Z");
    // Text where a number belongs is no window; a list of limits holds none.
    assert_eq!(
        quota(json!({ "primary": { "used_percent": "99" } }))["usedPercent"],
        Value::Null
    );
    assert_eq!(quota(json!(["x"]))["state"], "ok");
}

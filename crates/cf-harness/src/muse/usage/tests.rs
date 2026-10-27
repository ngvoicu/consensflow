use std::path::PathBuf;

use cf_base::env::Env;
use serde_json::{json, Value};

use super::*;
use crate::seams::processes::Failed;
use crate::testing::{finished, ManualTime, ScriptedProcesses};

/// A recorded answer of `muse usage --json`.
fn recorded(name: &str) -> Value {
    let text = match name {
        "allowance" => include_str!("../../../tests/fixtures/muse/usage-allowance.json"),
        "no-allowance" => include_str!("../../../tests/fixtures/muse/usage-no-allowance.json"),
        other => panic!("no fixture {other}"),
    };
    serde_json::from_str(text).unwrap()
}

/// 2026-10-26T18:10:18Z, the minute the recorded allowance was read.
const NOW: i64 = 1_793_038_218_267;

#[test]
fn a_paid_account_is_read_by_its_fullest_window_and_that_ones_reset() {
    // The recording: 3 percent of the window, 1 of the week.
    let quota = quota_of(&recorded("allowance"), NOW).unwrap().unwrap();
    assert_eq!(
        quota,
        Quota::Usage {
            level: Level::Ok,
            used_percent: Some(3.into()),
            resets_at: Some("2026-10-26T21:56:41.000Z".to_owned()),
        }
    );
    let answer = |window: u64, weekly: u64| {
        json!({ "allowance": {
            "window": { "usedPercent": window, "resetsAtMs": 1_793_051_801_000_u64 },
            "weekly": { "usedPercent": weekly, "resetsAtMs": 1_793_577_600_000_u64 },
        } })
    };
    // 95 percent is low; the week the fuller one names the week's reset.
    let low = quota_of(&answer(40, 95), NOW).unwrap().unwrap();
    assert_eq!(
        (low.level(), low.resets_at()),
        (Level::Low, Some("2026-11-02T00:00:00.000Z"))
    );
    assert_eq!(
        quota_of(&answer(94, 10), NOW).unwrap().unwrap().level(),
        Level::Ok
    );
    // A window used up is exhausted until it resets, and both used up until the later.
    let spent = quota_of(&answer(100, 20), NOW).unwrap().unwrap();
    assert_eq!(
        (spent.level(), spent.resets_at()),
        (Level::Exhausted, Some("2026-10-26T21:56:41.000Z"))
    );
    let both = quota_of(&answer(100, 130), NOW).unwrap().unwrap();
    assert_eq!(both.resets_at(), Some("2026-11-02T00:00:00.000Z"));
    // The wire's `usage/read` says the same through `usage`, and nothing observed says none.
    let wire = json!({ "usage": { "tier": "x",
        "window": { "usedPercent": 97, "windowDurationMins": 300, "resetsAtMs": 1_793_051_801_000_u64 },
        "weekly": { "usedPercent": 1, "resetsAtMs": 1_793_577_600_000_u64 } } });
    assert_eq!(quota_of(&wire, NOW).unwrap().unwrap().level(), Level::Low);
    assert_eq!(quota_of(&json!({}), NOW).unwrap(), None);
}

#[test]
fn an_account_with_no_allowance_is_unavailable_before_any_turn_is_refused() {
    // T-705's answer of 2026-10-26: no plan. The account is unavailable, no reset (MUS-23).
    let quota = quota_of(&recorded("no-allowance"), NOW).unwrap().unwrap();
    assert_eq!(
        quota,
        Quota::Unavailable {
            at: Some("2026-10-26T18:10:18.267Z".to_owned()),
            because: NO_ALLOWANCE.to_owned(),
        }
    );
    // A reason Muse gives that is not that one says nothing known: `server_sent_no_usage` is
    // what a paid account answered on 2026-10-27 minutes after it answered a count, so the
    // account is no worse for it and nothing is held (the hint it carries is Muse's own).
    for reason in ["sign_in_needed", "server_sent_no_usage"] {
        let other = json!({ "allowance": null, "reason": reason, "hint": "try later" });
        assert_eq!(quota_of(&other, NOW).unwrap(), None, "{reason}");
    }
}

#[test]
fn the_wires_refusal_names_the_reset_it_gives() {
    // The schema's `QuotaDetail`: `{limitKind: "tierLimit", resetsAtMs}`.
    let detail = json!({ "limitKind": "tierLimit", "resetsAtMs": 1_793_051_801_000_u64 });
    let at = Some("2026-10-26T18:10:18.267Z");
    assert_eq!(
        wire_quota(&detail, at).unwrap(),
        Some(Quota::Exhausted {
            at: at.map(str::to_owned),
            resets_at: Some("2026-10-26T21:56:41.000Z".to_owned()),
        })
    );
    assert_eq!(
        wire_quota(&json!({ "limitKind": "tierLimit" }), at).unwrap(),
        None
    );
}

/// `muse usage --json` as the daemon runs it, answered by `answer`.
fn read_as(answer: Result<String, Failed>) -> Result<Option<Quota>, String> {
    let processes = ScriptedProcesses::default();
    processes.run_answer("muse usage --json", answer);
    let time = ManualTime::new(NOW);
    let program = program(PathBuf::from("/home/jane/.local/bin/muse"), &Env::default());
    finished(Box::pin(read(&processes, &time, program)))
}

#[test]
fn what_muse_usage_says_is_read_as_the_account_s_quota_and_a_failure_is_a_reason() {
    // The launcher is not asked for an update check, and the program is `usage --json`.
    let program = program(PathBuf::from("/home/jane/.local/bin/muse"), &Env::default());
    assert_eq!(program.args, ["usage", "--json"]);
    assert_eq!(program.env.text("MUSE_NO_AUTO_UPDATE"), Some("1"));
    // The paid account's count and the account with no allowance, as they were recorded.
    let said = read_as(Ok(recorded("allowance").to_string()))
        .unwrap()
        .unwrap();
    assert_eq!(
        (said.level(), said.resets_at()),
        (Level::Ok, Some("2026-10-26T21:56:41.000Z"))
    );
    let none = read_as(Ok(recorded("no-allowance").to_string()))
        .unwrap()
        .unwrap();
    assert!(none.unavailable());
    assert_eq!(none.at(), Some("2026-10-26T18:10:18.267Z"));
    // A program that fails, and words that are no JSON, are a failure in the reader's words.
    let failed = Failed {
        message: "Command failed: muse usage --json".to_owned(),
        code: Some(1),
        killed: false,
        stdout: String::new(),
    };
    assert_eq!(
        read_as(Err(failed)),
        Err("Command failed: muse usage --json".to_owned())
    );
    let refused = read_as(Ok("not json at all".to_owned())).unwrap_err();
    assert!(
        refused.starts_with("muse usage did not answer JSON: "),
        "{refused}"
    );
}

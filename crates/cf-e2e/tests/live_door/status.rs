//! What Claude's status said over the whole wait of a hook, read as the daemon
//! reads it (the `sessions/<pid>.json` file that names the session): if it ever
//! read idle there, the daemon's look at rest would let an answer be pasted into
//! a window that is mid-hook, whatever `--hold` stretches the wait to.

use cf_e2e::live::claude::{runs, Sample};
use cf_e2e::live::members::{ms_of, Account};
use cf_e2e::live::seconds;
use serde_json::Value;

/// The reads of the status made from `from` to `to`, both in milliseconds since
/// the epoch.
pub fn over(samples: &[Sample], from: i64, to: i64) -> Vec<&Sample> {
    samples
        .iter()
        .filter(|sample| sample.at_ms >= from && sample.at_ms <= to)
        .collect()
}

/// How many of `reads` say the window was at rest: `idle`, or `shell` (a window
/// at a shell prompt is at rest too).
pub fn at_rest(reads: &[&Sample]) -> usize {
    reads
        .iter()
        .filter(|sample| matches!(sample.status.as_deref(), Some("idle" | "shell")))
        .count()
}

/// What the daemon itself saw of the window `handle` over the same wait, from
/// its own trace: a minute before the wait began to the end of it, each reading
/// of the window and when it was made, `idle at +3.1 s`.
pub fn the_daemons_reading(events: &[Value], handle: &str, from: i64, to: i64) -> String {
    let saw: Vec<String> = events
        .iter()
        .filter(|event| {
            event["kind"] == "window.activity"
                && event["participant"] == handle
                && ms_of(&event["at"]).is_some_and(|at| at >= from - 60_000 && at <= to)
        })
        .map(|event| {
            format!(
                "{} at {} s",
                event["state"].as_str().unwrap_or("undefined"),
                seconds(ms_of(&event["at"]).unwrap_or(from) - from)
            )
        })
        .collect();
    if saw.is_empty() {
        "none".to_owned()
    } else {
        saw.join(", ")
    }
}

/// Says what Claude's status was over the hook's wait, which is from the
/// question to the answer's receipt (or to `to_ms`, the paste that replaced it),
/// in `account`: held where the window never read at rest.
pub fn say(
    account: &mut Account,
    samples: &[Sample],
    events: &[Value],
    handle: &str,
    session: &str,
    (from, to): (i64, i64),
) {
    let wait = over(samples, from, to);
    let rest = at_rest(&wait);
    account.check(
        &format!(
            "claude status over the hook's wait ({} s, {} reads of sessions/<pid>.json, session {session})",
            seconds(to - from),
            wait.len()
        ),
        rest == 0,
        if rest == 0 {
            "never idle".to_owned()
        } else {
            format!("idle in {rest} reads")
        },
    );
    let kept: Vec<Sample> = wait.into_iter().cloned().collect();
    for line in runs(&kept, from) {
        account.say(format!("       {line}"));
    }
    account.say(format!(
        "     the daemon's reading of the window (window.activity): {}",
        the_daemons_reading(events, handle, from, to)
    ));
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn read(at_ms: i64, status: Option<&str>) -> Sample {
        Sample {
            at_ms,
            status: status.map(str::to_owned),
            waiting_for: None,
            changed_at_ms: None,
        }
    }

    #[test]
    fn the_reads_of_the_wait_are_the_ones_between_the_question_and_the_receipt() {
        let samples = [
            read(900, Some("busy")),
            read(1_000, Some("busy")),
            read(2_000, Some("waiting")),
            read(3_000, Some("busy")),
            read(3_001, Some("idle")),
        ];
        let wait = over(&samples, 1_000, 3_000);
        let at: Vec<i64> = wait.iter().map(|sample| sample.at_ms).collect();
        assert_eq!(at, [1_000, 2_000, 3_000]);
        assert_eq!(at_rest(&wait), 0);
    }

    #[test]
    fn a_window_that_read_idle_or_at_a_shell_during_the_wait_was_at_rest_for_the_daemon() {
        let samples = [
            read(1, Some("busy")),
            read(2, Some("idle")),
            read(3, Some("shell")),
            read(4, None),
            read(5, Some("waiting")),
        ];
        let all: Vec<&Sample> = samples.iter().collect();
        assert_eq!(at_rest(&all), 2);
    }

    #[test]
    fn the_hooks_wait_says_never_idle_or_how_often_the_window_read_idle() {
        let samples = [read(1_000, Some("busy")), read(1_150, Some("busy"))];
        let mut held = Account::new();
        say(
            &mut held,
            &samples,
            &[],
            "bench-claude-1",
            "s-1",
            (1_000, 1_150),
        );
        assert!(held.held());
        assert_eq!(
            held.lines()[0],
            "ok   claude status over the hook's wait (0.1 s, 2 reads of sessions/<pid>.json, session s-1): never idle"
        );
        assert_eq!(held.lines()[1], "       busy 0.0–0.1 s (2 reads)");
        assert_eq!(
            held.lines()[2],
            "     the daemon's reading of the window (window.activity): none"
        );

        let samples = [read(1_000, Some("busy")), read(1_150, Some("idle"))];
        let mut failed = Account::new();
        say(
            &mut failed,
            &samples,
            &[],
            "bench-claude-1",
            "s-1",
            (1_000, 1_150),
        );
        assert!(!failed.held());
        assert!(failed.lines()[0].starts_with("FAIL claude status over the hook's wait"));
        assert!(failed.lines()[0].ends_with("idle in 1 reads"));
    }

    #[test]
    fn the_daemons_own_readings_of_the_window_are_told_with_when_it_made_them() {
        let event = |at: &str, who: &str, state: &str| json!({ "kind": "window.activity", "participant": who, "state": state, "at": at });
        let events = [
            // A minute before the wait: the state the window was in as it began.
            event("2026-10-11T09:29:30.000Z", "bench-claude-1", "working"),
            event("2026-10-11T09:30:03.100Z", "bench-claude-1", "idle"),
            event("2026-10-11T09:30:04.000Z", "chief", "working"),
            event("2026-10-11T09:31:30.000Z", "bench-claude-1", "working"),
            json!({ "kind": "message.sent", "data": {} }),
        ];
        let from = 1_791_711_000_000;
        assert_eq!(
            the_daemons_reading(&events, "bench-claude-1", from, from + 20_000),
            "working at -30.0 s, idle at 3.1 s"
        );
        assert_eq!(
            the_daemons_reading(&events, "nobody", from, from + 20_000),
            "none"
        );
    }
}

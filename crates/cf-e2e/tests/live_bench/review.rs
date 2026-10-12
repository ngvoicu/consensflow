//! The two steps that close the bench:
//!
//! - A review, live: the chief puts it on the board for the reviewer's tier like any
//!   task, the reviewer takes it in a session of its own, and its findings come
//!   back to the chief as the result. Nothing is reviewed unless the chief asks.
//! - The restart: a new daemon and pane host over the same home, in the app's quit
//!   order. The project must come back on its chief's own conversation.

use std::time::{Duration, Instant};

use cf_e2e::live::members::{find, Members};
use cf_e2e::live::Harness;
use cf_e2e::Result;
use serde_json::{json, Value};

use crate::bench::{first_chars, seconds_since, took, Bench};

/// The id of the reviewer's agent in the roster, and the marker its findings carry.
pub const REVIEWER: &str = "bench-reviewer";
pub const MARKER: &str = "BENCH_REVIEW_OK";

/// How long the project has to come back after the restart, and the chief's window
/// to be opened again on it.
const BACK: Duration = Duration::from_secs(120);
const REOPENED: Duration = Duration::from_secs(60);

/// The flag that names the reviewer's tier on `cf task add`: critical work names a
/// purpose, and a review's is `critical-review`. A reviewer that was not added has no tier.
pub fn review_flag(tier: Option<&str>) -> String {
    match tier {
        Some("critical") => "--tier critical --purpose critical-review".to_owned(),
        Some(tier) => format!("--tier {tier}"),
        None => "--tier undefined".to_owned(),
    }
}

/// What the human types to the chief to have it put a review on the board.
pub fn review_brief(flag: &str) -> String {
    format!(
        "Run exactly this command in your shell, then reply with one line:\ncf task add --review {flag} \"Review this one-line result of T-1 for spelling: BENCH_OK_PI. No files are involved. Reply with exactly: {MARKER}\""
    )
}

/// The last `last` words of a command line, as a window was opened: what the check
/// says of how the chief's window was opened before the quit and after it.
pub fn last_words(open: Option<&Value>, last: usize) -> Value {
    open.and_then(|open| open["argv"].as_array())
        .map_or(Value::Null, |argv| {
            Value::Array(argv[argv.len().saturating_sub(last)..].to_vec())
        })
}

/// A review of the reviewer's tier: the reviewer is added to the staff, the chief puts
/// the task on the board, and its findings are delivered to the chief.
pub fn review(bench: &mut Bench, reviewer: Harness) -> Result<()> {
    let started = Instant::now();
    // Only the task the chief creates from here on counts, not the baseline's.
    let before = bench
        .board_tasks()?
        .iter()
        .filter_map(|task| task["number"].as_i64())
        .max()
        .unwrap_or(0)
        .max(0);
    let added = bench.members.page(
        "member.add",
        json!({ "agent": REVIEWER, "roles": ["reviewer"] }),
    )?;
    let setup = added["ok"] == true;
    let tier = added["member"]["tier"].as_str();
    bench.record(
        "review-setup",
        setup,
        &if setup {
            json!({ "reviewer": reviewer.name(), "tier": tier })
        } else {
            json!({ "reviewer": reviewer.name(), "error": added["error"] })
        },
    );
    let told = bench.tell(&review_brief(&review_flag(tier)));
    let review = if told {
        bench.wait("the review to be done", || {
            Ok(bench.board_tasks()?.into_iter().find(|task| {
                task["number"].as_i64().unwrap_or_default() > before
                    && task["pool"] == "reviewer"
                    && task["state"] == "done"
            }))
        })?
    } else {
        None
    };
    let mut more = json!({
        "reviewer": review.as_ref().map_or_else(
            || bench.members.lane(REVIEWER).map_or(Value::Null, |lane| lane["activity"].clone()),
            |review| review["assignee"].clone()
        ),
    });
    if !told {
        more["chief"] = bench.chief_activity()?;
    }
    bench.record(
        "review-finished",
        review.is_some(),
        &took(seconds_since(started), more),
    );
    let delivered = match &review {
        Some(review) => bench.wait("the review's findings to be delivered to the chief", || {
            Ok(bench.chief_inbox()?.into_iter().find(|message| {
                message["kind"] == "result"
                    && message["taskNumber"] == review["number"]
                    && message["state"] == "delivered"
            }))
        })?,
        None => None,
    };
    let findings = delivered
        .as_ref()
        .and_then(|delivered| delivered["body"].as_str());
    bench.record(
        "review-received-by-chief",
        findings.is_some_and(|findings| findings.contains(MARKER)),
        &took(
            seconds_since(started),
            findings.map_or_else(
                || json!({}),
                |findings| json!({ "findings": first_chars(findings, 120) }),
            ),
        ),
    );
    Ok(())
}

/// What the restart came to: the project is open again, and the chief's window was
/// opened again. `before` is how the chief's window was opened before the quit.
pub fn restarted(bench: &mut Bench, before: Option<&Value>) -> Result<()> {
    let members: &Members = bench.members;
    let pane = format!("p{}-chief", members.project_id());
    let back = find(
        bench.log,
        "the project to be open again",
        BACK,
        Duration::from_secs(1),
        || Ok(members.is_open()?.then_some(())),
    )?;
    let reopened = find(
        bench.log,
        "the chief's window to be opened again",
        REOPENED,
        Duration::from_secs(1),
        || {
            Ok(members
                .rig()
                .open_frames()
                .into_iter()
                .rev()
                .find(|frame| frame["id"] == pane.as_str()))
        },
    )?;
    bench.record(
        "restart-restores-project",
        back.is_some() && reopened.is_some(),
        &json!({
            "back": back.is_some(),
            "before": last_words(before, 4),
            "after": last_words(reopened.as_ref(), 4),
        }),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reviews_critical_work_names_the_purpose_of_a_review_and_other_work_only_its_tier() {
        assert_eq!(
            review_flag(Some("critical")),
            "--tier critical --purpose critical-review"
        );
        assert_eq!(review_flag(Some("standard")), "--tier standard");
        // A reviewer that was not added has no tier, and the task says so.
        assert_eq!(review_flag(None), "--tier undefined");
    }

    #[test]
    fn the_human_has_the_chief_put_a_spelling_review_on_the_board_with_the_flag() {
        assert_eq!(
            review_brief("--tier light"),
            "Run exactly this command in your shell, then reply with one line:\n\
             cf task add --review --tier light \"Review this one-line result of T-1 for spelling: BENCH_OK_PI. No files are involved. Reply with exactly: BENCH_REVIEW_OK\""
        );
    }

    #[test]
    fn a_window_is_told_by_the_last_words_it_was_opened_with_and_none_if_it_never_was() {
        let open = json!({ "argv": ["claude", "--settings", "s", "--session-id", "id-1", "--model", "m"] });
        assert_eq!(
            last_words(Some(&open), 4),
            json!(["--session-id", "id-1", "--model", "m"])
        );
        assert_eq!(last_words(Some(&json!({ "argv": ["a"] })), 4), json!(["a"]));
        assert_eq!(last_words(None, 4), Value::Null);
        assert_eq!(last_words(Some(&json!({})), 4), Value::Null);
    }
}

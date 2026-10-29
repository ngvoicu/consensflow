//! Live: does a Codex window that compacts its thread before it takes its first message
//! keep it? It is opt-in (`cargo xtask live compacting`, `npm run live:compacting`): it
//! spends the machine's quota on a Codex window, and needs Codex installed and logged
//! in, so it is ignored unless asked for and is no part of `check` or of CI.
//!
//! The field's case (T-901, 2026-10-28, 3.0.6-alpha.2): a Codex window opened again on a
//! long thread took the chief's tell from its broker and compacted the thread before it
//! wrote the tell to its rollout; the daemon gave the first message up after its three
//! minutes, "the window never showed its first message", and closed the window.
//!
//! Here Codex runs on the kit's home of the run's own (a copy of the machine's login, whose
//! refresh the kit fails the run for, and a `config.toml` that sets
//! `model_auto_compact_token_limit` low), so that a thread past a
//! thousand tokens is compacted at the start of its next turn, as Codex 0.165 does: the
//! turn begins (`task_started`), the compaction is written (`compacted`), and only then
//! the message. A member is given a task and ends it, and its window closes with the
//! task; a follow-up for the same window (`cf task add --after`) opens it again on its
//! thread with the follow-up as its first message, which the thread compacts before it
//! takes. The daemon gives a first message eight seconds here, not three minutes: a
//! compaction that outlasts them is the field's case exactly, and a shorter one shows
//! the order all the same. The check holds when the follow-up is answered, nothing of it
//! failed or went again, and its thread compacted before it took it; it says how long
//! the compaction took against the time the daemon gave.
//!
//! ```text
//! cargo xtask live compacting
//! ```

use std::path::Path;
use std::time::Duration;

use cf_e2e::live::members::{find, is_over, last_result, member, stand_in_chief, Members, Plan};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{agents, Harness, Log};
use cf_e2e::rollout::{self, Turn};
use cf_e2e::Result;
use serde_json::Value;

/// The stand-in for the chief's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// How long a member may take to end a task, and a window to close once it has.
const DONE: Duration = Duration::from_secs(300);
const CLOSED: Duration = Duration::from_secs(60);

/// How often the board is looked at.
const LOOK: Duration = Duration::from_millis(500);

/// How long the daemon gives a first message to show, in milliseconds.
const LAUNCH_MS: &str = "8000";

/// What Codex's home is configured with: a thread past this many tokens is compacted at
/// the start of its next turn.
const CONFIG: &str = "model_auto_compact_token_limit = 1000\n";

/// What the check saw.
#[derive(Debug, Clone, PartialEq)]
struct Seen {
    done: bool,
    result: Option<String>,
    /// What failed or went again of the follow-up's delivery, by the ledger's events.
    redone: Vec<String>,
    turn: Turn,
    state: String,
    screen: String,
}

/// What came of it, and the words that say so.
fn judge(seen: &Seen) -> (bool, String) {
    let limit = LAUNCH_MS.parse::<f64>().unwrap_or(0.0) / 1000.0;
    if !seen.redone.is_empty() {
        return (
            false,
            format!(
                "the follow-up failed or went again: {} ({}): {}",
                seen.redone.join("; "),
                seen.state,
                seen.screen
            ),
        );
    }
    if !seen.done
        || !seen
            .result
            .as_deref()
            .is_some_and(|said| said.to_lowercase().contains("again"))
    {
        return (
            false,
            format!(
                "the follow-up was not answered in {} s ({}): {}",
                DONE.as_secs(),
                seen.state,
                seen.screen
            ),
        );
    }
    match seen.turn.compacted {
        Some(took) => (
            true,
            format!(
                "the thread compacted for {took:.1} s before it took its first message (the daemon gives {limit:.0} s), and the message was answered once"
            ),
        ),
        None => (
            false,
            format!(
                "the follow-up was answered, but its thread {} before it took it: the check proves nothing",
                if seen.turn.taken { "did not compact" } else { "never took it" }
            ),
        ),
    }
}

/// The pane's window has ended, as the pane host told.
fn exited(members: &Members, pane: &Value) -> bool {
    members
        .rig()
        .exits()
        .iter()
        .any(|exit| exit["id"] == pane["id"] && exit["generation"] == pane["generation"])
}

fn check(log: &Log) -> Result<Check> {
    let plan = Plan::new("compacting", stand_in_chief())
        .with(member(Harness::Codex))
        .with_home_file(Harness::Codex, "CODEX_HOME", "config.toml", CONFIG)
        .with_daemon_var("CONSENSFLOW_LAUNCH_TIMEOUT_MS", LAUNCH_MS);
    let mut members = Members::open(&plan, Path::new(FAKE_AGENT), log)?;
    let home = members.codex_home();
    log.say(format!(
        "Codex runs on the kit's home of the run's own, {}, a copy of the machine's login and the config.toml that compacts early",
        home.display()
    ));
    let agent = agents::id(Harness::Codex);
    members.add(&agent, &[])?;

    // A fresh window: the task opens one, ends with the member's result, and its thread
    // is past the limit.
    let first = members.give(&agent, "Reply with only: ok")?;
    let opened = find(log, "the member's window to open", DONE, LOOK, || {
        members.window_of(&agent)
    })?;
    let ended = find(log, "the first task to end", DONE, LOOK, || {
        Ok(is_over(&members.task(first)?).then_some(()))
    })?;
    let (ok, detail) = match (opened, ended) {
        (Some(window), Some(())) => {
            let closed = find(log, "the first window to close", CLOSED, LOOK, || {
                Ok(exited(&members, &window.pane).then_some(()))
            })?;
            if closed.is_none() {
                let _ = members.rig().host("pane.kill", window.pane.clone());
                find(
                    log,
                    "the first window to close once killed",
                    CLOSED,
                    LOOK,
                    || Ok(exited(&members, &window.pane).then_some(())),
                )?;
            }
            let after = format!("T-{first}");
            let follow = members.as_chief([
                "task",
                "add",
                "--after",
                after.as_str(),
                "--json",
                "Reply with only: again",
            ])?;
            let number = serde_json::from_str::<Value>(&follow.stdout)
                .ok()
                .and_then(|said| said["task"]["number"].as_i64());
            match (follow.code, number) {
                (Some(0), Some(number)) => {
                    let done = find(log, "the follow-up to end", DONE, LOOK, || {
                        Ok(is_over(&members.task(number)?).then_some(()))
                    })?;
                    let thread = members.task(number)?;
                    let redone = members
                        .events()
                        .into_iter()
                        .filter(|event| {
                            event["kind"] == "delivery.failed"
                                || event["kind"] == "delivery.retried"
                        })
                        .map(|event| {
                            event["data"]["reason"]
                                .as_str()
                                .unwrap_or_default()
                                .to_owned()
                        })
                        .collect();
                    let mut turn = Turn {
                        compacted: None,
                        taken: false,
                    };
                    for found in rollout::rollouts(&home) {
                        let records = rollout::records(&rollout::read(&found)?);
                        let this = rollout::turn_taking(&records, "Reply with only: again");
                        if this.taken {
                            turn = this;
                        }
                    }
                    judge(&Seen {
                        done: done.is_some(),
                        result: last_result(&thread)
                            .and_then(|message| message["body"].as_str().map(str::to_owned)),
                        redone,
                        turn,
                        state: format!(
                            "T-{number}: {}",
                            thread["state"].as_str().unwrap_or("undefined")
                        ),
                        screen: members.tail(window.id()),
                    })
                }
                _ => (false, format!("cf task add --after T-{first}: {follow}")),
            }
        }
        (opened, _) => (
            false,
            format!(
                "a fresh window never {} in {} s: {}",
                if opened.is_some() {
                    "ended its task"
                } else {
                    "opened"
                },
                DONE.as_secs(),
                opened.map_or_else(String::new, |window| members.tail(window.id()))
            ),
        ),
    };
    let (ok, detail) = match members.close() {
        Ok(()) => (ok, detail),
        Err(failed) => (false, format!("{detail}; the rig did not end: {failed}")),
    };
    Ok(Check::new(
        "codex",
        "a window that compacts before its first message",
        ok,
        detail,
    ))
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a Codex window; cargo xtask live compacting (npm run live:compacting)"]
fn a_codex_window_that_compacts_its_thread_before_its_first_message_takes_it() -> Result {
    let log = Log::begin("live compacting")?;
    let checks = vec![check(&log)?];
    report::conclude(&log, "what the check came to", &checks)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn records() -> Vec<Value> {
        vec![
            json!({ "timestamp": "2026-10-28T20:15:16.623Z", "type": "event_msg", "payload": { "type": "task_complete" } }),
            json!({ "timestamp": "2026-10-28T20:15:25.686Z", "type": "event_msg", "payload": { "type": "task_started" } }),
            json!({ "timestamp": "2026-10-28T20:15:27.845Z", "type": "compacted", "payload": { "message": "" } }),
            json!({ "timestamp": "2026-10-28T20:15:27.880Z", "type": "response_item", "payload": { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "[ConsensFlow m-9 · T-2 · task from @chief]\nReply with only: again" }] } }),
        ]
    }

    fn seen(turn: Turn) -> Seen {
        Seen {
            done: true,
            result: Some("again".to_owned()),
            redone: Vec::new(),
            turn,
            state: "T-2: done".to_owned(),
            screen: "its screen".to_owned(),
        }
    }

    #[test]
    fn reads_the_compaction_its_turn_began_with_before_the_message_as_probed_on_codex_0_165() {
        let turn = rollout::turn_taking(&records(), "Reply with only: again");
        assert!(turn.taken);
        let took = turn.compacted.expect("compacted first");
        assert!((took - 2.159).abs() < 0.001, "{took}");
        // A message taken with no compaction before it, and one never taken.
        let mut plain = records();
        plain.remove(2);
        assert_eq!(
            rollout::turn_taking(&plain, "Reply with only: again").compacted,
            None
        );
        // A compaction after the message is not one its turn began with.
        let mut after = records();
        let compaction = after.remove(2);
        after.push(compaction);
        assert_eq!(
            rollout::turn_taking(&after, "Reply with only: again").compacted,
            None
        );
        assert!(!rollout::turn_taking(&records(), "Reply with only: elsewhere").taken);
    }

    #[test]
    fn holds_only_where_the_follow_up_was_answered_once_after_a_compaction() {
        let compacted = Turn {
            compacted: Some(2.2),
            taken: true,
        };
        let (ok, detail) = judge(&seen(compacted.clone()));
        assert!(ok, "{detail}");
        assert_eq!(
            detail,
            "the thread compacted for 2.2 s before it took its first message (the daemon gives 8 s), and the message was answered once"
        );
        let mut failed = seen(compacted);
        failed.redone = vec!["the window never showed its first message".to_owned()];
        let (ok, detail) = judge(&failed);
        assert!(!ok);
        assert!(
            detail.starts_with(
                "the follow-up failed or went again: the window never showed its first message"
            ),
            "{detail}"
        );
        let (ok, detail) = judge(&seen(Turn {
            compacted: None,
            taken: true,
        }));
        assert!(!ok);
        assert!(detail.contains("did not compact"), "{detail}");
    }
}

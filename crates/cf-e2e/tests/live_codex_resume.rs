//! Live: does Codex's broker name the thread of a window opened again on it? It
//! is opt-in (`cargo xtask live codex-resume`, `npm run live:codex-resume`): it
//! spends the machine's quota on a Codex window, and needs Codex installed and
//! logged in, so it is ignored unless asked for and is no part of `check` or of CI.
//!
//! A Codex window takes its messages through ConsensFlow's broker, which knows the
//! thread the window shows from the TUI's own requests; a window it never names
//! reads as starting, and nothing is delivered to it. A member's window opened
//! again on its conversation for a reopened task took the task and asked the chief,
//! and the chief's answer never reached it (2026-10-03, on the Mac's Codex 0.159.2
//! and Windows' 0.160.0): their TUI resumes with its workspace roots null, where the
//! broker looked for a list.
//!
//! This is the daemon's own path to it, on the rig of the live receipt checks
//! ([`cf_e2e::live::members`]): a Codex member of a project on the native daemon is
//! given a task and ends it, and the daemon closes its window with the task. Then a
//! follow-up for the same window (`cf task add --after`) has the daemon open the
//! window again on its conversation, with the follow-up as its first message. It
//! needs the window to open on the thread the first one ran in (`codex resume
//! <thread>`), the follow-up to reach it, and its result to come back: a broker that
//! does not name the thread again leaves the follow-up undelivered.
//!
//! ```text
//! cargo xtask live codex-resume
//! ```
//!
//! The run fails when the window opened again did not take its message.

use std::path::Path;
use std::time::{Duration, Instant};

use cf_e2e::live::members::{find, is_over, last_result, member, stand_in_chief, Members, Plan};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{agents, seconds, Harness, Log};
use cf_e2e::Result;
use serde_json::{json, Value};

/// The stand-in for the chief's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// How long a member may take to end a task, and a window to close once it has.
const DONE: Duration = Duration::from_secs(300);
const CLOSED: Duration = Duration::from_secs(60);

/// How often the board is looked at.
const LOOK: Duration = Duration::from_millis(500);

/// What the second window did, as far as the check can tell.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    /// The command line the window the follow-up opened was given: none when no
    /// window of the first one's pane was opened.
    argv: Vec<String>,
    /// The threads the daemon named as the first window's conversation.
    named: Vec<String>,
    /// Whether the follow-up's task ended in the time.
    done: bool,
    /// The follow-up's result.
    result: Option<String>,
    /// The follow-up's state, and the end of the first window's screen: for what
    /// a failure says.
    state: String,
    screen: String,
    /// How long the follow-up took, in seconds.
    seconds: String,
}

/// The thread a window was opened on: the word after `resume` in its command line.
fn thread_of(argv: &[String]) -> Option<&str> {
    let at = argv.iter().position(|word| word == "resume")?;
    argv.get(at + 1).map(String::as_str)
}

/// The threads the daemon named, from its events (`conversation.bound`: the
/// broker saw the TUI's own request and knows the thread the window shows).
fn named_threads(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .filter(|event| event["kind"] == "conversation.bound")
        .filter_map(|event| event["data"]["nativeSession"].as_str().map(str::to_owned))
        .collect()
}

/// What came of it: whether the window opened again took its message, and the
/// words that say so.
fn judge(seen: &Seen) -> (bool, String) {
    let thread = thread_of(&seen.argv);
    match thread {
        Some(thread) if seen.named.iter().any(|named| named == thread) => {
            if seen.done
                && seen
                    .result
                    .as_deref()
                    .is_some_and(|result| result.to_lowercase().contains("again"))
            {
                (
                    true,
                    format!(
                        "opened again on {thread}, took its message and answered in {} s",
                        seen.seconds
                    ),
                )
            } else {
                (
                    false,
                    format!(
                        "opened again on {thread}, but its message was not taken and answered in {} s ({}): {}",
                        DONE.as_secs(),
                        seen.state,
                        seen.screen
                    ),
                )
            }
        }
        _ => (
            false,
            format!(
                "the window was not opened again on a thread the first one ran in (named: {}): {}",
                if seen.named.is_empty() {
                    "none".to_owned()
                } else {
                    seen.named.join(", ")
                },
                if seen.argv.is_empty() {
                    "no window opened".to_owned()
                } else {
                    seen.argv.join(" ")
                }
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

/// The check: a task, its end, the window's close, and a follow-up for the same
/// window.
fn check(log: &Log) -> Result<Check> {
    let plan = Plan::new("codex-resume", stand_in_chief()).with(member(Harness::Codex));
    let mut members = Members::open(&plan, Path::new(FAKE_AGENT), log)?;
    let agent = agents::id(Harness::Codex);
    members.add(&agent, &[])?;

    // A fresh window: the task opens one, and ends with the member's result.
    let first = members.give(&agent, "Reply with only: ok")?;
    let opened = find(log, "the member's window to open", DONE, LOOK, || {
        members.window_of(&agent)
    })?;
    let ended = find(log, "the first task to end", DONE, LOOK, || {
        Ok(is_over(&members.task(first)?).then_some(()))
    })?;
    let (ok, detail) = match (opened, ended) {
        (Some(window), Some(())) => {
            // The daemon closes a window with its task; one it left open is closed here, so
            // that the follow-up has to open it again.
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
            std::thread::sleep(Duration::from_secs(2));
            let named = named_threads(&members.events());
            // As the daemon opens it for a reopened task: on the thread, with the message.
            let started = Instant::now();
            let before = members.rig().open_frames().len();
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
                    // The window the follow-up opened is the same session's: its pane has the same name.
                    let again = members
                        .rig()
                        .open_frames()
                        .into_iter()
                        .skip(before)
                        .find(|frame| frame["id"] == window.pane["id"]);
                    let thread = members.task(number)?;
                    let seen = Seen {
                        argv: again
                            .as_ref()
                            .and_then(|frame| frame["argv"].as_array())
                            .map(|argv| argv.iter().map(cf_e2e::live::text).collect())
                            .unwrap_or_default(),
                        named,
                        done: done.is_some(),
                        result: last_result(&thread)
                            .and_then(|message| message["body"].as_str().map(str::to_owned)),
                        state: format!(
                            "T-{number}: {}",
                            thread["state"].as_str().unwrap_or("undefined")
                        ),
                        screen: members.tail(window.id()),
                        seconds: seconds(
                            i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX),
                        ),
                    };
                    judge(&seen)
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
    // A rig that would not end fails the check beside what it saw.
    let (ok, detail) = match members.close() {
        Ok(()) => (ok, detail),
        Err(failed) => (false, format!("{detail}; the rig did not end: {failed}")),
    };
    Ok(Check::new(
        "codex",
        "a window opened again on its thread",
        ok,
        detail,
    ))
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a Codex window; cargo xtask live codex-resume (npm run live:codex-resume)"]
fn a_codex_window_opened_again_on_its_thread_takes_the_message_it_was_opened_with() -> Result {
    let log = Log::new("live codex-resume");
    let checks = vec![check(&log)?];
    report::conclude(&log, "what the check came to", &checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(argv: &[&str], named: &[&str], done: bool, result: Option<&str>) -> Seen {
        Seen {
            argv: argv.iter().map(|word| (*word).to_owned()).collect(),
            named: named.iter().map(|word| (*word).to_owned()).collect(),
            done,
            result: result.map(str::to_owned),
            state: "T-2: working".to_owned(),
            screen: "its screen".to_owned(),
            seconds: "41.2".to_owned(),
        }
    }

    #[test]
    fn a_window_opened_again_is_on_the_thread_that_follows_resume_in_its_command_line() {
        let argv = ["codex", "resume", "t-1", "-c", "model=x"].map(str::to_owned);
        assert_eq!(thread_of(&argv), Some("t-1"));
        assert_eq!(thread_of(&["codex".to_owned()]), None);
        assert_eq!(thread_of(&["codex".to_owned(), "resume".to_owned()]), None);
        assert_eq!(thread_of(&[]), None);
    }

    #[test]
    fn the_threads_the_daemon_named_are_the_native_sessions_of_the_conversations_it_bound() {
        let events = [
            json!({ "kind": "conversation.bound", "data": { "nativeSession": "t-1" } }),
            json!({ "kind": "message.sent", "data": { "nativeSession": "no" } }),
            json!({ "kind": "conversation.bound", "data": { "nativeSession": "t-2" } }),
            json!({ "kind": "conversation.bound", "data": {} }),
        ];
        assert_eq!(named_threads(&events), ["t-1", "t-2"]);
        assert!(named_threads(&[]).is_empty());
    }

    #[test]
    fn a_window_opened_again_on_a_thread_the_first_ran_in_that_took_its_message_holds() {
        let (ok, detail) = judge(&seen(
            &["codex", "resume", "t-1"],
            &["t-1"],
            true,
            Some("AGAIN"),
        ));
        assert!(ok, "{detail}");
        assert_eq!(
            detail,
            "opened again on t-1, took its message and answered in 41.2 s"
        );
    }

    #[test]
    fn a_window_not_opened_on_a_named_thread_is_the_failure_the_broker_that_names_none_makes() {
        // Opened afresh, with no `resume` in its command line.
        let (ok, detail) = judge(&seen(&["codex", "-c", "x"], &["t-1"], true, Some("again")));
        assert!(!ok);
        assert_eq!(
            detail,
            "the window was not opened again on a thread the first one ran in (named: t-1): codex -c x"
        );
        // Opened on a thread the daemon never named, or with none named.
        let (ok, detail) = judge(&seen(
            &["codex", "resume", "t-9"],
            &["t-1", "t-2"],
            true,
            None,
        ));
        assert!(!ok);
        assert!(
            detail.contains("(named: t-1, t-2): codex resume t-9"),
            "{detail}"
        );
        let (ok, detail) = judge(&seen(&[], &[], false, None));
        assert!(!ok);
        assert_eq!(
            detail,
            "the window was not opened again on a thread the first one ran in (named: none): no window opened"
        );
    }

    #[test]
    fn a_window_that_was_opened_again_but_never_took_its_message_says_where_it_stood() {
        let (ok, detail) = judge(&seen(&["codex", "resume", "t-1"], &["t-1"], false, None));
        assert!(!ok);
        assert_eq!(
            detail,
            "opened again on t-1, but its message was not taken and answered in 300 s (T-2: working): its screen"
        );
        // An answer that is not the follow-up's is not the follow-up taken.
        let (ok, _) = judge(&seen(
            &["codex", "resume", "t-1"],
            &["t-1"],
            true,
            Some("ok"),
        ));
        assert!(!ok);
    }
}

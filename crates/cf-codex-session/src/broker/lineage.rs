//! Whether Codex says a thread is top-level: the one place that decides whether a resume
//! or a fork is given the window's role.
//!
//! A thread is top-level when Codex's `thread/read` of it names it (the same id), with
//! no parent (`parentThreadId` null), a source of a user's client (`source` "cli",
//! "vscode", "exec" or "appServer": not a sub-agent's, not "unknown", which is what
//! Codex's internal sessions such as the memory job read as, and not a custom one), and a
//! user's thread source (`threadSource` "user", or none, which a fork has); and when the
//! thread it was forked from (`forkedFromId`) is top-level too, and so on up. Anything
//! Codex does not say, it is not: a read refused (a Codex with no `thread/read`, a thread
//! it does not have), unanswered by the deadline, of a thread whose forks go deeper than
//! [`MAX_FORKS`], or on a broker whose own connection is lost.
//!
//! Read in codex-rs 0.167.0 (`app-server-protocol/src/protocol/v2/thread_data.rs`, the
//! `Thread` and `SessionSource`; `thread-store/src/local/read_thread.rs`, a thread Codex
//! has not loaded read from its rollout's `session_meta`), and answered so by a real
//! Codex 0.167.0 app-server over kept rollouts of Codex 0.164.0 (T-1414,
//! `tests/codex_0_167_threads.json`): a main thread `vscode`, no parent, `user`; a
//! sub-agent's, `fork_context` too, `subAgent.thread_spawn`, its parent, `subagent`; a
//! fork of either `vscode`, no parent, no thread source, forked from it. `source` and
//! `parentThreadId` are on a read's thread from Codex 0.149.0, the oldest Codex
//! ConsensFlow runs (its `codex queue`).

use serde_json::{json, Value};

use super::control::Asked;
use super::{now_ms, Shared};

/// How many threads up a fork's lineage is read before it is taken for not top-level.
pub(crate) const MAX_FORKS: usize = 8;

/// How long the reads that decide one request may take, together.
#[cfg(not(test))]
const READ_WAIT_MS: f64 = 5000.0;
#[cfg(test)]
const READ_WAIT_MS: f64 = 300.0;

/// What one `thread/read` answer says of the thread asked.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Read {
    /// Top-level, forked from this thread if it was.
    TopLevel { forked_from: Option<String> },
    /// Not top-level, or Codex did not say.
    Not,
}

/// What `answer`, Codex's answer to a `thread/read` of `asked`, says of it.
pub(crate) fn read(asked: &str, answer: &Value) -> Read {
    let Some(thread) = answer.get("result").and_then(|result| result.get("thread")) else {
        return Read::Not;
    };
    let named = thread
        .get("id")
        .and_then(Value::as_str)
        .is_some_and(|id| id.eq_ignore_ascii_case(asked));
    let no_parent = thread.get("parentThreadId").is_some_and(Value::is_null);
    let users_client = matches!(
        thread.get("source").and_then(Value::as_str),
        Some("cli" | "vscode" | "exec" | "appServer")
    );
    let users_thread = match thread.get("threadSource") {
        Some(Value::Null) => true,
        Some(source) => source.as_str() == Some("user"),
        None => false,
    };
    if !(named && no_parent && users_client && users_thread) {
        return Read::Not;
    }
    match thread.get("forkedFromId") {
        None | Some(Value::Null) => Read::TopLevel { forked_from: None },
        Some(Value::String(from)) => Read::TopLevel {
            forked_from: Some(from.clone()),
        },
        Some(_) => Read::Not,
    }
}

impl Shared {
    /// Whether Codex says `thread` is top-level, and every thread it was forked from: asked
    /// on the broker's own connection, a read at a time up the forks, all by one deadline.
    pub(super) async fn is_top_level(&self, thread: &str) -> bool {
        let expires_at = now_ms() + READ_WAIT_MS;
        let mut asked = thread.to_owned();
        for _ in 0..=MAX_FORKS {
            let params = json!({ "threadId": asked, "includeTurns": false });
            let Asked::Answered(answer) = self.request("thread/read", params, expires_at).await
            else {
                return false;
            };
            match read(&asked, &answer) {
                Read::Not => return false,
                Read::TopLevel { forked_from: None } => return true,
                Read::TopLevel {
                    forked_from: Some(from),
                } => asked = from,
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::{read, Read};

    const RECORDED: &str = include_str!("tests/codex_0_167_threads.json");

    fn recorded(name: &str) -> Value {
        let recorded: Value = serde_json::from_str(RECORDED).unwrap();
        recorded["reads"][name].clone()
    }

    fn answer(thread: &Value) -> Value {
        json!({ "id": "r", "result": { "thread": thread } })
    }

    fn of(thread: &Value) -> Read {
        read(thread["id"].as_str().unwrap(), &answer(thread))
    }

    #[test]
    fn codexs_own_reads_of_a_main_thread_its_sub_agents_and_their_forks() {
        let main = recorded("main");
        assert_eq!(of(&main), Read::TopLevel { forked_from: None });
        for child in ["child", "forkContextChild"] {
            assert_eq!(of(&recorded(child)), Read::Not, "{child}");
        }
        // A fork reads as top-level whatever it was forked from: its source decides.
        for (fork, from) in [("forkOfMain", "main"), ("forkOfChild", "child")] {
            assert_eq!(
                of(&recorded(fork)),
                Read::TopLevel {
                    forked_from: Some(recorded(from)["id"].as_str().unwrap().to_owned())
                },
                "{fork}"
            );
        }
    }

    #[test]
    fn a_thread_is_top_level_only_when_every_field_says_so() {
        let main = recorded("main");
        let with = |key: &str, value: Value| {
            let mut thread = main.clone();
            thread[key] = value;
            thread
        };
        let without = |key: &str| {
            let mut thread = main.clone();
            thread.as_object_mut().unwrap().remove(key);
            thread
        };
        for source in ["cli", "vscode", "exec", "appServer"] {
            assert_eq!(
                of(&with("source", source.into())),
                Read::TopLevel { forked_from: None },
                "{source}"
            );
        }
        for (what, thread) in [
            ("an internal session", with("source", "unknown".into())),
            ("a custom source", with("source", json!({ "custom": "x" }))),
            ("a review", with("source", json!({ "subAgent": "review" }))),
            (
                "a compaction",
                with("source", json!({ "subAgent": "compact" })),
            ),
            ("no source", without("source")),
            ("a parent", with("parentThreadId", main["id"].clone())),
            ("no word of a parent", without("parentThreadId")),
            ("a sub-agent's", with("threadSource", "subagent".into())),
            (
                "the memory job's",
                with("threadSource", "memory_consolidation".into()),
            ),
            ("a review's", with("threadSource", "guardian_review".into())),
            ("no word of its source", without("threadSource")),
            ("a fork of no id", with("forkedFromId", json!(7))),
        ] {
            assert_eq!(of(&thread), Read::Not, "{what}");
        }
        assert_eq!(
            of(&with("threadSource", Value::Null)),
            Read::TopLevel { forked_from: None }
        );
    }

    #[test]
    fn an_answer_that_is_no_read_of_the_thread_asked_says_nothing() {
        let main = recorded("main");
        let id = main["id"].as_str().unwrap();
        assert_eq!(
            read(&id.to_uppercase(), &answer(&main)),
            Read::TopLevel { forked_from: None },
            "ids in any case"
        );
        assert_eq!(
            read(recorded("child")["id"].as_str().unwrap(), &answer(&main)),
            Read::Not
        );
        for answer in [
            json!({ "id": "r", "error": { "code": -32600, "message": format!("thread not loaded: {id}") } }),
            json!({ "id": "r", "result": {} }),
            json!({ "id": "r", "result": { "thread": null } }),
            json!({ "id": "r" }),
        ] {
            assert_eq!(read(id, &answer), Read::Not, "{answer}");
        }
    }
}

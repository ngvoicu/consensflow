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
//! [`MAX_FORKS`], or on a broker whose own connection is lost. Each such refusal is a line in
//! the daemon's log ([`Shared::log_refusal`]), with the thread and why.
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
    /// Not top-level, as this says (the field that said so, and what it said), or Codex's
    /// answer said nothing of the thread asked.
    Not(String),
    /// Codex refused the read, with its error's code if it gave one.
    Refused(Option<i64>),
}

/// What `answer`, Codex's answer to a `thread/read` of `asked`, says of it.
pub(crate) fn read(asked: &str, answer: &Value) -> Read {
    let Some(thread) = answer.get("result").and_then(|result| result.get("thread")) else {
        return match answer.get("error") {
            Some(error) => Read::Refused(error.get("code").and_then(Value::as_i64)),
            None => Read::Not("its answer holds no thread".to_owned()),
        };
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
    let field = |key: &str| format!("its {key} is {}", shown(thread.get(key)));
    let why = if !named {
        field("id")
    } else if !no_parent {
        field("parentThreadId")
    } else if !users_client {
        field("source")
    } else if !users_thread {
        field("threadSource")
    } else {
        return match thread.get("forkedFromId") {
            None | Some(Value::Null) => Read::TopLevel { forked_from: None },
            Some(Value::String(from)) => Read::TopLevel {
                forked_from: Some(from.clone()),
            },
            Some(_) => Read::Not(field("forkedFromId")),
        };
    };
    Read::Not(why)
}

/// A field's value as the log says it: a string as it is, anything else as JSON, and
/// `missing` for none.
fn shown(value: Option<&Value>) -> String {
    match value {
        None => "missing".to_owned(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

/// Why a resume or fork is given no role: what Codex said, or did not.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// Codex's read of `thread` (the one asked, or one it was forked from) says it is not
    /// top-level, as `why` says.
    Not { thread: String, why: String },
    /// Codex refused the read of `thread`, with its error's code if it gave one.
    Refused { thread: String, code: Option<i64> },
    /// No answer by the deadline.
    Unanswered,
    /// The connection to Codex was lost before it answered.
    Lost,
    /// The read could not be sent: the broker's connection to Codex is closed.
    NotSent,
    /// The thread's forks go back further than [`MAX_FORKS`].
    Forks,
}

impl Refusal {
    /// The refusal of a resume or fork of `thread`, as the log says it.
    pub(crate) fn said_of(&self, thread: &str) -> String {
        let of = |other: &str| {
            if other.eq_ignore_ascii_case(thread) {
                String::new()
            } else {
                format!(" of thread {other}, which it was forked from,")
            }
        };
        match self {
            Self::Not { thread: asked, why } => {
                format!(
                    "Codex's thread/read{} says it is not top-level: {why}",
                    of(asked)
                )
            }
            Self::Refused {
                thread: asked,
                code,
            } => {
                let code =
                    code.map_or_else(|| "no code".to_owned(), |code| format!("error {code}"));
                format!(
                    "Codex refused its thread/read{} ({code})",
                    of(asked).trim_end_matches(',')
                )
            }
            Self::Unanswered => format!(
                "Codex did not answer its thread/read within {} s",
                READ_WAIT_MS / 1000.0
            ),
            Self::Lost => {
                "the connection to Codex was lost before it answered its thread/read".to_owned()
            }
            Self::NotSent => {
                "its thread/read could not be sent: the connection to Codex is closed".to_owned()
            }
            Self::Forks => format!("its forks go back more than {MAX_FORKS} threads"),
        }
    }
}

impl Shared {
    /// Whether Codex says `thread` is top-level, and every thread it was forked from: asked
    /// on the broker's own connection, a read at a time up the forks, all by one deadline.
    /// Why not, if not.
    pub(super) async fn top_level(&self, thread: &str) -> Result<(), Refusal> {
        let expires_at = now_ms() + READ_WAIT_MS;
        let mut asked = thread.to_owned();
        for _ in 0..=MAX_FORKS {
            let params = json!({ "threadId": asked, "includeTurns": false });
            let answer = match self.request("thread/read", params, expires_at).await {
                Asked::Answered(answer) => answer,
                Asked::NotSent => return Err(Refusal::NotSent),
                Asked::Unanswered if now_ms() >= expires_at => return Err(Refusal::Unanswered),
                Asked::Unanswered => return Err(Refusal::Lost),
            };
            match read(&asked, &answer) {
                Read::Not(why) => return Err(Refusal::Not { thread: asked, why }),
                Read::Refused(code) => {
                    return Err(Refusal::Refused {
                        thread: asked,
                        code,
                    })
                }
                Read::TopLevel { forked_from: None } => return Ok(()),
                Read::TopLevel {
                    forked_from: Some(from),
                } => asked = from,
            }
        }
        Err(Refusal::Forks)
    }

    /// Tells the daemon's log, in one line, that the TUI's `method` (a resume or a fork) of
    /// `thread` goes without the window's role, and why: its id and Codex's word, nothing
    /// of the role or the conversation. Nobody is told otherwise: nobody can act on it.
    pub(super) fn log_refusal(&self, method: &str, thread: &str, refusal: &Refusal) {
        if let Some(home) = &self.log_home {
            crate::log::info_in(
                home,
                &format!(
                    "cf codex-session gave Codex's {method} of thread {thread} no role: {}",
                    refusal.said_of(thread)
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::{read, Read, Refusal};

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
        for name in ["child", "forkContextChild"] {
            let child = recorded(name);
            assert_eq!(
                of(&child),
                Read::Not(format!(
                    "its parentThreadId is {}",
                    child["parentThreadId"].as_str().unwrap()
                )),
                "{name}"
            );
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
        let parent = format!("its parentThreadId is {}", main["id"].as_str().unwrap());
        for (what, thread, why) in [
            (
                "an internal session",
                with("source", "unknown".into()),
                "its source is unknown",
            ),
            (
                "a custom source",
                with("source", json!({ "custom": "x" })),
                r#"its source is {"custom":"x"}"#,
            ),
            (
                "a review",
                with("source", json!({ "subAgent": "review" })),
                r#"its source is {"subAgent":"review"}"#,
            ),
            (
                "a compaction",
                with("source", json!({ "subAgent": "compact" })),
                r#"its source is {"subAgent":"compact"}"#,
            ),
            ("no source", without("source"), "its source is missing"),
            (
                "a parent",
                with("parentThreadId", main["id"].clone()),
                parent.as_str(),
            ),
            (
                "no word of a parent",
                without("parentThreadId"),
                "its parentThreadId is missing",
            ),
            (
                "a sub-agent's",
                with("threadSource", "subagent".into()),
                "its threadSource is subagent",
            ),
            (
                "the memory job's",
                with("threadSource", "memory_consolidation".into()),
                "its threadSource is memory_consolidation",
            ),
            (
                "a review's",
                with("threadSource", "guardian_review".into()),
                "its threadSource is guardian_review",
            ),
            (
                "no word of its source",
                without("threadSource"),
                "its threadSource is missing",
            ),
            (
                "a fork of no id",
                with("forkedFromId", json!(7)),
                "its forkedFromId is 7",
            ),
        ] {
            assert_eq!(of(&thread), Read::Not(why.to_owned()), "{what}");
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
            Read::Not(format!("its id is {id}"))
        );
        assert_eq!(
            read(
                id,
                &json!({ "id": "r", "error": { "code": -32600, "message": format!("thread not loaded: {id}") } })
            ),
            Read::Refused(Some(-32600))
        );
        assert_eq!(
            read(id, &json!({ "id": "r", "error": {} })),
            Read::Refused(None)
        );
        for (answer, why) in [
            (
                json!({ "id": "r", "result": {} }),
                "its answer holds no thread",
            ),
            (
                json!({ "id": "r", "result": { "thread": null } }),
                "its id is missing",
            ),
            (json!({ "id": "r" }), "its answer holds no thread"),
        ] {
            assert_eq!(read(id, &answer), Read::Not(why.to_owned()), "{answer}");
        }
    }

    #[test]
    fn a_refusal_says_of_the_thread_what_codex_said_or_did_not() {
        const A: &str = "01a09094-938f-7fd1-a2d3-315cf92b4559";
        const B: &str = "01a09094-a559-7db0-bf50-e2309856c3c0";
        let why = "its threadSource is subagent".to_owned();
        for (refusal, said) in [
            (
                Refusal::Not { thread: A.to_owned(), why: why.clone() },
                "Codex's thread/read says it is not top-level: its threadSource is subagent".to_owned(),
            ),
            (
                Refusal::Not { thread: B.to_owned(), why },
                format!("Codex's thread/read of thread {B}, which it was forked from, says it is not top-level: its threadSource is subagent"),
            ),
            (
                Refusal::Refused { thread: A.to_owned(), code: Some(-32600) },
                "Codex refused its thread/read (error -32600)".to_owned(),
            ),
            (
                Refusal::Refused { thread: B.to_owned(), code: None },
                format!("Codex refused its thread/read of thread {B}, which it was forked from (no code)"),
            ),
            (Refusal::Unanswered, "Codex did not answer its thread/read within 0.3 s".to_owned()),
            (
                Refusal::Lost,
                "the connection to Codex was lost before it answered its thread/read".to_owned(),
            ),
            (
                Refusal::NotSent,
                "its thread/read could not be sent: the connection to Codex is closed".to_owned(),
            ),
            (Refusal::Forks, "its forks go back more than 8 threads".to_owned()),
        ] {
            assert_eq!(refusal.said_of(A), said);
        }
    }
}

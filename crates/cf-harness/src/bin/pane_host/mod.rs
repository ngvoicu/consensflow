//! The pane host, for the test binaries that send through a channel
//! (`codex-send`, `opencode-channel`): the process that started them, put
//! each request over the same two pipes as `common`'s question and answer. A
//! request to it is a line `{"ask": {"op", "body"}}` on stdout, and its answer
//! one line on stdin: `{"answer": value}`, or `{"throws": message, "error":
//! word}` for a host that never answered.

use cf_harness::contract::{HostError, Pane, PaneHost, Work};
use cf_harness::tooling::text;
use serde_json::{json, Value};

use crate::common::{line, say};

/// The pane a send is claimed through.
pub fn pane(asked: &Value) -> Result<Pane, String> {
    Ok(Pane {
        id: text(asked, "pane")?.to_owned(),
        generation: asked
            .get("generation")
            .and_then(Value::as_u64)
            .ok_or("the question names no generation")?,
    })
}

/// The pane host: the process that started this one, asked for each request
/// and waited for. A send asks for its claim before it begins any wait of its
/// own, so nothing else is held up while this one waits.
pub struct Asking;

impl PaneHost for Asking {
    fn request<'a>(&'a self, op: &'a str, body: Value) -> Work<'a, Result<Value, HostError>> {
        Box::pin(async move { ask(op, &body) })
    }
}

/// One request to the process that started this one, and its answer.
fn ask(op: &str, body: &Value) -> Result<Value, HostError> {
    let failed = |cause: &dyn std::fmt::Display| HostError {
        error: None,
        message: cause.to_string(),
    };
    say(&json!({ "ask": { "op": op, "body": body } })).map_err(|cause| failed(&cause))?;
    let said = line().map_err(|cause| failed(&cause))?;
    match said.get("throws").and_then(Value::as_str) {
        Some(message) => Err(HostError {
            error: said.get("error").and_then(Value::as_str).map(str::to_owned),
            message: message.to_owned(),
        }),
        None => Ok(said.get("answer").cloned().unwrap_or(Value::Null)),
    }
}

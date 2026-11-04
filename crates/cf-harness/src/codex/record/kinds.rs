//! A failed turn's `codex_error_info`, in plain words (T-823). The board said a turn Codex's
//! safety filter refused ("This content was flagged for possible cybersecurity risk") as
//! "the harness reported a failure", and the chief could not tell a refusal for content from
//! a crash, a conversation grown too long or an API's error. Codex's words are quoted
//! beside ([`Record::failure`](crate::records::Record::failure)); these say the kind.
//!
//! The kinds are `CodexErrorInfo`'s in the binary of Codex 0.166.0, in snake case: a name, or
//! an object that holds one (`{"http_connection_failed": {"http_status_code": 503}}`). The
//! limits (`usage_limit_exceeded`, `rate_limit_exceeded`), the moments a server refused for
//! (`server_overloaded`, `internal_server_error`, `policy_capacity_unavailable`) and a login
//! Codex refused (`unauthorized`) are no failures the reader holds as one: it says them in
//! the quota ([`super::Rollout::refusal`]). `other` names nothing.

use serde_json::Value;

use super::fields::field;

/// The kinds with a plain name: the kind, what it was, and the way on where there is one.
const KINDS: [(&str, &str, &str); 15] = [
    (
        "cyber_policy",
        "Codex's safety filter refused the turn as a cybersecurity risk",
        "reopen it with the work rephrased, or give security work to another harness",
    ),
    (
        "bio_policy",
        "Codex's safety filter refused the turn as a biological risk",
        "reopen it with the work rephrased, or give it to another harness",
    ),
    (
        "misalignment_policy_violation",
        "Codex's safety filter refused the turn under its misalignment policy",
        "reopen it with the work rephrased, or give it to another harness",
    ),
    (
        "context_window_exceeded",
        "the conversation outgrew the model's context window",
        "compact it, or give the work to a new conversation",
    ),
    (
        "session_budget_exceeded",
        "the session used up the budget set for it",
        "raise the budget, or open a new session",
    ),
    (
        "model_not_found",
        "Codex does not know the model the window asked for",
        "change the window's model",
    ),
    (
        "invalid_prompt",
        "the API rejected the prompt as invalid",
        "reopen the turn with the request rephrased",
    ),
    (
        "bad_request",
        "the API rejected the request as malformed",
        "",
    ),
    (
        "sandbox_error",
        "Codex's sandbox failed under a command",
        "",
    ),
    (
        "active_turn_not_steerable",
        "Codex could not take a message into the turn at work, which cannot be steered",
        "",
    ),
    (
        "thread_rollback_failed",
        "Codex could not roll the conversation back",
        "",
    ),
    (
        "http_connection_failed",
        "Codex could not reach the API",
        "",
    ),
    (
        "response_stream_connection_failed",
        "Codex could not open the API's response stream",
        "",
    ),
    (
        "response_stream_disconnected",
        "the API's response stream was cut off before the turn ended",
        "",
    ),
    (
        "response_too_many_failed_attempts",
        "Codex gave up on a request after too many failed attempts",
        "",
    ),
];

/// What a turn's `error` says its kind is, in plain words (the kind's name after them, then the
/// way on where there is one), where it is a kind with a plain name: `Codex's safety filter
/// refused the turn as a cybersecurity risk (cyber_policy): reopen it with the work
/// rephrased, or give security work to another harness`.
pub(super) fn plain(error: Option<&Value>) -> Option<String> {
    let info = field(error, "codex_error_info")?;
    let name = match info {
        Value::String(name) => name.as_str(),
        Value::Object(kinds) => kinds.keys().next()?.as_str(),
        _ => return None,
    };
    let (kind, what, on) = KINDS.iter().find(|(kind, _, _)| *kind == name)?;
    Some(if on.is_empty() {
        format!("{what} ({kind})")
    } else {
        format!("{what} ({kind}): {on}")
    })
}

//! The tooling's look at the API, which only a test switches on ([`TOOLING_API_CALLS`]): each
//! request a window's token answered, as a line of the event file (`api.answered`), so that a
//! test that seats a real chief can tell what the chief's own `cf` asked and how the daemon
//! answered it, a refusal's code among it, though `cf` says a refusal in words (T-1133's
//! design, section 2; the live matrix, T-1152). The line names the participant, the method, the
//! path, the status and the refusal's code: never the token, never the body, never the answer's
//! words. The app never sets the variable, and a daemon without it writes no such line.

use serde_json::{json, Value};

use super::answer::{Answer, Failure};
use super::callers::Caller;
use crate::files::Trace;

/// The variable the daemon is started with, set to `1`, that has it write the lines: a test's
/// rig sets it for its daemon alone. `CONSENSFLOW_` names reach no window
/// (`seams::WindowEnv`), so a window never sees it.
pub const TOOLING_API_CALLS: &str = "CONSENSFLOW_TOOLING_API_CALLS";

/// The kind of the line.
pub const ANSWERED: &str = "api.answered";

/// Whether the daemon's environment switches the lines on: the variable set to `1`.
pub fn switched_on(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|value| value == "1")
}

/// What the line says of the request `method path` that `caller` made, answered `answered`.
pub fn line(
    caller: &Caller,
    method: &str,
    path: &str,
    answered: &Result<Answer, Failure>,
) -> Value {
    let (status, error) = match answered {
        Ok(answer) => (answer.status, Value::Null),
        Err(Failure::Refused(refusal)) => (refusal.status, json!(refusal.code)),
        Err(Failure::Internal(_)) => (500, json!("internal")),
    };
    json!({
        "participant": caller.participant.handle,
        "role": caller.participant.role,
        "method": method,
        "path": path,
        "status": status,
        "error": error,
    })
}

/// Writes the line to `trace`.
pub fn answered(
    trace: &Trace,
    caller: &Caller,
    method: &str,
    path: &str,
    answered: &Result<Answer, Failure>,
) {
    trace.tooling(
        caller.project.id,
        ANSWERED,
        line(caller, method, path, answered),
    );
}

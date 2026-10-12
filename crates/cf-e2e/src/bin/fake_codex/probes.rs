//! What ConsensFlow asks of a Codex before it opens a window on it: whether the
//! CLI has the native queue a window is reached by (`codex queue --help`), and
//! what its own instructions are, which a role's text is appended to (`codex
//! app-server` spoken to on its standard input and output: initialized, then asked
//! for its configuration).

use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use serde_json::{json, Value};

/// What Codex's own instructions say.
const INSTRUCTIONS: &str = "Codex's own instructions.";

/// `codex queue --help`: the words of a Codex that has the native queue, which a
/// window needs to be reached.
pub fn queue_help() -> ExitCode {
    let _ = writeln!(
        io::stdout(),
        "Queue a message on a thread\n\nUsage: codex queue --thread <THREAD> --message <MESSAGE>"
    );
    ExitCode::SUCCESS
}

/// `codex app-server` with no address to listen on: it answers each request of
/// its standard input with a line of its standard output, the configuration with
/// Codex's instructions, and ends when its input does.
pub fn configuration() -> ExitCode {
    let mut out = io::stdout().lock();
    for line in io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        // A notification has no id, and no answer.
        let Some(id) = request.get("id") else {
            continue;
        };
        let result = if request["method"] == "config/read" {
            json!({ "config": { "developer_instructions": INSTRUCTIONS } })
        } else {
            json!({})
        };
        let answered =
            writeln!(out, "{}", json!({ "id": id, "result": result })).and_then(|()| out.flush());
        if answered.is_err() {
            break;
        }
    }
    ExitCode::SUCCESS
}

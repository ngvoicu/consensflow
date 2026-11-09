//! What ConsensFlow asks of a Codex before it opens a window on it: whether the
//! CLI has the native queue a window is reached by (`codex queue --help`), and
//! what its own instructions are, which a role's text is appended to (`codex
//! app-server` spoken to on its standard input and output: initialized, then asked
//! for its configuration).

use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::ExitCode;

use cf_e2e::process::own_var;
use cf_e2e::stand_in_lists;
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
/// Codex's instructions and its models the rig's stand-ins list, and ends when its input does.
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
        let result = match request["method"].as_str() {
            Some("config/read") => {
                json!({ "config": { "developer_instructions": INSTRUCTIONS } })
            }
            // The models the daemon reads as a dialog opens: the rig's stand-ins list.
            Some("model/list") => stand_in_lists::codex_page(&stand_in_lists::listed(
                "codex",
                Path::new(&own_var("CONSENSFLOW_HOME").unwrap_or_default()),
            )),
            _ => json!({}),
        };
        let answered =
            writeln!(out, "{}", json!({ "id": id, "result": result })).and_then(|()| out.flush());
        if answered.is_err() {
            break;
        }
    }
    ExitCode::SUCCESS
}

//! The models this Claude Code lists, when the daemon reads them as a dialog opens: `claude -p
//! --input-format stream-json`, spoken to on its standard input and output, no window and no
//! turn. It answers the SDK's `initialize` control request with the models the rig's stand-ins
//! list (`cf_e2e::stand_in_lists`), and ends when its input does.

use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::ExitCode;

use cf_e2e::process::own_var;
use cf_e2e::stand_in_lists;
use serde_json::Value;

/// Whether `args` are the read of the models (`claude -p --input-format stream-json`), not a
/// window.
pub fn is_read(args: &[String]) -> bool {
    // On Windows the shim of a window opens the stand-in with its own file first.
    args.iter().take(2).any(|arg| arg == "-p")
        && args
            .windows(2)
            .any(|pair| pair[0] == "--input-format" && pair[1] == "stream-json")
}

/// Answers each `initialize` request of the standard input with the models listed, until the
/// input ends.
pub fn answer() -> ExitCode {
    let home = own_var("CONSENSFLOW_HOME").unwrap_or_default();
    let models = stand_in_lists::listed("claude-code", Path::new(&home));
    let mut out = io::stdout().lock();
    for line in io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if request["type"] != "control_request" || request["request"]["subtype"] != "initialize" {
            continue;
        }
        let id = request["request_id"].as_str().unwrap_or_default();
        let said = writeln!(out, "{}", stand_in_lists::claude_answer(id, &models))
            .and_then(|()| out.flush());
        if said.is_err() {
            break;
        }
    }
    ExitCode::SUCCESS
}

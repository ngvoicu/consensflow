//! The MCP servers Codex would start, and the flags that switch them off for
//! a member (`codexMcpServers` and `mcpIsolation`, `src/adapters/codex.js`).
//! A member runs in full-permission mode and reads what others wrote, so every
//! MCP server Codex would start is switched off: this Mac's Codex drives the
//! browser and the screen through them (the ChatGPT app's, since 2026-09-26).
//! Each gets a harmless, disabled definition; a bare `enabled=false` is refused
//! for servers defined outside config.toml, and a name that needs quotes would
//! define a new server instead, so such a name stops the launch.

use std::borrow::Cow;
use std::path::PathBuf;
use std::time::Duration;

use cf_base::env::Env;
use cf_base::js;
use cf_base::json::from_slice_lossy;
use serde_json::Value;

use crate::seams::processes::{Limits, Processes, Program};

/// What `codex mcp list --json` may take and say.
const LIMITS: Limits = Limits {
    timeout: Duration::from_secs(15),
    max_buffer: 1024 * 1024,
};

/// The servers `codex mcp list --json` names.
///
/// Kept from Node on purpose: an answer that is no JSON fails in words of
/// Rust's own, where V8 gave its `JSON.parse` message; and JSON nested past
/// 127 levels, or holding a number past a double's range, is no JSON here.
pub(super) async fn listed(
    processes: &dyn Processes,
    env: &Env,
    executable: &str,
) -> Result<Vec<Value>, String> {
    let refused =
        |cause: &str| format!("could not list Codex's MCP servers to switch them off: {cause}");
    let program = Program {
        executable: PathBuf::from(executable),
        args: ["mcp", "list", "--json"].map(str::to_owned).to_vec(),
        cwd: None,
        env: env.clone(),
    };
    let stdout = processes
        .run(program, LIMITS)
        .await
        .map_err(|failed| refused(&failed.message))?;
    match from_slice_lossy(stdout.as_bytes()) {
        Ok(Value::Array(servers)) => Ok(servers),
        Ok(_) => Ok(Vec::new()),
        Err(_) => Err(refused("its answer cannot be read as JSON")),
    }
}

/// The name a listed server goes by: none where it has none.
///
/// Kept from Node on purpose: a server listed as `null` fails in words of
/// Rust's own, where V8 threw a `TypeError` reading its name.
fn name_of(server: &Value) -> Result<Option<&Value>, String> {
    match server {
        Value::Null => {
            Err("cannot switch off a Codex MCP server listed as null for a member".to_owned())
        }
        Value::Object(fields) => Ok(fields.get("name")),
        _ => Ok(None),
    }
}

/// The flags that give each of `servers` a disabled definition. A name is
/// taken as JavaScript's pattern test and its template take it: whatever text
/// it makes, a number and a flag too.
///
/// Kept from Node on purpose: a name that is an object with a `toString` of
/// its own fails in words of Rust's own, where V8 threw a `TypeError`.
pub(super) fn isolation(servers: &[Value]) -> Result<Vec<String>, String> {
    let mut flags = Vec::new();
    for server in servers {
        let name = name_of(server)?;
        let text = match name {
            None | Some(Value::Null) => Cow::Borrowed(""),
            Some(value) => js::string(Some(value))?,
        };
        let plain = |character: char| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        };
        if text.is_empty() || !text.chars().all(plain) {
            let shown = name.map_or_else(|| "undefined".to_owned(), js::stringify);
            return Err(format!(
                "cannot switch off the Codex MCP server {shown} for a member"
            ));
        }
        flags.extend([
            "-c".to_owned(),
            format!("mcp_servers.{text}.command=\"/usr/bin/true\""),
            "-c".to_owned(),
            format!("mcp_servers.{text}.enabled=false"),
        ]);
    }
    Ok(flags)
}

#[cfg(test)]
mod tests;

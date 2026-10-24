//! The MCP server a live run gives the real Claude Code of its window (`live mcp`,
//! `live monitor-wait`): a stdio server that
//! speaks as much of the protocol as Claude needs to list it, offers no tool and
//! asks for nothing, and writes its process id where the run looks. A program that
//! only reads its input (`cat`) is marked failed by Claude, which waits for the
//! answer to its `initialize`; this one answers it, with the protocol version the
//! client asked for, and a list with no tool in it.

use std::path::{Path, PathBuf};

use crate::process::{is_alive, started};
use crate::{files, Result};
use serde_json::json;

/// What the server's configuration calls it: the name Claude lists it by.
pub const NAME: &str = "cf-quiet";

/// What its program and the file its process id is written to are called, before their
/// extensions: what its command line holds, which no other process of the machine's does.
pub const STEM: &str = "quiet-server";

/// What the server that leaves the window's process group is called, and its program and
/// the file its process id is written to ([`Server::write_leaving_group`]).
pub const DETACHED_NAME: &str = "cf-detached";
pub const DETACHED_STEM: &str = "detached-server";

/// The server, a Python program (a macOS and a Linux machine has `python3`). It
/// leaves the process group it was started in for a session of its own where it is
/// told to (`leave-group`, after the file), writes its process id to the file it is
/// given, answers each request with a line of JSON as MCP's stdio transport has it,
/// says nothing to a notification (a message with no id), and ends with its input,
/// which is when Claude ends.
const SCRIPT: &str = r#"import json
import os
import sys

if sys.argv[2:] == ["leave-group"]:
    os.setsid()

with open(sys.argv[1], "w") as pid_file:
    pid_file.write(str(os.getpid()))

for line in sys.stdin:
    try:
        request = json.loads(line)
    except ValueError:
        continue
    ident = request.get("id")
    if ident is None:
        continue
    method = request.get("method")
    if method == "initialize":
        asked = (request.get("params") or {}).get("protocolVersion", "2024-11-05")
        result = {
            "protocolVersion": asked,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "cf-quiet", "version": "1"},
        }
    elif method == "tools/list":
        result = {"tools": []}
    elif method == "ping":
        result = {}
    else:
        answer = {"jsonrpc": "2.0", "id": ident, "error": {"code": -32601, "message": "no such method"}}
        print(json.dumps(answer), flush=True)
        continue
    print(json.dumps({"jsonrpc": "2.0", "id": ident, "result": result}), flush=True)
"#;

/// The server written in a folder: its program, the configuration Claude is
/// given it by (`--mcp-config`), and the file its process id is written to.
pub struct Server {
    config: PathBuf,
    pid_file: PathBuf,
}

impl Server {
    /// Writes the server's program and its configuration in `folder`, run by the
    /// interpreter `python`.
    pub fn write(folder: &Path, python: &Path) -> Result<Self> {
        Self::write_as(folder, python, &[(NAME, STEM, false)])
    }

    /// Writes the server, and one more that leaves the window's process group as it
    /// starts (`setsid`), as a server or helper of an owner's may, both in one
    /// configuration: the server returned is the one that stays in the group, and
    /// [`Server::detached_pid`] says the other's process.
    pub fn write_leaving_group(folder: &Path, python: &Path) -> Result<Self> {
        let server = Self::write_as(
            folder,
            python,
            &[(NAME, STEM, false), (DETACHED_NAME, DETACHED_STEM, true)],
        )?;
        let _ = std::fs::remove_file(folder.join(format!("{DETACHED_STEM}.pid")));
        Ok(server)
    }

    /// Writes the program and one configuration of `servers` (each its name, the stem of
    /// its process id's file, and whether it leaves the group) in `folder`: the first is
    /// the server returned.
    fn write_as(folder: &Path, python: &Path, servers: &[(&str, &str, bool)]) -> Result<Self> {
        let script = folder.join(format!("{STEM}.py"));
        let config = folder.join("mcp.json");
        let pid_file = folder.join(format!("{}.pid", servers[0].1));
        files::write(&script, SCRIPT)?;
        let entries: Vec<(&str, PathBuf, bool)> = servers
            .iter()
            .map(|(name, stem, leaves)| (*name, folder.join(format!("{stem}.pid")), *leaves))
            .collect();
        files::write(
            &config,
            format!("{}\n", configuration(python, &script, &entries)),
        )?;
        let server = Self { config, pid_file };
        // A server of an earlier run has left its file, and says nothing of this one.
        server.forget();
        Ok(server)
    }

    /// Takes away the process id the server wrote: whatever wrote it is no longer
    /// the one the run looks for.
    pub fn forget(&self) {
        let _ = std::fs::remove_file(&self.pid_file);
    }

    /// The configuration Claude Code is given the server by.
    pub fn config(&self) -> &Path {
        &self.config
    }

    /// Its process, once it has written it: a server Claude has not started yet
    /// has none.
    pub fn pid(&self) -> Option<u32> {
        files::read_string(&self.pid_file).ok()?.trim().parse().ok()
    }

    /// Whether its process is there now.
    pub fn runs(&self) -> bool {
        self.pid().is_some_and(is_alive)
    }

    /// The process of the server written beside it that left the window's group, once it
    /// has written it ([`Server::write_leaving_group`]).
    pub fn detached_pid(&self) -> Option<u32> {
        let file = self.pid_file.with_file_name(format!("{DETACHED_STEM}.pid"));
        files::read_string(&file).ok()?.trim().parse().ok()
    }

    /// The server as one process: its number and when that process began, as the
    /// system says it. None until it has written its process id and while it is not
    /// there; the same process gives the same words, and one that took its number,
    /// others.
    pub fn identity(&self) -> Option<String> {
        let pid = self.pid()?;
        Some(format!("{pid} {}", started(pid)?))
    }
}

/// What Claude Code reads of the servers `entries` (each its name, the file it writes its
/// process id to, and whether it leaves the group): the program and its words.
fn configuration(python: &Path, script: &Path, entries: &[(&str, PathBuf, bool)]) -> String {
    let mut servers = serde_json::Map::new();
    for (name, pid_file, leaves) in entries {
        let mut args = vec![json!(script), json!(pid_file)];
        if *leaves {
            args.push(json!("leave-group"));
        }
        servers.insert(
            (*name).to_owned(),
            json!({ "type": "stdio", "command": python, "args": args }),
        );
    }
    json!({ "mcpServers": servers }).to_string()
}

#[cfg(test)]
mod tests {
    use cf_e2e::live;
    use cf_e2e::process::Run;
    use serde_json::Value;

    use super::*;

    #[test]
    fn claude_is_given_one_stdio_server_by_its_program_and_the_file_its_process_is_written_to() {
        let said: Value = serde_json::from_str(&configuration(
            Path::new("/usr/bin/python3"),
            Path::new("/run/quiet-server.py"),
            &[(NAME, PathBuf::from("/run/quiet-server.pid"), false)],
        ))
        .unwrap();
        assert_eq!(
            said,
            json!({
                "mcpServers": {
                    "cf-quiet": {
                        "type": "stdio",
                        "command": "/usr/bin/python3",
                        "args": ["/run/quiet-server.py", "/run/quiet-server.pid"],
                    }
                }
            })
        );
    }

    #[test]
    fn the_server_that_leaves_the_group_is_told_so_beside_the_one_that_stays() {
        let folder = tempfile::tempdir().unwrap();
        let server = Server::write_leaving_group(folder.path(), Path::new("python3")).unwrap();
        let said: Value =
            serde_json::from_str(&std::fs::read_to_string(server.config()).unwrap()).unwrap();
        let args = |name: &str| said["mcpServers"][name]["args"].clone();
        let pid_file = |stem: &str| folder.path().join(format!("{stem}.pid"));
        assert_eq!(
            args(NAME),
            json!([folder.path().join("quiet-server.py"), pid_file(STEM)])
        );
        assert_eq!(
            args(DETACHED_NAME),
            json!([
                folder.path().join("quiet-server.py"),
                pid_file(DETACHED_STEM),
                "leave-group"
            ])
        );
        assert_eq!(server.detached_pid(), None);
        std::fs::write(pid_file(DETACHED_STEM), "4244").unwrap();
        assert_eq!(server.detached_pid(), Some(4244));
        assert_eq!(server.pid(), None, "the one that stays has its own file");
    }

    #[test]
    fn a_server_that_has_not_started_has_no_process_and_is_not_running() {
        let folder = tempfile::tempdir().unwrap();
        let server = Server::write(folder.path(), Path::new("python3")).unwrap();
        assert_eq!(server.pid(), None);
        assert!(!server.runs());
        assert!(server.config().is_file());
        // What an earlier run's server left is taken away: it says nothing of this run.
        std::fs::write(folder.path().join("quiet-server.pid"), "4242").unwrap();
        let again = Server::write(folder.path(), Path::new("python3")).unwrap();
        assert_eq!(again.pid(), None);
        // And so is what a window wrote that the run did not mean to look at.
        std::fs::write(folder.path().join("quiet-server.pid"), "4243").unwrap();
        assert_eq!(again.pid(), Some(4243));
        again.forget();
        assert_eq!(again.pid(), None);
    }

    /// What the server answers to `lines`, one request a line, and the process it
    /// wrote: none where this machine has no `python3` that runs (a Windows machine
    /// may have only the Store's alias of one).
    fn answers(lines: &[&str]) -> Option<(Vec<Value>, String)> {
        let python = live::working_command("python3", &["--version"]).ok()?;
        let folder = tempfile::tempdir().unwrap();
        let server = Server::write(folder.path(), &python).unwrap();
        let script = folder.path().join("quiet-server.py");
        let ran = Run::new(python)
            .arg(script)
            .arg(folder.path().join("quiet-server.pid"))
            .inheriting_env()
            .input(format!("{}\n", lines.join("\n")))
            .run()
            .unwrap();
        assert_eq!(ran.code, Some(0), "it ends with its input: {ran}");
        let said = ran
            .stdout
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let pid = files::read_string(&folder.path().join("quiet-server.pid")).unwrap();
        assert_eq!(server.pid(), pid.trim().parse().ok());
        Some((said, pid))
    }

    #[test]
    fn it_answers_what_claude_asks_to_list_it_with_the_version_it_asked_for_and_no_tool() {
        let Some((said, pid)) = answers(&[
            r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"claude-code","version":"9"}}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
            "not json at all",
            r#"{"jsonrpc":"2.0","id":3,"method":"resources/list"}"#,
        ]) else {
            return;
        };
        // The notification and the line that was no request are not answered.
        assert_eq!(said.len(), 4, "{said:?}");
        assert_eq!(said[0]["id"], 0);
        assert_eq!(said[0]["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(said[0]["result"]["capabilities"], json!({ "tools": {} }));
        assert_eq!(said[0]["result"]["serverInfo"]["name"], NAME);
        assert_eq!(
            said[1],
            json!({ "jsonrpc": "2.0", "id": 1, "result": { "tools": [] } })
        );
        assert_eq!(said[2], json!({ "jsonrpc": "2.0", "id": 2, "result": {} }));
        assert_eq!(said[3]["id"], 3);
        assert_eq!(said[3]["error"]["code"], -32601);
        assert!(pid.trim().parse::<u32>().is_ok(), "{pid:?}");
    }
}

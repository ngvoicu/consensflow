//! Codex, as a live test starts it itself: with every MCP server it would
//! start switched off, and, where a test needs to see what Codex says on the
//! wire, its app-server driven over its standard streams. (A Codex home of a run's
//! own, holding a copy of the machine's login, is the kit's for every window:
//! [`super::isolation`].)
//!
//! The MCP servers are the human's (a plugin, the ChatGPT app and `config.toml`
//! each add some), and a window that runs unattended in full-permission mode on
//! their setup would reach them. The product leaves them all on for every agent;
//! test runs are the only place they are shut out ([`isolation`]).

use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::process::{Run, Sink, Spawned};
use crate::wire::{self, Pending};
use crate::{Error, Result};

/// The MCP servers Codex would start, as it lists them (`codex mcp list
/// --json`): each with its `name` and, in a Codex that says it, its `transport`.
pub fn mcp_servers(codex: &Path, env: &[(String, String)]) -> Result<Value> {
    let (home, safe) = mcp_environment(env)?;
    Run::new(codex)
        .args(["mcp", "list", "--json"])
        .vars(safe)
        .cwd(home)
        .run()?
        .json()
}

/// A Codex listing gets no caller profile or config, even when launched outside a rig.
fn mcp_environment(
    env: &[(String, String)],
) -> Result<(std::path::PathBuf, Vec<(String, String)>)> {
    let same_name = |a: &str, b: &str| {
        if cfg!(windows) {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    };
    let home = env
        .iter()
        .rev()
        .find(|(name, _)| same_name(name, "CODEX_HOME"))
        .map(|(_, value)| value)
        .filter(|value| !value.is_empty())
        .map(Path::new)
        .ok_or_else(|| Error::Daemon("the run gives Codex no CODEX_HOME".to_owned()))?;
    if !crate::door::is_made_or_below(home) {
        return Err(Error::Daemon(
            "Codex's home is outside the run's own roots".to_owned(),
        ));
    }
    let mut safe: Vec<(String, String)> =
        ["PATH", "PATHEXT", "SYSTEMROOT", "WINDIR", "COMSPEC", "LANG"]
            .into_iter()
            .filter_map(|name| {
                env.iter()
                    .rev()
                    .find(|(key, _)| same_name(key, name))
                    .map(|(_, value)| value.clone())
                    .or_else(|| crate::process::own_var(name))
                    .map(|value| (name.to_owned(), value))
            })
            .collect();
    for name in [
        "HOME",
        "USERPROFILE",
        "CODEX_HOME",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
        "APPDATA",
        "LOCALAPPDATA",
    ] {
        safe.push((name.to_owned(), home.to_string_lossy().into_owned()));
    }
    // Run adds Windows' required process variables when absent. Override both parts of
    // the Windows home as well, so neither can point back at the caller's profile.
    let home_text = home.to_string_lossy();
    let (drive, path) = if home_text.as_bytes().get(1) == Some(&b':') {
        home_text.split_at(2)
    } else {
        ("", home_text.as_ref())
    };
    safe.push(("HOMEDRIVE".to_owned(), drive.to_owned()));
    safe.push(("HOMEPATH".to_owned(), path.to_owned()));
    Ok((home.to_path_buf(), safe))
}

/// The `-c` overrides that switch off every MCP server of `servers` (the answer
/// of [`mcp_servers`]): each gets a harmless, disabled definition, which also
/// covers a server a plugin or the ChatGPT app adds outside `config.toml` (a
/// bare `enabled=false` is refused for those).
///
/// The definition is in the form of the server's own transport: Codex refuses a
/// command on a server it reaches by URL ("url is not supported for stdio"), its
/// app-server exits and the window closes at once (Codex 0.160.1, 2026-10-06), so
/// such a server gets a URL, the discard port on loopback, which nothing answers.
/// Codex lists `stdio` and `streamable_http`: a transport of another type is taken
/// for a URL's, and a server with none (an older Codex) for a command's.
///
/// A name `-c` would read as more than one key (it takes the key's segments
/// literally: a quoted name would define a new server and leave the real one on)
/// is refused: a server is never half isolated.
pub fn isolation(servers: &Value) -> Result<Vec<String>> {
    let servers = servers.as_array().ok_or_else(|| {
        Error::Daemon(format!(
            "codex mcp list --json did not answer a list of servers: {servers}"
        ))
    })?;
    let mut flags = Vec::new();
    for server in servers {
        let name = server["name"].as_str().unwrap_or_default();
        let plain = !name.is_empty()
            && name
                .chars()
                .all(|at| at.is_ascii_alphanumeric() || at == '_' || at == '-');
        if !plain {
            return Err(Error::Daemon(format!(
                "cannot switch off the Codex MCP server {}",
                server["name"]
            )));
        }
        let definition = match server["transport"]["type"].as_str() {
            Some(kind) if kind != "stdio" => r#"url="http://127.0.0.1:9/disabled""#,
            _ => r#"command="/usr/bin/true""#,
        };
        for setting in [definition, "enabled=false"] {
            flags.push("-c".to_owned());
            flags.push(format!("mcp_servers.{name}.{setting}"));
        }
    }
    Ok(flags)
}

/// A message of the app-server's that was no answer to a request of ours: a
/// notification, or a request of its own, as it arrived.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    /// How long after the app-server started it arrived.
    pub at: Duration,
    pub body: Value,
}

/// Codex's app-server, driven over its standard streams: JSON-RPC, a message to
/// a line. A request is answered by the response of its id; everything else it
/// says is kept, in order, for the test to look at when it chooses.
pub struct AppServer {
    sink: Sink,
    pending: Pending,
    inbox: Arc<Mutex<Vec<Message>>>,
    next_id: AtomicI64,
    started: Instant,
    // Its process: ended when this is.
    _process: Spawned,
}

impl AppServer {
    /// Starts the app-server `run` is (`codex app-server …`, in the folder it is
    /// to work in).
    pub fn start(run: Run) -> Result<Self> {
        let mut process = run.spawn()?;
        let missing = || Error::Daemon("the app-server has no stream to speak on".to_owned());
        let output = process.take_output().ok_or_else(missing)?;
        let sink = process.sink().ok_or_else(missing)?;
        let (pending, inbox) = (Pending::default(), Arc::new(Mutex::new(Vec::new())));
        let started = Instant::now();
        {
            let (pending, inbox) = (pending.clone(), Arc::clone(&inbox));
            wire::read_each(
                output,
                move |line| {
                    let Ok(body) = serde_json::from_str::<Value>(&line) else {
                        return true;
                    };
                    if body.get("method").is_none() && body.get("id").is_some() {
                        // The answer to a request of ours (to one nobody waits for, nothing).
                        pending.answer(&key_of(&body["id"]), &body);
                    } else if body.get("method").is_some() {
                        inbox
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .push(Message {
                                at: started.elapsed(),
                                body,
                            });
                    }
                    true
                },
                || {},
            );
        }
        Ok(Self {
            sink,
            pending,
            inbox,
            next_id: AtomicI64::new(1),
            started,
            _process: process,
        })
    }

    /// Asks `method` with `params`, and waits up to `within` for its answer: the
    /// `result` of it, or the error it was answered with.
    pub fn request(&self, method: &str, params: &Value, within: Duration) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let answer = self.pending.expect(&id.to_string());
        self.sink.send(format!(
            "{}\n",
            json!({ "id": id, "method": method, "params": params })
        ));
        let response = answer.recv_timeout(within).map_err(|_| {
            self.pending.forget(&id.to_string());
            Error::Timeout(format!(
                "the app-server did not answer {method} in {} s",
                within.as_secs()
            ))
        })?;
        match response.get("error") {
            Some(error) => Err(Error::Daemon(format!("{method}: {error}"))),
            None => Ok(response["result"].clone()),
        }
    }

    /// Tells the app-server `method`, which asks nothing of it (`initialized`).
    pub fn notify(&self, method: &str) {
        self.sink.send(format!("{}\n", json!({ "method": method })));
    }

    /// How many messages it has said so far: a mark to ask what came after.
    pub fn mark(&self) -> usize {
        self.inbox
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// What it has said since the mark `from`, in order.
    pub fn since(&self, from: usize) -> Vec<Message> {
        self.inbox
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .skip(from)
            .cloned()
            .collect()
    }

    /// The first message since the mark `from` that `matches` says yes to,
    /// waited for up to `within`: none if none came.
    pub fn next(
        &self,
        from: usize,
        matches: impl Fn(&Value) -> bool,
        within: Duration,
    ) -> Option<Message> {
        super::until(within, Duration::from_millis(20), || {
            self.since(from)
                .into_iter()
                .find(|message| matches(&message.body))
        })
    }

    /// How long ago it started.
    pub fn age(&self) -> Duration {
        self.started.elapsed()
    }
}

/// The id of a message as the key its request waits under.
fn key_of(id: &Value) -> String {
    id.as_i64()
        .map_or_else(|| super::text(id), |number| number.to_string())
}

/// The messages of `messages`, by the method each says, in order: for a line
/// that says what came and when.
pub fn methods(messages: &[Message]) -> Vec<String> {
    messages
        .iter()
        .map(|message| super::text(&message.body["method"]))
        .collect()
}

/// What a Codex window's broker says Codex keeps running for its thread (`GET /terminals`,
/// which asks Codex's server for its background terminals, T-1137), as the daemon asks it
/// before it closes a window that did not stop: the broker found by the bridge in the
/// window's `pane.open` (`open`, its environment's `CF_CODEX_SESSION_BRIDGE`). Each by its
/// command line; an error where the window is no Codex's, or its broker gave no list.
pub fn kept_commands(open: &Value) -> Result<Vec<String>> {
    // `cf_proto::codex::BRIDGE_VARIABLE`, which the kit's library does not depend on.
    let bridge = open["env"]["CF_CODEX_SESSION_BRIDGE"]
        .as_str()
        .ok_or_else(|| Error::Daemon("the window was opened with no Codex bridge".to_owned()))?;
    let bridge: Value = serde_json::from_str(bridge).map_err(|source| Error::Json {
        text: bridge.to_owned(),
        source,
    })?;
    let (port, token) = (bridge["port"].as_u64(), bridge["token"].as_str());
    let (Some(port), Some(token)) = (port, token) else {
        return Err(Error::Daemon(format!(
            "a Codex bridge with no port or token: {bridge}"
        )));
    };
    let reply =
        crate::http::Http::new().get(&format!("http://127.0.0.1:{port}/terminals"), Some(token))?;
    let said = reply.json()?;
    said["commands"]
        .as_array()
        .and_then(|commands| {
            commands
                .iter()
                .map(|command| command.as_str().map(str::to_owned))
                .collect()
        })
        .ok_or_else(|| {
            Error::Daemon(format!(
                "the broker gave no list ({}): {said}",
                reply.status
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kit_disclosure_codex_mcp_environment_has_only_the_run_home() {
        let run = crate::door::made("cf-codex-env-").unwrap();
        let foreign = tempfile::tempdir().unwrap();
        let home = run.path().join("codex");
        let env = vec![
            ("HOME".to_owned(), foreign.path().display().to_string()),
            (
                "USERPROFILE".to_owned(),
                foreign.path().display().to_string(),
            ),
            (
                "XDG_CONFIG_HOME".to_owned(),
                foreign.path().display().to_string(),
            ),
            ("CODEX_HOME".to_owned(), home.display().to_string()),
            ("FOREIGN_SETTING".to_owned(), "not inherited".to_owned()),
        ];
        let (chosen, safe) = mcp_environment(&env).unwrap();
        assert_eq!(chosen, home);
        for name in [
            "HOME",
            "USERPROFILE",
            "CODEX_HOME",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "APPDATA",
            "LOCALAPPDATA",
        ] {
            assert_eq!(
                safe.iter().find(|(key, _)| key == name).unwrap().1,
                home.to_string_lossy()
            );
        }
        assert!(!safe.iter().any(|(name, _)| name == "FOREIGN_SETTING"));
        let get = |name: &str| safe.iter().find(|(key, _)| key == name).unwrap().1.clone();
        assert_eq!(
            format!("{}{}", get("HOMEDRIVE"), get("HOMEPATH")),
            home.to_string_lossy()
        );
        assert!(mcp_environment(&[]).is_err());
        assert!(mcp_environment(&[(
            "CODEX_HOME".to_owned(),
            foreign.path().display().to_string()
        )])
        .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn kit_disclosure_codex_mcp_uses_the_runs_home() {
        let run = crate::door::made("cf-codex-mcp-").unwrap();
        let program = run.path().join("fake-codex");
        let home = run.path().join("codex");
        std::fs::create_dir_all(&home).unwrap();
        crate::door::write_executable(&program, format!("#!/bin/sh\nif [ \"$CODEX_HOME\" = '{}' ]; then printf '[true]'; else printf '[false]'; fi\n", home.display())).unwrap();
        let env = vec![("CODEX_HOME".to_owned(), home.display().to_string())];
        assert_eq!(
            mcp_servers(&program, &env).unwrap(),
            json!([true]),
            "the listing did not use the run's Codex home"
        );
        assert!(mcp_servers(&program, &[]).is_err());
        let foreign = tempfile::tempdir().unwrap();
        assert!(mcp_servers(
            &program,
            &[(
                "CODEX_HOME".to_owned(),
                foreign.path().display().to_string()
            )]
        )
        .is_err());
    }

    fn flags(servers: Value) -> Vec<String> {
        isolation(&servers).unwrap()
    }

    #[test]
    fn a_server_started_by_a_command_gets_a_disabled_command_and_is_switched_off() {
        assert_eq!(
            flags(json!([{ "name": "files", "transport": { "type": "stdio" } }])),
            [
                "-c",
                r#"mcp_servers.files.command="/usr/bin/true""#,
                "-c",
                "mcp_servers.files.enabled=false"
            ]
        );
        // An older Codex says no transport, and a command is what it starts.
        assert_eq!(
            flags(json!([{ "name": "old" }])),
            [
                "-c",
                r#"mcp_servers.old.command="/usr/bin/true""#,
                "-c",
                "mcp_servers.old.enabled=false"
            ]
        );
    }

    #[test]
    fn a_server_reached_by_a_url_gets_a_url_on_the_discard_port_never_a_command() {
        // A command on such a server kills Codex 0.160.1's app-server, and with it the window.
        for kind in ["streamable_http", "sse", "websocket"] {
            let set = flags(json!([{ "name": "chatgpt-app", "transport": { "type": kind } }]));
            assert_eq!(
                set[1], r#"mcp_servers.chatgpt-app.url="http://127.0.0.1:9/disabled""#,
                "{kind}"
            );
            assert_eq!(set[3], "mcp_servers.chatgpt-app.enabled=false", "{kind}");
            assert!(!set.iter().any(|flag| flag.contains("command=")), "{kind}");
        }
    }

    #[test]
    fn every_server_listed_is_switched_off_in_the_order_listed_and_none_is_none_to_do() {
        let set = flags(json!([
            { "name": "a", "transport": { "type": "stdio" } },
            { "name": "b_2", "transport": { "type": "streamable_http" } },
        ]));
        assert_eq!(set.len(), 8);
        assert!(set[1].starts_with("mcp_servers.a.") && set[5].starts_with("mcp_servers.b_2."));
        assert!(flags(json!([])).is_empty());
    }

    #[test]
    fn a_server_that_cannot_be_switched_off_whole_is_refused_and_so_is_a_list_that_is_none() {
        // `-c` takes the segments of a key literally: a name with a dot or a quote would
        // define a new server and leave the real one on.
        for name in ["a.b", "has space", "quo\"te", "", "a/b"] {
            let failed = isolation(&json!([{ "name": name }])).unwrap_err();
            assert!(
                failed
                    .to_string()
                    .starts_with("cannot switch off the Codex MCP server"),
                "{name:?}: {failed}"
            );
        }
        assert!(isolation(&json!([{ "transport": {} }])).is_err());
        let failed = isolation(&json!({ "error": "no" })).unwrap_err();
        assert!(
            failed.to_string().contains("did not answer a list"),
            "{failed}"
        );
    }

    #[test]
    fn the_methods_a_server_said_are_listed_in_the_order_it_said_them() {
        let say = |method: &str| Message {
            at: Duration::ZERO,
            body: json!({ "method": method }),
        };
        assert_eq!(methods(&[say("a/b"), say("c")]), ["a/b", "c"]);
        assert!(methods(&[]).is_empty());
    }

    /// An app-server that is a shell: it answers `initialize`, answers an error
    /// to `boom`, and to `ping` says something of its own first.
    #[cfg(unix)]
    const SERVER: &str = r#"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*) printf '%s\n' '{"id":1,"result":{"userAgent":"fake"}}' ;;
    *'"method":"boom"'*) printf '%s\n' '{"id":2,"error":{"code":-1,"message":"it broke"}}' ;;
    *'"method":"ping"'*) printf '%s\n' '{"method":"thing/happened","params":{"threadId":"t1"}}' '{"id":3,"result":{"pong":true}}' ;;
    *'"method":"initialized"'*) printf '%s\n' '{"method":"ready","params":{}}' ;;
  esac
done
"#;

    #[cfg(unix)]
    fn server() -> AppServer {
        AppServer::start(Run::new("/bin/sh").args(["-c", SERVER])).unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn a_request_is_answered_by_the_result_of_its_id_or_by_the_error_it_was_answered_with() {
        let server = server();
        let wait = Duration::from_secs(20);
        assert_eq!(
            server.request("initialize", &json!({}), wait).unwrap(),
            json!({ "userAgent": "fake" })
        );
        let failed = server.request("boom", &json!({}), wait).unwrap_err();
        assert_eq!(
            failed.to_string(),
            r#"boom: {"code":-1,"message":"it broke"}"#
        );
        // What it says besides is kept, in order, and is no answer.
        let mark = server.mark();
        assert_eq!(
            server.request("ping", &json!({}), wait).unwrap(),
            json!({ "pong": true })
        );
        let said = server.since(mark);
        assert_eq!(methods(&said), ["thing/happened"]);
        assert_eq!(said[0].body["params"]["threadId"], "t1");
    }

    #[cfg(unix)]
    #[test]
    fn what_the_server_says_is_waited_for_from_a_mark_and_what_came_before_it_is_not_looked_at() {
        let server = server();
        let wait = Duration::from_secs(20);
        server.notify("initialized");
        let ready = server.next(0, |message| message["method"] == "ready", wait);
        assert!(ready.is_some());
        let mark = server.mark();
        // Nothing of that kind comes after the mark.
        let none = server.next(
            mark,
            |message| message["method"] == "ready",
            Duration::from_millis(200),
        );
        assert_eq!(none, None);
        assert!(server.age() > Duration::ZERO);
    }

    #[cfg(unix)]
    #[test]
    fn a_request_the_server_does_not_answer_is_a_timeout_that_names_it() {
        let server = AppServer::start(Run::new("/bin/sh").args(["-c", "cat > /dev/null"])).unwrap();
        let failed = server
            .request("thread/start", &json!({}), Duration::from_millis(200))
            .unwrap_err();
        assert!(matches!(failed, Error::Timeout(_)), "{failed}");
        assert!(failed.to_string().contains("thread/start"), "{failed}");
    }
}

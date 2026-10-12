//! The stand-in Codex itself, where the cases that run it do not reach. Its server
//! on loopback, the transport Windows' supervisor uses (on Unix it listens on a
//! private socket, which the supervisor's cases cover): a bearer token whose
//! SHA-256 it was given gets in, and any other, or none, is turned away at the
//! handshake. And what a window the daemon opens runs on (the regressions stand on
//! it): what the daemon asks of the CLI before it opens one, the plugin servers the
//! window opens with, and the turns its server takes and the rollout it writes them in.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use cf_e2e::files;
use cf_e2e::process::{Run, Spawned};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tungstenite::client::IntoClientRequest;
use tungstenite::http::HeaderValue;
use tungstenite::Message;

#[cfg(unix)]
use crate::stand_ins::{beside, until_there};
use crate::Outcome;

const FAKE_CODEX: &str = env!("CARGO_BIN_EXE_fake-codex");

/// The token the stand-in's server lets in, and the hash it is given.
const TOKEN: &str = "capability-token-for-the-test";

/// The request to connect to `port` with `authorization` as the header, if any.
fn request_to(
    port: u16,
    authorization: Option<&str>,
) -> Result<tungstenite::handshake::client::Request, Box<dyn std::error::Error>> {
    let mut request = format!("ws://127.0.0.1:{port}").into_client_request()?;
    if let Some(value) = authorization {
        request
            .headers_mut()
            .insert("authorization", HeaderValue::from_str(value)?);
    }
    Ok(request)
}

/// The port the stand-in Codex's server on loopback took, which it says on its
/// error output.
fn listening_on(server: &Spawned) -> u16 {
    let started = Instant::now();
    loop {
        let said = server.errors();
        if let Some(port) = said
            .split("listening on: ws://127.0.0.1:")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|port| port.parse::<u16>().ok())
        {
            return port;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "no port: {said}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_stand_in_codexs_server_on_loopback_lets_in_only_the_token_whose_hash_it_was_given() -> Outcome
{
    let _turn = cf_e2e::serial::turn();
    let root = tempfile::tempdir()?;
    let log = root.path().join("codex.jsonl");
    let hash: String = Sha256::digest(TOKEN.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let server = Run::new(FAKE_CODEX)
        .args(["app-server", "--listen", "ws://127.0.0.1:0"])
        .args(["--ws-auth", "capability-token", "--ws-token-sha256", &hash])
        .var("CF_TEST_CODEX_LOG", &log)
        .closed_input()
        .spawn()?;
    let port = listening_on(&server);

    // The token it was given the hash of gets in, and starts a thread.
    let (mut socket, _) =
        tungstenite::connect(request_to(port, Some(&format!("Bearer {TOKEN}")))?)?;
    socket.send(Message::text(
        json!({ "id": 7, "method": "thread/start", "params": { "ephemeral": false } }).to_string(),
    ))?;
    let Message::Text(answer) = socket.read()? else {
        panic!("the server did not answer with text");
    };
    let answer: Value = serde_json::from_str(answer.as_str())?;
    assert_eq!(answer["id"], 7);
    assert_eq!(
        answer["result"]["thread"],
        json!({ "id": "01a09094-938f-7fd1-a2d3-315cf92b4559", "turns": [], "status": { "type": "idle" } })
    );
    let log = files::read_string(&log)?;
    assert!(log.contains("\"started\":{\"ephemeral\":false}"), "{log}");

    // Any other token, a token that is not a bearer's, and none are turned away.
    for authorization in [
        Some("Bearer another-token"),
        Some("Basic abc"),
        Some(TOKEN),
        None,
    ] {
        let refused = tungstenite::connect(request_to(port, authorization)?);
        assert!(
            matches!(&refused, Err(tungstenite::Error::Http(response)) if response.status() == 401),
            "{authorization:?}: {:?}",
            refused.err()
        );
    }
    Ok(())
}

/// What the daemon asks of a Codex before it opens a window on it: whether its CLI
/// has the native queue, and what its own instructions are.
#[test]
fn the_stand_in_codex_answers_what_the_daemon_asks_of_a_codex_before_it_opens_a_window() -> Outcome
{
    let help = Run::new(FAKE_CODEX).args(["queue", "--help"]).run()?;
    assert_eq!(help.code, Some(0), "{help}");
    for flag in ["--thread", "--message"] {
        assert!(help.stdout.contains(flag), "{help}");
    }

    // `codex app-server` on its standard input and output: initialized, then asked
    // for its configuration. It answers each request with a line, and ends with its input.
    let asked = Run::new(FAKE_CODEX)
        .arg("app-server")
        .input(concat!(
            r#"{"id":1,"method":"initialize","params":{}}"#,
            "\n",
            r#"{"method":"initialized","params":{}}"#,
            "\n",
            r#"{"id":2,"method":"config/read","params":{"cwd":"/work"}}"#,
            "\n",
        ))
        .run()?;
    let answers = asked
        .stdout
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<Vec<Value>, _>>()?;
    assert_eq!(answers.len(), 2, "{asked}");
    assert_eq!(answers[0], json!({ "id": 1, "result": {} }));
    assert_eq!(answers[1]["id"], 2);
    assert!(
        answers[1]["result"]["config"]["developer_instructions"].is_string(),
        "{asked}"
    );
    Ok(())
}

/// A plugin server Codex opens with its window: in a group of its own, and where a
/// tool is called it starts the workers it was told to, in that group, once.
#[cfg(unix)]
#[test]
fn the_stand_in_codexs_plugin_server_leads_a_group_of_its_own_and_starts_its_workers_when_a_tool_is_called(
) -> Outcome {
    use cf_e2e::process::{group_of, is_alive};

    let folder = tempfile::tempdir()?;
    let file = folder.path().join("plugin-1-1");
    let mut server = Run::new(FAKE_CODEX)
        .arg("--server")
        .arg(&file)
        .arg("2")
        .spawn()?;
    assert!(
        until_there(&beside(&file, "pid")),
        "the server says it runs"
    );
    assert_eq!(
        group_of(server.id()),
        Some(server.id()),
        "it leads a group of its own"
    );
    let worker = |number: u32| {
        folder
            .path()
            .join(format!("worker-{}-{number}", server.id()))
    };
    assert!(
        !beside(&worker(1), "pid").exists(),
        "none before a tool is called"
    );

    server.send("call\n");
    let mut started = Vec::new();
    for number in 1..=2 {
        assert!(
            until_there(&beside(&worker(number), "pid")),
            "worker {number} runs"
        );
        let pid: u32 = files::read_string(&beside(&worker(number), "pid"))?
            .trim()
            .parse()?;
        assert_eq!(
            group_of(pid),
            Some(server.id()),
            "it is in its server's group"
        );
        started.push(pid);
    }
    // A second call starts no more.
    server.send("call\n");
    thread::sleep(Duration::from_millis(300));
    assert!(!beside(&worker(3), "pid").exists());

    // Its input ends with the window, and what it started goes with it.
    server.end_input();
    server
        .wait(Duration::from_secs(10))?
        .ok_or("the server did not end with its input")?;
    let waited = Instant::now();
    while started.iter().any(|pid| is_alive(*pid)) && waited.elapsed() < Duration::from_secs(10) {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(started.iter().all(|pid| !is_alive(*pid)), "{started:?}");
    Ok(())
}

/// The rollout the daemon reads: a turn handed to the server is a thread's record,
/// in the folder of its day, from its start to the answer it is complete with.
#[test]
fn the_stand_in_codexs_server_takes_a_turn_and_writes_its_rollout_where_the_daemon_reads_it(
) -> Outcome {
    let _turn = cf_e2e::serial::turn();
    let root = tempfile::tempdir()?;
    let hash: String = Sha256::digest(TOKEN.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let server = Run::new(FAKE_CODEX)
        .args(["app-server", "--listen", "ws://127.0.0.1:0"])
        .args(["--ws-auth", "capability-token", "--ws-token-sha256", &hash])
        .var("CF_TEST_CODEX_LOG", root.path().join("codex.jsonl"))
        .var("CF_TEST_CODEX_THREADS", "unique")
        .var("CODEX_HOME", root.path())
        .closed_input()
        .spawn()?;
    let port = listening_on(&server);
    let (mut socket, _) =
        tungstenite::connect(request_to(port, Some(&format!("Bearer {TOKEN}")))?)?;
    let mut call =
        |id: u64, method: &str, params: Value| -> Result<Value, Box<dyn std::error::Error>> {
            socket.send(Message::text(
                json!({ "id": id, "method": method, "params": params }).to_string(),
            ))?;
            let Message::Text(answer) = socket.read()? else {
                panic!("the server did not answer with text");
            };
            Ok(serde_json::from_str(answer.as_str())?)
        };
    let started = call(1, "thread/start", json!({ "ephemeral": false }))?;
    let thread = started["result"]["thread"]["id"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert_ne!(
        thread, "01a09094-938f-7fd1-a2d3-315cf92b4559",
        "a thread of its own"
    );
    let input = json!([{
        "type": "text",
        "text": "[ConsensFlow m-4 · T-1 · task from @chief]\nSLEEP 1 Reply with exactly: DONE",
        "text_elements": [],
    }]);
    let handed = call(
        2,
        "turn/start",
        json!({ "threadId": thread, "input": input }),
    )?;
    assert!(handed["result"]["turn"].is_object(), "{handed}");

    // The rollout is in `sessions/YYYY/MM/DD/`, its name holding the thread, and it ends
    // with the turn complete once the message's seconds are worked.
    let rollout = loop_until(|| find_rollout(&root.path().join("sessions"), &thread))
        .ok_or("no rollout was written")?;
    let records = loop_until(|| {
        let records: Vec<Value> = files::read_string(&rollout)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        records
            .iter()
            .any(|record| record["payload"]["type"] == "task_complete")
            .then_some(records)
    })
    .ok_or("the turn never completed")?;
    let kinds: Vec<String> = records
        .iter()
        .map(|record| {
            format!(
                "{}/{}",
                record["type"].as_str().unwrap_or_default(),
                record["payload"]["type"].as_str().unwrap_or("-")
            )
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "session_meta/-",
            "event_msg/task_started",
            "response_item/message",
            "event_msg/item_completed",
            "event_msg/item_completed",
            "response_item/message",
            "event_msg/task_complete",
        ]
    );
    let last = &records[records.len() - 1]["payload"];
    assert_eq!(last["last_agent_message"], "DONE");
    assert_eq!(records[2]["payload"]["role"], "user");
    assert!(
        records[2]["payload"]["content"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("[ConsensFlow m-4")),
        "the message is in the record whole"
    );
    Ok(())
}

/// What `look` finds, looked for up to fifteen seconds.
fn loop_until<T>(mut look: impl FnMut() -> Option<T>) -> Option<T> {
    let started = Instant::now();
    loop {
        if let Some(found) = look() {
            return Some(found);
        }
        if started.elapsed() > Duration::from_secs(15) {
            return None;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// The rollout of `thread` under Codex's `sessions` folder: the first file whose
/// name holds it, as the daemon finds one.
fn find_rollout(folder: &Path, thread: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(folder).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_rollout(&path, thread) {
                return Some(found);
            }
        } else if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains(thread))
        {
            return Some(path);
        }
    }
    None
}

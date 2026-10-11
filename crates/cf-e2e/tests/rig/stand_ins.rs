//! The stand-ins themselves, where the cases that run them do not reach: the
//! stand-in Codex's server on loopback, the transport Windows' supervisor uses
//! (on Unix it listens on a private socket, which the supervisor's cases cover).
//! A bearer token whose SHA-256 it was given gets in, and any other, or none,
//! is turned away at the handshake. And the stand-in agent's part in how a
//! window closes (the cases of `background_work` hold what ConsensFlow does
//! with the work a window left running): the agent leaves when the pane host
//! asks it to end, and the job it leaves running writes its process, ends by
//! itself saying so, says it was asked to end where it is, and a deaf one is
//! only killed.

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

use crate::{Outcome, FAKE_AGENT};

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
    // It takes a port, and says which on its error output.
    let started = Instant::now();
    let port = loop {
        let said = server.errors();
        if let Some(port) = said
            .split("listening on: ws://127.0.0.1:")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|port| port.parse::<u16>().ok())
        {
            break port;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "no port: {said}"
        );
        thread::sleep(Duration::from_millis(20));
    };

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

/// A window the pane host closes: its stand-in agent is asked to end (TERM and
/// HUP to its group) and leaves by itself, at once and with the code 0, where
/// the host would otherwise wait out its grace and kill it. Every case of the
/// rig closes windows so, and the rig waits five seconds for its programs to
/// end, a grace of ten being the host's.
#[cfg(unix)]
#[test]
fn the_stand_in_agent_a_pane_host_closes_leaves_when_it_is_asked_to_end_and_is_not_killed(
) -> Outcome {
    use crate::secs;

    let rig = crate::rig()?;
    let opened = rig.host(
        "pane.open",
        json!({
            "id": "closed-1",
            "generation": 1,
            "cwd": "/tmp",
            "argv": [FAKE_AGENT, "--session-id", "closed-session"],
            "env": {},
            "size": { "rows": 24, "cols": 80 },
            "backlogBytes": 65536,
        }),
    )?;
    assert_eq!(opened["ok"], true, "{opened}");
    // It says it is idle once it has set itself up to be asked to end.
    let sessions = Path::new(rig.var("CLAUDE_CONFIG_DIR").unwrap_or_default()).join("sessions");
    rig.wait_for("the agent to be idle", secs(10), || {
        Ok(rig
            .started()
            .first()
            .is_some_and(|agent| sessions.join(format!("{}.json", agent.pid)).exists()))
    })?;

    let closing = Instant::now();
    let closed = rig.host("pane.kill", json!({ "id": "closed-1", "generation": 1 }))?;
    assert_eq!(closed["ok"], true, "{closed}");
    assert!(
        closing.elapsed() < secs(5),
        "the agent left when it was asked, and the close did not wait out its grace: {:?}",
        closing.elapsed()
    );
    rig.wait_for("the window's exit to be told", secs(10), || {
        Ok(!rig.exits().is_empty())
    })?;
    let exit = rig.exits().into_iter().next().unwrap_or(Value::Null);
    assert_eq!(exit["exitCode"], 0, "it left by itself: {exit}");
    assert!(exit["signal"].is_null(), "and was not killed: {exit}");
    rig.close()?;
    Ok(())
}

/// The job a window's stand-in agent leaves running, started as the agent starts
/// it: this program, `--background-job <seconds> <file>`, and `--deaf` if it
/// is to ignore the requests to end.
fn start_job(seconds: &str, file: &Path, deaf: bool) -> cf_e2e::Result<Spawned> {
    let mut run = Run::new(FAKE_AGENT)
        .arg("--background-job")
        .arg(seconds)
        .arg(file);
    if deaf {
        run = run.arg("--deaf");
    }
    run.closed_input().spawn()
}

/// The file of the kind `kind` the job `file` writes.
fn beside(file: &Path, kind: &str) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".");
    name.push(kind);
    PathBuf::from(name)
}

/// Waits until `file` is there, for up to ten seconds.
fn until_there(file: &Path) -> bool {
    let started = Instant::now();
    while !file.exists() && started.elapsed() < Duration::from_secs(10) {
        thread::sleep(Duration::from_millis(10));
    }
    file.exists()
}

#[test]
fn the_stand_in_agents_job_writes_its_process_runs_its_seconds_and_says_it_finished() -> Outcome {
    let folder = tempfile::tempdir()?;
    let file = folder.path().join("job-1-1");
    let mut job = start_job("1", &file, false)?;

    assert!(until_there(&beside(&file, "pid")), "the job says it runs");
    assert_eq!(
        files::read_string(&beside(&file, "pid"))?,
        job.id().to_string()
    );
    let ended = job
        .wait(Duration::from_secs(10))?
        .ok_or("the job did not end in ten seconds")?;
    assert!(ended.success(), "{ended:?}");
    assert!(beside(&file, "finished").exists());
    assert!(!beside(&file, "released").exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn the_stand_in_agents_job_asked_to_end_says_so_and_goes_and_a_deaf_one_is_only_killed() -> Outcome
{
    use cf_e2e::process::Signal;

    let folder = tempfile::tempdir()?;
    // Asked to end: it writes that it was, and goes with the code 0.
    let file = folder.path().join("job-1-1");
    let mut job = start_job("60", &file, false)?;
    assert!(until_there(&beside(&file, "pid")), "the job says it runs");
    job.signal(Signal::Terminate);
    let ended = job
        .wait(Duration::from_secs(10))?
        .ok_or("the job did not end when it was asked to")?;
    assert!(ended.success(), "{ended:?}");
    assert!(beside(&file, "released").exists());
    assert!(!beside(&file, "finished").exists());

    // A deaf one ignores the request, and the interrupt, and runs on until it is killed.
    let file = folder.path().join("job-1-2");
    let mut deaf = start_job("60", &file, true)?;
    assert!(until_there(&beside(&file, "pid")), "the job says it runs");
    deaf.signal(Signal::Terminate);
    deaf.signal(Signal::Interrupt);
    thread::sleep(Duration::from_secs(1));
    assert!(!deaf.has_exited(), "it ignored what asked it to end");
    deaf.kill();
    assert!(deaf.has_exited());
    assert!(!beside(&file, "released").exists());
    Ok(())
}

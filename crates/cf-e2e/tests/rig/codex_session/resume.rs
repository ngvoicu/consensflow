//! What the supervisor's window comes to when it is resumed on a thread, on the
//! stand-in Codex, which keeps what Codex keeps: a thread has a rollout once it
//! has had a turn, and a window resumed on one that has none ends at once, with
//! what Codex's server said (2026-10-13: a chief that was opened and closed
//! before its first turn, resumed, took its project with it).

use std::path::Path;

use cf_e2e::process::is_alive;
use cf_e2e::{files, serial, Result};
use serde_json::{json, Value};

use super::{of, pid, Supervised};
use crate::Outcome;

/// A thread whose rollout the home the case writes holds, as Codex keeps one: a
/// thread that had a turn.
const B: &str = "01a0a6ee-7d0c-7a5e-8a14-4b8f0f6f3f10";

/// A thread that never had a turn, which no home holds a rollout of.
const NEVER: &str = "01a0a6ee-0000-7000-8000-000000000000";

#[test]
fn a_window_resumed_on_a_thread_that_never_had_a_turn_ends_at_once_with_what_codex_said() -> Outcome
{
    let _turn = serial::turn();
    let home = tempfile::tempdir()?;
    // The home keeps the rollout of another thread, as a home that has seen Codex would.
    held_by(home.path(), B)?;
    let mut s = Supervised::start(
        &["resume", NEVER],
        &[("CODEX_HOME", home.path().to_string_lossy().into_owned())],
        None,
    )?;
    assert_eq!(s.exit()?, Some(1), "{}", s.stderr());
    let said = format!("no rollout found for thread id {NEVER}");
    assert!(s.stderr().contains(&said), "{}", s.stderr());
    let runs = s.runs();
    let (servers, tuis) = (of(&runs, "server"), of(&runs, "tui"));
    assert!(
        servers.iter().any(|run| run["resumed"] == NEVER),
        "the server was asked for the thread: {runs:?}"
    );
    assert!(
        tuis.iter().any(|run| run["refused"] == said.as_str()),
        "the TUI was refused it: {runs:?}"
    );
    assert!(
        servers.iter().all(|run| run.get("started").is_none()),
        "no thread was started in its place"
    );
    assert!(!tuis.iter().any(|run| run.get("session").is_some()));
    assert!(
        !is_alive(pid(&servers[0])),
        "the server went with the window"
    );
    Ok(())
}

#[test]
fn a_window_resumed_on_a_thread_that_had_a_turn_shows_it_and_the_broker_names_it() -> Outcome {
    let _turn = serial::turn();
    let home = tempfile::tempdir()?;
    held_by(home.path(), B)?;
    let mut s = Supervised::start(
        &["resume", B],
        &[("CODEX_HOME", home.path().to_string_lossy().into_owned())],
        None,
    )?;
    assert_eq!(s.exit()?, Some(7), "{}", s.stderr());
    let runs = s.runs();
    let (servers, tuis) = (of(&runs, "server"), of(&runs, "tui"));
    assert!(servers.iter().any(|run| run["resumed"] == B), "{runs:?}");
    let args: Vec<&str> = tuis[0]["args"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(
        args[4..],
        ["resume", B],
        "the TUI is given the resume after the options the broker adds"
    );
    assert_eq!(tuis[1]["session"]["sessionId"], B);
    assert_eq!(tuis[1]["session"]["available"], true);
    Ok(())
}

/// The rollout of `thread` in the Codex home `home`, as Codex keeps one: in the
/// folder of the day it was begun, its name ending with the thread.
fn held_by(home: &Path, thread: &str) -> Result {
    files::write(
        &home
            .join("sessions")
            .join("2026")
            .join("10")
            .join("13")
            .join(format!("rollout-2026-10-13T10-00-00-{thread}.jsonl")),
        format!(
            "{}\n",
            json!({ "type": "session_meta", "payload": { "id": thread } })
        ),
    )
}

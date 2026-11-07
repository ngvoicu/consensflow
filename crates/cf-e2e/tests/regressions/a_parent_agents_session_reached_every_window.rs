//! A ConsensFlow started from inside an agent's session handed that session to every
//! window (seen 2026-09-19 in a Claude Code session's environment; the issue
//! `inherited-claude-env-in-panes` in the brain; T-971).
//!
//! An agent's CLI marks its session in its own environment, which every program it
//! starts inherits: Claude Code's `CLAUDECODE`, `CLAUDE_CODE_SESSION_ID`,
//! `CLAUDE_CODE_CHILD_SESSION` and its messaging socket and token, Codex's
//! `CODEX_THREAD_ID`, Pi's `PI_SESSION_ID`, and the rest of each harness's; a terminal
//! marks its pane, as cmux does (`cf_proto::agents::parent_session_variables`). The app, a
//! `tauri dev` or a test run started from an agent's shell has them; the pane host starts each window from that environment, and
//! the login shell the app asks for a terminal's environment has them too, so they came
//! back named in `CONSENSFLOW_TERMINAL_ENV`, which the daemon gives every window. A
//! Claude window given `CLAUDE_CODE_CHILD_SESSION` saves no transcript, so no result can
//! be read of it; one given the parent's session id is taken for the parent. The app took
//! Claude's out of its own environment as it started, and no harness's else, and the
//! daemon and the pane host took none out at all.
//!
//! The rig plays such an app: the daemon and the pane host are started with every
//! harness's session variables, and the daemon is told they are what it took of a
//! terminal. What the owner should have: no window given one, in its `pane.open` or, where
//! the system shows a process's environment to its owner, in its own process.

use cf_e2e::rig::{Project, Rig};
use cf_e2e::Error;
use cf_proto::agents::parent_session_variables;
use serde_json::{json, Value};

use crate::a_workers_still_going_was_given_as_its_result::tier;
use crate::{config, secs, Outcome};

/// The variable the app names what it took in (`cf_base::env::TERMINAL_ENV`).
const TERMINAL_ENV: &str = "CONSENSFLOW_TERMINAL_ENV";

/// What the parent session's variables hold, which no window may.
const VALUE: &str = "the-parent-session-T971";

/// Every harness's session variables and a terminal's, as the build lists them: the one
/// list but for a board's window's, which make `cf ui` refuse to start, and which the
/// daemon gives each window of its own.
fn marked() -> Vec<&'static str> {
    parent_session_variables()
        .filter(|name| !name.starts_with("CONSENSFLOW_"))
        .collect()
}

/// The names a `pane.open` gives its window.
fn names(frame: &Value) -> Vec<String> {
    match &frame["env"] {
        Value::Object(map) => map.keys().cloned().collect(),
        Value::Array(pairs) => pairs
            .iter()
            .filter_map(|pair| pair[0].as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

#[test]
fn no_window_is_given_the_session_of_an_agent_the_app_was_started_from() -> Outcome {
    let marked = marked();
    let mut config = config();
    for name in &marked {
        config = config.var(name, VALUE);
    }
    let rig = Rig::start(config.daemon_var(TERMINAL_ENV, json!(marked).to_string()))?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let handle = added["member"]["handle"]
        .as_str()
        .ok_or_else(|| Error::Daemon(format!("the member has no handle: {added}")))?
        .to_owned();
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {} Read the parser", tier(&added)),
    )?;
    let worker = format!("p{}-{handle}", project.id());
    let is_worker = |frame: &Value| {
        frame["id"]
            .as_str()
            .is_some_and(|id| id.starts_with(&worker))
    };
    rig.wait_for("the worker's window to be opened", secs(60), || {
        Ok(rig.open_frames().iter().any(is_worker))
    })?;
    let chief = format!("p{}-chief", project.id());
    let frames: Vec<Value> = rig
        .open_frames()
        .into_iter()
        .filter(|frame| is_worker(frame) || frame["id"] == chief)
        .collect();
    assert!(
        frames.len() >= 2,
        "the chief's window and the worker's: {frames:?}"
    );
    for frame in &frames {
        let given: Vec<String> = names(frame)
            .into_iter()
            .filter(|name| marked.contains(&name.as_str()))
            .collect();
        assert!(
            given.is_empty(),
            "{}'s pane.open gives it the parent session's {given:?}",
            frame["id"]
        );
    }
    // Nor does its own process have one, where the system shows a process's environment.
    #[cfg(target_os = "macos")]
    {
        rig.wait_for("both windows' processes to start", secs(30), || {
            Ok(rig.started().len() >= 2)
        })?;
        for started in rig.started() {
            let shown = cf_e2e::process::Run::new("ps")
                .args(["eww", "-o", "command=", "-p", &started.pid.to_string()])
                .inheriting_env()
                .run()?
                .stdout;
            // What every window is given shows, so that a `ps` that shows no environment
            // (Darwin 27 shows none for the system's own programs) fails here, not passes.
            assert!(
                shown.contains("CONSENSFLOW_URL="),
                "ps shows no environment of the window's process (pid {}): {shown}",
                started.pid
            );
            let held: Vec<&str> = marked
                .iter()
                .copied()
                .filter(|name| shown.contains(&format!("{name}={VALUE}")))
                .collect();
            assert!(
                held.is_empty(),
                "the window's process (pid {}) has the parent session's {held:?}",
                started.pid
            );
        }
    }
    rig.close()?;
    Ok(())
}

/// The daemon's own programs, which it starts on its own environment and in no pane: the
/// probes it asks a Codex before it opens a window on it (`codex queue --help`, and its
/// `app-server`'s configuration), git, a harness's throwaway server. A `cf ui` started in an
/// agent's shell or a terminal's pane gave every one of them that session, as the app did
/// before it took them out of its own (T-1024; Zeus's note 4 on T-971). The rig plays such
/// a `cf ui`, a Codex chief its project's, and the stand-in Codex writes down the names of
/// every run's variables: none may be a session's.
#[cfg(unix)]
#[test]
fn no_program_the_daemon_starts_of_its_own_is_given_the_session_cf_ui_was_started_in() -> Outcome {
    let marked = marked();
    let folder = tempfile::tempdir()?;
    let names_log = folder.path().join("codex-names.jsonl");
    let mut config = crate::config_with_codex();
    for name in &marked {
        config = config.var(name, VALUE);
    }
    let rig = Rig::start(config.var("CF_TEST_CODEX_ENV_LOG", names_log.to_string_lossy()))?;
    let roster = json!({
        "schemaVersion": 1,
        "agents": [{ "id": "codex-chief", "kind": "codex", "model": "fake" }],
    });
    rig.stand_ins(&roster)?;
    let project = Project::open(&rig, "codex-chief", json!({}))?;
    let chief = format!("p{}-chief", project.id());
    rig.wait_for("the chief's window to be opened", secs(60), || {
        Ok(rig
            .open_frames()
            .iter()
            .any(|frame| frame["id"] == chief.as_str()))
    })?;
    let runs: Vec<Value> = std::fs::read_to_string(&names_log)?
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let probe = |run: &Value| {
        let args: Vec<&str> = run["args"]
            .as_array()
            .map(|args| args.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        args.first() == Some(&"queue")
            || (args.contains(&"app-server") && !args.contains(&"--listen"))
    };
    assert!(
        runs.iter().any(probe),
        "the daemon asked the stand-in Codex nothing before it opened the window: {runs:?}"
    );
    for run in &runs {
        let given: Vec<&str> = run["names"]
            .as_array()
            .map(|names| names.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let held: Vec<&str> = marked
            .iter()
            .copied()
            .filter(|name| given.contains(name))
            .collect();
        assert!(
            held.is_empty(),
            "the stand-in Codex run {} (probe: {}) was given the session's {held:?}",
            run["args"],
            probe(run)
        );
    }
    rig.close()?;
    Ok(())
}

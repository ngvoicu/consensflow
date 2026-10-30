//! A ConsensFlow started from inside an agent's session handed that session to every
//! window (seen 2026-09-19 in a Claude Code session's environment; the issue
//! `inherited-claude-env-in-panes` in the brain; T-971).
//!
//! An agent's CLI marks its session in its own environment, which every program it
//! starts inherits: Claude Code's `CLAUDECODE`, `CLAUDE_CODE_SESSION_ID`,
//! `CLAUDE_CODE_CHILD_SESSION` and its messaging socket and token, Codex's
//! `CODEX_THREAD_ID`, Pi's `PI_SESSION_ID`, and the rest of each harness's
//! ([`Harness::session_variables`]). The app, a `tauri dev` or a test run started from an
//! agent's shell has them; the pane host starts each window from that environment, and
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
use cf_proto::agents::Harness;
use serde_json::{json, Value};

use crate::a_workers_still_going_was_given_as_its_result::tier;
use crate::{config, secs, Outcome};

/// The variable the app names what it took in (`cf_base::env::TERMINAL_ENV`).
const TERMINAL_ENV: &str = "CONSENSFLOW_TERMINAL_ENV";

/// What the parent session's variables hold, which no window may.
const VALUE: &str = "the-parent-session-T971";

/// Every harness's session variables, as the build lists them.
fn marked() -> Vec<&'static str> {
    Harness::ALL
        .into_iter()
        .flat_map(Harness::session_variables)
        .copied()
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

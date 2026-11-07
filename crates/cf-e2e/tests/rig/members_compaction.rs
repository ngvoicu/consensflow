//! The size a project chose for its members' windows to compact at, end to end through the
//! daemon and the pane host (smaller-contexts, SCX-28 to SCX-30, T-1265): chosen on the page
//! (`project.compaction`), it reaches every member's window, whatever its harness, by that
//! harness's own route, and never the chief's: the stand-in Claude's launch settings file
//! (`autoCompactWindow`), the stand-in Codex's command line (`-c
//! model_auto_compact_token_limit=`), the stand-in Devin's environment
//! (`DEVIN_COMPACTION_THRESHOLDS`), and the machine's Muse on its echo provider, whose own
//! parser takes `-c context_compaction.provider_context_limit_tokens=` (a key it does not know
//! refuses the run, so the task's end is Muse's word that it took it). Pi has no stand-in in the
//! rig: its route is held by its launch tests and the extension's own (`tests/pi-extension.test.mjs`).
//! (The windows of the stand-ins run scripts, so these cases are Unix's; the Muse case is
//! skipped, in plain words, on a machine with no Muse Code installed: CI's.)

use cf_e2e::muse::members::{echo_worker, project as muse_project};
use cf_e2e::muse::{self};
use cf_e2e::rig::{Config, Rig};
use serde_json::json;

use crate::{config, secs, Outcome};

/// The rig's Muse on Windows, which runs the machine's ([`muse::StandIn`]).
const MUSE_STAND_IN: &str = env!("CARGO_BIN_EXE_muse-stand-in");

/// The size the human chooses.
const SIZE: u32 = 400_000;

/// The stand-ins' case: their windows run scripts, so Unix's.
#[cfg(unix)]
mod stand_ins {
    use std::fs;

    use cf_e2e::rig::{Project, Rig, OPEN};
    use serde_json::{json, Value};

    use super::SIZE;
    use crate::{after, config, on_command_line, secs, Outcome};

    /// The stand-ins for Codex and Devin, built for these tests.
    const FAKE_CODEX: &str = env!("CARGO_BIN_EXE_fake-codex");
    const FAKE_DEVIN: &str = env!("CARGO_BIN_EXE_fake-devin");

    /// Codex's flag for the size.
    fn codex_limit(size: u32) -> String {
        format!("model_auto_compact_token_limit={size}")
    }

    /// The size a Claude window's own settings file gives it, if it gives one.
    fn claude_window(frame: &Value) -> Option<Value> {
        let file = after(frame, "--settings");
        let text = fs::read_to_string(file).ok()?;
        serde_json::from_str::<Value>(&text)
            .ok()?
            .get("autoCompactWindow")
            .cloned()
    }

    /// The thresholds a window's environment gives Devin, if it gives any.
    fn devin_thresholds(frame: &Value) -> Option<String> {
        frame["env"]["DEVIN_COMPACTION_THRESHOLDS"]
            .as_str()
            .map(str::to_owned)
    }

    /// The window the rig opened whose frame `wanted` holds, once there is one.
    fn window_where(
        rig: &Rig,
        what: &str,
        wanted: impl Fn(&Value) -> bool,
    ) -> cf_e2e::Result<Value> {
        rig.wait_for(what, secs(120), || {
            Ok(rig.open_frames().iter().any(&wanted))
        })?;
        Ok(rig
            .open_frames()
            .into_iter()
            .find(|frame| wanted(frame))
            .unwrap_or(Value::Null))
    }

    /// The roster of the case: the chief and a worker on the stand-in Claude, a worker on the
    /// stand-in Codex and one on the stand-in Devin, each of its own tier so that the chief's
    /// `DISPATCH` reaches each.
    fn write_roster(rig: &Rig) -> cf_e2e::Result {
        rig.stand_ins(&json!({
            "schemaVersion": 1,
            "agents": [
                { "id": "chief", "kind": "claude-code", "model": "fake-chief" },
                { "id": "claude-worker", "kind": "claude-code", "model": "fake", "workTier": "light" },
                { "id": "codex-worker", "kind": "codex", "model": "gpt-6-astra", "effort": "medium", "workTier": "standard" },
                { "id": "devin-worker", "kind": "devin", "model": "fake", "workTier": "complex" },
            ],
        }))
    }

    #[test]
    fn every_members_window_is_given_the_projects_size_by_its_harnesss_route_and_the_chiefs_none(
    ) -> Outcome {
        let rig = Rig::start(config().codex(FAKE_CODEX).devin(FAKE_DEVIN))?;
        write_roster(&rig)?;
        let project = Project::open(&rig, "chief", json!({}))?;
        let id = project.id();
        for agent in ["claude-worker", "codex-worker", "devin-worker"] {
            project.add_member(agent)?;
        }
        let chosen = rig.page(
            "project.compaction",
            json!({ "project": id, "compaction": SIZE }),
        )?;
        assert_eq!(chosen["project"]["compaction"], json!(SIZE), "{chosen}");
        rig.tell(
            id,
            "DISPATCH --tier light Reply with exactly: CLAUDE_DONE\n\
             DISPATCH --tier standard Reply with exactly: CODEX_DONE\n\
             DISPATCH --tier complex Reply with exactly: DEVIN_DONE",
        )?;
        let chief = rig.open_frame(&format!("p{id}-chief"), OPEN)?;

        let claude = window_where(&rig, "the Claude worker's window", |frame| {
            frame["id"] != chief["id"] && claude_window(frame).is_some()
        })?;
        assert_eq!(claude_window(&claude), Some(json!(SIZE)), "{claude}");
        let codex = window_where(&rig, "the Codex worker's window", |frame| {
            on_command_line(frame, "codex-session") && frame["id"] != chief["id"]
        })?;
        assert!(
            on_command_line(&codex, &codex_limit(SIZE)),
            "{}",
            codex["argv"]
        );
        let devin = window_where(&rig, "the Devin worker's window", |frame| {
            on_command_line(frame, "--prompt-file") || devin_thresholds(frame).is_some()
        })?;
        assert_eq!(
            devin_thresholds(&devin).as_deref(),
            Some("400000,400000"),
            "{}",
            devin["env"]
        );

        // The chief's window is given none of them.
        assert_eq!(claude_window(&chief), None, "{chief}");
        assert!(!on_command_line(&chief, "model_auto_compact_token_limit"));
        assert_eq!(devin_thresholds(&chief), None);
        // And each worker's task goes on as it always did.
        rig.wait_for("the three tasks to be done", secs(120), || {
            Ok((1..=3).all(|n| project.task(n).is_ok_and(|task| task["state"] == "done")))
        })?;
        rig.close()?;
        Ok(())
    }
}

/// A rig whose Muse windows run the machine's Muse on its echo provider, or why there is none.
fn muse_rig() -> Option<Config> {
    match muse::installed() {
        Ok(install) => Some(config().muse(install.through(MUSE_STAND_IN), 2_000)),
        Err(why) => {
            #[allow(clippy::print_stderr)] // Said in the output of the case it skips.
            {
                eprintln!("skipped: {why}");
            }
            None
        }
    }
}

#[test]
fn a_muse_members_window_takes_the_projects_size_as_its_context_limit() -> Outcome {
    let Some(config) = muse_rig() else {
        return Ok(());
    };
    let rig = Rig::start(config)?;
    let project = muse_project(&rig, &echo_worker())?;
    rig.page(
        "project.compaction",
        json!({ "project": project.id(), "compaction": SIZE }),
    )?;
    rig.tell(
        project.id(),
        "DISPATCH --tier standard Reply with exactly: T1_DONE",
    )?;
    // Muse refuses a run given a key it does not know: the task's end is its word that it
    // took this one.
    rig.wait_for("T-1 to be answered", secs(120), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let launches = rig.muse_launches();
    assert_eq!(launches.len(), 1, "{launches:?}");
    let limit = format!("context_compaction.provider_context_limit_tokens={SIZE}");
    assert!(
        launches[0]
            .windows(2)
            .any(|pair| pair[0] == "-c" && pair[1] == limit),
        "{:?}",
        launches[0]
    );
    rig.close()?;
    Ok(())
}

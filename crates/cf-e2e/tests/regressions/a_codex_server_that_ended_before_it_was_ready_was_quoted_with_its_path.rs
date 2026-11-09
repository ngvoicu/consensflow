//! A Codex window whose server ended before it was ready was told with its path.
//!
//! Seen by the joint review T-1322 on 2026-11-08, on `c2e7cba2c` (the third reading of T-1328's
//! secret screening): Codex's app-server, started by the window's program (`cf codex-session`),
//! writes its reason to its error stream and ends before its endpoint is ready, as an installed
//! wrapper does whose native executable is gone:
//! `/opt/hunter22/codex: line 2: /opt/hunter22/bin/codex: No such file or directory`. The window
//! ended with 1, which the engine reads as Codex's own, so the failure the chief was told said what
//! the window's screen ended with, `hunter22` and all, at every width. A path is the person's, and
//! can name a credential.
//!
//! What the chief should have seen: T-1 failed, why in ConsensFlow's words (Codex's server stopped
//! before it was ready, and where to look), and no word of the program's or its path.
//! The stand-in Codex's server ends this way with `CF_TEST_CODEX_BACKEND=missing-native` (it ends
//! with 127); the case is the review's otherwise: a Codex worker given a task, through the real
//! daemon and pane host (a pty on the Mac, a ConPTY on Windows, which carry the window's status).
//!
//! The statuses a window's program (`cf codex-session`) ends with for a start that failed (70 to 80)
//! are its own and no status Codex returns: Codex's TUI that ends with one of them (a window that
//! then read "Node.js was not found for Codex" of a Codex that came up and ended with 72) ends the
//! window with 1, the plain failure, and its screen is quoted as any harness's is.

use cf_e2e::rig::{Config, Project, Rig, OPEN};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::{config_with_codex, secs, Outcome};

/// A rig whose chief is a stand-in Claude and whose worker is a Codex given a task by the chief,
/// with the stand-in Codex set up as `config` says. The project's id.
fn a_codex_worker_given_a_task(config: Config) -> cf_e2e::Result<(Rig, i64)> {
    let rig = Rig::start(config.var("CONSENSFLOW_LAUNCH_TIMEOUT_MS", "10000"))?;
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "claude-code", "model": "fake" },
            { "id": "worker", "kind": "codex", "model": "fake", "workTier": "standard" },
        ],
    });
    rig.stand_ins(&roster)?;
    let project = Project::open_with_staff(&rig, "chief", None, &[("worker", "worker")])?;
    let tier = project.tiers()?.get("worker").cloned().unwrap_or_default();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    let ran = rig.cf_in_window(
        &chief,
        [
            "task",
            "add",
            "--tier",
            &tier,
            "Reply with exactly: NEVER_RUNS",
        ],
    )?;
    if ran.code != Some(0) {
        return Err(Error::Daemon(format!("cf task add: {ran}")));
    }
    let id = project.id();
    Ok((rig, id))
}

/// The note to the chief that begins with `start`, once T-1 is `state`.
fn told_of_t1(rig: &Rig, id: i64, state: &str, start: &str) -> cf_e2e::Result<String> {
    let project = Project::new(rig, id);
    let waited = rig.wait_for(&format!("T-1 to be {state}"), secs(60), || {
        Ok(project.task(1)?["state"] == state)
    });
    if let Err(failed) = waited {
        return Err(Error::Daemon(format!(
            "{failed}; the chief's inbox {:?}; the exits {:?}",
            project.inbox("chief")?,
            rig.exits()
        )));
    }
    let told: Value = project
        .inbox("chief")?
        .into_iter()
        .find(|message| {
            message["kind"] == "note"
                && message["body"]
                    .as_str()
                    .is_some_and(|body| body.starts_with(start))
        })
        .ok_or_else(|| Error::Daemon(format!("the chief was not told: {start}")))?;
    Ok(told["body"].as_str().unwrap_or_default().to_owned())
}

#[test]
fn the_failure_of_a_codex_worker_whose_server_ended_before_it_was_ready_says_why_and_quotes_no_path(
) -> Outcome {
    let (rig, project) = a_codex_worker_given_a_task(
        config_with_codex().var("CF_TEST_CODEX_BACKEND", "missing-native"),
    )?;
    let told = told_of_t1(&rig, project, "failed", "T-1 failed:")?;
    for word in [
        "hunter22",
        "/opt/",
        "No such file",
        "ConsensFlow could not open",
    ] {
        assert!(!told.contains(word), "`{word}` is in the failure: {told}");
    }
    assert!(
        told.contains(
            "Codex's server stopped before it was ready; its cause is in ConsensFlow's log"
        ),
        "{told}"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_codex_that_ended_with_a_status_a_start_that_failed_uses_is_a_plain_failure_not_a_start_that_failed(
) -> Outcome {
    for status in ["72", "70", "80"] {
        let (rig, project) = a_codex_worker_given_a_task(
            config_with_codex().var("CF_TEST_CODEX_TUI_STATUS", status),
        )?;
        let told = told_of_t1(&rig, project, "failed", "T-1 failed:")?;
        assert!(
            told.contains("(exit code 1)"),
            "Codex's {status} is a plain failure: {told}"
        );
        for word in [
            "Node.js was not found",
            "program was not found",
            "could not be set up",
        ] {
            assert!(
                !told.contains(word),
                "Codex's {status} is no start that failed (`{word}`): {told}"
            );
        }
        rig.close()?;
    }
    Ok(())
}

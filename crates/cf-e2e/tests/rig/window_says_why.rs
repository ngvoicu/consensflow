//! A window that does not come up says why on its own screen, and the screen
//! goes with the window: Pi on a machine with no login prints "No API key found
//! for the selected model … Use /login" and waits, and every task given to it
//! failed with only "the window never showed its first message". The daemon
//! keeps what the window last showed (the real pane host's screen, and the
//! program's exit code where it ended) and says it in the failure the requester
//! and the human hear, and in one line of its log. A screen that names a secret
//! is not quoted (the lead's decision of 2026-10-16: no masker holds every way a
//! credential is written), and Pi's names an API key: the failure says its lines
//! were left out, keeps the exit code, and none of the lines reaches the chief's
//! window. A fake harness in a real PTY prints Pi's words and stays (or ends
//! with the code 3), and the daemon is told to wait eight seconds, not three
//! minutes, for its first message (`CONSENSFLOW_LAUNCH_TIMEOUT_MS`): long enough
//! for a slow machine's window to have printed. A screen that is quoted, through
//! the same host and daemon, is the regression `devin_window_closed_mid_task`'s.

use cf_e2e::rig::{Project, Rig, OPEN};
use regex::Regex;
use serde_json::{json, Value};

use crate::{config, secs, session_of, Outcome};

/// What the failure says in place of the two lines the window printed.
const SCREEN: &str = r"its last screen lines named a secret, so they are left out";

/// The worker's window is a session of it: `@worker-amber-pine`.
const WORKER: &str = "@worker-[a-z]+-[a-z]+";

/// A rig whose chief is a fake Claude that works, and whose worker is one with
/// no login (`variable` says how), given a task by the chief. Once the task
/// failed: the project and the chief's conversation.
fn a_task_that_failed(variable: &str) -> cf_e2e::Result<(Rig, i64, String)> {
    let rig = Rig::start(
        config()
            .var("CONSENSFLOW_LAUNCH_TIMEOUT_MS", "8000")
            .var(variable, "worker"),
    )?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} Reply with exactly: NEVER_SHOWN",
            added["member"]["tier"].as_str().unwrap_or_default()
        ),
    )?;
    rig.wait_for("T-1 to have failed", secs(60), || {
        Ok(project.task(1)?["state"] == "failed")
    })?;
    let (id, session) = (project.id(), session_of(&chief));
    Ok((rig, id, session))
}

/// The note of the chief's inbox that says T-1 failed.
fn failure_told(rig: &Rig, project: i64) -> cf_e2e::Result<Value> {
    let inbox = Project::new(rig, project).inbox("chief")?;
    inbox
        .iter()
        .find(|message| {
            message["kind"] == "note"
                && message["body"]
                    .as_str()
                    .is_some_and(|body| body.starts_with("T-1 failed:"))
        })
        .cloned()
        .ok_or_else(|| {
            cf_e2e::Error::Daemon(format!("the chief was not told T-1 failed: {inbox:?}"))
        })
}

#[test]
fn says_a_window_that_stayed_named_a_secret_in_the_failure_the_chiefs_window_and_the_log() -> Outcome
{
    let (rig, project, session) = a_task_that_failed("CF_TEST_NO_LOGIN")?;
    let told = failure_told(&rig, project)?;
    let body = told["body"].as_str().unwrap_or_default();
    let failure = Regex::new(&format!(
        r#"^T-1 failed: the window never showed its first message; {SCREEN}\. Reopen it with: cf task reopen T-1 "…"$"#
    ))?;
    assert!(failure.is_match(body), "{body}");
    // The human hears it too, with whom it did not reach.
    let human = Regex::new(&format!(
        r#"^m-\d+, a task from @chief on T-1, did not reach {WORKER}: the window never showed its first message; {SCREEN}\.$"#
    ))?;
    let heard = Project::new(&rig, project).inbox("human")?;
    assert!(
        heard
            .iter()
            .any(|message| human.is_match(message["body"].as_str().unwrap_or_default())),
        "{heard:?}"
    );
    // The chief's own window shows it, and none of the screen's lines: it was pasted
    // in, and its record kept it.
    rig.wait_for(
        "the chief's record to say the lines were left out",
        secs(30),
        || Ok(rig.transcript(&session).contains("so they are left out")),
    )?;
    assert!(
        !rig.transcript(&session).contains("No API key found"),
        "a line of the screen reached the chief's window"
    );
    // One line of the daemon's log.
    let logged = Regex::new(&format!(
        r#"(?m)^\S+ warn the launch of p{project}-worker-[a-z]+-[a-z]+ failed: the window never showed its first message; {SCREEN}$"#
    ))?;
    let log = rig.log();
    assert!(logged.is_match(&log), "{log}");
    assert!(!log.contains("No API key found"), "{log}");
    rig.close()?;
    Ok(())
}

#[test]
fn says_the_exit_code_of_a_window_that_ended_and_that_its_screen_named_a_secret() -> Outcome {
    let (rig, project, _) = a_task_that_failed("CF_TEST_NO_LOGIN_EXITS")?;
    let told = failure_told(&rig, project)?;
    let body = told["body"].as_str().unwrap_or_default();
    let failure = Regex::new(&format!(
        r#"^T-1 failed: {WORKER}'s window closed \(exit code 3\); {SCREEN}\. Reopen it with: cf task reopen T-1 "…"$"#
    ))?;
    assert!(failure.is_match(body), "{body}");
    let logged = Regex::new(&format!(
        r#"(?m)^\S+ warn the launch of p{project}-worker-[a-z]+-[a-z]+ failed: {WORKER}'s window closed \(exit code 3\); {SCREEN}$"#
    ))?;
    let log = rig.log();
    assert!(logged.is_match(&log), "{log}");
    rig.close()?;
    Ok(())
}

//! A panic in a command the page asks does not end the app (T-1050). On macOS the page's
//! invoke reaches the app inside wry's `extern "C"` handler of the `ipc` protocol, and Tauri
//! runs a synchronous command right there: a panic that unwinds out of the command into that
//! frame, which cannot unwind, aborts the app. The app stops it at its edge, writes it down,
//! tells the person, and answers the page with the error. An asynchronous command runs as a
//! task, whose panic Tauri's task catches but never answers the page of: the app runs its body
//! as a task of its own, and answers and tells the same.
//!
//! The commands that panic (`selftest_panic`, `selftest_panic_async`) are in an app built for
//! this case alone, with the app's feature `test-support`, which the smoke is told of with
//! `CONSENSFLOW_SMOKE_TEST_SUPPORT=1`. Every other app, the release's first, must not have
//! them: the page is told each is a command the app does not have. Either way the page asks
//! them and then a command after them, which an app that ran on answers as ever.

use std::path::Path;

use cf_e2e::process::own_var;
use serde_json::json;

use crate::app::{app_log, App};
use crate::machine::Machine;
use crate::{bundle, patience_of, Outcome, PASTE_READER};

/// What the commands panic with (app/src-tauri/src/lib.rs, `SELFTEST_PANIC`).
const PANICKED: &str = "the deliberate panic of a command the page asked (T-1050)";

/// What the page is answered of its command `command` that panicked
/// (app/src-tauri/src/panics.rs, `failed`).
fn answered_of(command: &str) -> String {
    format!(
        "{command} failed inside the app; the app stopped the error and runs on, and wrote what \
         happened in its error log"
    )
}

/// What the app writes in its error log of its command `command` that it stopped.
fn stopped_of(command: &str) -> String {
    format!(
        "consensflow: the app's command {command} panicked; the app stopped it there and runs on"
    )
}

#[test]
#[ignore = "opt-in, on a built app: cargo xtask smoke (npm run smoke)"]
fn the_built_app_answers_a_command_that_panics_and_runs_on() -> Outcome {
    let bundle = bundle::locate(&bundle::requested(
        own_var("CONSENSFLOW_SMOKE_APP").as_deref(),
    ))?;
    let patience = patience_of(own_var("CONSENSFLOW_SMOKE_TIMEOUT_MS").as_deref())?;
    let carries = own_var("CONSENSFLOW_SMOKE_TEST_SUPPORT").as_deref() == Some("1");
    let machine = Machine::new(Path::new(PASTE_READER), patience)?;
    let bundle = bundle::to_start(&bundle, &machine.root)?;
    let outcome = drive(&bundle, &machine, patience, carries);
    let keep = own_var("CONSENSFLOW_SMOKE_KEEP").as_deref() == Some("1");
    machine.say_where_kept(&mut std::io::stderr(), outcome.is_err() || keep);
    outcome?;
    bundle::assert_sealed(&bundle.app)?;
    if !keep {
        machine.cleanup()?;
    }
    Ok(())
}

/// The case, on the app of `bundle` started on `machine`, which `carries` the commands that
/// panic or must not.
fn drive(
    bundle: &bundle::Bundle,
    machine: &Machine,
    patience: std::time::Duration,
    carries: bool,
) -> Outcome {
    let mut app = App::start(
        bundle::run_of(bundle)
            .vars(machine.vars())
            .var("CONSENSFLOW_SELFTEST_PANIC", "1")
            .cwd(&machine.root),
        &machine.root,
        &machine.state,
        patience,
    )?;
    let answers = [
        ("selftest_panic", app.wait_for("panic-answered")?),
        (
            "selftest_panic_async",
            app.wait_for("async-panic-answered")?,
        ),
    ];
    let after = app.wait_for("after-panic")?;
    // Asked after them, the app's update state is answered as ever: it names its channel.
    assert!(
        after["data"]["channel"].is_string(),
        "the app did not answer a command after the panics: {after}"
    );
    if carries {
        for (command, answer) in &answers {
            assert_eq!(
                answer["data"],
                json!({"ok": false, "error": answered_of(command)}),
                "the page was not answered with the error of {command}"
            );
        }
        let told = app.wait_for("told")?;
        assert!(
            told["data"]["message"]
                .as_str()
                .is_some_and(|message| message.starts_with("ConsensFlow stopped an error")),
            "the person was not told: {told}"
        );
        let log = app_log(&machine.state).unwrap_or_default();
        assert!(
            log.contains(PANICKED),
            "the panic is not in the app's log:\n{log}"
        );
        for (command, _) in &answers {
            assert!(
                log.contains(&stopped_of(command)),
                "the stop of {command} is not in the app's log:\n{log}"
            );
        }
    } else {
        for (command, answer) in &answers {
            assert_eq!(
                answer["data"],
                json!({"rejected": format!("Command {command} not found")}),
                "an app built without test-support has {command}"
            );
        }
    }
    app.quit();
    let ended = app.exit()?;
    assert!(
        ended.is_some_and(|ended| ended.success()),
        "the app did not end cleanly once its input ended: {ended:?}"
    );
    Ok(())
}

//! The update that is taken: install and restart, and what it leaves.

use std::fs;

use serde_json::json;

use super::{ledger_of, offer_update, projects_of, quit, BRIDGE_SCHEMA};
use crate::updater_smoke::bundle::{cf_of, digest_manifest, verify_seal};
use crate::updater_smoke::case::{
    blocked_evidence, boot_evidence, held_evidence, release_panes, staging_left, Case,
};
use crate::updater_smoke::evidence::{app_log, assert_started_daemon, Kind};
use crate::updater_smoke::launchers::{assert_repaired, plant_commands, Repaired};
use crate::updater_smoke::ledger::{assert_projects, assert_sound, assert_traced, traced_events};
use crate::updater_smoke::processes::{alive, gone};
use crate::updater_smoke::{files, Result};

/// The update from the installed app, install and restart, and what it leaves:
/// the update's app on the bundle's `cf`, the ledger whole, and the terminal
/// command of the home running the update's `cf`. With `way_back` the home took
/// the flip release's way back to Node before the installed app started (a
/// `use-node` file in it), so the installed app's daemon is Node's whichever
/// release it is, the terminal commands are not looked at (the `cf` that wrote
/// them handed `setup` to Node), and the file is the user's to the end.
pub(super) fn update_flow(kase: &mut Case, way_back: bool) -> Result {
    let inputs = kase.inputs;
    let way_back_file = kase.sandbox.state.join("use-node");
    // Both names of this home's, one of which serves another home, and the other home's own.
    let planted = if way_back {
        None
    } else {
        Some(plant_commands(
            &kase.installed,
            &kase.sandbox,
            true,
            inputs.release,
        )?)
    };
    if way_back {
        fs::write(&way_back_file, "").map_err(files("write", &way_back_file))?;
    }
    offer_update(kase, None)?;
    let app = kase.start_app(&inputs.to.version, &[])?;

    // The installed app: its process, its daemon, ready, and holding the ledger.
    let first = boot_evidence(kase, &app, &inputs.from.version, None)?;
    let expected = if way_back {
        Kind::Node
    } else {
        inputs.release.daemon()
    };
    ensure!(
        first.daemon.kind == expected,
        "the installed app started {}",
        first.daemon.runtime
    );
    let chiefs = blocked_evidence(kase, &app, &first)?;
    // The page has had its answers (two projects, two windows): the daemon has the ledger, and refuses a second.
    held_evidence(kase)?;
    kase.note(format!(
        "installed {}: app pid {}, daemon pid {} ({}), ledger held",
        inputs.from.version, first.app, first.daemon.pid, first.daemon.runtime
    ));
    let events = kase.sandbox.state.join("events.jsonl");
    let traced = traced_events(&fs::read_to_string(&events).map_err(files("read", &events))?);
    ensure!(
        !traced.is_empty(),
        "the daemon traced no ledger event before the update"
    );

    // Both windows close, the install goes through, and the app starts again as the update.
    release_panes(kase, &app, &chiefs)?;
    let second = boot_evidence(kase, &app, &inputs.to.version, Some(first.app))?;
    let restarted = app.wait_for("the update restart", |event| {
        event.name() == "update-restarted"
    })?;
    ensure!(
        restarted.data("currentVersion") == Some(&json!(inputs.to.version)),
        "the restart did not report the update"
    );
    ensure!(
        restarted.data("blockers") == Some(&json!(0)),
        "the restart reported open panes"
    );
    ensure!(
        restarted.pid() == Some(second.app),
        "the restart was reported by pid {:?}, and the app that booted is {}",
        restarted.pid(),
        second.app
    );
    ensure!(
        restarted.pid() != Some(first.app),
        "the update restarted in the original process"
    );
    ensure!(
        alive(second.app),
        "the restarted app was not alive when it reported ready"
    );
    kase.waits
        .until("the first app exits", || Ok(gone(first.app).then_some(())))?;
    kase.waits
        .until("the first daemon exits and lets go of the ledger", || {
            Ok(gone(first.daemon.pid).then_some(()))
        })?;
    // The page read the board of the update's daemon: it is ready, and has the ledger.
    held_evidence(kase)?;

    // The update's app: its daemon is the bundle's cf, which the app's log names, and the way
    // back's file, which nothing reads now, is there or not as the user left it.
    let cf = cf_of(&kase.sandbox.copy);
    ensure!(
        second.daemon.kind == Kind::Native,
        "the update started {}",
        second.daemon.runtime
    );
    ensure!(
        second.daemon.pid != first.daemon.pid,
        "the update's daemon is the installed app's"
    );
    assert_started_daemon(&app_log(&kase.sandbox), &cf)?;
    ensure!(
        way_back_file.exists() == way_back,
        "the way back to Node was taken or given back"
    );
    kase.note(format!(
        "updated {}: app pid {}, daemon pid {} ({}), ledger held, the app log names the cf it started",
        inputs.to.version, second.app, second.daemon.pid, second.daemon.runtime
    ));

    // The terminal command of this home runs the update's cf; no other command changed.
    if let Some(planted) = &planted {
        let version = second.daemon.runtime.split(' ').nth(1).unwrap_or_default();
        assert_repaired(&Repaired {
            sandbox: &kase.sandbox,
            planted,
            cf: &cf,
            app_log: &app_log(&kase.sandbox),
            version,
            release: inputs.release,
        })?;
        kase.note(format!(
            "the terminal command of {} runs {}; the commands of {} and the one pinned to it are as they were",
            kase.sandbox.state.display(),
            cf.display(),
            kase.sandbox.other.display()
        ));
    }

    // The bundle in place is the update, byte for byte, sealed, with nothing left of the install.
    verify_seal(&kase.sandbox.copy, &inputs.env)?;
    ensure!(
        digest_manifest(&kase.sandbox.copy)? == inputs.to_manifest,
        "installed copy bytes do not equal TO_APP"
    );
    ensure!(
        digest_manifest(&inputs.to.app)? == inputs.to_manifest,
        "TO_APP was mutated by the smoke"
    );
    ensure!(
        staging_left(&kase.sandbox)?.is_empty(),
        "the install left staging files in the home"
    );

    // The quit takes everything with it, and the ledger is whole.
    let ended = quit(kase, &[first.daemon.pid, second.daemon.pid])?;
    kase.note(format!(
        "the original app exited {:?} / {:?}",
        ended.code, ended.signal
    ));
    ensure!(
        !app.events()
            .iter()
            .any(|event| event.name() == "update-failure"),
        "the successful updater smoke reported a failure"
    );
    let ledger = ledger_of(kase)?;
    assert_sound(&ledger, BRIDGE_SCHEMA)?;
    assert_projects(&ledger, &projects_of(kase))?;
    assert_traced(&traced, &ledger)?;
    kase.note(format!(
        "the ledger: schema {}, sound, the {} events traced before the update are in it",
        ledger.version,
        traced.len()
    ));
    Ok(())
}

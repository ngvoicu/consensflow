//! The update that is taken: install and restart, and what it leaves.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::{ledger_of, live_ledger_of, offer_update, projects_of, quit, BRIDGE_SCHEMA};
use crate::updater_smoke::app::App;
use crate::updater_smoke::bundle::{cf_of, digest_manifest, inspect_bundle, verify_seal};
use crate::updater_smoke::case::{
    blocked_evidence, boot_evidence, held_evidence, release_panes, staging_left, Booted, Case,
};
#[cfg(unix)]
use crate::updater_smoke::descriptors::{compare, hands_on, held_by, Held, Left, NOT_MARKED};
use crate::updater_smoke::evidence::{
    app_log, assert_daemon_is, assert_started_daemon_since, mark, Kind, Mark,
};
use crate::updater_smoke::launchers::{assert_repaired, plant_commands, Planted, Repaired};
use crate::updater_smoke::ledger::{
    assert_kept, assert_projects, assert_sound, assert_traced, dropped_in, traced_events, Ledger,
    WINDOWS_CLOSED,
};
use crate::updater_smoke::processes::{alive, gone, programs_of, Watch};
use crate::updater_smoke::{files, Result};

/// How long after the installed app has exited a second start of the update is looked for.
const SETTLE: std::time::Duration = std::time::Duration::from_secs(2);

/// The installed app, up, with what was true of it before the update.
struct Installed {
    app: App,
    first: Booted,
    /// The pids of the two chiefs' windows.
    chiefs: Vec<u32>,
    /// The ledger with the installed daemon on it.
    held_before: Ledger,
    /// The events the installed daemon traced.
    traced: Vec<Value>,
    #[cfg(unix)]
    first_held: Vec<Held>,
    #[cfg(unix)]
    left: Left,
}

/// The update's app, up after the restart.
struct Restarted {
    second: Booted,
    /// Where the app's log stood when the installed app began to close.
    log_mark: Mark,
}

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
    #[cfg(unix)]
    let mut installed = start_installed(kase, way_back)?;
    #[cfg(not(unix))]
    let installed = start_installed(kase, way_back)?;
    let restarted = restart(kase, &installed)?;
    #[cfg(unix)]
    assert_nothing_inherited(kase, &mut installed, restarted.second.app)?;
    // The page read the board of the update's daemon: it is ready, and has the ledger.
    held_evidence(kase)?;
    let cf = assert_update_daemon(kase, &installed, &restarted, &way_back_file, way_back)?;
    if let Some(planted) = &planted {
        assert_terminal_command(kase, planted, &cf, &restarted.second)?;
    }
    assert_bundle_in_place(kase)?;
    finish(kase, &installed, &restarted.second)
}

/// The installed app started: its process, its daemon, ready, and holding the ledger,
/// and the ledger as the update will find it.
fn start_installed(kase: &mut Case, way_back: bool) -> Result<Installed> {
    let inputs = kase.inputs;
    // Two descriptors a program that starts the app may leave it, which the app started again
    // after the update must not hold (see `descriptors`).
    #[cfg(unix)]
    let mut left = Left::make(&kase.sandbox.root)?;
    #[cfg(unix)]
    left.leave()?;
    let app = kase.start_app(&inputs.to.version, &[])?;
    #[cfg(unix)]
    left.given();

    // The installed app: its process, its daemon, ready, and holding the ledger.
    let first = boot_evidence(kase, &app, &inputs.from.version, None)?;
    #[cfg(unix)]
    let first_held = held_by(first.app)?;
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

    // What the ledger holds now, with the installed daemon on it: the update must keep every
    // row and every value of it (T-1338: a live update was only looked at for its soundness, the
    // projects it has and the events it traced, and let a project lose its name).
    let held_before = live_ledger_of(kase)?;
    assert_projects(&held_before, &projects_of(kase))?;
    Ok(Installed {
        app,
        first,
        chiefs,
        held_before,
        traced,
        #[cfg(unix)]
        first_held,
        #[cfg(unix)]
        left,
    })
}

/// Both windows close, the install goes through, and the app starts again as the
/// update: once, by the installed app, and the installed app and its daemon gone.
fn restart(kase: &Case, installed: &Installed) -> Result<Restarted> {
    let inputs = kase.inputs;
    let Installed {
        app, first, chiefs, ..
    } = installed;
    // Every start of the bundle's app from here on: the update's app is started once, by the
    // installed app, and by nothing else (T-941). The app, not the programs beside it: a bridge's
    // bundle holds the Node its daemon runs on in the same folder, and that daemon runs already.
    let watch = Watch::start(programs_of(
        &inspect_bundle(&kase.sandbox.copy, "the app in place")?.binary,
    ));
    // What the app's log holds now is the installed app's: the line of the update's app that says
    // which cf it started is the one written past it (the installed app's says the same).
    let log_mark = mark(&app_log(&kase.sandbox));
    // Both windows close, the install goes through, and the app starts again as the update.
    release_panes(kase, app, chiefs)?;
    let second = boot_evidence(kase, app, &inputs.to.version, Some(first.app))?;
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
    // A second start of the update (Tauri's restart, after the app's own) would be launchd's
    // child with no input, which quits at once and says nothing: it is counted, not heard.
    std::thread::sleep(SETTLE);
    let started: Vec<u32> = watch
        .stop()?
        .into_iter()
        .filter(|pid| *pid != first.app)
        .collect();
    ensure!(
        started == [second.app],
        "the update's app was started {} times, pids {started:?}, and the one that booted is {}",
        started.len(),
        second.app
    );
    Ok(Restarted { second, log_mark })
}

/// The new app holds nothing of the old one's but its streams: the bridge release's update is
/// let hold what the bridge is known to hand on (it never marks a descriptor close-on-exec,
/// and is a shipped release), which the report lists, and nothing beyond that.
#[cfg(unix)]
fn assert_nothing_inherited(kase: &mut Case, installed: &mut Installed, second_app: u32) -> Result {
    let Installed {
        first_held, left, ..
    } = installed;
    let tolerated = hands_on(kase.inputs.release);
    let (faults, notes) = compare(first_held, &held_by(second_app)?, left, tolerated);
    for note in notes {
        kase.note(note);
    }
    ensure!(
        faults.is_empty(),
        "the app started again holds what the installed app held:\n{}",
        faults.join("\n")
    );
    // The pipe the case left reaches its end once the old app has gone, unless a release
    // that hands on every descriptor it holds gave it to the new app, which holds its
    // writing end as long as it runs.
    if tolerated.is_none() {
        left.closed_everywhere(&kase.waits)?;
    }
    let log = app_log(&kase.sandbox);
    ensure!(
        !log.contains(NOT_MARKED),
        "the installed app's log says it {NOT_MARKED}: {}",
        log.lines()
            .filter(|line| line.contains(NOT_MARKED))
            .collect::<Vec<_>>()
            .join("\n")
    );
    match tolerated {
        None => kase.note(format!(
            "the app started again holds nothing of the installed app's: neither the file it was left at fd {1} nor the pipe at fd {0}, which no process holds, and nothing it could not mark",
            left.pipe, left.file_fd
        )),
        Some(known) => kase.note(format!(
            "the app started again holds only what the {} release is known to hand on, the file the case left at fd {} and the pipe at fd {} among it, and nothing it could not mark ({}: {})",
            known.release.name(),
            left.file_fd,
            left.pipe,
            known.files.join(", "),
            known.why
        )),
    }
    Ok(())
}

/// The update's app: its daemon is the bundle's cf, which the app's log names, and the way
/// back's file, which nothing reads now, is there or not as the user left it. Answers that `cf`.
fn assert_update_daemon(
    kase: &mut Case,
    installed: &Installed,
    restarted: &Restarted,
    way_back_file: &Path,
    way_back: bool,
) -> Result<PathBuf> {
    let inputs = kase.inputs;
    let (first, second) = (&installed.first, &restarted.second);
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
    assert_started_daemon_since(&app_log(&kase.sandbox), restarted.log_mark, &cf)?;
    assert_daemon_is(&second.daemon, &cf, inputs.daemon_version.as_deref())?;
    ensure!(
        way_back_file.exists() == way_back,
        "the way back to Node was taken or given back"
    );
    kase.note(format!(
        "updated {}: app pid {}, daemon pid {} ({}), ledger held, the app log names the cf it started",
        inputs.to.version, second.app, second.daemon.pid, second.daemon.runtime
    ));
    Ok(cf)
}

/// The terminal command of this home runs the update's cf; no other command changed.
fn assert_terminal_command(
    kase: &mut Case,
    planted: &Planted,
    cf: &Path,
    second: &Booted,
) -> Result {
    let version = second.daemon.runtime.split(' ').nth(1).unwrap_or_default();
    assert_repaired(&Repaired {
        sandbox: &kase.sandbox,
        planted,
        cf,
        app_log: &app_log(&kase.sandbox),
        version,
        release: kase.inputs.release,
    })?;
    kase.note(format!(
        "the terminal command of {} runs {}; the commands of {} and the one pinned to it are as they were",
        kase.sandbox.state.display(),
        cf.display(),
        kase.sandbox.other.display()
    ));
    Ok(())
}

/// The bundle in place is the update, byte for byte, sealed, with nothing left of the install.
fn assert_bundle_in_place(kase: &Case) -> Result {
    let inputs = kase.inputs;
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
    Ok(())
}

/// The quit takes everything with it, and the ledger is whole.
fn finish(kase: &mut Case, installed: &Installed, second: &Booted) -> Result {
    let inputs = kase.inputs;
    let Installed {
        app,
        first,
        held_before,
        traced,
        ..
    } = installed;
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
    assert_traced(traced, &ledger)?;
    assert_kept(
        held_before,
        &ledger,
        &dropped_in(&inputs.checkout)?,
        &WINDOWS_CLOSED,
    )?;
    kase.note(format!(
        "the ledger: schema {}, sound, the {} events traced before the update are in it",
        ledger.version,
        traced.len()
    ));
    Ok(())
}

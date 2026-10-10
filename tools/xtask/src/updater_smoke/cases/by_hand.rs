//! An app replaced by hand, as a disk image's copy does, with the app quit: its
//! first start repairs the terminal command and keeps the ledger.

use super::{ledger_of, projects_of, quit, BRIDGE_SCHEMA};
use crate::updater_smoke::bundle::{cf_of, copy_over, digest_manifest, remove_all, verify_seal};
use crate::updater_smoke::case::{boot_evidence, held_evidence, Case};
use crate::updater_smoke::evidence::{app_log, assert_started_daemon, Kind};
use crate::updater_smoke::launchers::{assert_repaired, plant_commands, Repaired};
use crate::updater_smoke::ledger::{assert_kept, assert_projects, assert_sound};
use crate::updater_smoke::Result;

/// How the update's app got where the old one was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum How {
    /// The old app taken away first.
    Replaced,
    /// Copied over the old app, which leaves what the new one lacks.
    CopiedOver,
}

impl How {
    /// What the case's name says of it.
    pub(super) fn words(self) -> &'static str {
        match self {
            Self::Replaced => "replaced",
            Self::CopiedOver => "copied over",
        }
    }
}

pub(super) fn by_hand(kase: &mut Case, how: How) -> Result {
    let inputs = kase.inputs;
    // Both names of this home's serve it, and the other home has its own.
    let planted = plant_commands(&kase.installed, &kase.sandbox, false, inputs.release)?;
    // The installed app has its session, and no update is offered: two projects opened and closed, then it is quit.
    let old = kase.start_app(&inputs.to.version, &["update-failure"])?;
    let first = boot_evidence(kase, &old, &inputs.from.version, None)?;
    let none = old.wait_for("no update to take", |event| {
        event.name() == "update-failure"
    })?;
    ensure!(
        none.data("error")
            .and_then(|error| error.as_str())
            .is_some_and(|error| error.contains("update feed is unavailable")),
        "the update was not unavailable"
    );
    let left = quit(kase, &[first.daemon.pid])?;
    ensure!(
        left.code == Some(0),
        "the app exited {:?} / {:?}",
        left.code,
        left.signal
    );
    let before = ledger_of(kase)?;
    assert_sound(&before, BRIDGE_SCHEMA)?;
    assert_projects(&before, &projects_of(kase))?;

    // What a disk image's copy does: the update's app where the old one was.
    if how == How::Replaced {
        remove_all(&kase.sandbox.copy)?;
    }
    copy_over(&inputs.to.app, &kase.sandbox.copy, &inputs.env)?;
    // Every file of the update is there as it is. A bundle with nothing of the old app's left
    // in it is sealed as the update is; one with files the update has not (a Node the update
    // ships none of) is not a sealed bundle, and what it must do is run.
    let merged = digest_manifest(&kase.sandbox.copy)?;
    for entry in &inputs.to_manifest {
        ensure!(
            merged.iter().find(|each| each.name == entry.name) == Some(entry),
            "{} is not the update's",
            entry.name
        );
    }
    let stale = merged
        .iter()
        .filter(|entry| {
            !inputs
                .to_manifest
                .iter()
                .any(|each| each.name == entry.name)
        })
        .count();
    kase.note(format!("files of the old app left in the copy: {stale}"));
    if stale == 0 {
        verify_seal(&kase.sandbox.copy, &inputs.env)?;
    }

    let app = kase.start_app(&inputs.to.version, &[])?;
    let second = boot_evidence(kase, &app, &inputs.to.version, None)?;
    app.wait_for("the first start", |event| {
        event.name() == "update-restarted"
    })?;
    held_evidence(kase)?;
    let cf = cf_of(&kase.sandbox.copy);
    ensure!(
        second.daemon.kind == Kind::Native,
        "the first start ran {}",
        second.daemon.runtime
    );
    assert_started_daemon(&app_log(&kase.sandbox), &cf)?;
    let version = second.daemon.runtime.split(' ').nth(1).unwrap_or_default();
    assert_repaired(&Repaired {
        sandbox: &kase.sandbox,
        planted: &planted,
        cf: &cf,
        app_log: &app_log(&kase.sandbox),
        version,
        release: inputs.release,
    })?;
    kase.note(format!(
        "first start of {}: daemon pid {} ({}), the terminal command runs the update's cf",
        inputs.to.version, second.daemon.pid, second.daemon.runtime
    ));
    quit(kase, &[second.daemon.pid])?;
    let after = ledger_of(kase)?;
    assert_kept(&before, &after)?;
    kase.note(format!(
        "the ledger: schema {} then {}, every row it had is there",
        before.version, after.version
    ));
    Ok(())
}

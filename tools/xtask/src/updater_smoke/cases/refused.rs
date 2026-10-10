//! The updates the installed app refuses: a signature that is not the key's, and
//! bundles its check refuses. The installed bundle is intact and the app still runs.

use serde_json::Value;

use super::{ledger_of, projects_of, quit, BRIDGE_SCHEMA};
use crate::updater_smoke::app::App;
use crate::updater_smoke::bundle::{digest_manifest, refused_bundle, verify_seal, Refusal};
use crate::updater_smoke::case::{
    blocked_evidence, boot_evidence, held_evidence, release_panes, staging_left, Booted, Case,
};
use crate::updater_smoke::evidence::daemon_of;
use crate::updater_smoke::feed::signed_update;
use crate::updater_smoke::ledger::{assert_projects, assert_sound};
use crate::updater_smoke::processes::alive;
use crate::updater_smoke::signing::generate_key;
use crate::updater_smoke::Result;

/// What a run to the refusal found.
struct Refused {
    app: App,
    first: Booted,
    failure: String,
}

/// The steps of an update the installed app refuses: it runs to the install, or to the download.
fn refused_flow(kase: &mut Case, blocked: bool) -> Result<Refused> {
    let inputs = kase.inputs;
    let app = kase.start_app(&inputs.to.version, &["update-failure"])?;
    let first = boot_evidence(kase, &app, &inputs.from.version, None)?;
    ensure!(
        first.daemon.kind == inputs.release.daemon(),
        "the installed app started {}",
        first.daemon.runtime
    );
    if blocked {
        let chiefs = blocked_evidence(kase, &app, &first)?;
        release_panes(kase, &app, &chiefs)?;
    }
    let to_version = inputs.to.version.as_str();
    let failure = app.wait_for_unless(
        "the refusal",
        |event| event.name() == "update-failure",
        |event| {
            let took = event.name() == "update-restarted"
                || (event.name() == "update-boot"
                    && event.data("currentVersion").and_then(Value::as_str) == Some(to_version));
            took.then_some("the app took an update it should have refused")
        },
    )?;
    let failure = failure
        .data("error")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok(Refused {
        app,
        first,
        failure,
    })
}

/// What a refused update leaves: the installed bundle as it was, the app and its daemon running, the ledger held.
fn intact_evidence(kase: &mut Case, refused: &Refused) -> Result {
    let inputs = kase.inputs;
    let Refused { app, first, .. } = refused;
    ensure!(
        digest_manifest(&kase.sandbox.copy)? == inputs.from_manifest,
        "the installed bundle changed"
    );
    verify_seal(&kase.sandbox.copy, &inputs.env)?;
    ensure!(
        staging_left(&kase.sandbox)?.is_empty(),
        "the refused install left staging files in the home"
    );
    ensure!(alive(first.app), "the app stopped");
    ensure!(alive(first.daemon.pid), "the daemon stopped");
    let again = daemon_of(
        &kase.sandbox,
        &kase.waits,
        first.app,
        &kase.sandbox.copy,
        &kase.probes,
    )?;
    ensure!(again.pid == first.daemon.pid, "the app has another daemon");
    held_evidence(kase)?;
    ensure!(
        !app.events()
            .iter()
            .any(|event| event.name() == "update-restarted"),
        "the app restarted"
    );
    kase.note(format!(
        "app pid {} and daemon pid {} still run, the bundle is byte for byte as installed, the ledger is held",
        first.app, first.daemon.pid
    ));
    Ok(())
}

/// The refused app quit well, and its ledger is whole.
fn quit_whole(kase: &Case, refused: &Refused) -> Result {
    let ended = quit(kase, &[refused.first.daemon.pid])?;
    ensure!(
        ended.code == Some(0),
        "the app exited {:?} / {:?}",
        ended.code,
        ended.signal
    );
    assert_sound(&ledger_of(kase)?, BRIDGE_SCHEMA)
}

/// An update signed by another key is refused and the installed app stays intact and running.
pub(super) fn stranger(kase: &mut Case) -> Result {
    let inputs = kase.inputs;
    let stranger = generate_key(
        &inputs.checkout,
        &kase.sandbox.tls.join("stranger"),
        &inputs.env,
    )?;
    let update = signed_update(
        &inputs.checkout,
        &stranger.private_key,
        &inputs.to.app,
        &kase.sandbox.probe,
        &inputs.env,
    )?;
    kase.feed
        .offer(&update.version, &update.signature, update.bytes.clone());
    let flow = refused_flow(kase, false)?;
    ensure!(
        flow.failure.to_lowercase().contains("signature"),
        "the refusal does not name the signature: {}",
        flow.failure
    );
    kase.note(format!("refused: {}", flow.failure));
    intact_evidence(kase, &flow)?;
    quit_whole(kase, &flow)?;
    assert_projects(&ledger_of(kase)?, &projects_of(kase))
}

/// A signed update whose bundle fails the check is refused and the installed app stays intact and running.
pub(super) fn bundle(kase: &mut Case, kind: Refusal) -> Result {
    let inputs = kase.inputs;
    let refused = refused_bundle(
        kind,
        &inputs.to.app,
        &kase.sandbox.root.join("refused"),
        &inputs.env,
    )?;
    super::offer_update(kase, Some(&refused))?;
    let flow = refused_flow(kase, true)?;
    ensure!(
        flow.failure.contains(kind.words()),
        "the refusal is not the check's: {}",
        flow.failure
    );
    kase.note(format!("refused: {}", flow.failure));
    intact_evidence(kase, &flow)?;
    quit_whole(kase, &flow)
}

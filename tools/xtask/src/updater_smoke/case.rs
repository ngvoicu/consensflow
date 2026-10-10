//! What every case of the updater smoke is made of: the two built apps, held to
//! what the installed app's check takes of a bundle; one machine per case, with
//! the installed app copied into it and a feed served for it; the app started on
//! it; and the steps the update's cases share, each proved by what the machine
//! shows (processes.rs, evidence.rs).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_base::env::Env;

use super::app::{launch_app, App, Launch};
use super::build::Release;
use super::bundle::{
    assert_ad_hoc, copy_bundle, digest_manifest, inspect_bundle, remove_all, verify_seal,
    BundleInfo, Entry,
};
use super::evidence::{app_log, assert_app, daemon_log, daemon_of, ledger_held, Daemon};
use super::feed::{make_tls, serve_updates, Feed, Tls};
use super::processes::{alive, gone, process_table, Waits};
use super::sandbox::{Sandbox, SelfTest};
use super::say::Say;
use super::signing::Key;
use super::versions::compare_versions;
use super::{files, Error, Result};

use std::cmp::Ordering;

/// How much longer than a wait the page's own deadline is.
const PAGE_GRACE: Duration = Duration::from_secs(10);

/// What of a machine's logs a failure shows.
const LOG_SHOWN: usize = 3000;

/// The inputs of the run for one installed release: both apps, as built and
/// checked once (identity, versions, the code signature, ad hoc), the release the
/// installed one is, and the updater key the run signs with.
#[derive(Debug)]
pub struct Inputs {
    pub from: BundleInfo,
    pub release: Release,
    pub to: BundleInfo,
    pub from_manifest: Vec<Entry>,
    pub to_manifest: Vec<Entry>,
    pub key: Key,
    /// Whether the machines of the cases are kept.
    pub keep: bool,
    pub waits: Waits,
    /// Where a case's machine is made.
    pub machines: PathBuf,
    /// The checkout whose Tauri CLI signs.
    pub checkout: PathBuf,
    pub env: Env,
}

/// What `Inputs::load` is given.
pub struct Given<'a> {
    pub from_app: &'a Path,
    pub release: Release,
    pub to_app: &'a Path,
    pub key: &'a Key,
    pub keep: bool,
    pub waits: Waits,
    pub machines: &'a Path,
    pub checkout: &'a Path,
    pub env: &'a Env,
}

/// Whether `path` is under /Applications, where the apps the user installed are.
fn is_installed(path: &Path) -> bool {
    path.starts_with("/Applications")
}

/// A built app, which is never the installed one in /Applications.
fn built_path(path: &Path, what: &str) -> Result<PathBuf> {
    let path = fs::canonicalize(path).map_err(files("find", path))?;
    ensure!(
        path.extension().is_some_and(|extension| extension == "app"),
        "{what} must point to a .app bundle: {}",
        path.display()
    );
    ensure!(
        !is_installed(&path),
        "{what} may not point into /Applications: {}",
        path.display()
    );
    Ok(path)
}

impl Inputs {
    /// Reads and holds the two apps to what the installed app's check takes of a bundle.
    pub fn load(given: &Given) -> Result<Self> {
        let from_app = built_path(given.from_app, "--from-app")?;
        let to_app = built_path(given.to_app, "--to-app")?;
        ensure!(
            from_app != to_app,
            "FROM_APP and TO_APP must be distinct source bundles"
        );
        let from = inspect_bundle(&from_app, "FROM_APP")?;
        let to = inspect_bundle(&to_app, "TO_APP")?;
        ensure!(
            compare_versions(&to.version, &from.version)? == Ordering::Greater,
            "the update ({}) must be newer than the installed app ({})",
            to.version,
            from.version
        );
        for info in [&from, &to] {
            // The check reads the plist's version; a CLI manifest, where the bundle has one, says the same.
            if let Some(cli) = &info.cli_version {
                ensure!(
                    *cli == info.version,
                    "{}: its CLI's version is not its own",
                    info.label
                );
            }
            verify_seal(&info.app, given.env)?;
            assert_ad_hoc(&info.app, &info.label, given.env)?;
        }
        Ok(Self {
            from_manifest: digest_manifest(&from_app)?,
            to_manifest: digest_manifest(&to_app)?,
            from,
            release: given.release,
            to,
            key: given.key.clone(),
            keep: given.keep,
            waits: given.waits,
            machines: given.machines.to_path_buf(),
            checkout: given.checkout.to_path_buf(),
            env: given.env.clone(),
        })
    }
}

/// What an install leaves behind that it should not: in the staging folder of the
/// home it extracts into, empty or gone once an install is over, and beside the app
/// it replaces, where the folder holds the app and nothing else.
pub fn staging_left(sandbox: &Sandbox) -> Result<Vec<String>> {
    let mut left = Vec::new();
    let folder = sandbox.state.join("app").join("updates");
    if folder.exists() {
        for entry in fs::read_dir(&folder).map_err(files("list", &folder))? {
            let entry = entry.map_err(files("list", &folder))?;
            left.push(format!("updates/{}", entry.file_name().to_string_lossy()));
        }
    }
    for entry in fs::read_dir(&sandbox.apps).map_err(files("list", &sandbox.apps))? {
        let name = entry.map_err(files("list", &sandbox.apps))?.file_name();
        if name != "ConsensFlow.app" {
            left.push(name.to_string_lossy().into_owned());
        }
    }
    Ok(left)
}

/// One case's machine: the installed app copied in, its feed served, and the
/// app started on it when asked. A case that passed (`finished`) takes its machine
/// away; one that did not leaves it where it is, for whoever has to read it.
pub struct Case<'a> {
    pub sandbox: Sandbox,
    pub feed: Feed,
    pub installed: BundleInfo,
    pub app: Option<App>,
    /// The pids of the second ConsensFlows the case started to see the ledger refuse them.
    pub probes: BTreeSet<u32>,
    pub finished: bool,
    pub inputs: &'a Inputs,
    pub waits: Waits,
    tls: Tls,
    say: Say,
    /// Whether what the case started has been ended, which is once.
    ended: bool,
}

impl<'a> Case<'a> {
    /// The machine of a case, with the installed app in place and its feed up.
    pub fn start(inputs: &'a Inputs, say: &Say) -> Result<Self> {
        let sandbox = Sandbox::make(&inputs.machines)?;
        let tls = make_tls(&sandbox.tls, &inputs.env)?;
        let feed = serve_updates(&tls, say)?;
        copy_bundle(&inputs.from.app, &sandbox.copy, &inputs.env)?;
        ensure!(
            digest_manifest(&sandbox.copy)? == inputs.from_manifest,
            "the installed copy differs from FROM_APP"
        );
        verify_seal(&sandbox.copy, &inputs.env)?;
        let installed = inspect_bundle(&sandbox.copy, "the installed copy")?;
        Ok(Self {
            sandbox,
            feed,
            installed,
            app: None,
            probes: BTreeSet::new(),
            finished: false,
            inputs,
            waits: inputs.waits.for_a_case(),
            tls,
            say: say.clone(),
            ended: false,
        })
    }

    /// Starts the app of the copy now in place, told to find `expected` once it has
    /// been updated, and the failures it reports that are the case's to wait for.
    pub fn start_app(&mut self, expected: &str, failures: &[&str]) -> Result<App> {
        let bundle = inspect_bundle(&self.sandbox.copy, "the app in place")?;
        let env = self.sandbox.app_env(&SelfTest {
            feed: &self.feed.url,
            certificate: &self.tls.ca_cert,
            public_key_file: &self.inputs.key.public_key_file,
            expected,
            deadline: self.inputs.waits.each() + PAGE_GRACE,
        });
        let app = launch_app(
            &Launch {
                binary: &bundle.binary,
                env: &env,
                cwd: &self.sandbox.root,
                expected: failures,
                waits: self.waits,
            },
            &self.say,
        )?;
        self.app = Some(app.clone());
        Ok(app)
    }

    /// Says a line of the case's own: what it found, which a person reading a run wants to know.
    pub fn note(&self, text: impl AsRef<str>) {
        self.say.out(format!("    # {}", text.as_ref()));
    }

    /// Ends what the case started, and takes its machine away if it passed. A case
    /// that is let go of without it (a panic) is ended all the same.
    pub fn cleanup(&mut self) {
        if std::mem::replace(&mut self.ended, true) {
            return;
        }
        if let Some(app) = &self.app {
            app.kill_recorded();
        }
        let pids: BTreeSet<u32> = self
            .sandbox
            .recorded_pids()
            .unwrap_or_default()
            .into_iter()
            .collect();
        for pid in pids {
            super::processes::kill(pid);
        }
        self.feed.close();
        if !self.finished || self.inputs.keep {
            self.say.out(format!(
                "updater smoke: the case's machine is kept at {}",
                self.sandbox.root.display()
            ));
            return;
        }
        if let Err(cause) = remove_all(&self.sandbox.root) {
            self.say.err(format!("updater smoke: {cause}"));
        }
    }

    /// What a failure should say of the machine it happened on.
    pub fn report(&self) -> String {
        format!(
            "\nthe machine is kept at {}\napp.log:\n{}\ndaemon.log:\n{}",
            self.sandbox.root.display(),
            super::evidence::tail(&app_log(&self.sandbox), LOG_SHOWN),
            super::evidence::tail(&daemon_log(&self.sandbox), LOG_SHOWN)
        )
    }
}

impl Drop for Case<'_> {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// The app and its daemon, as the first look at them found them.
#[derive(Debug, Clone)]
pub struct Booted {
    pub app: u32,
    pub daemon: Daemon,
}

/// The app up and its daemon started, from the machine's own words: the page said
/// it booted at the version asked, the app's process is the copy's executable,
/// and the daemon the app chose (Node's or the native one) logged its start, is
/// the app's child, runs this bundle's `cf ui`, and is the only one that runs.
pub fn boot_evidence(
    kase: &Case,
    app: &App,
    version: &str,
    replaced: Option<u32>,
) -> Result<Booted> {
    let boot = app.wait_for(&format!("an update boot at {version}"), |event| {
        event.name() == "update-boot"
            && event.data("currentVersion").and_then(|seen| seen.as_str()) == Some(version)
    })?;
    let pid = boot
        .pid()
        .ok_or_else(|| Error::new(format!("the boot report names no pid: {}", boot.json())))?;
    ensure!(
        replaced != Some(pid),
        "the app that booted is the one that was replaced"
    );
    ensure!(
        alive(pid),
        "the app (pid {pid}) was not alive at update boot"
    );
    let bundle = inspect_bundle(&kase.sandbox.copy, "the app in place")?;
    assert_app(&process_table()?, pid, &bundle.binary.to_string_lossy())?;
    let daemon = daemon_of(
        &kase.sandbox,
        &kase.waits,
        pid,
        &kase.sandbox.copy,
        &kase.probes,
    )?;
    Ok(Booted { app: pid, daemon })
}

/// The daemon holds the ledger: a second ConsensFlow on the home is refused. Asked
/// once the page has had an answer of the daemon (it has the ledger by then, and a
/// probe before that could be the one to take it).
pub fn held_evidence(kase: &mut Case) -> Result {
    let bundle = inspect_bundle(&kase.sandbox.copy, "the app in place")?;
    ledger_held(&bundle, &kase.sandbox, &mut kase.probes)
}

/// The page has two windows open and the install refused for them: the stand-ins
/// are two live processes, and nothing the page started has changed (the app, its
/// daemon). Returns the two stand-ins' pids.
pub fn blocked_evidence(kase: &Case, app: &App, booted: &Booted) -> Result<Vec<u32>> {
    let blocked = app.wait_for("blocked update install report", |event| {
        event.name() == "update-blocked"
    })?;
    let phase = blocked.data("phase").and_then(|phase| phase.as_str());
    ensure!(
        phase == Some("ready"),
        "the blocked install's phase is {}, not ready",
        phase.unwrap_or("none")
    );
    let blockers = blocked
        .data("blockers")
        .and_then(|blockers| blockers.as_array())
        .map(Vec::len);
    ensure!(
        blockers == Some(2),
        "the ready snapshot did not expose both open panes"
    );
    let chiefs = kase.waits.until("two stand-in chief processes", || {
        let pids: BTreeSet<u32> = kase.sandbox.recorded_pids()?.into_iter().collect();
        Ok((pids.len() == 2).then(|| pids.into_iter().collect::<Vec<_>>()))
    })?;
    ensure!(
        chiefs.iter().all(|pid| alive(*pid)),
        "the chiefs were not alive before the blocked install: {}",
        chiefs
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    );
    ensure!(
        alive(booted.app),
        "a blocked install changed the app process"
    );
    ensure!(
        alive(booted.daemon.pid),
        "a blocked install changed the daemon"
    );
    Ok(chiefs)
}

/// Tells the page to go on, and waits for the two windows to close, which is what the install waited for.
pub fn release_panes(kase: &Case, app: &App, chiefs: &[u32]) -> Result {
    app.continue_updater()?;
    kase.waits.until("two stand-in chiefs close", || {
        Ok(chiefs.iter().all(|pid| gone(*pid)).then_some(()))
    })
}

#[cfg(test)]
mod tests;

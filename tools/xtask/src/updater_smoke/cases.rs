//! The packaged update path, end to end: an installed app (the bridge, or the flip
//! release) is given this checkout's app, the release that ships no Node, as an
//! update by its own updater, from a feed this run serves, on a machine of the
//! case's own. Every case here runs once for each installed release that is asked
//! for (`--from`).
//!
//! The cases:
//!
//! - the update: install and restart, the daemon the bundle's `cf` on the same
//!   home, the ledger whole, the terminal command of the home running the
//!   update's `cf` (rewritten, if the installed release's named Node's) and no
//!   other's changed;
//! - the same from a home that took the flip release's way back to Node (a
//!   `use-node` file): the installed app's daemon is Node's, the update's is the
//!   `cf`, on the ledger Node's wrote, and the file is left as the user made it;
//! - two updates the app refuses (a signature that is not the key's, and bundles its
//!   check refuses): the installed bundle is intact and the app still runs;
//! - an app replaced by hand, as a disk image's copy does, with the app quit:
//!   its first start repairs the command and keeps the ledger.

use std::path::PathBuf;
use std::time::Instant;

use super::build::Release;
use super::bundle::Refusal;
use super::case::{Case, Inputs};
use super::evidence::{assert_only_probes_refused, daemon_log};
use super::feed::{signed_update, SignedUpdate};
use super::ledger::{read_ledger, Ledger};
use super::processes::{alive, gone};
use super::say::Say;
use super::{Error, Result};

mod by_hand;
mod refused;
mod update;

/// The schema the bridge's ledger is at: a ledger the update touches is at it or past it.
const BRIDGE_SCHEMA: i64 = 10;

/// A case, by what it is called, whether it applies to the release that is installed, and what it does.
pub struct Spec {
    pub name: String,
    /// Why the case is skipped for the installed release, where it is.
    pub skip: fn(&Inputs) -> Option<String>,
    run: Box<dyn Fn(&mut Case) -> Result>,
}

/// Why the way back to Node is no case of the installed release, where it is
/// none. The bridge has none to take: its daemon is Node's in every home. And a
/// release that ships no Node has none either: nothing in it reads the `use-node`
/// file (the newest release before this checkout's may be one, once the flip
/// is long out).
fn no_way_back(inputs: &Inputs) -> Option<String> {
    if inputs.release != Release::Flip {
        return Some(format!(
            "the {} release has no use-node file to take",
            inputs.release.name()
        ));
    }
    (!inputs.from.node)
        .then(|| "the installed app ships no Node: there is no way back to take".to_string())
}

/// Every case, in the order they run.
pub fn specs() -> Vec<Spec> {
    let mut specs = vec![
        Spec {
            name: "the update installs and restarts on the bundle's cf, keeps the ledger and repairs its own terminal command".into(),
            skip: |_| None,
            run: Box::new(|kase| update::update_flow(kase, false)),
        },
        Spec {
            name: "the update of a home that took the way back to Node starts the bundle's cf on the ledger Node's daemon wrote, and leaves the file".into(),
            skip: no_way_back,
            run: Box::new(|kase| update::update_flow(kase, true)),
        },
        Spec {
            name: "an update signed by another key is refused and the installed app stays intact and running".into(),
            skip: |_| None,
            run: Box::new(refused::stranger),
        },
    ];
    for kind in Refusal::ALL {
        specs.push(Spec {
            name: format!(
                "a signed update whose bundle fails the check ({}) is refused and the installed app stays intact and running",
                kind.name()
            ),
            skip: |_| None,
            run: Box::new(move |kase| refused::bundle(kase, kind)),
        });
    }
    for how in [by_hand::How::Replaced, by_hand::How::CopiedOver] {
        specs.push(Spec {
            name: format!(
                "an app {} by hand, with the app quit, repairs the terminal command at its first start and keeps the ledger",
                how.words()
            ),
            skip: |_| None,
            run: Box::new(move |kase| by_hand::by_hand(kase, how)),
        });
    }
    specs
}

/// How a case ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Passed,
    Skipped(String),
    NotAsked,
    Failed(String),
}

/// What a leg of the smoke, the cases for one installed release, came to.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
}

/// Whether a case of this name is among those asked for: any, or one with a word in its name.
pub fn asked(name: &str, only: &[String]) -> bool {
    only.is_empty() || only.iter().any(|word| name.contains(word.as_str()))
}

/// Runs one case on a machine of its own, which is kept if the case failed, and
/// answers how it ended.
fn run_case(spec: &Spec, inputs: &Inputs, say: &Say) -> Verdict {
    let mut kase = match Case::start(inputs, say) {
        Ok(kase) => kase,
        Err(cause) => return Verdict::Failed(cause.to_string()),
    };
    let ran = (spec.run)(&mut kase);
    let verdict = match ran {
        Ok(()) => {
            kase.finished = true;
            Verdict::Passed
        }
        Err(cause) => Verdict::Failed(cause.then(&kase.report()).to_string()),
    };
    kase.cleanup();
    verdict
}

/// Runs the cases asked for, each on a machine of its own, saying how each ended.
pub fn run_cases(inputs: &Inputs, only: &[String], say: &Say) -> Tally {
    let mut tally = Tally::default();
    for spec in specs() {
        let verdict = if !asked(&spec.name, only) {
            Verdict::NotAsked
        } else if let Some(reason) = (spec.skip)(inputs) {
            Verdict::Skipped(reason)
        } else {
            say.out(format!("-- {}", spec.name));
            let started = Instant::now();
            let verdict = run_case(&spec, inputs, say);
            let seconds = started.elapsed().as_secs_f64();
            match &verdict {
                Verdict::Passed => say.out(format!("ok   {} ({seconds:.1} s)", spec.name)),
                Verdict::Failed(cause) => {
                    say.out(format!("FAIL {} ({seconds:.1} s)", spec.name));
                    for line in cause.lines() {
                        say.out(format!("     {line}"));
                    }
                }
                _ => {}
            }
            verdict
        };
        match &verdict {
            Verdict::Passed => tally.passed += 1,
            Verdict::Failed(_) => tally.failed += 1,
            Verdict::Skipped(reason) => {
                tally.skipped += 1;
                say.out(format!("skip {}: {reason}", spec.name));
            }
            Verdict::NotAsked => {
                tally.skipped += 1;
                say.out(format!(
                    "skip {}: not among the cases asked for ({})",
                    spec.name,
                    only.join(",")
                ));
            }
        }
    }
    tally
}

/// The app's quit, and everything it had gone: the app, its daemons, the windows'
/// stand-ins. The ledger had one holder to the end: no daemon of the app was refused it.
fn quit(kase: &Case, daemons: &[u32]) -> Result<super::app::Exit> {
    let Some(app) = &kase.app else {
        return Err(Error::new("there is no app to quit"));
    };
    app.close_input();
    kase.waits.until("every app process exits", || {
        Ok((!app.any_alive()).then_some(()))
    })?;
    let ended = app.exited()?;
    for pid in daemons {
        ensure!(gone(*pid), "the daemon (pid {pid}) outlived the app");
    }
    let surviving: Vec<u32> = kase
        .sandbox
        .recorded_pids()?
        .into_iter()
        .filter(|pid| alive(*pid))
        .collect();
    ensure!(
        surviving.is_empty(),
        "a stand-in chief survived app shutdown"
    );
    assert_only_probes_refused(&daemon_log(&kase.sandbox), &kase.probes)?;
    Ok(ended)
}

/// The ledger of the case's home, read once no daemon holds it.
fn ledger_of(kase: &Case) -> Result<Ledger> {
    read_ledger(&kase.sandbox.state.join("consensflow.db"))
}

/// The two folders the page opens its projects in.
fn projects_of(kase: &Case) -> Vec<PathBuf> {
    vec![
        kase.sandbox.workspace.clone(),
        kase.sandbox.workspace.join(".consensflow-updater-second"),
    ]
}

/// Offers the update of the app at `app` (the run's update where none is named), signed by the run's key.
fn offer_update(kase: &Case, app: Option<&std::path::Path>) -> Result<SignedUpdate> {
    let inputs = kase.inputs;
    let update = signed_update(
        &inputs.checkout,
        &inputs.key.private_key,
        app.unwrap_or(&inputs.to.app),
        &kase.sandbox.probe,
        &inputs.env,
    )?;
    kase.feed
        .offer(&update.version, &update.signature, update.bytes.clone());
    Ok(update)
}

#[cfg(test)]
mod tests;

//! The daemon's front as a trace's test had it, built from what the daemon
//! gives: the screens over the folder's roster and programs, the agents' API
//! beneath them on the ledger, both served on loopback by the engine's executor.
//! The harness admin asks no feed and runs no program: its seams are scripted
//! and empty, so a trace that reached for either would fail, not reach out.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use cf_base::env::Env;
use cf_catalog::{roster_path, Catalog};
use cf_daemon::api::context::AgentRows;
use cf_daemon::api::{serve, Api};
use cf_daemon::roster::Agents;
use cf_daemon::screens::Screens;
use cf_harness::admin::HarnessAdmin;
use cf_harness::testing::{ManualTime, ScriptedCapture, ScriptedLatest, EPOCH_MS};
use cf_ledger::{open_ledger, Ledger, Options};

use crate::front::Front;
use crate::support::daemon::Kicks;

pub struct Rig {
    pub api: Api,
    pub ledger: Rc<RefCell<Ledger>>,
    pub ledger_file: PathBuf,
    front: Front,
    /// How many times the roster's change was told.
    told: Kicks,
    /// The folder of the daemon's own log and trace, which goes with the rig.
    _state: tempfile::TempDir,
}

/// The front for the UI token `token`, over the environment `env` and a ledger
/// opened at `ledger_file`.
pub async fn start(token: &str, env: Env, ledger_file: &Path) -> Rig {
    let state = tempfile::tempdir().unwrap();
    let ledger = Rc::new(RefCell::new(
        open_ledger(ledger_file, Options::default()).unwrap(),
    ));
    let agents = Rc::new(Agents::new(
        Catalog::bundled().unwrap(),
        roster_path(&env).unwrap(),
    ));
    let front = Front::new(
        state.path(),
        Rc::clone(&ledger),
        Rc::clone(&agents) as Rc<dyn AgentRows>,
    );
    let told = Kicks::new();
    let tell = told.waker();
    let screens = Rc::new(Screens {
        token: token.to_owned(),
        on_roster_change: Rc::new(move || {
            tell();
            Ok(())
        }),
        env: env.clone(),
        agents,
        admin: HarnessAdmin::new(
            env,
            Rc::new(ManualTime::new(EPOCH_MS)),
            Rc::new(ScriptedLatest::default()),
            Rc::new(ScriptedCapture::default()),
        ),
    });
    let api = serve(Rc::clone(&front.context), screens, Rc::clone(&front.spawn))
        .await
        .unwrap();
    Rig {
        api,
        ledger,
        ledger_file: ledger_file.to_path_buf(),
        front,
        told,
        _state: state,
    }
}

impl Rig {
    /// How many times the roster's change was told since this was last asked.
    pub fn take_told(&self) -> usize {
        self.told.take()
    }

    /// How many times the dispatcher was woken since this was last asked.
    pub fn take_kicks(&self) -> usize {
        self.front.take_kicks()
    }
}

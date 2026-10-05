//! The daemon's front as a trace's test had it, built from what the daemon
//! gives: the screens over the folder's roster and programs, the agents' API
//! beneath them on the ledger, both served on loopback by the engine's executor.
//! The harness admin asks no feed and runs no program: its seams are scripted
//! and empty, so a trace that reached for either would fail, not reach out.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use cf_base::env::Env;
use cf_catalog::{roster_path, Catalog};
use cf_daemon::api::context::{AgentRows, Closing, Context};
use cf_daemon::api::credentials::Credentials;
use cf_daemon::api::{serve, Api};
use cf_daemon::errors::Errors;
use cf_daemon::files::{Log, Trace};
use cf_daemon::roster::Agents;
use cf_daemon::screens::Screens;
use cf_daemon::seams::DaemonSpawn;
use cf_harness::admin::HarnessAdmin;
use cf_harness::testing::{ManualTime, ScriptedCapture, ScriptedLatest, EPOCH_MS};
use cf_ledger::{open_ledger, Ledger, Options};

pub struct Rig {
    pub api: Api,
    pub ledger: Rc<RefCell<Ledger>>,
    pub ledger_file: PathBuf,
    /// How many times the roster's change was told, and the dispatcher woken.
    pub told: Rc<Cell<u32>>,
    pub kicks: Rc<Cell<u32>>,
    /// The folder of the daemon's own log and trace, which goes with the rig.
    _state: tempfile::TempDir,
}

/// The front for the UI token `token`, over the environment `env` and a ledger
/// opened at `ledger_file`.
pub async fn start(token: &str, env: Env, ledger_file: &Path) -> Rig {
    let state = tempfile::tempdir().unwrap();
    let log = Rc::new(Log::new(state.path()));
    let trace = Rc::new(Trace::new(state.path()));
    let spawn = Rc::new(DaemonSpawn::new(Rc::new(Errors::new(
        Rc::clone(&log),
        Rc::clone(&trace),
    ))));
    spawn.drive();
    let ledger = Rc::new(RefCell::new(
        open_ledger(ledger_file, Options::default()).unwrap(),
    ));
    let agents = Rc::new(Agents::new(
        Catalog::bundled().unwrap(),
        roster_path(&env).unwrap(),
    ));
    let (told, kicks) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let counted = Rc::clone(&kicks);
    let context = Rc::new(Context {
        ledger: Rc::clone(&ledger),
        credentials: Rc::new(Credentials::new()),
        kick: Rc::new(move || counted.set(counted.get() + 1)),
        closing: Closing::new(),
        roster: Rc::clone(&agents) as Rc<dyn AgentRows>,
        log,
        trace,
    });
    let changed = Rc::clone(&told);
    let screens = Rc::new(Screens {
        token: token.to_owned(),
        on_roster_change: Rc::new(move || {
            changed.set(changed.get() + 1);
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
    let api = serve(context, screens, spawn).await.unwrap();
    Rig {
        api,
        ledger,
        ledger_file: ledger_file.to_path_buf(),
        told,
        kicks,
        _state: state,
    }
}

impl Rig {
    /// `host:port`, where the API listens.
    pub fn address(&self) -> String {
        self.api
            .url()
            .strip_prefix("http://")
            .unwrap_or_else(|| panic!("an address: {}", self.api.url()))
            .to_owned()
    }
}

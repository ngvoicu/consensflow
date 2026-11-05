//! The daemon's front as a trace's test had it, built from what the daemon
//! gives: the screens over the folder's programs, the agents' API
//! beneath them on the ledger, both served on loopback by the engine's executor.
//! The harness admin asks no feed and runs no program: its seams are scripted
//! and empty, so a trace that reached for either would fail, not reach out.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use cf_base::env::Env;
use cf_daemon::api::{serve, Api};
use cf_ledger::{open_ledger, Ledger, Options};

use crate::front::{screens, Front};

pub struct Rig {
    pub api: Api,
    pub ledger: Rc<RefCell<Ledger>>,
    pub ledger_file: PathBuf,
    front: Front,
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
    let front = Front::new(state.path(), Rc::clone(&ledger));
    let screens = screens(token, env);
    let api = serve(Rc::clone(&front.context), screens, Rc::clone(&front.spawn))
        .await
        .unwrap();
    Rig {
        api,
        ledger,
        ledger_file: ledger_file.to_path_buf(),
        front,
        _state: state,
    }
}

impl Rig {
    /// How many times the dispatcher was woken since this was last asked.
    pub fn take_kicks(&self) -> usize {
        self.front.take_kicks()
    }
}

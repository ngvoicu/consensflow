//! The switch: the adapter that launches a harness's windows, built with
//! the engine's services.

use std::path::PathBuf;
use std::rc::Rc;

use cf_base::env::Env;
use cf_proto::agents::Harness;

use crate::claude::ClaudeAdapter;
use crate::codex::CodexAdapter;
use crate::contract::Adapter;
use crate::devin::DevinAdapter;
use crate::muse::MuseAdapter;
use crate::pi::PiAdapter;
use crate::seams::Services;

/// The adapter of `harness`'s windows.
pub fn adapter(harness: Harness, services: &Services) -> Rc<dyn Adapter> {
    match harness {
        Harness::Claude => Rc::new(ClaudeAdapter::new(services)),
        Harness::Codex => Rc::new(CodexAdapter::new(services)),
        Harness::Devin => Rc::new(DevinAdapter::new(services)),
        Harness::Pi => Rc::new(PiAdapter::new(services)),
        Harness::Muse => Rc::new(MuseAdapter::new(services)),
    }
}

/// The background work `harness` runs beside its windows, with the config of one of them: the
/// folders it works in, and the words that tell a `cf` run there why it changes nothing on the
/// board. None for a harness that runs none (Codex's memory job is the one there is:
/// `codex::memories`; the others' background forks run no board command, read from their
/// binaries, 2026-11-08).
pub(crate) fn background_work(harness: Harness, env: &Env) -> Option<(Vec<PathBuf>, &'static str)> {
    match harness {
        Harness::Codex => Some((
            crate::codex::memories::memories_folders(env),
            crate::codex::memories::WORDS,
        )),
        Harness::Claude | Harness::Devin | Harness::Pi | Harness::Muse => None,
    }
}

//! What the cases share: the runtime they run on, a folder as the system names
//! it, a pattern to match, and the wires a channel is given.

use std::future::Future;
use std::path::Path;

use cf_harness::contract::Pane;
use cf_harness::opencode::{real_path, Wires};
use cf_harness::seams::{SystemLoopback, SystemProcesses, SystemTime};
use regex::Regex;
use tokio::task::LocalSet;

/// `test`, on one thread with the local tasks the stand-in servers spawn, as
/// the engine's work runs.
pub fn run<T>(test: impl Future<Output = T>) -> T {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    LocalSet::new().block_on(&runtime, test)
}

/// Whether `pattern` matches anywhere in `text`: what the suites' `rejects(…,
/// /…/)` asked of a failure.
pub fn matches(text: &str, pattern: &str) -> bool {
    Regex::new(pattern).unwrap().is_match(text)
}

/// `folder` as the channel names it to a server, and a server names it back:
/// with its links followed, as the system writes it.
pub fn real_folder(folder: &Path) -> String {
    real_path(&folder.to_string_lossy()).unwrap()
}

/// A window's pane, by the pane host's id and the generation it opened in.
pub fn pane(id: &str, generation: u64) -> Pane {
    Pane {
        id: id.to_owned(),
        generation,
    }
}

/// What OpenCode's channel is given to wait and ask with: the system's clock
/// and loopback, and `processes`.
pub fn wires(processes: &SystemProcesses) -> Wires<'_> {
    Wires {
        time: &SystemTime,
        loopback: &SystemLoopback,
        processes,
    }
}

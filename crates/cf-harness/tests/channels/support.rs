//! What the cases share: the runtime they run on, a pattern to match, and a
//! window's pane.

use std::future::Future;

use cf_harness::contract::Pane;
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

/// A window's pane, by the pane host's id and the generation it opened in.
pub fn pane(id: &str, generation: u64) -> Pane {
    Pane {
        id: id.to_owned(),
        generation,
    }
}

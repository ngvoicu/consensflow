//! A case of a test binary that a test of its runs again, as a program of its own: to see what
//! the case says of itself (the rig's reports, `tests/rig/reports.rs`), which only its own
//! process can show.
//!
//! The process is held to what it runs, not to the 30 s a verb of `cf` is given
//! ([`crate::process::LIMIT`]): a case that starts a rig may take as long as the rig's own waits
//! let it, and those are longer (the daemon and the pane host each have a minute to say they are
//! ready). And it builds nothing: the programs under test are the ones this run has
//! ([`crate::cf::BUILT`]), so it runs no cargo of its own, nor waits on the build lock where
//! other processes build (nextest's). A nested case on main's CI after 3.0.6-alpha.3's
//! integration B ran cargo for `cf` and the pane host and copied both into its home, on a runner
//! of three processors beside the run's other rig cases, and was ended at the 30 s; on a Mac at
//! a load of 80 one was ended between its copy of `cf` and its pane host's (T-848).

use std::path::Path;
use std::time::Duration;

use super::launch::READY;
use super::{EXIT, GONE};
use crate::process::{self, Run};
use crate::{bundle, cf, Error, Result};

/// The most a rig's own waits let its start and its close take, the case's own waits aside: the
/// copies of `cf` and of the pane host, each tried while a build has taken it away; the daemon's
/// handle line and the pane host's first line, each waited for [`READY`]; then the daemon and the
/// pane host each given [`EXIT`] to end, and the windows' processes [`GONE`] to be gone.
pub const RIG_BOUND: Duration = Duration::from_secs(
    2 * bundle::COPY_WAIT.as_secs() + 2 * READY.as_secs() + 2 * EXIT.as_secs() + GONE.as_secs(),
);

/// The case `case` of the test binary `binary` (this one, `std::env::current_exe()`), run alone
/// as its own program: with the programs under test this run has, and as long as a rig's start
/// and close may take ([`RIG_BOUND`]) and `waits` more, the case's own. The words after are the
/// caller's (`--nocapture`, `--ignored`).
pub fn case_of(binary: &Path, case: &str, waits: Duration) -> Result<Run> {
    let built = cf::binary()?;
    let host = cf::pane_host()?;
    let folder = built
        .parent()
        .filter(|folder| host.parent() == Some(*folder))
        .ok_or_else(|| {
            Error::Build(format!(
                "the programs under test are not in one folder: {} and {}",
                built.display(),
                host.display()
            ))
        })?;
    Ok(process::case_of(binary, case, RIG_BOUND + waits)
        .arg("--test-threads=1")
        .inheriting_env()
        .var(cf::BUILT, folder))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bound is the rig's own: never shorter than its start may take, and so longer than a
    /// verb of `cf` is given.
    #[test]
    fn a_nested_rig_case_is_given_as_long_as_a_rigs_start_and_close_may_take() {
        assert!(
            RIG_BOUND >= 2 * bundle::COPY_WAIT + 2 * READY + 2 * EXIT + GONE,
            "{RIG_BOUND:?}"
        );
        assert!(RIG_BOUND > crate::process::LIMIT, "{RIG_BOUND:?}");
    }
}

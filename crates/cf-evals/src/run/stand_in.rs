//! The stand-in reviewer of a run: a scenario with `stand_in_reviews` has one reviewer in its
//! staff, a roster agent on the model [`MODEL`], and the eval's `claude` wrapper starts the
//! window of that model as the stand-in agent (`fake-agent` of `crates/cf-e2e`) with the
//! folder of this run's results in `CF_TEST_SCRIPTED_RESULTS`: the stand-in answers each task
//! brief with the next review of the scenario ([`crate::scenarios::Scenario::stand_in_reviews`]).
//!
//! The folder is the run's and nothing else's. It holds a marker, `.eval-run`, and the reviews
//! as `1.md`, `2.md`, and so on; it is made again as each play begins (the stand-in counts the
//! briefs it has taken in the folder, since every review is a fresh window), and removed when
//! the run is over, so that a wrapper left on disk refuses the model. Both the wrapper and the
//! stand-in refuse to answer without the marker: the stand-in never answers a real launch.

use std::fs;
use std::path::{Path, PathBuf};

use cf_e2e::files;

use super::machine::Machine;
use crate::scenarios::Scenario;
use crate::Result;

/// The model the stand-in reviewer's roster agent is on, which the wrapper reads in the
/// arguments of a window ([`crate::plan::with_stand_in_reviewer`] puts the agent on it).
pub use crate::plan::STAND_IN_MODEL as MODEL;

/// The variable that names the folder of the results, as the stand-in reads it
/// (`crates/cf-e2e/src/bin/fake_agent/scripted.rs`).
pub const FOLDER: &str = "CF_TEST_SCRIPTED_RESULTS";

/// The file that says the folder is an eval run's.
pub const MARKER: &str = ".eval-run";

/// The stand-in reviewer of one run.
#[derive(Debug)]
pub struct Reviews {
    folder: PathBuf,
    agent: PathBuf,
    results: Vec<String>,
}

impl Reviews {
    /// The stand-in of a run of `scenario` on `machine`, played by the program `agent`;
    /// none for a scenario that has no stand-in. The folder is not made yet.
    pub fn of(machine: &Machine, scenario: &Scenario, agent: &Path) -> Option<Self> {
        (!scenario.stand_in_reviews.is_empty()).then(|| Self {
            folder: machine.scripted_reviews(),
            agent: agent.to_path_buf(),
            results: scenario.stand_in_reviews.clone(),
        })
    }

    /// The program that plays the stand-in.
    pub fn agent(&self) -> &Path {
        &self.agent
    }

    /// The folder of the results.
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// The folder as a play begins: made again, with the marker and the reviews, and no brief
    /// taken.
    pub fn restart(&self) -> Result {
        // A folder that is not there is already fresh.
        let _ = fs::remove_dir_all(&self.folder);
        files::make_dir(&self.folder)?;
        files::write(&self.folder.join(MARKER), "")?;
        for (at, result) in self.results.iter().enumerate() {
            files::write(&self.folder.join(format!("{}.md", at + 1)), result)?;
        }
        Ok(())
    }
}

impl Drop for Reviews {
    /// The run is over: the folder goes with it, and a wrapper left on disk refuses.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.folder);
    }
}

#[cfg(test)]
mod tests;

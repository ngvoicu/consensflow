//! The agents of the run: one for each harness, on its cheap model
//! ([`cf_e2e::live::agents`], which says which), written to the roster of the
//! run's home before a project opens. Each is the chief of one project and a
//! member of others, so one row serves both.

use cf_e2e::live::agents as shared;
use cf_e2e::live::{Harness, Log};
use cf_e2e::rig::Rig;
use cf_e2e::Result;

pub use shared::{id, question_tool, DESIGNER};

/// The tier every member of the run is of. A project has one member in each
/// seat, and a task names its seat's pool (a worker's, an advisor's, a
/// reviewer's), so the tier tells nothing apart; a tier that is not critical
/// asks for no purpose.
pub const TIER: &str = "standard";

/// Writes the roster of the rig's home: the agents of `harnesses`, over the
/// one the rig started with, each noted in the run's `log` as what its windows
/// are launched on.
pub fn write_roster(rig: &Rig, harnesses: &[Harness], log: &Log) -> Result {
    let rows: Vec<_> = harnesses
        .iter()
        .map(|harness| shared::row(*harness, TIER))
        .collect();
    shared::write_roster(rig, &rows, log)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_member_of_the_run_is_of_one_tier_which_asks_for_no_purpose() {
        for harness in Harness::ALL {
            assert_eq!(shared::row(harness, TIER)["workTier"], TIER);
        }
        assert_ne!(TIER, "critical");
    }
}

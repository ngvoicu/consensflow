//! The live matrix (spec `live-matrix`, LMX-04): a real harness in every position of a board,
//! a real chief driven by a scripted human, and an oracle that reads the ledger and the
//! windows' own records, never the models' words. The design is T-1133's (LMX-03); this is its
//! first row, the Mac's vertical slice: board 0 ([`seating::BOARD_0`]) and the `smoke`
//! scenario ([`scenario::smoke`]).
//!
//! - [`seating`]: who sits where, each member in one role at `standard`, added as the human
//!   adds staff;
//! - [`human`]: the scripted human, who types each ask as a person does and proves it went in,
//!   within the scenario's deadline and turn ceiling;
//! - [`observe`]: the board, the events and the records, and the tasks bound by what they are;
//! - [`scenario`]: the fixture, the asks and the milestones;
//! - [`oracle`]: the checks;
//! - [`evidence`]: the run's folder under `evals/reports/live-matrix/`, and the owner's files;
//! - [`run`]: one seating and one scenario, start to end.
//!
//! The live suite is `tests/live_matrix.rs` (`cargo xtask live matrix --board 0 --scenario
//! smoke -- --test-threads 1`); its control runs the same driver and oracle on the rig's
//! stand-ins, which is no coverage of any harness.

pub mod evidence;
pub mod human;
pub mod machine;
pub mod observe;
pub mod oracle;
pub mod run;
pub mod scenario;
pub mod seating;

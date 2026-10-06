//! ConsensFlow's integration with each coding-agent harness: one module per
//! harness, each importing only what all of them share (`shared`), never a
//! sibling. It holds the hooks a harness runs through `cf hook`, the
//! readers of what each harness's own record of a conversation says
//! ([`records`]), where each harness's CLI is ([`detect`]), how the engine
//! launches a harness's window and works with it ([`contract`]), what it is
//! given to do so ([`seams`]), which adapter launches which harness
//! ([`launch::adapter`]), the files a launch leaves ([`forget_launch`],
//! [`sweep_launches`]), and what opening the app prepares: the terminal
//! command beside the Pi and OpenCode extensions ([`prepare`], which, like
//! the adapters' table, sits above the harnesses' modules).

#![forbid(unsafe_code)]

pub mod admin;
pub mod claude;
pub mod codex;
pub mod contract;
pub mod detect;
pub mod devin;
pub mod launch;
pub mod opencode;
pub mod pi;
pub mod prepare;
pub mod records;
pub mod seams;
mod shared;

pub use shared::launch_files::{forget_launch, sweep_launches};

// Fakes other crates' tests compile too: a failure in one is the test's.
#[cfg(any(test, feature = "test-support"))]
#[allow(clippy::expect_used, clippy::unwrap_used)]
pub mod testing;

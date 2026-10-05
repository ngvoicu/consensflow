//! ConsensFlow's engine (`src/core/`): the dispatcher, which steps every
//! window on each pass and answers the human's operations, and the owners of
//! what it orchestrates. Like Node's, it runs on one thread: its work
//! interleaves where it waits, each participant held by one piece of work at
//! a time ([`runtime`]), and a window's pane is opened, killed and ended
//! through the pane host ([`host`]).

#![forbid(unsafe_code)]

pub mod host;
pub mod runtime;

// The kit other crates' tests run the engine with: a failure in it is the
// test's.
#[cfg(any(test, feature = "test-support"))]
#[allow(clippy::expect_used, clippy::unwrap_used)]
pub mod testing;

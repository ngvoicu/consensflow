//! ConsensFlow's engine (`src/core/`): the dispatcher, which steps every
//! window on each pass and answers the human's operations, and the owners of
//! what it orchestrates. Like Node's, it runs on one thread: its work
//! interleaves where it waits, each participant held by one piece of work at
//! a time ([`runtime`]), and a window's pane is opened, killed and ended
//! through the pane host ([`host`]).
//!
//! What it writes into windows is text alone, apart: how a message reads in
//! its recipient's pane ([`delivery_text`]), what a chief the human switched
//! in is first told and can read of the chiefs before it ([`handoff`]), and
//! the instructions each role's window starts with ([`roles`]). Those touch
//! no window, ledger or file the daemon owns: the ledger's rows come in as
//! `cf_proto::ledger`'s views, and what a text is made of that is not in them
//! (a message by its id, a role's card) is handed in.

#![forbid(unsafe_code)]

pub mod delivery_text;
pub mod handoff;
pub mod host;
pub mod roles;
pub mod runtime;

// The kit other crates' tests run the engine with: a failure in it is the
// test's.
#[cfg(any(test, feature = "test-support"))]
#[allow(clippy::expect_used, clippy::unwrap_used)]
pub mod testing;

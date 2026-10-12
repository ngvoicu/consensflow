//! The eval's numbers come from the ledger file, read with SQLite. Each test reads
//! a ledger recorded while Node's ledger could still build one
//! (`tests/fixtures/ledgers`, fixed since Node went), where each number is the one
//! the sequence of operations behind it leaves. The plants of the daemon (`cargo
//! xtask plants daemon board decided`) run this suite: it is what holds the
//! measure to the board's own rules.

#![allow(clippy::unwrap_used)]

mod support;

// The cases are in `tests/measure/`: cargo looks for the modules of a test's root
// file beside it, where each would be a suite of its own.
#[path = "measure/chief.rs"]
mod chief;
#[path = "measure/open.rs"]
mod open;
#[path = "measure/parallel.rs"]
mod parallel;
#[path = "measure/placed.rs"]
mod placed;
#[path = "measure/recordings.rs"]
mod recordings;

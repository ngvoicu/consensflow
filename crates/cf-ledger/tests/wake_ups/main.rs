//! A project's wake-up calls in the ledger: the setting and what it is by
//! default, the migration that made room for it, when a wake-up is due (the
//! interval and what it counts from, what moved since the last one, the first
//! after the calls were turned on or the project resumed), and what is
//! withdrawn when the project closes or the calls go off. The clock is the
//! test's: a wake-up is due at the minute the test says it is.

// The tests' own scaffolding: a failure in it is the test's.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod due;
mod migration;
mod moved;
mod setting;
mod withdrawn;
mod world;

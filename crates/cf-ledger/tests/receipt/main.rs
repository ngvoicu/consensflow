//! The receipt and stop redesign in the ledger: what a pause keeps, what
//! rides in the paste of the words that resume it, what makes a message
//! received and what that does to its task, the doors and their claims, the
//! stops a window owes, and what a ledger Node wrote means to this one (the
//! rules are the docs of `messages/carrying.rs`, `messages/receipt.rs` and
//! `tasks/pausing.rs`). Each test is a sequence of what the callers do, on a
//! ticking clock, and asserts what the ledger then says.

// The tests' own scaffolding: a failure in it is the test's.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod carrying;
mod doors;
mod fixture;
mod gate;
mod legacy;
mod questions;
mod sessions;
mod transfers;

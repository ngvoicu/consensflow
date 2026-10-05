//! Every shape that crosses a process boundary between ConsensFlow's
//! programs, and nothing else: wire types and the values that name them. Two
//! programs that talk read the same definitions here, so a field renamed on
//! one side cannot drift from the other.

#![forbid(unsafe_code)]

pub mod agents;
pub mod bridge;
pub mod codex;
pub mod ledger;
pub mod page;
pub mod questions;
pub mod trace;

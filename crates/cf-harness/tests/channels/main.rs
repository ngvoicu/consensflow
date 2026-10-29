//! The channels of Codex and Pi against what they run on in a window:
//! the machine's own clock, sockets, processes and randomness, and a server on
//! loopback standing where the harness's own would (`testing::server`). They
//! are the only Rust tests that drive the channels on `SystemTime`,
//! `SystemLoopback` and `SystemEntropy`: `tests/launch` and
//! the channels' unit tests play the same channels on a clock and a loopback
//! that a test moves by hand. Here the timers are real, so a case that waits
//! for one takes that long.
//!
//! - `contract`: one contract for every channel's answer to a send;
//! - `codex`: Codex's channel to its broker;
//! - `pi_window`: a stand-in for Pi's window, which the contract's Pi rows send to.
//!
//! Each case is one of Node's delivery-contract and Codex channel suites,
//! which ran these channels through test binaries, ported under its sentence.

// A stand-in's own work is the test's: a failure in it is the test's answer.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod codex;
mod contract;
mod pi_window;
mod support;

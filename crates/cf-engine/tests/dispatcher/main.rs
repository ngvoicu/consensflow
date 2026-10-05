//! The dispatcher's tests (`tests/core-dispatcher.test.mjs`), ported under
//! their sentences, each held to the Node trace of the same test
//! ([`traces`]).

// The tests' own scaffolding: a failure in it is the test's.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod chief_not_up;
mod chiefs;
mod lanes;
mod switching;
mod switching_deleted;
mod switching_handoffs;
mod switching_refused;
mod the_dispatcher;
mod traces;

//! The launch's goldens (`tests/goldens/launch/`, recorded from Node and fixed
//! since): what Node answered for step 3.4, held case by case. The tables of
//! the pure functions `cf-base` exports for it (the text a window takes, the
//! text Windows' console carries, a path as a file URL), each adapter's
//! scenarios, played step by step, and each adapter's JavaScript tests, ported
//! under their sentences.

// The goldens' own reading: a failure in it is the test's.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod claude;
mod codex;
mod coverage;
mod devin;
mod opencode;
mod pi;
mod scenarios;
mod tables;

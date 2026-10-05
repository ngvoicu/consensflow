//! The harness admin's goldens (`npm run goldens:admin`, from
//! `tests/goldens/admin/`): what Node answers for step 3.6's admin and the
//! detection beside it, held case by case. The tables of layouts that are no
//! more than a text, which hold on every system, and each scenario played
//! step by step, one set a platform: the files it starts with, the answers of
//! the programs it runs and of the feeds it asks (or of the network, where the
//! admin asks Node's own feed), and the clock it moves, each step's answer, its
//! calls and what it left waiting.
//!
//! The scenarios of a platform are recorded on it (`npm run goldens:admin`).

// The goldens' own reading: a failure in it is the test's.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod scenarios;
mod shape;
mod tables;

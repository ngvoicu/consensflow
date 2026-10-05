//! The launch's goldens (`npm run goldens:launch`, from
//! `tests/goldens/launch/`): what Node answers for step 3.4, held case by
//! case. Here, the tables of the pure functions `cf-base` exports for it:
//! the text a window takes, the text Windows' console carries, a path as a
//! file URL.

// The goldens' own reading: a failure in it is the test's.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod tables;

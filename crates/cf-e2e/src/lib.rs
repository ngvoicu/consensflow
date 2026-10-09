//! ConsensFlow's black-box suites. The built `cf` is run as a process, in a
//! home of each case's own, and judged by what it prints, writes and exits
//! with. This crate depends on no crate of the product: what a suite proves is
//! what a person's terminal would see, and it talks to the product through
//! processes, files and HTTP alone.
//!
//! The support is a module for each thing it does:
//!
//! - [`process`] starts a program and keeps what it printed, said and exited
//!   with. It is the only module that starts one.
//! - [`cf`] builds the `cf` under test, once for a run of the tests.
//! - [`scratch_home`] is a home of a case's own, with the environment that
//!   points `cf` at it.
//! - [`files`] sets up and reads the files a case looks at.
//! - [`checkout`] finds the repository's own files for the suites that read
//!   its words.
//!
//! The suites are the tests of the crate, a file for each (`tests/cli.rs` is
//! the CLI's, with its cases in `tests/cli/`). Run one with
//! `cargo test -p cf-e2e --test cli`; `npm run test:clis` does.

#![forbid(unsafe_code)]

pub mod cf;
pub mod checkout;
mod error;
pub mod files;
pub mod process;
pub mod scratch_home;
mod stand_in;

pub use error::{Error, Result};
pub use scratch_home::ScratchHome;

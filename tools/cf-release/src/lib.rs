//! The release's steps as one command, `cf-release`: `version` (what the
//! sources agree on), `prepare-update` (the update feed's entry for a build) and
//! `sign-mac` (the Developer ID signing). The release workflow is to build it
//! with `--locked` and run the binary; `cargo xtask` asks its [`version`] module
//! the one question of the sources. It depends on no crate of the product, only
//! on `cf-base` for the environment, which `main` reads once.
//!
//! Who edits what. `prepare_update` is S4's and `sign_mac` is S5's: each landing
//! builds its step in that module and its tests, and uses [`process`] for the
//! programs it runs. The lead's are `cli` (the list of steps), the manifest and
//! the lockfile, and the call sites in the workflows.

#![forbid(unsafe_code)]

mod cli;
mod prepare_update;
pub mod process;
mod sign_mac;
pub mod version;

pub use cli::run;

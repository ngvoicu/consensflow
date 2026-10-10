//! `cargo xtask <command>`: what builds, stages, tests and drives ConsensFlow,
//! from the root of the checkout or from any folder below it. The alias is in
//! `.cargo/config.toml`; `cargo xtask --help` lists the commands.
//!
//! One module for each landing that ported a script holds the commands it owns as
//! a table, and the tables are joined in [`dispatch`]. Every command runs in
//! Rust: the last two that handed their arguments to a Node script and answered
//! its exit status (`smoke` and `candidate`) were ported at S11. It depends on
//! no crate of the product, only on `cf-base` for the environment (read once, in
//! `main`) and on `cf-process` to find a program on the PATH, so it builds with
//! no staged resources and no GUI toolchain.
//!
//! Who edits what. The landing that ports a command edits the module that owns
//! it and that module's tests: `sidecar` (build-cf, stage, conpty) is S1's,
//! `portable` is S2's, `app`, `clippy_windows`, `departures` and `bench` are
//! S10's (the thin drivers, which run cargo and nothing else), `suites` is S8's
//! and S9's, `smoke` (the packaged smoke, which runs the smoke test of
//! `crates/cf-e2e`) and `candidate` are S11's, and `updater_smoke` is S12's: the
//! updater smoke in Rust, which `smoke` lists as `smoke-updater`. The lead's are
//! this list of modules, `dispatch`, `process`, `context`, `check`, the manifests
//! and the lockfile, the alias, and the call sites (the npm scripts, the
//! workflows, the drivers that run a command).

#![forbid(unsafe_code)]

mod app;
mod bench;
mod candidate;
mod check;
mod clippy_windows;
pub mod context;
mod departures;
mod dispatch;
mod portable;
pub mod process;
mod sidecar;
mod smoke;
mod suites;
mod updater_smoke;

pub use dispatch::run;

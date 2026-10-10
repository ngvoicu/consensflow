//! `cargo xtask <command>`: what builds, stages, tests and drives ConsensFlow,
//! from the root of the checkout or from any folder below it. The alias is in
//! `.cargo/config.toml`; `cargo xtask --help` lists the commands.
//!
//! One module for each landing that ports a script holds the commands it owns as
//! a table, and the tables are joined in [`dispatch`]. A command runs in Rust,
//! or, until its landing, hands its arguments to the Node script it replaces and
//! answers that script's exit status. It depends on no crate of the product,
//! only on `cf-base` for the environment (read once, in `main`) and on
//! `cf-process` to find a program on the PATH, so it builds with no staged
//! resources and no GUI toolchain.
//!
//! Who edits what. The landing that ports a command edits the module that owns
//! it and that module's tests, and turns the command's row from `Run::Node` into
//! `Run::Native`: `sidecar` (build-cf, stage, conpty) is S1's, `portable` is
//! S2's, `app`, `clippy_windows`, `departures` and `bench` are S10's (the thin
//! drivers, which run cargo and nothing else), `suites`, `smoke` and
//! `candidate` are S6's, and `updater_smoke` is S12's: the updater smoke in
//! Rust, which `smoke` lists as `smoke-updater`. The lead's are this list of
//! modules, `dispatch`, `process`, `context`, `check`, the manifests and the
//! lockfile, the alias, and the call sites (the npm scripts, the workflows, the
//! drivers that run a command).

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
#[cfg(test)]
mod testing;
mod updater_smoke;

pub use dispatch::run;

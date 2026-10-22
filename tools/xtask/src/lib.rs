//! `cargo xtask <command>`: what builds, stages, tests and drives ConsensFlow,
//! from the root of the checkout or from any folder below it. The alias is in
//! `.cargo/config.toml`; `cargo xtask --help` lists the commands.
//!
//! Every command runs in Rust. A module holds the commands it owns as a table,
//! and the tables are joined in `dispatch`. It depends on no crate of the
//! product, only on `cf-base` for the environment (read once, in `main`), on
//! `cf-process` to find a program on the PATH, on the release tools `cf-release`
//! and `cf-publish` (the order of releases, and what a feed names), and on the
//! crate `cf-portable` that writes and reads the portable exe, so it
//! builds with no staged resources and no GUI toolchain.
//!
//! The modules, by what they do:
//!
//! - what the app's bundle is made of: `sidecar` (`build-cf`, `stage`, `conpty`),
//!   `portable` (the portable Windows exe) and `candidate` (this checkout built
//!   as another app, installed beside the live one);
//! - the gate, and what runs a part of it or a suite beside it: `check` (the whole
//!   gate), `app` and `clippy_windows` (the lint and the tests where the app or
//!   the platform cannot be built as it ships), `suites` (`test daemons`,
//!   `daemon-and-rig`, `integration`, `clis`, `agents`, `load`: the black-box
//!   suites of `crates/cf-e2e`), `departures` and `bench` (thin drivers of cargo);
//! - the app as built, run as a user would: `smoke` (the packaged smoke, and
//!   `smoke-updater`, which is `updater_smoke`);
//! - the mutation tests: `plants`, with their data in `tests/plants`;
//! - the releases: `release` (`release alpha`, and `release promote` of a tested
//!   alpha to its plain version);
//! - what spends real quota and is no part of the gate: `live` (one command for
//!   every live test of `crates/cf-e2e`, whatever its name) and `evals` (`eval`,
//!   `eval summary`, `eval long` and `watch`, which hand their words to
//!   `crates/cf-evals`);
//! - the Windows machine: `windows` (`windows` and `windows-matrix`: a command,
//!   and the matrix of the chief evals, run there over ssh), with `stops`, what
//!   ends a run early, which it shares with `plants`;
//! - `scrub_transcript`: the fixtures that are made of a Claude transcript;
//! - `specmint`: the Specmint skills vendored in `skill/specmint`, refreshed from
//!   checkouts of their repositories;
//! - what every command shares: `dispatch` (the command line), `context` (where
//!   it works) and `process` (the one place a program starts).

#![forbid(unsafe_code)]

mod app;
mod bench;
mod candidate;
mod check;
mod clippy_windows;
pub mod context;
mod departures;
mod dispatch;
mod evals;
mod live;
mod plants;
mod portable;
pub mod process;
mod release;
mod scrub_transcript;
mod sidecar;
mod smoke;
mod specmint;
mod stops;
mod suites;
mod updater_smoke;
mod windows;

pub use dispatch::run;

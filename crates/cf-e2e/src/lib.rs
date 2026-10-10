//! ConsensFlow's black-box suites. The built `cf` is run as a process, in a
//! home of each case's own, and judged by what it prints, writes and exits
//! with; the daemon it starts, and the pane host that gives each window its
//! terminal, are run as the app runs them and judged by what they answer. This
//! crate depends on no crate of the product: what a suite proves is what a
//! person's terminal, or the page, would see, and it talks to the product
//! through processes, files and HTTP alone.
//!
//! The support is a module for each thing it does:
//!
//! - [`process`] starts a program and keeps what it printed, said and exited
//!   with, or leaves it running for a case to drive. It is the only module
//!   that starts one.
//! - [`cf`] builds the `cf` and the pane host under test, once for a run of the
//!   tests.
//! - [`scratch_home`] is a home of a case's own, with the environment that
//!   points `cf` at the folders in it.
//! - [`files`] sets up and reads the files a case looks at.
//! - [`checkout`] finds the repository's own files for the suites that read
//!   its words.
//! - [`wire`] reads the bridge's frames off a program's streams.
//! - [`daemon`] is `cf ui` as a process, on a home of its own.
//! - [`rig`] is the daemon and the pane host together, with stand-in agents in
//!   the windows, driven as the page and the app drive them.
//! - [`daemon_log`] reads a daemon's log; [`agents_proof`] holds a daemon's
//!   agents screens to what they must do; [`http`] asks a daemon's API. (The
//!   packaged smoke uses all three, on the daemon the built app chose.)
//! - [`live`] is what the opt-in live tests share: the machine's own Codex,
//!   found as a shell finds it, and the variable that keeps what a run made.
//!   [`png`] reads the structure of a PNG file, to say that an image a harness
//!   saved is a whole one.
//!
//! The suites are the tests of the crate, a file for each (`tests/cli.rs` is
//! the CLI's, with its cases in `tests/cli/`; `tests/daemon.rs` the daemon's as
//! a process; `tests/rig.rs` the rig's, windows included; `tests/load.rs` the
//! daemon under load; `tests/smoke.rs` the built app's, which no other suite
//! has: the packaged smoke, a macOS bundle's, with the parts it is made of in
//! `tests/smoke/`; `tests/live_designer.rs` a real Codex drawing an image).
//! Run them with `cargo xtask test clis`, `daemons`, `integration`, `agents`
//! and `load`, and `cargo xtask smoke` (the `npm run` names are the same), the
//! live one with `cargo xtask live designer`, or one with `cargo test -p cf-e2e
//! --test cli`.
//!
//! The stand-ins the suites run in place of what ConsensFlow runs are
//! binaries of this crate, behind the `test-support` feature the crate's own
//! tests turn on: `fake-agent`, `fake-codex`, `liar-daemon` and the
//! packaged smoke's `smoke-paste-reader`.

#![forbid(unsafe_code)]

pub mod agents_proof;
pub mod cf;
pub mod checkout;
pub mod daemon;
pub mod daemon_log;
mod error;
pub mod files;
pub mod http;
pub mod live;
pub mod pattern;
pub mod png;
pub mod process;
pub mod rig;
pub mod scratch_home;
pub mod serial;
mod stand_in;
pub mod wire;

pub use error::{Error, Result};
pub use scratch_home::ScratchHome;

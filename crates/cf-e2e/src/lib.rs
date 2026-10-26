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
//! - [`devin_store`] is where Devin keeps its store of sessions, and the messages
//!   it holds, for the stand-in that writes it and the cases that read it.
//! - [`daemon`] is `cf ui` as a process, on a home of its own.
//! - [`rig`] is the daemon and the pane host together, with stand-in agents in
//!   the windows, driven as the page and the app drive them. A case that returns
//!   [`Outcome`] lets the rigs it started know that it failed.
//! - [`daemon_log`] reads a daemon's log; [`agents_proof`] holds a daemon's
//!   agents screens to what they must do; [`http`] asks a daemon's API. (The
//!   packaged smoke uses all three, on the daemon the built app chose.)
//! - [`live`] is what the opt-in live tests share (its own doc lists its parts):
//!   the machine's own harnesses (Claude Code, OpenCode, Pi, Devin and Codex),
//!   the rig whose windows run them, a real harness's window opened as the app
//!   opens one, a project of real members whose chief a check plays, what the
//!   harnesses keep and say and what the daemon wrote as it went, what the
//!   pane host says a worker's turn left running, what a run says and how it
//!   waits, and the variables `cargo xtask live` sets from its flags.
//! - [`rollout`] reads a Codex thread's rollout, what it began with and was told,
//!   in the rig's Codex home and the machine's.
//! - [`ports`] names a port on loopback for a program to listen on, one that no
//!   program listens on once the case has let go of it.
//! - [`serial`] has the cases that start a daemon take turns; [`pattern`] builds
//!   the patterns the support reads text by; [`png`] reads the structure of a PNG
//!   file, to say that an image a harness saved is a whole one.
//!
//! The suites are the tests of the crate, a file for each (`tests/cli.rs` is
//! the CLI's, with its cases in `tests/cli/`, and the guards of what the
//! repository's own files say; `tests/daemon.rs` the daemon's as a process;
//! `tests/rig.rs` the rig's, windows included; `tests/regressions.rs` the bugs
//! the owner met in the app, a rig case for each, in the shape they were met;
//! `tests/load.rs` the daemon under load; `tests/smoke.rs` the built app's,
//! which no other suite has: the packaged smoke, a macOS bundle's, with the
//! parts it is made of in `tests/smoke/`; and the live ones,
//! `tests/live_<name>.rs`, each of which spends the machine's quota and runs
//! only when asked for: `live_paste` a real window taking what the daemon
//! pastes into it, `live_interrupt` its interrupt at an idle window,
//! `live_agent` a catalog agent answering as its model, `live_asks` Claude
//! stopping for a removal it should, `live_input_box` what clears its input
//! box, `live_terminal` Devin's terminal on Windows, `live_door` a member's
//! question received once at the door of its harness, `live_harnesses` the Admin
//! page's look at the machine's CLIs and its update of a managed Pi, `live_stops` the stops
//! the daemon presses, `live_codex_resume` and `live_codex_interrupt` Codex's
//! thread and its interrupted question, `live_resume_cut` and
//! `live_restart_cut` a window and a chief that a quit cut, `live_devin_resume` a
//! Devin member's window opened again after a cut, asked to write, edit and run `cf`,
//! `live_resume_fresh`
//! a chief closed and resumed, whether it never had a turn or the human's typed
//! turn was closed on at once, `live_bench` the
//! board between a real chief and real workers, `live_designer` a real Codex
//! drawing an image, `live_approval` real harnesses in every seat of five
//! projects sending what the human approves, behind the approval gate, and
//! `live_mcp` a real Claude Code worker that has an MCP server finishing its
//! task, `live_finish` a real worker of any harness running a shell
//! command and finishing its task with nothing of its own ended, and leaving a
//! long command running that its answer ends, `live_specs` a real chief of each
//! harness told of specs where its project has them on, planning one and
//! carrying it to the board, and `live_notes` a real chief, Devin's or Claude's,
//! in a turn when two notes come for it, handed each once). Run them with
//! `cargo xtask test clis`, `daemons`, `integration`,
//! `regressions`, `agents` and `load`, and `cargo xtask smoke` (the `npm run`
//! names are the same), a live one with `cargo xtask live <name>`, or one with
//! `cargo test -p cf-e2e --test cli`.
//!
//! The stand-ins the suites run in place of what ConsensFlow runs are
//! binaries of this crate, behind the `test-support` feature the crate's own
//! tests turn on: `fake-agent`, `fake-codex`, `fake-devin`, `fake-opencode-2`,
//! `fake-cf`, `liar-daemon` and the packaged smoke's `smoke-paste-reader`.

// The two modules that call the system themselves allow it, block by block: the live checks'
// hold on a process they end (`live::members::descent`'s native calls), and the count a
// terminal keeps of what its reader has not read (`terminal`).
#![deny(unsafe_code)]

pub mod agents_proof;
mod bundle;
pub mod cf;
pub mod checkout;
pub mod daemon;
pub mod daemon_log;
pub mod devin_store;
mod error;
pub mod files;
pub mod http;
pub mod live;
mod outcome;
pub mod pattern;
pub mod png;
pub mod ports;
pub mod process;
pub mod rig;
pub mod rollout;
pub mod scratch_home;
pub mod serial;
mod stand_in;
pub mod terminal;
pub mod wire;

pub use error::{Error, Result};
pub use outcome::{Failed, Outcome};
pub use scratch_home::ScratchHome;

//! ConsensFlow's daemon (`src/core/daemon.js` and what it runs): the one
//! process that owns its state. It opens the ledger (whose lock refuses a
//! second daemon on the same home), marks the projects that were open for a
//! resume, serves the agents' API and the human's screens on loopback, prints
//! its handle line for the app, then speaks the bridge on its standard input
//! and output: the pane host's requests and events come in, pane operations
//! go out, and the page's requests are answered here. The dispatcher runs once
//! a second and whenever something changes. The end of its input stops it.
//!
//! Everything runs on one thread, as Node did, on a `LocalSet` over tokio's
//! current-thread runtime. The engine's work runs on the engine's executor
//! ([`seams::DaemonSpawn`]), in the order Node's microtasks ran, and is drained
//! where Node's event loop went on to its next callback: after the frames of
//! one read of the bridge ([`host::daemon_bridge`]), the first part of an HTTP
//! request ([`api`]) and a timer of the pass loop ([`pass`]). A panic is what
//! an exception was: caught where work runs, written down, and gone past
//! ([`errors`]).
//!
//! - [`start`] is the daemon's start, in Node's order, and [`stop`] its stop:
//!   one latch, one deadline, and a tail that waits for nothing.
//! - [`api`] is the HTTP front, [`screens`] the human's pages it serves, and
//!   [`page`] what the board page asks over the bridge; their tables are
//!   frozen, and each landing fills in its own handlers.
//! - [`host`] is the pane host over the bridge, [`pass`] the pass loop and the
//!   throttles, [`files`] the log and the trace, and [`cli`] the verb that runs
//!   it all: `cf ui`.

#![forbid(unsafe_code)]

pub mod api;
pub mod cli;
pub mod console;
pub mod errors;
pub mod files;
pub mod host;
pub mod machine;
pub mod page;
pub mod pass;
pub mod roster;
pub mod screens;
pub mod seams;
pub mod start;
pub mod stop;

#[cfg(test)]
mod testing;

pub use cli::ui;
pub use start::{start, Daemon, Options, StartError};

//! The Codex window's supervisor and broker, `cf codex-session <codex> <args…>`.
//!
//! A Codex window runs Codex as two programs, its app-server and its TUI,
//! with a broker between them that knows which thread the window shows and
//! queues the daemon's messages on it. This is what the window runs in place
//! of Codex: it starts the three, ends them with the window, and leaves
//! nothing behind.

#![forbid(unsafe_code)]

mod arguments;
mod broker;
mod endpoint;
mod questions;
mod supervisor;
mod tail;

use std::ffi::OsString;
use std::future::Future;
use std::io::{self, Write};

use cf_base::env::Env;
use tokio::task::LocalSet;

/// Runs a Codex window: `args` are the Codex program and the arguments it is
/// run with, `env` the window's environment, which names its broker
/// (`CF_CODEX_SESSION_BRIDGE`). Returns the exit code: the TUI's, 0 when a
/// signal ended it, 1 with the reason on standard error when Codex could not
/// be opened.
pub fn run(env: &Env, args: &[OsString]) -> i32 {
    match block_on_local(supervisor::supervise(env, args)) {
        Ok(Ok(code)) => code,
        Ok(Err(cause)) => fail(&cause.to_string()),
        Err(cause) => fail(&format!("could not start its runtime: {cause}")),
    }
}

/// Runs `future` to its end on this thread, in a runtime of its own with the
/// local tasks it starts (the broker's state is not shared between threads).
/// A question may be held at the board for up to an hour, on a thread of the
/// runtime's blocking pool: the window ends when `future` does, and those
/// threads with it, not when they have ended.
fn block_on_local<F: Future>(future: F) -> io::Result<F::Output> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let local = LocalSet::new();
    let output = local.block_on(&runtime, future);
    drop(local);
    runtime.shutdown_background();
    Ok(output)
}

/// Says why Codex could not be opened, on standard error.
fn fail(reason: &str) -> i32 {
    let _ = writeln!(io::stderr(), "ConsensFlow could not open Codex: {reason}");
    1
}

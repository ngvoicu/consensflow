//! The pane host: every window's program running in a PTY of its own, and
//! the operations the daemon asks of them over the bridge. The PTY and the
//! process tree it owns, each pane's input in order past the arbiter that
//! keeps a paste out of the human's typing, its output on the way to whoever
//! reads it and the screen it leaves for the last lines to be read from, how
//! its program ended, the bridge handlers that answer the daemon, and the headless host
//! that serves them over stdin and stdout are here; the window that draws them
//! is not. Nothing in the crate knows which program hosts it, so the app, the
//! headless helper and the Rust daemon to come run the same code, and none
//! keeps a copy that could drift.
//!
//! A window closes the way a terminal does on Unix (`pty::close`): its programs
//! are asked to end and given a grace, and only what is left is killed. On
//! Windows nothing is asked, and the job object of the pane ends its tree at
//! once. What its agent started and left running past a turn is the pane's tree
//! less what it had when the daemon said the turn began (`background`), which
//! the daemon asks for and the window's exit lists.

#![deny(clippy::undocumented_unsafe_blocks)]

pub mod arbiter;
mod background;
pub mod ended;
pub mod headless;
pub mod input_queue;
#[cfg(windows)]
mod job_object;
pub mod output_hub;
pub mod pane_handlers;
#[cfg(target_os = "macos")]
mod process_tree;
pub mod pty;
pub mod screen;
pub mod validation;

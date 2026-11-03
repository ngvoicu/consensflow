//! A window's process that waits on a macOS privacy dialog drawn in the app's name (T-1194).
//!
//! On a Mac every window's process has the app as its responsible process, so a gated
//! folder or library that a worker's command touches (`ls ~/Music`, `find ~`, a project on
//! the home folder) is asked of the system in the app's name. The system draws the dialog on
//! the screen of the user logged in, the command waits with no timeout of its own until a
//! person answers, the dialog outlives the command, and nothing on the board says so. The
//! system's own log of those requests names the service, the process that asked and the
//! responsible app, within a second, to anyone in the admin group (`log stream`).
//!
//! This reads that log: [`lines`] reads its lines and joins the three that tell one prompt;
//! [`watch`] (macOS) keeps the reader running while a window is open and tells the daemon.

pub(crate) mod lines;
#[cfg(target_os = "macos")]
pub(crate) mod real;
pub(crate) mod watch;

#[cfg(test)]
mod tests;

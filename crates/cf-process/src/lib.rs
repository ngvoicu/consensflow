//! The one place a ConsensFlow program starts another process: how a
//! program starts here (Windows scripts and npm's shims included) and how a
//! window's program does, where a command is on PATH, running one to its end
//! (`execFile`) or beside this one a line at a time (`spawn`), ending one,
//! whether one is alive, and how much memory this one holds.

#![deny(unsafe_code)]
#![deny(clippy::undocumented_unsafe_blocks)]

mod alive;
mod capture;
mod child;
mod execute;
mod group;
mod job;
mod memory;
mod runnable;
mod search;
mod terminate;
#[cfg(test)]
mod testing;

pub use alive::alive;
pub use capture::{capture, CaptureFailed, Captured};
pub use child::{spawn, Child, Ender, Streams};
pub use execute::{execute, Failed, Limits};
pub use job::with_required;
pub use memory::{megabytes, rss};
pub use runnable::{pane_argv, runnable, Run};
pub use search::{find_in, on_path};
pub use terminate::{terminate, Ending};

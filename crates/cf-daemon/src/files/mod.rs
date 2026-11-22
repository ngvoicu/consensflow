//! The two files the daemon keeps in the home for whoever watches it from
//! outside: its log (`daemon.log`) and its trace (`events.jsonl`). Their
//! formats are Node's, line for line.

mod log;
mod trace;

pub use log::Log;
pub use trace::{Trace, QUEUE, SETTLE};
#[cfg(test)]
pub(crate) use trace::Durable;

#[cfg(test)]
pub(crate) use trace::{settle_in, settled_trace};

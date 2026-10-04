//! ConsensFlow's integration with each coding-agent harness: one module per
//! harness, each importing only what all of them share (`shared`), never a
//! sibling. It holds the hooks a harness runs through `cf hook`, and the
//! readers of what each harness's own record of a conversation says
//! ([`records`]).

#![forbid(unsafe_code)]

pub mod claude;
pub mod codex;
pub mod devin;
pub mod records;
mod shared;

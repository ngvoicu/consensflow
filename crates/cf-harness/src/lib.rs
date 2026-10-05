//! ConsensFlow's integration with each coding-agent harness: one module per
//! harness, each importing only what all of them share (`shared`), never a
//! sibling. It holds the hooks a harness runs through `cf hook`, the
//! readers of what each harness's own record of a conversation says
//! ([`records`]), where each harness's CLI is ([`detect`]), and how the
//! engine launches a harness's window and works with it ([`contract`]).

#![forbid(unsafe_code)]

pub mod claude;
pub mod codex;
pub mod contract;
pub mod detect;
pub mod devin;
pub mod opencode;
pub mod pi;
pub mod records;
mod shared;

//! ConsensFlow's integration with each coding-agent harness: one module per
//! harness, each importing only what all of them share (`shared`), never a
//! sibling. Today it holds the hooks a harness runs through `cf hook`.

#![forbid(unsafe_code)]

pub mod claude;
pub mod devin;
mod shared;

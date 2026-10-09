//! The portable Windows app is one file: the app's own exe, then the runtime
//! it needs (the `cf` the daemon and every window run, and the terminals'
//! console host) as a payload, then a footer that finds it. This is the one
//! place that knows the file's layout, written down once:
//!
//! ```text
//! ConsensFlow_<version>_x64-portable.exe
//!   the built ConsensFlow.exe, byte for byte
//!   the payload: a gzip-compressed tar of cli/ (its bin/cf.exe), conpty.dll,
//!     OpenConsole.exe and OpenConsole-LICENSE.txt
//!   the footer, 16 bytes: the payload's length in bytes, as an unsigned
//!     64-bit little-endian integer, then the tag "CFPAYLD1"
//! ```
//!
//! An exe that ends with the tag carries its runtime; one that does not (the
//! installed app, the Mac's) finds its runtime beside it. The payload's own
//! gzip trailer holds the CRC32 of the tar, which names the folder a runtime
//! is kept in ([`Payload::folder`]): a folder for each build.
//!
//! - [`pack`] writes the file, from the app's exe and the build's release
//!   folder; `cargo xtask portable pack` says where.
//! - [`Payload::find`] reads the footer, and [`inspect`] does so for a file by
//!   its path (`cargo xtask portable inspect`): where the payload is, how long,
//!   and its CRC.
//! - [`Payload::extract`] unpacks the payload into a folder: files and folders
//!   only, none that lands outside it, the gzip stream read to its end.
//!
//! What is the app's own stays in the app (`app/src-tauri/src/portable.rs`):
//! which folder the runtime is unpacked into, how it is published once whole,
//! which older runtimes go, and how Windows is told where its libraries are.

#![forbid(unsafe_code)]

mod archive;
mod error;
mod format;
mod pack;

pub use error::Error;
pub use format::{footer, inspect, Payload};
pub use pack::{pack, Packed};

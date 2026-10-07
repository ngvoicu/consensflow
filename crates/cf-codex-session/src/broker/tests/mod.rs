//! The broker against a fake Codex server, driven the way a window and the
//! daemon drive it: a TUI that connects and says what Codex's TUI says, the
//! daemon's `GET /session` and `POST /deliver`, and a server that answers each
//! request when the test says.

mod delivery;
/// The fake Codex server and the rest of what these tests stand on, which the
/// supervisor's tests use too.
pub(crate) mod fixture;
mod handoff;
/// The hold a test puts on the writer of a pair's connection to Codex's server
/// (`Shared::native_hold`, the pair's `native_sink`).
mod hold;
mod lifecycle;
mod questions;
mod switching;
mod transport;

pub(crate) use hold::{Held, Hold};

//! The local transport, held to the scenarios of the bridge case of
//! `crates/cf-e2e/tests/daemon/bridge.rs` (once `tests/bridge.test.mjs`), each
//! under its sentence, and to the rules those sentences do not say: what the
//! reader does on the frame order, how a bridge ends, and the whole of it
//! paired with the pane host's own transport over operating system pipes.

mod dispatch;
mod ended;
mod events;
mod exits;
mod failure;
mod input;
mod lifecycle;
mod limits;
mod pipes;
mod reads;
mod requests;
mod wire;

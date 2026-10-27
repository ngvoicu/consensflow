//! Muse Code, Meta's coding agent (`muse`): where it keeps its sessions, what a
//! session's log says, what its index says of resuming one, and which Muse a machine
//! has. No window is opened on it yet ([`MuseAdapter`] refuses a launch).

mod adapter;
pub(crate) mod index;
pub mod models;
mod msp;
pub(crate) mod paths;
pub mod record;
pub mod usage;
pub(crate) mod version;

pub use adapter::MuseAdapter;
pub(crate) use adapter::NOT_YET;

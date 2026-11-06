//! Muse Code, Meta's coding agent (`muse`): its windows, the hook that tells a window
//! its session and gives it its role text, where it keeps its sessions, what a
//! session's log says, what its index says of resuming one, and which Muse a machine
//! has.

mod adapter;
pub(crate) mod index;
pub(crate) mod last_used;
mod launch;
pub mod models;
mod msp;
pub(crate) mod paths;
pub mod record;
mod screen;
mod session_hook;
pub mod usage;
pub(crate) mod version;
mod window;

pub use adapter::MuseAdapter;
pub(crate) use screen::read as read_input_box;
pub(crate) use screen::words as input_box_words;
pub use session_hook::{
    blocks, hook_output, parts, session_hook, FILLER, MARK, MOST_PARTS, OUTPUT_LIMIT,
};

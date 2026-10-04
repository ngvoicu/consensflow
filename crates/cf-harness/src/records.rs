//! What a harness's own record of a conversation says
//! (`hosts/lib/completion.js`): read from its native store, never written,
//! read on from where the last look stopped. A reading is what a reading of
//! the whole record would say: a record that shrank or was replaced is read
//! again from its start. A final unterminated JSONL append may be
//! incomplete; a malformed whole line fails the look.
//!
//! Each harness's reader is in its own module; the readers kept from look
//! to look are a [`Cache`].

pub use crate::shared::quota::{Level, Quota};
pub use crate::shared::record::cache::{Cache, Look, Open, IDLE_MS};
pub use crate::shared::record::reading::{Item, Reading, Record, Role, Settlement};

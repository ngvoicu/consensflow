//! What every ConsensFlow crate shares and none should keep a copy of: the
//! environment as the process found it, the folder ConsensFlow keeps its
//! things in, a file told apart from another renamed over it, JavaScript's
//! readings of values and text, text cut where JavaScript cut it, and JSON
//! read and written the way Node read and wrote it.

#![forbid(unsafe_code)]

pub mod env;
pub mod file;
pub mod home;
pub mod js;
pub mod json;
pub mod refusal;
pub mod text;
pub mod time;

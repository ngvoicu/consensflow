//! What every ConsensFlow crate shares and none should keep a copy of: the
//! environment as the process found it, the folder ConsensFlow keeps its
//! things in, a file told apart from another renamed over it, JavaScript's
//! readings of values and text, paths joined as Node joins them, text cut
//! where JavaScript cut it, and JSON read and written the way Node read and
//! wrote it, the words after a verb as Node's `parseArgs` reads them, the
//! programs a test starts that end when the test does, and the app bundles a
//! test may start: none of the live app's identity.

#![forbid(unsafe_code)]

pub mod args;
pub mod bundle;
pub mod env;
pub mod file;
pub mod fixture;
pub mod home;
pub mod js;
pub mod json;
pub mod path;
pub mod refusal;
pub mod text;
pub mod time;

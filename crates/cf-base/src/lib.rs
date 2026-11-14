//! What every ConsensFlow crate shares and none should keep a copy of: the
//! environment as the process found it, the folder ConsensFlow keeps its
//! things in, a file told apart from another renamed over it, JavaScript's
//! readings of values and text, paths joined as Node joins them, text cut
//! where JavaScript cut it, and JSON read and written the way Node read and
//! wrote it, the words after a verb as Node's `parseArgs` reads them, the
//! programs a test starts that end when the test does, the app bundles a test
//! may start (none of the live app's identity), and how much of a role `cf`
//! printed its window reads whole.

#![forbid(unsafe_code)]

pub mod args;
pub mod bundle;
pub mod env;
pub mod file;
#[cfg(any(test, feature = "test-support"))]
pub mod fixture;
pub mod home;
pub mod js;
pub mod json;
pub mod path;
pub mod refusal;
pub mod role_read;
pub mod startup;
pub mod text;
pub mod time;

//! What the three players of Node's traces share, each piece once: the API's
//! (`tests/api/`), the page's (`tests/page/`) and the screens' (`tests/screens/`),
//! which `tests/goldens/daemon/FORMAT.md` describes.
//!
//! A test binary compiles what it includes, and what it includes and does not
//! use is dead code in it. So the pieces all three use whole are the children
//! of this module (`#[path = "../support/mod.rs"] mod support;`), and each
//! player takes the others, one by one, with `#[path]`, if it uses them whole:
//!
//! - [`trace`]: the traces, found and read, played on one thread, and what a
//!   player held of them;
//! - [`compare`]: what a player made, held to what Node recorded, as bytes;
//!   the database a ledger left;
//! - [`daemon`]: the daemon's executor, log and trace, and the wake-ups its
//!   handlers ask for;
//! - `front.rs` (API, screens): the daemon's front served over real sockets,
//!   and a client of it that writes a request as the trace has it;
//! - `world.rs` (page, screens): the folder and the environment a trace starts
//!   from, the programs on its `PATH` included;
//! - `ledger.rs` (API, page): the ledger the trace's own calls are made again
//!   on, with the clock and the names Node's drew.
//!
//! What only the API's windows have (`«api»`, `«token:T1»`) is the API's own
//! (`tests/api/names.rs`): nothing else reads those names.

pub mod compare;
pub mod daemon;
pub mod trace;

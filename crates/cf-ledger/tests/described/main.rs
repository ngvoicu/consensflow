//! What the human calls a project and says of it, in the ledger: the migration
//! that makes room for the line under a project's name (every row and sequence of
//! a ledger before it stays), a rename that has the rules a name has when the
//! project is opened, the line itself (set, changed, taken away; one line of at
//! most 200 characters), the event each change logs and tells the trace, and that
//! a project that is not there is refused before anything is written.

// The tests' own scaffolding: a failure in it is the test's.
#![allow(clippy::expect_used)]

mod both;
mod line;
mod migration;
mod name;
mod world;

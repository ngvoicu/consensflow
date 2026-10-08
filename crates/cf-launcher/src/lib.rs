//! The command ConsensFlow puts in the user's terminal: `consensflow` and `cf`
//! in the `bin` folder of ConsensFlow's home, `.cmd` where Windows has them,
//! each a small script that runs the app's own `cf`, so a terminal and a window
//! never drift apart. The app already carries a working program, so this is a
//! launcher pointing at it, not a second installation to keep in sync, and
//! nothing to do with npm: the same move VS Code makes with its `code` command.
//!
//! - [`install`] writes it, and [`status`] says whether it is there, where,
//!   and whether that place is on `PATH`. A command someone else put there
//!   is not ours to report or to replace: ours holds a mark.
//! - [`runtime`] says what the command runs and whether that is the copy
//!   asking, in the shape of every build, and the line `cf doctor` says of
//!   it ([`Wiring::report`]).
//! - [`repair`] is what the app runs at its start: a command of ours that
//!   serves the app's own home and does not name this bundle's `cf` is
//!   rewritten, so the one an older build wrote (its bundled Node and
//!   `cf.mjs`) keeps working after the bundle has neither. One that serves
//!   another home is left to the app of that home.
//!
//! It sits below the daemon, the native `cf` and the app, and knows no
//! layout of a bundle: every caller names the `cf` of its own, the daemon's
//! `machine::bundle_of` beside its binary, the app the folder of its
//! resources. It reads the home the way every other crate does, from the
//! environment it is handed, and starts no process.

#![forbid(unsafe_code)]

mod install;
mod places;
mod repair;
mod text;
mod wiring;

pub use install::{install, status, Installed};
pub use places::Places;
pub use repair::{repair, Repair, Repaired};
pub use wiring::{runtime, Shape, Wiring};

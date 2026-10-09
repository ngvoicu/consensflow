//! `cf-release prepare-update` as a command: every case of
//! `tests/update-release.test.mjs`, which tested the script it replaced, and
//! what the port checks that the script left to Python and to `plutil`. Each
//! test makes the release it needs (an app as a folder and as an archive, the
//! sources, a signature, notes) and runs the command on it; nothing of this
//! checkout is read, and nothing is written outside the folder of the test.
//!
//! The release is made on macOS: the tests need a Unix, for the modes an app's
//! files have and for the `cf` of the bundle, which is a script.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod archive;
mod arguments;
mod bundle;
mod files;
mod fixture;
mod happy;
mod signature;

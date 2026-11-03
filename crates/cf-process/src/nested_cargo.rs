//! The `cargo` a test starts inside the `cargo test` that runs it.
//!
//! `cargo test` gives each test the variables of its crate (`CARGO_MANIFEST_DIR`,
//! `CARGO_PKG_NAME`, ...), and a cargo the test starts takes them as its own. A build script can
//! ask cargo to watch a variable (`ring`'s does, for `CARGO_MANIFEST_DIR`), and a variable that
//! has another value than in the build before makes cargo run the script again and build the
//! package, and every package over it, again: `ring`, then `rustls` and `reqwest`, which
//! `cf-daemon` is built over.
//!
//! The package is built into the folder the running build uses, under the name that build gave
//! it (the hash in a file's name is of the package, its features and its profile, not of what it
//! was built from), so the build the test starts writes over files of the build that runs it.
//! Built again from the same inputs a crate is the same crate; built from other inputs it is
//! another, and the libraries of the running build that were made over the old one can no longer
//! be loaded (E0460). `RUSTC_BOOTSTRAP=1` is such an input: rustc hashes whether unstable
//! features may be used into the crate, and cargo watches the variable only for the packages
//! whose build scripts ask it to, so a run with it set keeps the files of the others as an
//! earlier run without it made them. `cargo test` ends with rustdoc, which meets a library it
//! cannot load and says only "can't find crate" (E0463) of the first: `Doc-tests cf` on zeewin
//! (T-365, 2026-10-23).
//!
//! So the cargo a test starts is started as a shell would start it: in the environment of the
//! test but for the variables cargo gave the test.

// The one place a test of ours names the real cargo (`tools/xtask/tests/cargo_in_tests.rs` holds it
// so): it starts it as a shell would, and reads the environment to do so. It uses std alone, because
// `tools/cf-publish` and `tools/cf-release` take no dependency and include this file by its path.
#![allow(clippy::disallowed_methods)]

use std::ffi::OsString;
use std::process::Command;

/// The cargo that built this test, which is the one it starts.
pub const CARGO: &str = env!("CARGO");

/// `cargo`, to start with the words of the build, in the environment the test has but for what
/// cargo set in it for the test.
#[must_use]
pub fn cargo() -> Command {
    let mut command = Command::new(CARGO);
    without_the_tests_own(&mut command, std::env::vars_os().map(|(name, _)| name));
    command
}

/// Takes from `command`'s environment the variables among `names` that cargo gives a test.
pub fn without_the_tests_own(command: &mut Command, names: impl IntoIterator<Item = OsString>) {
    for name in names {
        if name.to_str().is_some_and(given_to_a_test) {
            command.env_remove(name);
        }
    }
}

/// Whether cargo sets the variable `name` for the test of a crate (cargo's list, "Environment
/// variables Cargo sets for crates"). What whoever runs cargo set stays (`CARGO_HOME`,
/// `CARGO_TARGET_DIR`, `RUSTC_BOOTSTRAP`, ...), as the build the test is part of has it too;
/// so does `CARGO`, which says where the cargo is.
#[must_use]
pub fn given_to_a_test(name: &str) -> bool {
    // A name is the same name in any case on Windows.
    let name = name.to_ascii_uppercase();
    name.starts_with("CARGO_PKG_")
        || name.starts_with("CARGO_BIN_EXE_")
        || matches!(
            name.as_str(),
            "CARGO_MANIFEST_DIR"
                | "CARGO_MANIFEST_PATH"
                | "CARGO_MANIFEST_LINKS"
                | "CARGO_CRATE_NAME"
                | "CARGO_BIN_NAME"
                | "CARGO_PRIMARY_PACKAGE"
                | "CARGO_TARGET_TMPDIR"
        )
}

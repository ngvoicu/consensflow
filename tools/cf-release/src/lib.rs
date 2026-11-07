//! The release's steps as one command, `cf-release`: `ci-passed` (CI's full
//! gate passed the commit on both platforms), `version` (what the
//! sources agree on), `prepare-update` (the update feed's entry for a build) and
//! `sign-mac` (the Developer ID signing). The release workflow is to build it
//! with `--locked` and run the binary; `cargo xtask` asks its [`version`] module
//! the one question of the sources. It depends on no crate of the product, only
//! on `cf-base` for the environment, which `main` reads once.
//!
//! Who edits what. `update` (the `prepare-update` step) is built; `sign_mac` is
//! S5's: that landing builds its step in that module and its tests, and uses
//! [`process`] for the programs it runs. The lead's are `cli` (the list of
//! steps), the manifest and the lockfile, and the call sites in the workflows.

#![forbid(unsafe_code)]

mod args;
pub mod ci_passed;
mod cli;
pub mod process;
mod sign_mac;
mod update;
pub mod version;

pub use cli::run;

/// The cargo a test starts: one file for every crate that starts one
/// (`crates/cf-process/src/nested_cargo.rs`), which this crate includes because it takes no
/// dependency on the product.
#[cfg(test)]
#[path = "../../../crates/cf-process/src/nested_cargo.rs"]
#[allow(dead_code)] // The file serves crates that start more than the probes here do.
mod nested_cargo;

/// This process's environment but for the variables cargo gave the test: what the cargo a test
/// starts is given (`nested_cargo`).
#[cfg(test)]
fn env_without_the_tests_own() -> cf_base::env::Env {
    let env = cf_base::env::Env::from_process();
    cf_base::env::Env::from_vars(
        env.iter()
            .filter(|(name, _)| !name.to_str().is_some_and(nested_cargo::given_to_a_test))
            .map(|(name, value)| (name.to_os_string(), value.to_os_string()))
            .collect::<Vec<_>>(),
    )
}

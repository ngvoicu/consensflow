//! The cargo door (K3, `live-kit-isolation-doors.md` section 2, and T-2002's correction on
//! cargo): the two ways the kit starts cargo, each with its words fixed here.
//!
//! - [`build`]: a release build of one program of the checkout, which the run's own suites ask
//!   for. Refused where the run builds nothing (`CF_E2E_NO_BUILD`), checked here, where cargo
//!   would start, after the look for a program built before.
//! - [`check_offline`]: `cargo check --offline` of a fixture this run made, in a build folder of
//!   its own (the nested compiler test of the rig, which `isolated-test-runner.sh` runs with
//!   `CF_E2E_NO_BUILD=1` for the whole rig): it builds nothing of the checkout's.
//!
//! Cargo is given the machine's list, a scratch home of the run's, and the toolchain's own
//! variables by name (where cargo and rustup are, which toolchain, where it builds, and what makes
//! a crate the same crate: `RUSTFLAGS`, `RUSTC_BOOTSTRAP`, …, which a nested build must share with
//! the build that runs it, or it writes over that build's files with others: `nested_cargo`). It
//! is never given what cargo gave the test (`CARGO_MANIFEST_DIR`, `CARGO_PKG_*`, …), the board's
//! variables, or any other of the caller's. Where the account sets no `CARGO_HOME` or
//! `RUSTUP_HOME`, cargo is told the account's own (`~/.cargo`, `~/.rustup`), which its scratch
//! home would otherwise hide; neither is a store of a harness's.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::process::{Ran, Run};
use crate::{Error, Result};

#[allow(dead_code)] // Only the cargo's name and the list of variables are used here.
#[path = "../../../cf-process/src/nested_cargo.rs"]
mod nested_cargo;

/// The cargo that built this test, the one the door starts.
pub const CARGO: &str = nested_cargo::CARGO;

/// The variable that says the run builds nothing.
pub const NO_BUILD: &str = "CF_E2E_NO_BUILD";

/// The toolchain's variables cargo is given by name, where the test's environment has them.
const TOOLCHAIN: [&str; 21] = [
    "CARGO",
    "CARGO_HOME",
    "CARGO_TARGET_DIR",
    "CARGO_BUILD_JOBS",
    "CARGO_INCREMENTAL",
    "CARGO_ENCODED_RUSTFLAGS",
    "CARGO_NET_OFFLINE",
    "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
    "RUSTC",
    "RUSTC_WRAPPER",
    "RUSTC_BOOTSTRAP",
    "RUSTFLAGS",
    "RUSTDOCFLAGS",
    // A Mac's SDK, where a build script's compiler looks it up.
    "SDKROOT",
    "DEVELOPER_DIR",
    "MACOSX_DEPLOYMENT_TARGET",
    // Windows' compiler and libraries, where a developer's shell set them.
    "INCLUDE",
    "LIB",
    "LIBPATH",
    "VSINSTALLDIR",
];

/// The prefixes of the toolchain's variables cargo is given (its build, profile and target
/// settings), but for what cargo gave the test.
const TOOLCHAIN_PREFIXES: [&str; 4] = ["CARGO_BUILD_", "CARGO_PROFILE_", "CARGO_TARGET_", "CARGO_TERM_"];

/// Whether `name` is a toolchain variable cargo is given.
pub fn is_toolchain_variable(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    !nested_cargo::given_to_a_test(&upper)
        && (TOOLCHAIN.contains(&upper.as_str())
            || TOOLCHAIN_PREFIXES
                .iter()
                .any(|prefix| upper.starts_with(prefix)))
}

/// Whether the run builds nothing.
pub fn builds_nothing() -> bool {
    crate::process::own_var(NO_BUILD).is_some_and(|said| !said.is_empty() && said != "0")
}

/// The environment cargo is given, from the test's own (`all`): the machine's list, the
/// toolchain's variables, the account's cargo and rustup homes where it sets none (`account`, its
/// home where this process knows it), and the homes of `scratch`. An error where a variable names
/// a store of a harness's or of the account.
pub fn environment_of(
    all: impl IntoIterator<Item = (OsString, OsString)>,
    account: Option<&Path>,
    scratch: &Path,
    in_protected: &dyn Fn(&Path) -> bool,
) -> std::result::Result<Vec<(OsString, OsString)>, String> {
    let mut env: Vec<(OsString, OsString)> = all
        .into_iter()
        .filter(|(name, _)| {
            name.to_str().is_some_and(|name| {
                crate::process::is_machine_variable(name) || is_toolchain_variable(name)
            })
        })
        .collect();
    let has = |env: &[(OsString, OsString)], wanted: &str| {
        env.iter()
            .any(|(name, _)| name.to_string_lossy().eq_ignore_ascii_case(wanted))
    };
    if let Some(home) = account {
        for (name, below) in [("CARGO_HOME", ".cargo"), ("RUSTUP_HOME", ".rustup")] {
            if !has(&env, name) {
                env.push((OsString::from(name), home.join(below).into_os_string()));
            }
        }
    }
    for (name, value) in crate::standin_process::home_vars(scratch) {
        env.retain(|(held, _)| !held.to_string_lossy().eq_ignore_ascii_case(name));
        env.push((OsString::from(name), value.into_os_string()));
    }
    for (name, value) in &env {
        let text = name.to_string_lossy();
        if text.eq_ignore_ascii_case("PATH")
            || crate::door::spawn::HOME_VARS
                .iter()
                .any(|home| home.eq_ignore_ascii_case(&text))
        {
            continue;
        }
        let whole = std::iter::once(PathBuf::from(value));
        if let Some(inside) = whole
            .chain(std::env::split_paths(value))
            .filter(|path| path.is_absolute())
            .find(|path| in_protected(path))
        {
            return Err(format!(
                "cargo's {text} names {}, inside a store of a harness or of the account",
                inside.display()
            ));
        }
    }
    Ok(env)
}

/// Cargo, as the door starts it: [`environment_of`] the test's own, in `cwd`.
fn cargo_in(cwd: &Path) -> Result<Run> {
    let scratch = crate::door::made("cf-cargo-home-")
        .map_err(Error::file("make a folder in", &std::env::temp_dir()))?
        .hand_on();
    crate::door::account::remove_at_exit(&scratch);
    let home = scratch.join("home");
    crate::door::make_dir(&home)?;
    #[allow(clippy::disallowed_methods)] // The door reads the test's environment for cargo.
    let all = std::env::vars_os();
    let roots = crate::door::Roots::now();
    let env = environment_of(
        all,
        crate::door::account::project_home().as_deref(),
        &home,
        &|path| roots.holds(path),
    )
    .map_err(Error::Build)?;
    Ok(Run::operation("cargo", CARGO)
        .vars(env)
        .cwd(cwd)
        .unlimited())
}

/// A release build in the checkout at `root`, with `words` after `build`: refused where the run
/// builds nothing.
pub fn build(root: &Path, words: &[&str]) -> Result<Ran> {
    if builds_nothing() {
        return Err(Error::Build(format!(
            "cargo build {} was asked of a run that builds nothing ({NO_BUILD}): build it first, \
             or run through `cargo xtask live`",
            words.join(" ")
        )));
    }
    cargo_in(root)?.arg("build").args(words).run()
}

/// `cargo check --offline` of `fixture`, a folder this run made with a package in it, in a build
/// folder of its own (`<fixture>/target`). On a stand-in's bounded search path cargo is given the
/// compiler it was built with (`RUSTC`), and no search path at all: it finds its compiler without
/// one.
pub fn check_offline(fixture: &Path) -> Result<Ran> {
    if !crate::door::is_made(fixture) {
        return Err(Error::Build(format!(
            "{} is no fixture this run made, and cargo checks only one",
            fixture.display()
        )));
    }
    let mut run = cargo_in(fixture)?
        .var("CARGO_TARGET_DIR", fixture.join("target"))
        .args(["check", "--offline", "-j", "4"]);
    if crate::process::own_var("CF_E2E_STAND_IN").as_deref() == Some("1")
        && crate::process::own_var("RUSTC").is_none()
    {
        if let Some(compiler) = crate::standin_process::build_compiler() {
            run = run.var("RUSTC", compiler).var("PATH", OsStr::new(""));
        }
    }
    run.run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_is_given_the_machines_list_the_toolchain_and_a_scratch_home_and_nothing_else() {
        let all: Vec<(OsString, OsString)> = [
            ("PATH", "/usr/bin"),
            ("HOME", "/owner"),
            ("CARGO_MANIFEST_DIR", "/checkout/crates/cf-e2e"),
            ("CARGO_PKG_NAME", "cf-e2e"),
            ("CONSENSFLOW_TOKEN", "board"),
            ("CONSENSFLOW_URL", "http://127.0.0.1:1/"),
            ("CARGO_TARGET_DIR", "/checkout/target"),
            ("RUSTC_BOOTSTRAP", "1"),
            ("RUSTUP_TOOLCHAIN", "1.99.1"),
            ("CARGO_BUILD_JOBS", "4"),
            ("AWS_SECRET", "x"),
            ("XDG_CONFIG_HOME", "/owner/.config"),
        ]
        .into_iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect();
        let env = environment_of(all, Some(Path::new("/owner")), Path::new("/run/home"), &|_| {
            false
        })
        .unwrap();
        let get = |name: &str| {
            env.iter()
                .find(|(held, _)| held == name)
                .map(|(_, value)| value.to_string_lossy().into_owned())
        };
        for gone in [
            "CARGO_MANIFEST_DIR",
            "CARGO_PKG_NAME",
            "CONSENSFLOW_TOKEN",
            "CONSENSFLOW_URL",
            "AWS_SECRET",
        ] {
            assert_eq!(get(gone), None, "{gone} reached cargo");
        }
        for (kept, value) in [
            ("PATH", "/usr/bin"),
            ("CARGO_TARGET_DIR", "/checkout/target"),
            ("RUSTC_BOOTSTRAP", "1"),
            ("RUSTUP_TOOLCHAIN", "1.99.1"),
            ("CARGO_BUILD_JOBS", "4"),
            ("CARGO_HOME", "/owner/.cargo"),
            ("RUSTUP_HOME", "/owner/.rustup"),
            ("HOME", "/run/home"),
            ("XDG_CONFIG_HOME", "/run/home/.config"),
        ] {
            assert_eq!(get(kept).as_deref(), Some(value), "{kept}");
        }
    }

    #[test]
    fn cargo_is_refused_a_variable_that_names_a_store() {
        let all = [(OsString::from("CARGO_TARGET_DIR"), OsString::from("/owner/.codex/t"))];
        let refused = environment_of(all, None, Path::new("/run/home"), &|path| {
            path.starts_with("/owner/.codex")
        })
        .unwrap_err();
        assert!(refused.contains("CARGO_TARGET_DIR"), "{refused}");
    }
}

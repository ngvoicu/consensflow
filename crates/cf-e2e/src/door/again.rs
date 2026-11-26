//! A fixture test run again as a say-only run runs it.
//!
//! The tests of what a caller names for the run to write (a fixture file, a folder to export
//! into) and of what a caller hands the kit (a Claude config folder) hold the files that were
//! there byte for byte, and refuse where a named file is there. A say-only run (`CF_LIVE_AUDIT=1`)
//! writes what it would end or take away in an audit file and writes below its own folders as an
//! acting run does, so the same test is run again as one: the test program starts itself on the
//! test with the switch set, in a temp folder of the run's own, and holds that run to a pass.

use std::fs;
use std::time::Duration;

use super::{Door, AUDIT, AUDIT_FILE, AUDIT_MADE};
use crate::process::Run;
use crate::{Error, Result};

/// How long the test run again is given.
const LIMIT: Duration = Duration::from_secs(300);

/// What a passing run of one test says of itself.
const PASSED: &str = "test result: ok. 1 passed;";

/// The first words of an audit file a say-only run made.
const HEADER: &str = "# say-only run (CF_LIVE_AUDIT) of process ";

/// Runs the test `test` (its name as `cargo test --exact` takes it, `tests::the_name`) of this
/// test program again as a say-only run, after the test itself ran as it ran, and fails with what
/// the run said where it did not pass or was no say-only run. Everything that run makes, its temp
/// folders and its audit file, is in a folder of its own, which goes when this returns.
///
/// The test calls it last. In the run it starts (the switch is set) it makes the door and
/// returns: no run starts another.
pub fn again_under_audit(test: &str) -> Result {
    if Door::audit_asked() {
        // The audit file is made when the door is, which a test that never asked for the door
        // would leave until now.
        Door::init();
        return Ok(());
    }
    // A folder the registry does not hold: what the run makes in its temp folder is below it, and
    // must be no folder of the run's the program that is started is handed.
    let place = tempfile::Builder::new()
        .prefix("cf-audit-again-")
        .tempdir()
        .map_err(Error::file("make a folder in", &std::env::temp_dir()))?;
    let temp = place.path().join("tmp");
    fs::create_dir(&temp).map_err(Error::file("make", &temp))?;
    let audit = place.path().join("audit.txt");
    let program = std::env::current_exe().map_err(Error::file(
        "find the test program",
        std::path::Path::new("."),
    ))?;
    let ran = say_only_run(program, test, &audit, &temp).run()?;
    let said = format!("{}{}", ran.stdout, ran.stderr);
    if ran.code != Some(0) || !said.contains(PASSED) {
        return Err(Error::Daemon(format!(
            "{test} run again as a say-only run did not pass (it ended with {:?}): {said}",
            ran.code
        )));
    }
    let audited = fs::read_to_string(&audit).map_err(Error::file("read", &audit))?;
    if !audited.starts_with(HEADER) {
        return Err(Error::Daemon(format!(
            "{test} run again was no say-only run: its audit file begins {:?}",
            audited.lines().next().unwrap_or_default()
        )));
    }
    Ok(())
}

/// The run of `test` again as a say-only run: the test program with the switch set, its audit file
/// at `audit` and the temp folder it makes its folders in `temp`, with the marks of this process
/// and the cargo and rustup folders a nested cargo of the test finds its toolchain by (no other
/// child is told them).
fn say_only_run(
    program: std::path::PathBuf,
    test: &str,
    audit: &std::path::Path,
    temp: &std::path::Path,
) -> Run {
    let mut run = Run::new(program).args(["--exact", test, "--test-threads=1"]);
    // The stand-in's own marks, which the program started finds at its start: the folders this
    // process made (its audit file's folder is below one), and that it is a stand-in.
    for name in [super::MADE_ROOTS, "CF_E2E_STAND_IN"] {
        // The door hands on its own marks as this process holds them.
        #[allow(clippy::disallowed_methods)]
        let held = std::env::var_os(name);
        if let Some(value) = held {
            run = run.var(name, value);
        }
    }
    for (name, value) in super::account::toolchain_homes() {
        run = run.var(name, value);
    }
    run.var(AUDIT, "1")
        .var(AUDIT_FILE, audit)
        .without(AUDIT_MADE)
        // What the run makes in the temp folder is made in this one.
        .var("TMPDIR", temp)
        .var("TMP", temp)
        .var("TEMP", temp)
        .limit(LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The run again is told the cargo and rustup folders the nested cargo of a test finds its
    /// toolchain by (a Windows `live_approval` fails without them), and no other of the account's.
    #[test]
    fn the_say_only_child_is_told_the_toolchain_homes() {
        let folder = crate::door::made("cf-again-test-").unwrap();
        let run = say_only_run(
            std::path::PathBuf::from("/program"),
            "tests::a_test",
            &folder.path().join("audit.txt"),
            &folder.path().join("tmp"),
        );
        let env = run.environment().unwrap();
        let told = |name: &str| {
            env.iter()
                .find(|(held, _)| held == name)
                .map(|(_, value)| value.to_string_lossy().into_owned())
        };
        for (name, value) in super::super::account::toolchain_homes() {
            assert_eq!(told(&name), Some(value), "{name}");
        }
        assert!(told(AUDIT).is_some());
    }
}

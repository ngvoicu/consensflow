//! The one place cf-release starts a program. `clippy.toml` lets only cf-process
//! do that, and cf-process is async, with limits and process groups that a
//! step running `codesign` or `xcrun notarytool` to its end has no use for.
//!
//! A program runs to its end with exactly the environment it is handed (read
//! once, in `main`, as `cf_base::env::Env`). When one cannot be started, what is
//! said names the program and never its arguments: one of them may be a
//! password.
// The one place a process starts; see above.
#![allow(clippy::disallowed_methods)]

use std::ffi::{OsStr, OsString};
use std::io;
use std::process::{Command, ExitStatus, Stdio};

use cf_base::env::Env;

/// What a program that ran to its end left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// The code it exited with: 1 when a signal ended it and it left none, as
    /// the scripts this replaces read a status that was null.
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// A program that could not be started.
#[derive(Debug, thiserror::Error)]
pub enum Unstarted {
    #[error("{program} was not found: is it installed, and on the PATH?")]
    NotFound { program: String },
    #[error("could not start {program}: {cause}")]
    Refused { program: String, cause: io::Error },
}

/// Runs `program` with `args` to its end, its output going where this one's
/// does: the code it exited with.
pub fn run(program: &OsStr, args: &[OsString], env: &Env) -> Result<i32, Unstarted> {
    command(program, args, env)
        .status()
        .map(|status| code(&status))
        .map_err(|cause| unstarted(program, cause))
}

/// Runs `program` with `args` to its end and keeps what it wrote: a nonzero
/// code is an answer here, for the caller to read, and only a program that did
/// not start is an error. Its input is closed.
pub fn capture(program: &OsStr, args: &[OsString], env: &Env) -> Result<Output, Unstarted> {
    let output = command(program, args, env)
        .stdin(Stdio::null())
        .output()
        .map_err(|cause| unstarted(program, cause))?;
    Ok(Output {
        code: code(&output.status),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn command(program: &OsStr, args: &[OsString], env: &Env) -> Command {
    let mut command = Command::new(program);
    command.args(args).env_clear().envs(env.iter());
    command
}

fn code(status: &ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}

fn unstarted(program: &OsStr, cause: io::Error) -> Unstarted {
    let program = program.to_string_lossy().into_owned();
    if cause.kind() == io::ErrorKind::NotFound {
        Unstarted::NotFound { program }
    } else {
        Unstarted::Refused { program, cause }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cargo that runs these tests: a program that is on every machine that
    /// gets this far, and answers `--version` and refuses what it does not know.
    fn cargo() -> &'static OsStr {
        OsStr::new(env!("CARGO"))
    }

    fn args(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    /// What a nonzero code looks like is read through `capture` below: `run` answers the
    /// same code, and a refusal would write its complaint where these tests' output goes.
    #[test]
    fn a_program_runs_to_its_end_and_its_code_comes_back() {
        let env = Env::from_process();
        assert_eq!(run(cargo(), &args(&["--version"]), &env).unwrap(), 0);
    }

    #[test]
    fn what_a_program_wrote_is_kept_when_asked_with_the_code_it_left() {
        let env = Env::from_process();
        let answered = capture(cargo(), &args(&["--version"]), &env).unwrap();
        assert_eq!(answered.code, 0);
        assert!(answered.stdout.starts_with("cargo "), "{answered:?}");
        assert_eq!(answered.stderr, "");

        let refused = capture(cargo(), &args(&["--no-such-flag-in-cargo"]), &env).unwrap();
        assert_ne!(refused.code, 0);
        assert!(
            refused.stderr.contains("--no-such-flag-in-cargo"),
            "{refused:?}"
        );
        assert_eq!(refused.stdout, "");
    }

    #[test]
    fn a_program_that_is_not_there_is_told_by_its_name_alone() {
        let missing = OsStr::new("cf-release-test-no-such-program");
        let secret = args(&["--password", "hunter2"]);
        let env = Env::from_process();

        let said = capture(missing, &secret, &env).unwrap_err().to_string();
        assert_eq!(
            said,
            "cf-release-test-no-such-program was not found: is it installed, and on the PATH?"
        );
        let said = run(missing, &secret, &env).unwrap_err().to_string();
        assert!(
            !said.contains("hunter2") && !said.contains("--password"),
            "{said}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_cannot_be_started_is_told_by_its_name_and_the_systems_words() {
        let dir = tempfile::tempdir().unwrap();
        let not_runnable = dir.path().join("signer");
        std::fs::write(&not_runnable, "#!/bin/sh\n").unwrap();
        let secret = args(&["--password", "hunter2"]);

        let said = capture(not_runnable.as_os_str(), &secret, &Env::default())
            .unwrap_err()
            .to_string();
        assert_eq!(
            said,
            format!(
                "could not start {}: Permission denied (os error 13)",
                not_runnable.display()
            )
        );
        assert!(!said.contains("hunter2"), "{said}");
    }
}

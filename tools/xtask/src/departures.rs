//! `cargo xtask departures` (landing S10): the traces of the tests the Rust
//! engine departs from Node's on purpose, recorded again. Each test named in
//! `DEPARTED` (`crates/cf-engine/tests/dispatcher/traces.rs`) runs with
//! `CF_RERECORD_DEPARTED` set and writes the trace of what the engine did into
//! `crates/cf-engine/tests/departures/`, in the shape of Node's, where it is held
//! to it. The rest of the suite runs as it does, held to Node's traces, which are
//! fixed since Node's dispatcher was deleted, so a departure that spread shows.
//! When the engine does what Node's trace has, a departed test fails, saying so:
//! take it off its `DEPARTED` and delete its departure.
//!
//! The command takes no arguments and answers cargo's exit status.

use std::ffi::OsString;

use crate::context::Context;
use crate::dispatch::{Command, Console, Failure, Run};
use crate::process::{self, Invocation};

pub const COMMANDS: &[Command] = &[Command {
    words: &["departures"],
    about: "Record again the traces of the tests the engine departs from Node's on purpose",
    usage: "",
    run: Run::Native(run),
}];

fn run(context: &Context, args: &[OsString], _console: &mut Console) -> Result<i32, Failure> {
    if !args.is_empty() {
        return Err(Failure::Usage("departures takes no arguments".into()));
    }
    Ok(process::run(&rerecord(context), &context.env)?)
}

/// The engine's dispatcher tests, from the checkout's root, with the variable the
/// departed ones record by set.
fn rerecord(context: &Context) -> Invocation {
    Invocation::new("cargo", &context.root)
        .args(["test", "-p", "cf-engine", "--test", "dispatcher"])
        .var("CF_RERECORD_DEPARTED", "1")
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use cf_base::env::Env;

    fn context() -> Context {
        Context {
            root: PathBuf::from("checkout"),
            env: Env::default(),
        }
    }

    #[test]
    fn the_traces_are_recorded_by_the_engines_dispatcher_tests_run_from_the_root_with_the_variable_set(
    ) {
        let record = rerecord(&context());
        assert_eq!(
            record.display(),
            "cargo test -p cf-engine --test dispatcher"
        );
        assert_eq!(record.cwd, PathBuf::from("checkout"));
        assert_eq!(
            record.vars,
            [(
                OsString::from("CF_RERECORD_DEPARTED"),
                Some(OsString::from("1"))
            )]
        );
    }

    #[test]
    fn departures_takes_no_arguments_and_starts_nothing_without_them() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut console = Console {
            out: &mut out,
            err: &mut err,
        };
        let refused = run(&context(), &[OsString::from("--fast")], &mut console).unwrap_err();
        assert_eq!(refused.to_string(), "departures takes no arguments");
        assert!(matches!(refused, Failure::Usage(_)));
        assert!(out.is_empty() && err.is_empty());
    }

    #[test]
    fn departures_runs_in_rust_and_is_no_script() {
        assert!(matches!(COMMANDS[0].run, Run::Native(_)));
    }
}

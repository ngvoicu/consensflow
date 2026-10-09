//! The suites that run against the native `cf`. The CLI's are Rust's (landing
//! S8): the `cli` test of `crates/cf-e2e`, which builds the `cf` it runs, so
//! `test clis` has nothing to build first and hands its arguments to `cargo
//! test`. The daemons' and the agents' (landing S6) build the `cf` (and, for
//! the daemons, the pane host's bridge) and run the suites of one set with
//! `node --test`; for now each of those hands its arguments to the script it
//! replaces.

use std::ffi::OsString;

use crate::context::Context;
use crate::dispatch::{Command, Console, Failure, Run, Script};
use crate::process::{self, Invocation};

pub const COMMANDS: &[Command] = &[
    Command {
        words: &["test", "daemons"],
        about: "Build cf and the bridge, and run the daemon suites and the rig's against the native daemon",
        usage: "[--offline]",
        run: Run::Node(Script::at_root("tests/daemons.mjs")),
    },
    Command {
        words: &["test", "clis"],
        about: "Run the CLI suites (the cli test of cf-e2e) against the native cf, built for them",
        usage: "[cargo test arguments: --offline, or -- and a test's name]",
        run: Run::Native(clis),
    },
    Command {
        words: &["test", "agents"],
        about: "Build cf and run the proof of the agents screens against the native daemon",
        usage: "[--offline]",
        run: Run::Node(Script::at_root("tests/agents-daemons.mjs")),
    },
];

fn clis(context: &Context, args: &[OsString], _console: &mut Console) -> Result<i32, Failure> {
    Ok(process::run(&cli_suite(context, args), &context.env)?)
}

/// `cargo test -p cf-e2e --test cli` from the checkout's root, followed by the
/// arguments as they came.
fn cli_suite(context: &Context, args: &[OsString]) -> Invocation {
    Invocation::new("cargo", &context.root)
        .args(["test", "-p", "cf-e2e", "--test", "cli"])
        .args(args.iter().cloned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::delegates;

    use std::path::{Path, PathBuf};

    use cf_base::env::Env;

    #[test]
    fn each_node_suite_hands_its_offline_flag_to_its_script() {
        for (line, script) in [
            ("test daemons", "tests/daemons.mjs"),
            ("test agents", "tests/agents-daemons.mjs"),
        ] {
            delegates(COMMANDS, line, script, "", &[]);
            delegates(
                COMMANDS,
                &format!("{line} --offline"),
                script,
                "",
                &["--offline"],
            );
        }
    }

    #[test]
    fn the_cli_suite_runs_in_rust_and_is_no_script() {
        let command = COMMANDS
            .iter()
            .find(|command| command.words == ["test", "clis"]);
        assert!(matches!(command.map(|c| &c.run), Some(Run::Native(_))));
    }

    #[test]
    fn the_cli_suite_is_the_cli_test_of_cf_e2e_with_the_arguments_after_it_as_they_came() {
        let context = Context {
            root: PathBuf::from("checkout"),
            env: Env::default(),
        };
        let line = |words: &[&str]| {
            let args: Vec<OsString> = words.iter().map(OsString::from).collect();
            let suite = cli_suite(&context, &args);
            assert_eq!(suite.cwd, Path::new("checkout"));
            assert!(suite.vars.is_empty());
            suite.display()
        };
        assert_eq!(line(&[]), "cargo test -p cf-e2e --test cli");
        assert_eq!(
            line(&["--offline"]),
            "cargo test -p cf-e2e --test cli --offline"
        );
        assert_eq!(
            line(&["--", "adds_lists_edits_and_removes_an_agent", "--nocapture"]),
            "cargo test -p cf-e2e --test cli -- adds_lists_edits_and_removes_an_agent --nocapture"
        );
    }
}

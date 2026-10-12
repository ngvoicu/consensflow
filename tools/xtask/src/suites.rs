//! The suites that run against the native `cf`. They are Rust's (landings S8
//! and S9): the tests of `crates/cf-e2e`, which builds the `cf` and the pane
//! host they run, so a command here has nothing to build first and hands its
//! arguments to `cargo test`: the CLI's (`cli`), the daemon's as a process
//! (`daemon`), the rig's, with the daemon and the pane host in real terminals
//! (`rig`), the regressions (`regressions`: the rig's, one case for each bug the
//! owner hit in real use), and the daemon under load (`load`, which is ignored
//! unless asked for). The live tests are ignored too, and what they run is the machine's own
//! harness on its own login, which spends real quota: they are no part of
//! `check`, and a person asks for each by name, by [`crate::live`]'s one
//! command. The packaged smoke (`smoke`) is a test of the same crate, ignored
//! the same way, and is run by its own command, [`crate::smoke`], which takes
//! the app to run it on.

use std::ffi::OsString;

use crate::context::Context;
use crate::dispatch::{Command, Console, Failure, Run};
use crate::process::{self, Invocation};

pub const COMMANDS: &[Command] = &[
    Command {
        words: &["test", "daemons"],
        about: "Run the daemon suites, the rig's and the regressions (the daemon, rig and regressions tests of cf-e2e) against the native daemon, built for them",
        usage: "[cargo test arguments: --offline, or a test's name, or -- and libtest's]",
        run: Run::Native(daemons),
    },
    Command {
        words: &["test", "integration"],
        about: "Run the rig's suites (the rig test of cf-e2e): the daemon and the pane host in real terminals, with stand-in agents in the windows",
        usage: "[cargo test arguments: --offline, or a test's name, or -- and libtest's]",
        run: Run::Native(integration),
    },
    Command {
        words: &["test", "regressions"],
        about: "Run the regressions (the regressions test of cf-e2e): a case for each bug the owner hit in real use, on the rig, asserting what the owner should have seen",
        usage: "[cargo test arguments: --offline, or a test's name, or -- and libtest's]",
        run: Run::Native(regressions),
    },
    Command {
        words: &["test", "clis"],
        about: "Run the CLI suites (the cli test of cf-e2e) against the native cf, built for them",
        usage: "[cargo test arguments: --offline, or -- and a test's name]",
        run: Run::Native(clis),
    },
    Command {
        words: &["test", "agents"],
        about: "Run the proof of the agents screens (the agents_proof cases of the daemon test of cf-e2e) against the native daemon",
        usage: "[cargo test arguments: --offline, or -- and libtest's]",
        run: Run::Native(agents),
    },
    Command {
        words: &["test", "load"],
        about: "Run the daemon under load (the load test of cf-e2e, which cargo test ignores unless asked)",
        usage: "[cargo test arguments: --offline, or -- and libtest's; CONSENSFLOW_LOAD_PROJECTS, _WAVES and _TASKS set the size]",
        run: Run::Native(load),
    },
];

/// A set of the tests of `cf-e2e`: which test files, which cases of them, and
/// whether they are the ones `cargo test` ignores.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Suite {
    /// The test files, by name: `cargo test --test <name>` for each.
    tests: &'static [&'static str],
    /// The cases, by the start of their names, when not all of the files'.
    filter: Option<&'static str>,
    /// Whether the cases are the ignored ones, which run only when asked.
    ignored: bool,
}

const DAEMONS: Suite = Suite {
    tests: &["daemon", "rig", "regressions"],
    filter: None,
    ignored: false,
};
const INTEGRATION: Suite = Suite {
    tests: &["rig"],
    filter: None,
    ignored: false,
};
const REGRESSIONS: Suite = Suite {
    tests: &["regressions"],
    filter: None,
    ignored: false,
};
const CLIS: Suite = Suite {
    tests: &["cli"],
    filter: None,
    ignored: false,
};
const AGENTS: Suite = Suite {
    tests: &["daemon"],
    filter: Some("agents_proof::"),
    ignored: false,
};
const LOAD: Suite = Suite {
    tests: &["load"],
    filter: None,
    ignored: true,
};
/// The packaged smoke (`smoke`'s command): the one case of the smoke test, which
/// runs on a built app and which `cargo test` ignores for there being none.
pub(crate) const SMOKE: Suite = Suite {
    tests: &["smoke"],
    filter: None,
    ignored: true,
};

fn daemons(context: &Context, args: &[OsString], _: &mut Console) -> Result<i32, Failure> {
    run(context, DAEMONS, args)
}

fn integration(context: &Context, args: &[OsString], _: &mut Console) -> Result<i32, Failure> {
    run(context, INTEGRATION, args)
}

fn regressions(context: &Context, args: &[OsString], _: &mut Console) -> Result<i32, Failure> {
    run(context, REGRESSIONS, args)
}

fn clis(context: &Context, args: &[OsString], _: &mut Console) -> Result<i32, Failure> {
    run(context, CLIS, args)
}

fn agents(context: &Context, args: &[OsString], _: &mut Console) -> Result<i32, Failure> {
    run(context, AGENTS, args)
}

fn load(context: &Context, args: &[OsString], _: &mut Console) -> Result<i32, Failure> {
    run(context, LOAD, args)
}

fn run(context: &Context, suite: Suite, args: &[OsString]) -> Result<i32, Failure> {
    Ok(process::run(
        &cargo_test(context, suite, args),
        &context.env,
    )?)
}

/// `cargo test -p cf-e2e --test <file>…` from the checkout's root, with the
/// suite's cases named, followed by the arguments as they came. For the
/// ignored cases, `--ignored` is among the arguments for the test program: after
/// the `--` the caller gave, or after one this puts.
pub(crate) fn cargo_test(context: &Context, suite: Suite, args: &[OsString]) -> Invocation {
    let mut given: Vec<OsString> = args.to_vec();
    if suite.ignored {
        match given.iter().position(|word| word == "--") {
            Some(at) => given.insert(at + 1, "--ignored".into()),
            None => given.extend(["--".into(), "--ignored".into()]),
        }
    }
    let mut invocation = Invocation::new("cargo", &context.root).args(["test", "-p", "cf-e2e"]);
    for test in suite.tests {
        invocation = invocation.args(["--test", test]);
    }
    if let Some(filter) = suite.filter {
        invocation = invocation.arg(filter);
    }
    invocation.args(given)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::{Path, PathBuf};

    use cf_base::env::Env;

    fn context() -> Context {
        Context {
            root: PathBuf::from("checkout"),
            env: Env::default(),
        }
    }

    /// The line a suite runs as, given `words`, from the checkout's root with
    /// no variable of its own.
    fn line(suite: Suite, words: &[&str]) -> String {
        let args: Vec<OsString> = words.iter().map(OsString::from).collect();
        let invocation = cargo_test(&context(), suite, &args);
        assert_eq!(invocation.cwd, Path::new("checkout"));
        assert!(invocation.vars.is_empty());
        invocation.display()
    }

    #[test]
    fn every_suite_runs_in_rust_and_is_no_script() {
        for words in [
            ["test", "daemons"],
            ["test", "integration"],
            ["test", "regressions"],
            ["test", "clis"],
            ["test", "agents"],
            ["test", "load"],
        ] {
            let command = COMMANDS.iter().find(|command| command.words == words);
            assert!(
                matches!(command.map(|c| &c.run), Some(Run::Native(_))),
                "{words:?}"
            );
        }
    }

    #[test]
    fn the_cli_suite_is_the_cli_test_of_cf_e2e_with_the_arguments_after_it_as_they_came() {
        assert_eq!(line(CLIS, &[]), "cargo test -p cf-e2e --test cli");
        assert_eq!(
            line(CLIS, &["--offline"]),
            "cargo test -p cf-e2e --test cli --offline"
        );
        assert_eq!(
            line(
                CLIS,
                &["--", "adds_lists_edits_and_removes_an_agent", "--nocapture"]
            ),
            "cargo test -p cf-e2e --test cli -- adds_lists_edits_and_removes_an_agent --nocapture"
        );
    }

    #[test]
    fn the_daemon_suites_are_the_daemon_rig_and_regressions_tests_and_the_integration_suites_the_rigs_alone(
    ) {
        assert_eq!(
            line(DAEMONS, &[]),
            "cargo test -p cf-e2e --test daemon --test rig --test regressions"
        );
        assert_eq!(
            line(DAEMONS, &["--offline", "core_slice::"]),
            "cargo test -p cf-e2e --test daemon --test rig --test regressions --offline core_slice::"
        );
        assert_eq!(line(INTEGRATION, &[]), "cargo test -p cf-e2e --test rig");
        assert_eq!(
            line(INTEGRATION, &["--", "--nocapture"]),
            "cargo test -p cf-e2e --test rig -- --nocapture"
        );
    }

    #[test]
    fn the_regressions_suite_is_the_regressions_test_of_cf_e2e_and_no_other() {
        assert_eq!(
            line(REGRESSIONS, &[]),
            "cargo test -p cf-e2e --test regressions"
        );
        assert_eq!(
            line(REGRESSIONS, &["--offline", "--", "--nocapture"]),
            "cargo test -p cf-e2e --test regressions --offline -- --nocapture"
        );
    }

    #[test]
    fn the_agents_proof_is_the_agents_proof_cases_of_the_daemon_test() {
        assert_eq!(
            line(AGENTS, &[]),
            "cargo test -p cf-e2e --test daemon agents_proof::"
        );
        assert_eq!(
            line(AGENTS, &["--offline"]),
            "cargo test -p cf-e2e --test daemon agents_proof:: --offline"
        );
    }

    #[test]
    fn the_load_suite_asks_for_the_ignored_cases_after_the_arguments_for_cargo() {
        assert_eq!(
            line(LOAD, &[]),
            "cargo test -p cf-e2e --test load -- --ignored"
        );
        assert_eq!(
            line(LOAD, &["--offline"]),
            "cargo test -p cf-e2e --test load --offline -- --ignored"
        );
        // Words after a `--` the caller gave are the test program's: `--ignored` joins them.
        assert_eq!(
            line(LOAD, &["--", "--nocapture"]),
            "cargo test -p cf-e2e --test load -- --ignored --nocapture"
        );
    }

    #[test]
    fn no_suite_here_names_a_live_test_which_only_the_live_command_runs() {
        // Nothing that `check` or the workflows run asks for one: they are ignored,
        // and `live` is the one command that asks for the ignored.
        for suite in [DAEMONS, INTEGRATION, REGRESSIONS, CLIS, AGENTS, LOAD, SMOKE] {
            assert!(
                suite.tests.iter().all(|test| !test.starts_with("live_")),
                "{:?}",
                suite.tests
            );
        }
    }
}

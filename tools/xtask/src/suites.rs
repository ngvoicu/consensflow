//! The suites that run against the native `cf`. They are Rust's (landings S8
//! and S9): the tests of `crates/cf-e2e`, which builds the `cf` and the pane
//! host they run, so a command here has nothing to build first and hands its
//! arguments to `cargo test`: the CLI's (`cli`), the daemon's as a process
//! (`daemon`), the rig's, with the daemon and the pane host in real terminals
//! (`rig`), the regressions (`regressions`: the rig's, one case for each bug the
//! owner hit in real use), and the daemon under load (`load`, which is ignored
//! unless asked for). `test daemons` runs the first three together, for a person
//! and for the gate; CI runs it in two steps, each with a limit of its own: the
//! daemon's and the rig's (`test daemon-and-rig`), and the regressions. CI runs
//! `test daemons` in shards, a part of its cases on each of several runners
//! at once ([`shard`], named by its variable `CF_TEST_SHARD`). The live tests are ignored too, and what they run is the machine's own
//! harness on its own login, which spends real quota: they are no part of
//! `check`, and a person asks for each by name, by [`crate::live`]'s one
//! command. The packaged smoke (`smoke`) is a test of the same crate, ignored
//! the same way, and is run by its own command, [`crate::smoke`], which takes
//! the app to run it on.

mod shard;

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
        words: &["test", "daemon-and-rig"],
        about: "Run the daemon suites without the regressions (the daemon and rig tests of cf-e2e) against the native daemon, built for them: the part of `test daemons` that CI runs beside `test regressions`",
        usage: "[cargo test arguments: --offline, or a test's name, or -- and libtest's]",
        run: Run::Native(daemon_and_rig),
    },
    Command {
        words: &["test", "integration"],
        about: "Run the rig's suites (the rig and regressions tests of cf-e2e): the daemon and the pane host in real terminals, with stand-in agents in the windows",
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
const DAEMON_AND_RIG: Suite = Suite {
    tests: &["daemon", "rig"],
    filter: None,
    ignored: false,
};
const INTEGRATION: Suite = Suite {
    tests: &["rig", "regressions"],
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

/// The suite `cargo xtask test <name>` runs, by its name: the plants run its
/// `cargo test` under nextest.
pub(crate) fn named(name: &str) -> Option<Suite> {
    match name {
        "daemons" => Some(DAEMONS),
        "daemon-and-rig" => Some(DAEMON_AND_RIG),
        "integration" => Some(INTEGRATION),
        "regressions" => Some(REGRESSIONS),
        "clis" => Some(CLIS),
        "agents" => Some(AGENTS),
        "load" => Some(LOAD),
        _ => None,
    }
}

fn daemons(context: &Context, args: &[OsString], console: &mut Console) -> Result<i32, Failure> {
    run(context, DAEMONS, args, console)
}

fn daemon_and_rig(
    context: &Context,
    args: &[OsString],
    console: &mut Console,
) -> Result<i32, Failure> {
    run(context, DAEMON_AND_RIG, args, console)
}

fn integration(
    context: &Context,
    args: &[OsString],
    console: &mut Console,
) -> Result<i32, Failure> {
    run(context, INTEGRATION, args, console)
}

fn regressions(
    context: &Context,
    args: &[OsString],
    console: &mut Console,
) -> Result<i32, Failure> {
    run(context, REGRESSIONS, args, console)
}

fn clis(context: &Context, args: &[OsString], console: &mut Console) -> Result<i32, Failure> {
    run(context, CLIS, args, console)
}

fn agents(context: &Context, args: &[OsString], console: &mut Console) -> Result<i32, Failure> {
    run(context, AGENTS, args, console)
}

fn load(context: &Context, args: &[OsString], console: &mut Console) -> Result<i32, Failure> {
    run(context, LOAD, args, console)
}

/// Runs `suite` with `args`: whole, or the shard [`shard::SHARD`] names.
fn run(
    context: &Context,
    suite: Suite,
    args: &[OsString],
    console: &mut Console,
) -> Result<i32, Failure> {
    if let Some(named) = context.env.text(shard::SHARD) {
        let shard = shard::Shard::parse(named)?;
        return shard::run(context, suite, shard, args, console);
    }
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

    use std::fs;
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
            ["test", "daemon-and-rig"],
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
    fn every_suite_is_named_by_its_command_s_word_and_no_other_word_names_one() {
        let words: Vec<&str> = COMMANDS
            .iter()
            .filter_map(|command| match command.words {
                ["test", name] => Some(*name),
                _ => None,
            })
            .collect();
        assert_eq!(words.len(), COMMANDS.len());
        let suites = [
            DAEMONS,
            DAEMON_AND_RIG,
            INTEGRATION,
            REGRESSIONS,
            CLIS,
            AGENTS,
            LOAD,
        ];
        assert_eq!(words.len(), suites.len());
        for (word, suite) in words.iter().zip(suites) {
            assert_eq!(named(word), Some(suite), "{word}");
        }
        assert_eq!(named("smoke"), None);
        assert_eq!(named("daemon"), None);
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
    fn the_daemon_suites_are_the_daemon_rig_and_regressions_tests_and_the_integration_suites_the_rigs(
    ) {
        assert_eq!(
            line(DAEMONS, &[]),
            "cargo test -p cf-e2e --test daemon --test rig --test regressions"
        );
        assert_eq!(
            line(DAEMONS, &["--offline", "core_slice::"]),
            "cargo test -p cf-e2e --test daemon --test rig --test regressions --offline core_slice::"
        );
        // CI runs the daemons in two steps with a limit each, and this is the first: all of them but the regressions.
        assert_eq!(
            line(DAEMON_AND_RIG, &[]),
            "cargo test -p cf-e2e --test daemon --test rig"
        );
        assert_eq!(
            line(DAEMON_AND_RIG, &["--offline", "--", "--nocapture"]),
            "cargo test -p cf-e2e --test daemon --test rig --offline -- --nocapture"
        );
        // The regressions are rig cases, one for each bug the owner met: wherever the rig runs, they do.
        assert_eq!(
            line(INTEGRATION, &[]),
            "cargo test -p cf-e2e --test rig --test regressions"
        );
        assert_eq!(
            line(INTEGRATION, &["--", "--nocapture"]),
            "cargo test -p cf-e2e --test rig --test regressions -- --nocapture"
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
        for suite in [
            DAEMONS,
            DAEMON_AND_RIG,
            INTEGRATION,
            REGRESSIONS,
            CLIS,
            AGENTS,
            LOAD,
            SMOKE,
        ] {
            assert!(
                suite.tests.iter().all(|test| !test.starts_with("live_")),
                "{:?}",
                suite.tests
            );
        }
    }

    /// The scripts the gate's steps run, `npm run test:<name>`, each as the suite
    /// it hands over to.
    fn suites_the_gate_runs() -> Vec<Suite> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let workflow = fs::read_to_string(root.join(".github/workflows/ci.yml")).unwrap();
        let package: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join("package.json")).unwrap()).unwrap();
        workflow
            .lines()
            .filter_map(|line| line.trim().strip_prefix("- run: npm run test:"))
            .map(|script| {
                let command = package["scripts"][format!("test:{script}")]
                    .as_str()
                    .unwrap_or_else(|| panic!("ci.yml runs npm run test:{script}, which is none"));
                let name = command
                    .strip_prefix("cargo xtask test ")
                    .unwrap_or_else(|| panic!("npm run test:{script} is `{command}`"));
                named(name).unwrap_or_else(|| panic!("npm run test:{script} runs no suite"))
            })
            .collect()
    }

    /// The daemons' test files were one step of the gate and passed its limit, and
    /// are cut in two. However they are cut, between the steps each file of
    /// `test daemons` runs once: not left out of the gate, not run twice for the
    /// gate's time.
    #[test]
    fn the_steps_of_the_gate_between_them_run_each_test_file_of_the_daemons_once() {
        let run: Vec<&str> = suites_the_gate_runs()
            .iter()
            .flat_map(|suite| suite.tests.iter().copied())
            .collect();
        for file in DAEMONS.tests {
            let times = run.iter().filter(|ran| *ran == file).count();
            assert_eq!(
                times, 1,
                "ci.yml runs the {file} test {times} times: {run:?}"
            );
        }
    }

    /// A suite run with the shard's variable set is that shard: a shard it cannot be is
    /// refused before anything runs, and without the variable the suite runs whole
    /// (here, in a folder that is not there, which the shard never reaches).
    #[test]
    fn the_shard_a_run_is_comes_from_its_variable() {
        let with = |value: Option<&str>| Context {
            root: PathBuf::from("no-such-checkout"),
            env: match value {
                Some(value) => Env::from_vars([(shard::SHARD, value)]),
                None => Env::default(),
            },
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut console = Console {
            out: &mut out,
            err: &mut err,
        };
        for (value, suite) in [
            (Some("4/3"), DAEMONS),
            (Some("1/2"), AGENTS),
            (Some("1/2"), LOAD),
        ] {
            assert!(
                matches!(
                    run(&with(value), suite, &[], &mut console),
                    Err(Failure::Usage(_))
                ),
                "{value:?} {suite:?}"
            );
        }
        assert!(matches!(
            run(&with(None), DAEMONS, &[], &mut console),
            Err(Failure::Process(_))
        ));
    }

    /// CI's workflow, as written.
    fn workflow() -> String {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        fs::read_to_string(root.join(".github/workflows/ci.yml")).unwrap()
    }

    /// The jobs of the workflow, by id, each with its lines.
    fn jobs(workflow: &str) -> Vec<(String, Vec<String>)> {
        let mut jobs: Vec<(String, Vec<String>)> = Vec::new();
        let mut inside = false;
        for line in workflow.lines() {
            if line.trim_start().starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if !line.starts_with(' ') {
                inside = line == "jobs:";
                continue;
            }
            if !inside {
                continue;
            }
            match line.strip_prefix("  ").and_then(|id| id.strip_suffix(':')) {
                Some(id) if !id.starts_with(' ') => jobs.push((id.to_owned(), Vec::new())),
                _ => {
                    if let Some((_, lines)) = jobs.last_mut() {
                        lines.push(line.trim().to_owned());
                    }
                }
            }
        }
        jobs
    }

    /// The items of a flow list, `[a, 'b']`, after `key: ` in one of `lines`.
    fn listed(lines: &[String], key: &str) -> Vec<String> {
        let line = lines
            .iter()
            .find_map(|line| line.strip_prefix(&format!("{key}: [")))
            .unwrap_or_else(|| panic!("no `{key}: [` in {lines:?}"));
        line.trim_end_matches(']')
            .split(',')
            .map(|item| item.trim().trim_matches('\'').to_owned())
            .collect()
    }

    /// The daemons' suite runs in shards, and coverage rests on the shards being
    /// each of N once, `1/N` to `N/N`, on each platform, each named to the suite.
    #[test]
    fn ci_runs_the_daemons_in_each_shard_of_n_once_on_each_platform() {
        let jobs = jobs(&workflow());
        let (_, daemons) = jobs
            .iter()
            .find(|(_, lines)| {
                lines
                    .iter()
                    .any(|line| line == "- run: npm run test:daemons")
            })
            .expect("no job runs npm run test:daemons");
        let shards = listed(daemons, "shard");
        let count = shards.len();
        let each: Vec<String> = (1..=count)
            .map(|index| format!("{index}/{count}"))
            .collect();
        assert_eq!(shards, each);
        for index in 1..=count {
            assert!(shard::Shard::parse(&each[index - 1]).is_ok());
        }
        assert_eq!(listed(daemons, "os"), ["macos-latest", "windows-latest"]);
        assert!(
            daemons
                .iter()
                .any(|line| line == "CF_TEST_SHARD: ${{ matrix.shard }}"),
            "{daemons:?}"
        );
        assert_eq!(shard::SHARD, "CF_TEST_SHARD");
    }

    /// A shard says its time against its step's limit (T-836): the limit it is told is the
    /// step's own `timeout-minutes`, so that a change of either is a change of both.
    #[test]
    fn the_limit_a_shard_is_told_is_its_steps_timeout() {
        let jobs = jobs(&workflow());
        let (_, daemons) = jobs
            .iter()
            .find(|(_, lines)| {
                lines
                    .iter()
                    .any(|line| line == "- run: npm run test:daemons")
            })
            .expect("no job runs npm run test:daemons");
        let step: Vec<&String> = daemons
            .iter()
            .skip_while(|line| *line != "- run: npm run test:daemons")
            .skip(1)
            .take_while(|line| !line.starts_with("- "))
            .collect();
        let value = |key: &str| {
            step.iter()
                .find_map(|line| line.strip_prefix(&format!("{key}: ")))
                .unwrap_or_else(|| panic!("the step has no {key}: {step:?}"))
                .to_owned()
        };
        assert_eq!(value(shard::LIMIT), value("timeout-minutes"), "{step:?}");
        assert!(value("timeout-minutes").parse::<u64>().is_ok());
    }

    /// The workspace's tests on Windows open their panes' consoles with the console host
    /// the app ships, which the key tests ask for: CI puts it beside the build before
    /// them, as the Windows runner does. Without it they ran on the system's own, which
    /// made a mark typed as text the Alt key carrying it (main's CI, 2026-10-27).
    #[test]
    fn ci_puts_the_console_host_the_app_ships_beside_the_workspace_tests_on_windows() {
        let jobs = jobs(&workflow());
        let (_, workspace) = jobs
            .iter()
            .find(|(_, lines)| {
                lines
                    .iter()
                    .any(|line| line.starts_with("- run: cargo test --workspace "))
            })
            .expect("no job runs cargo test --workspace");
        let at = |wanted: &str| workspace.iter().position(|line| line == wanted);
        let tests = at("- run: cargo test --workspace --exclude app --exclude cf-e2e");
        let host = at("run: cargo xtask conpty --into app/src-tauri/target/debug");
        assert!(
            host.is_some_and(|host| tests.is_some_and(|tests| host < tests)),
            "{workspace:?}"
        );
        let condition = host.and_then(|host| workspace.get(host - 1));
        assert!(
            condition.is_some_and(|line| line == "- if: runner.os == 'Windows'"),
            "{workspace:?}"
        );
        assert_eq!(listed(workspace, "os"), ["macos-latest", "windows-latest"]);
    }

    /// The check a release and a branch rule trust, `gate (<os>)`: it waits for
    /// every other job of the workflow, runs whatever they did, and passes only
    /// when each passed: a job that failed, was cancelled or was skipped fails it,
    /// as does a run of none. Its test of the jobs' results is run with jq, the
    /// runner's, where this machine has one.
    #[test]
    fn the_gate_is_green_only_when_every_other_job_of_ci_passed() {
        let workflow = workflow();
        let jobs = jobs(&workflow);
        let (_, gate) = jobs
            .iter()
            .find(|(id, _)| id == "gate")
            .expect("ci.yml has no gate job");
        let mut others: Vec<String> = jobs
            .iter()
            .map(|(id, _)| id.clone())
            .filter(|id| id != "gate")
            .collect();
        let mut needs = listed(gate, "needs");
        others.sort();
        needs.sort();
        assert_eq!(needs, others);
        assert!(gate.iter().any(|line| line == "if: always()"), "{gate:?}");
        assert_eq!(listed(gate, "os"), ["macos-latest", "windows-latest"]);
        assert!(gate
            .iter()
            .any(|line| line == "NEEDS: ${{ toJSON(needs) }}"));
        let program = gate
            .iter()
            .find_map(|line| {
                line.strip_prefix("echo \"$NEEDS\" | jq -e '")?
                    .strip_suffix("' > /dev/null")
            })
            .expect("the gate tests the jobs' results with no `jq -e`");
        let results = |each: &[&str]| -> String {
            let entries: Vec<String> = each
                .iter()
                .enumerate()
                .map(|(at, result)| {
                    format!("\"job{at}\":{{\"result\":\"{result}\",\"outputs\":{{}}}}")
                })
                .collect();
            format!("{{{}}}", entries.join(","))
        };
        let passes = |each: &[&str]| -> Option<bool> {
            let jq = Invocation::new("jq", ".")
                .args(["-n", "-e", "--argjson", "needs"])
                .arg(results(each))
                .arg(format!("$needs | ({program})"));
            let answered = process::capture(&jq, &Env::from_process()).ok()?;
            Some(answered.code == 0)
        };
        let Some(all) = passes(&["success", "success", "success"]) else {
            eprintln!("there is no jq here: the gate's test of the results is not run");
            return;
        };
        assert!(all);
        for each in [
            &["success", "failure", "success"][..],
            &["success", "cancelled", "success"],
            &["skipped", "success", "success"],
            &[],
        ] {
            assert_eq!(passes(each), Some(false), "{each:?}");
        }
    }
}

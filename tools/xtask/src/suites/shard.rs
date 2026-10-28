//! One shard of a suite: a part of its cases, so that CI runs the daemons'
//! suites on several runners at once (T-558). Their cases take turns on a
//! machine ([`cf_e2e::serial`]), a rig at a time, and took 40 to 55 minutes in
//! one job; on three runners each takes a third, and each runner still runs a
//! rig at a time.
//!
//! The shard is named by [`SHARD`], `I/N` (the I-th of N, from 1): CI's step
//! is `npm run test:daemons` with it set, so the step is the one the gate's
//! test of the suites reads (`suites_the_gate_runs`). Each test file of the
//! suite says its cases (`--list`, ignored ones among them), the cases of all
//! the files are taken in the suite's order of files and each file's in the
//! order of their names, and the shard runs every N-th of them from its I-th,
//! each by its whole name (`--exact`). The N shards of one checkout on one
//! platform list the same cases, so between them they run each case once:
//! the cases of a module, which take about as long as each other (the Devin
//! regressions take two minutes each), go to different shards. The test
//! program is still libtest's, as without a shard: one process for a file's
//! cases, which take turns in it.
//!
//! A shard says how long it has taken after each test file, against the limit its
//! step has where CI gives it one ([`LIMIT`], the step's `timeout-minutes`), and
//! warns past [`crate::ci_times::SHARE`] of it: on GitHub as a warning of the run
//! (`::warning::`), so that a suite that grows is seen before a shard is cut short
//! (3.0.6-alpha.3's integration B doubled the daemons' cases' time, and every macOS
//! shard hit its 30 minutes in the first run on main, T-836).

use std::ffi::OsString;
use std::time::{Duration, Instant};

use crate::context::Context;
use crate::dispatch::{Console, Failure};
use crate::process::{self, Invocation};

use super::Suite;

/// The variable that names the shard of a suite a run is: `I/N`.
pub(crate) const SHARD: &str = "CF_TEST_SHARD";

/// The variable that says, in minutes, the limit of the step a shard runs in: CI's
/// `timeout-minutes` of that step, which ci.yml gives it beside (a test holds the two
/// equal).
pub(crate) const LIMIT: &str = "CF_TEST_SHARD_LIMIT_MINUTES";

/// The variable GitHub's runners set, where a warning of the run is a line of its own.
const ON_GITHUB: &str = "GITHUB_ACTIONS";

/// The most the names given to one run of a test file may come to, in bytes:
/// Windows starts no program whose command line passes 32,767 characters, and
/// a shard with more is run in several.
const NAMES_AT_MOST: usize = 8_000;

/// The I-th of N parts of a suite's cases, I from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Shard {
    index: usize,
    count: usize,
}

impl Shard {
    /// The shard `text` names, `I/N` with 1 ≤ I ≤ N.
    pub(crate) fn parse(text: &str) -> Result<Self, Failure> {
        let refused = || Failure::Usage(format!("{SHARD} is I/N with 1 <= I <= N, not `{text}`"));
        let (index, count) = text.split_once('/').ok_or_else(refused)?;
        let (index, count) = (
            index.trim().parse::<usize>().map_err(|_| refused())?,
            count.trim().parse::<usize>().map_err(|_| refused())?,
        );
        if index == 0 || index > count {
            return Err(refused());
        }
        Ok(Self { index, count })
    }

    /// Whether the case at `position` of the suite's list (from 0) is this shard's.
    fn takes(self, position: usize) -> bool {
        position % self.count == self.index - 1
    }
}

/// The cases a test program says it has, from what `--list --format terse` wrote:
/// a line `name: test` for each.
fn cases(listed: &str) -> Vec<String> {
    let mut names: Vec<String> = listed
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
        .map(str::to_owned)
        .collect();
    names.sort();
    names
}

/// The cases of `listed` (each test file of a suite with its cases, in the
/// suite's order) that are `shard`'s, file by file.
fn taken(shard: Shard, listed: &[(&'static str, Vec<String>)]) -> Vec<(&'static str, Vec<String>)> {
    let mut position = 0;
    listed
        .iter()
        .map(|(file, names)| {
            let mine = names
                .iter()
                .filter(|_| {
                    position += 1;
                    shard.takes(position - 1)
                })
                .cloned()
                .collect();
            (*file, mine)
        })
        .collect()
}

/// `cargo test -p cf-e2e --test <file>` with the words for cargo, from the checkout's root.
fn cargo_test(context: &Context, file: &str, words: &[OsString]) -> Invocation {
    Invocation::new("cargo", &context.root)
        .args(["test", "-p", "cf-e2e", "--test", file])
        .args(words)
}

/// The run that asks the test file `file` for its cases.
fn listing(context: &Context, file: &str, words: &[OsString]) -> Invocation {
    cargo_test(context, file, words).args(["--", "--list", "--format", "terse"])
}

/// The runs of the test file `file` that run `names`, each by its whole name, as
/// many as keep each run's names under [`NAMES_AT_MOST`].
fn runs(context: &Context, file: &str, words: &[OsString], names: &[String]) -> Vec<Invocation> {
    let mut parts: Vec<Vec<&String>> = Vec::new();
    let mut size = 0;
    for name in names {
        match parts.last_mut() {
            Some(part) if size + name.len() < NAMES_AT_MOST => part.push(name),
            _ => {
                parts.push(vec![name]);
                size = 0;
            }
        }
        size += name.len() + 1;
    }
    parts
        .into_iter()
        .map(|part| {
            cargo_test(context, file, words)
                .args(["--", "--exact"])
                .args(part)
        })
        .collect()
}

/// Runs `shard` of `suite`, with `args` for cargo: lists each test file's
/// cases, says how many are the shard's, and runs them, stopping at the first
/// run that fails, as `cargo test` stops at the first test program that does.
pub(crate) fn run(
    context: &Context,
    suite: Suite,
    shard: Shard,
    args: &[OsString],
    console: &mut Console,
) -> Result<i32, Failure> {
    if suite.filter.is_some() || suite.ignored {
        return Err(Failure::Usage(format!(
            "{SHARD} shards a suite all of whose cases run: not {:?}",
            suite.tests
        )));
    }
    if let Some(word) = args
        .iter()
        .find(|word| *word == "--" || !word.to_string_lossy().starts_with('-'))
    {
        return Err(Failure::Usage(format!(
            "a shard runs the cases it is given and takes cargo's options only, not `{}`",
            word.to_string_lossy()
        )));
    }
    let mut listed = Vec::new();
    for file in suite.tests {
        let said = process::capture(&listing(context, file, args), &context.env)?;
        console.err.write_all(said.stderr.as_bytes())?;
        if said.code != 0 {
            return Ok(said.code);
        }
        listed.push((*file, cases(&said.stdout)));
    }
    let mine = taken(shard, &listed);
    let all: usize = listed.iter().map(|(_, names)| names.len()).sum();
    let ours: usize = mine.iter().map(|(_, names)| names.len()).sum();
    let each: Vec<String> = mine
        .iter()
        .zip(&listed)
        .map(|((file, names), (_, of))| format!("{file} {} of {}", names.len(), of.len()))
        .collect();
    writeln!(
        console.err,
        "shard {}/{}: {ours} of the {all} cases ({})",
        shard.index,
        shard.count,
        each.join(", ")
    )?;
    let limit = context
        .env
        .text(LIMIT)
        .and_then(|minutes| minutes.trim().parse::<u64>().ok())
        .map(|minutes| Duration::from_secs(minutes * 60));
    let on_github = context.env.text(ON_GITHUB) == Some("true");
    let began = Instant::now();
    for (file, names) in &mine {
        for invocation in runs(context, file, args, names) {
            let code = process::run(&invocation, &context.env)?;
            if code != 0 {
                return Ok(code);
            }
        }
        for line in timing(shard, file, began.elapsed(), limit, on_github) {
            writeln!(console.err, "{line}")?;
        }
    }
    Ok(0)
}

/// What the shard says once it has run the cases of `file`, `elapsed` since it began:
/// how long it has taken, of its `limit` where it has one, and a warning past
/// [`crate::ci_times::SHARE`] of it (GitHub's own line `::warning` where `on_github`).
fn timing(
    shard: Shard,
    file: &str,
    elapsed: Duration,
    limit: Option<Duration>,
    on_github: bool,
) -> Vec<String> {
    let (index, count) = (shard.index, shard.count);
    let took = elapsed.as_secs();
    let Some(limit) = limit.filter(|limit| !limit.is_zero()) else {
        return vec![format!("shard {index}/{count}: {file} done, {took} s in")];
    };
    let of = limit.as_secs();
    let percent = elapsed.as_millis() * 100 / limit.as_millis();
    let mut lines = vec![format!(
        "shard {index}/{count}: {file} done, {took} s in, {percent} % of the step's {of} s"
    )];
    if percent >= u128::from(crate::ci_times::SHARE) {
        let said = format!(
            "shard {index}/{count} of the daemons has taken {percent} % of its step's {of} s \
             (past {} %): add a shard or raise the limit before it is cut short",
            crate::ci_times::SHARE
        );
        lines.push(if on_github {
            format!("::warning title=shard {index}/{count} near its limit::{said}")
        } else {
            format!("warning: {said}")
        });
    }
    lines
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

    fn names(count: usize, prefix: &str) -> Vec<String> {
        (0..count)
            .map(|n| format!("{prefix}::case_{n:03}"))
            .collect()
    }

    /// A shard says its time after each test file, against its step's limit where it has
    /// one, and warns past the share `ci-times` holds a step to; on GitHub as a warning of
    /// the run.
    #[test]
    fn a_shard_says_its_time_against_its_limit_and_warns_past_the_share() {
        let shard = Shard { index: 2, count: 6 };
        let minutes = |n: u64| Duration::from_secs(n * 60);
        assert_eq!(
            timing(shard, "rig", Duration::from_secs(400), None, true),
            ["shard 2/6: rig done, 400 s in"]
        );
        assert_eq!(
            timing(shard, "rig", minutes(15), Some(minutes(30)), true),
            ["shard 2/6: rig done, 900 s in, 50 % of the step's 1800 s"]
        );
        let near = timing(shard, "regressions", minutes(24), Some(minutes(30)), true);
        assert_eq!(
            near[0],
            "shard 2/6: regressions done, 1440 s in, 80 % of the step's 1800 s"
        );
        assert!(
            near[1].starts_with("::warning title=shard 2/6 near its limit::"),
            "{near:?}"
        );
        let off_github = timing(shard, "regressions", minutes(29), Some(minutes(30)), false);
        assert!(
            off_github[1].starts_with("warning: shard 2/6 of the daemons has taken 96 %"),
            "{off_github:?}"
        );
    }

    #[test]
    fn a_shard_is_i_of_n_from_one_and_nothing_else() {
        assert_eq!(Shard::parse("1/3").unwrap(), Shard { index: 1, count: 3 });
        assert_eq!(Shard::parse("3/3").unwrap(), Shard { index: 3, count: 3 });
        assert_eq!(Shard::parse("1/1").unwrap(), Shard { index: 1, count: 1 });
        for refused in ["0/3", "4/3", "3", "/3", "1/", "a/3", "1/0", "-1/3", ""] {
            assert!(
                matches!(Shard::parse(refused), Err(Failure::Usage(_))),
                "{refused}"
            );
        }
    }

    #[test]
    fn the_cases_are_the_lines_a_test_program_lists_as_tests_in_the_order_of_their_names() {
        let listed =
            "zeta::b: test\nalpha::a: test\nsome::bench: benchmark\n\n2 tests, 1 benchmark\n";
        assert_eq!(cases(listed), ["alpha::a", "zeta::b"]);
    }

    /// Whatever the files and however many the shards, the shards between them
    /// take every case once, and none takes more than one case over another.
    #[test]
    fn the_shards_between_them_take_each_case_once_and_about_as_many_each() {
        for (daemon, rig, regressions) in
            [(15, 152, 87), (0, 5, 0), (1, 0, 2), (0, 0, 0), (3, 4, 5)]
        {
            let listed = vec![
                ("daemon", names(daemon, "d")),
                ("rig", names(rig, "r")),
                ("regressions", names(regressions, "g")),
            ];
            for count in 1..=6 {
                let mut seen: Vec<(&str, String)> = Vec::new();
                let mut sizes = Vec::new();
                for index in 1..=count {
                    let mine = taken(Shard { index, count }, &listed);
                    assert_eq!(
                        mine.iter().map(|(file, _)| *file).collect::<Vec<_>>(),
                        ["daemon", "rig", "regressions"]
                    );
                    sizes.push(mine.iter().map(|(_, names)| names.len()).sum::<usize>());
                    for (file, names) in mine {
                        seen.extend(names.into_iter().map(|name| (file, name)));
                    }
                }
                let mut all: Vec<(&str, String)> = listed
                    .iter()
                    .flat_map(|(file, names)| names.iter().map(|name| (*file, name.clone())))
                    .collect();
                seen.sort();
                all.sort();
                assert_eq!(seen, all, "{count} shards of {daemon}/{rig}/{regressions}");
                let (most, least) = (sizes.iter().max(), sizes.iter().min());
                assert!(most.unwrap() - least.unwrap() <= 1, "{sizes:?}");
            }
        }
    }

    /// The cases of one module, side by side in the list, go to different shards.
    #[test]
    fn neighbouring_cases_go_to_different_shards() {
        let listed = vec![("regressions", names(6, "devin"))];
        for index in 1..=3 {
            let mine = taken(Shard { index, count: 3 }, &listed);
            assert_eq!(mine[0].1.len(), 2, "{mine:?}");
            assert_eq!(
                mine[0].1,
                [
                    format!("devin::case_{:03}", index - 1),
                    format!("devin::case_{:03}", index + 2)
                ]
            );
        }
    }

    #[test]
    fn a_file_is_asked_for_its_cases_and_run_with_them_by_their_whole_names() {
        let words: Vec<OsString> = vec!["--offline".into()];
        assert_eq!(
            listing(&context(), "rig", &words).display(),
            "cargo test -p cf-e2e --test rig --offline -- --list --format terse"
        );
        let given = vec!["a::one".to_owned(), "b::two".to_owned()];
        let lines: Vec<String> = runs(&context(), "rig", &words, &given)
            .iter()
            .map(Invocation::display)
            .collect();
        assert_eq!(
            lines,
            ["cargo test -p cf-e2e --test rig --offline -- --exact a::one b::two"]
        );
        assert!(runs(&context(), "rig", &words, &[]).is_empty());
    }

    /// A shard with more names than a command line holds on Windows is run in
    /// several runs, each under the bound, which between them name each case once.
    #[test]
    fn names_past_the_bound_of_one_command_line_are_run_in_several() {
        let given: Vec<String> = (0..400)
            .map(|n| format!("a_regression_with_a_long_name_as_they_have::case_{n:03}"))
            .collect();
        let parts = runs(&context(), "regressions", &[], &given);
        assert!(parts.len() > 1, "{}", parts.len());
        let mut named = Vec::new();
        for part in &parts {
            let after: Vec<String> = part
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .skip_while(|arg| arg != "--exact")
                .skip(1)
                .collect();
            let size: usize = after.iter().map(|name| name.len() + 1).sum();
            assert!(size <= NAMES_AT_MOST, "{size}");
            named.extend(after);
        }
        assert_eq!(named, given);
    }

    #[test]
    fn a_shard_is_refused_for_a_suite_it_would_not_run_whole_or_words_that_name_cases() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut console = Console {
            out: &mut out,
            err: &mut err,
        };
        let shard = Shard { index: 1, count: 2 };
        for suite in [super::super::AGENTS, super::super::LOAD] {
            assert!(matches!(
                run(&context(), suite, shard, &[], &mut console),
                Err(Failure::Usage(_))
            ));
        }
        for words in [
            &["core_slice::"][..],
            &["--", "--nocapture"],
            &["--offline", "--"],
        ] {
            let args: Vec<OsString> = words.iter().map(OsString::from).collect();
            assert!(
                matches!(
                    run(
                        &context(),
                        super::super::DAEMONS,
                        shard,
                        &args,
                        &mut console
                    ),
                    Err(Failure::Usage(_))
                ),
                "{words:?}"
            );
        }
    }
}

//! The limits as `ci.yml` writes them, and the times of runs GitHub would
//! answer, canned: nothing here reaches GitHub. The shape of a `jobs` answer is
//! the real one (run 39671630162, main's CI at 04bc9d66, 2026-10-21).

use std::ffi::OsString;
use std::path::Path;

use serde_json::{json, Value};

use super::*;
use crate::process::Captured;

const WORKSPACE_TESTS: &str = "Run cargo test --workspace --exclude app --exclude cf-e2e";

const WORKFLOW: &str = r#"# A comment.
name: CI

on:
  push:
    branches: [main]

jobs:
  gate:
    runs-on: ${{ matrix.os }}
    # A hang fails the job in two hours.
    timeout-minutes: 120
    steps:
      - uses: actions/checkout@v7
      - run: npm ci
      - run: npm run test:daemons
        timeout-minutes: 50
      - name: The app's own tests
        run: cargo test
        working-directory: app/src-tauri
        timeout-minutes: 15 # room for a slow runner
      - run: |
          cargo test --workspace
          echo done
        timeout-minutes: 40
      - run: cargo clippy --workspace
  other:
    runs-on: ubuntu-latest
    timeout-minutes: 30
    steps:
      - run: true
"#;

/// The part of `ci.yml` the check is asked about, as it writes it.
const CI: &str = "jobs:
  gate:
    timeout-minutes: 120
    steps:
      - run: cargo test --workspace --exclude app --exclude cf-e2e
        timeout-minutes: 40
";

fn step(name: &str, from: i64, to: i64, conclusion: &str) -> Value {
    json!({
        "name": name,
        "conclusion": conclusion,
        "started_at": clock(from),
        "completed_at": clock(to),
    })
}

/// `seconds` after 20:00:00 on 2026-10-21, as GitHub writes a time.
fn clock(seconds: i64) -> String {
    let (hours, minutes, seconds) = (20 + seconds / 3600, seconds / 60 % 60, seconds % 60);
    format!(
        "2026-10-{:02}T{:02}:{minutes:02}:{seconds:02}Z",
        21 + hours / 24,
        hours % 24
    )
}

/// A run's `jobs` answer: each job by its name, its seconds, and its steps as
/// (name, start, end, conclusion).
fn jobs(of: &[(&str, i64, Vec<Value>)]) -> Value {
    json!({
        "total_count": of.len(),
        "jobs": of.iter().enumerate().map(|(at, (name, seconds, steps))| json!({
            "id": 1_000 + at,
            "name": name,
            "status": "completed",
            "conclusion": "success",
            "started_at": clock(0),
            "completed_at": clock(*seconds),
            "steps": steps,
        })).collect::<Vec<_>>(),
    })
}

/// Main's run of 04bc9d66: Windows' job 86.2 minutes, its workspace tests 33.9.
fn main_at_04bc9d66() -> Value {
    jobs(&[
        (
            "gate (macos-latest)",
            3_897,
            vec![step(WORKSPACE_TESTS, 1_800, 2_300, "success")],
        ),
        (
            "gate (windows-latest)",
            5_175,
            vec![
                step("Run npm ci", 10, 70, "success"),
                step(WORKSPACE_TESTS, 3_005, 5_041, "success"),
            ],
        ),
    ])
}

/// The log of Windows' job of run 39671630162 as GitHub writes it, cut to the step and
/// to a few of its binaries: the time before each line, a library's unit tests before
/// the tests of its crate, a doc-test, and the step after.
const LOG: &str = "2026-10-21T21:25:39.0000000Z ##[group]Run npm ci
2026-10-21T21:25:40.1358729Z ##[group]Run cargo test --workspace --exclude app --exclude cf-e2e
2026-10-21T21:25:40.1359441Z cargo test --workspace --exclude app --exclude cf-e2e
2026-10-21T21:25:40.1377173Z ##[endgroup]
2026-10-21T21:25:40.6480722Z    Compiling cf-base v3.0.6-alpha.1 (D:\\a\\consensflow\\consensflow\\crates\\cf-base)
2026-10-21T21:29:13.8305838Z     Finished `test` profile [unoptimized + debuginfo] target(s) in 3m 33s
2026-10-21T21:29:13.9497000Z      Running unittests src\\lib.rs (app/src-tauri/target\\debug\\deps\\cf_daemon-abfcae788fc0bc04.exe)
2026-10-21T21:29:13.9702171Z running 2 tests
2026-10-21T21:29:13.9800281Z test board::cut::tests::a ... ok
2026-10-21T21:29:14.0000000Z     Finished `dev` profile [unoptimized + debuginfo] target(s) in 45s
2026-10-21T21:29:16.0525358Z test result: ok. 60 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 261.55s
2026-10-21T21:29:16.0566981Z      Running tests\\api\\main.rs (app/src-tauri/target\\debug\\deps\\api-fda0f298d31e30e4.exe)
2026-10-21T21:29:16.0723896Z test result: ok. 53 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 58.11s
2026-10-21T21:29:16.0743221Z      Running unittests src\\lib.rs (app/src-tauri/target\\debug\\deps\\cf_ledger-0f4511c365e3e775.exe)
2026-10-21T21:29:17.0599653Z test result: ok. 75 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.27s
2026-10-21T21:29:17.0648232Z      Running tests\\replay.rs (app/src-tauri/target\\debug\\deps\\replay-a7457ba2da560bbb.exe)
2026-10-21T21:29:21.9499912Z test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 279.85s
2026-10-21T21:59:36.0000000Z    Doc-tests cf_base
2026-10-21T21:59:36.1000000Z test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
2026-10-21T21:59:36.2000000Z ##[group]Run cargo test -p cf-e2e --lib --bins
2026-10-21T21:59:36.2500000Z      Running unittests src\\lib.rs (target\\debug\\deps\\cf_e2e-1c4d5b6a7e8f9012.exe)
2026-10-21T21:59:36.3000000Z test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 99.00s
";

fn limits_of_ci() -> Vec<Limit> {
    vec![
        Limit {
            held: Held::Job("gate".into()),
            minutes: 120,
        },
        Limit {
            held: Held::Step(WORKSPACE_TESTS.into()),
            minutes: 40,
        },
    ]
}

/// What `gh` answers: what it wrote, or the status it ended with and what it said.
type Answer = Result<String, (i32, &'static str)>;

/// A GitHub that is a script: what `gh` answers to the command line that holds
/// each key, and the command lines it was asked.
struct Script {
    answers: Vec<(&'static str, Answer)>,
    asked: Vec<String>,
}

impl System for Script {
    fn capture(&mut self, invocation: &Invocation) -> Result<Captured, Error> {
        let line = invocation.display();
        self.asked.push(line.clone());
        let (_, answer) = self
            .answers
            .iter()
            .find(|(key, _)| line.contains(key))
            .unwrap_or_else(|| panic!("gh was asked what the test did not set up: {line}"));
        Ok(match answer {
            Ok(said) => Captured {
                code: 0,
                stdout: said.clone(),
                stderr: String::new(),
            },
            Err((code, said)) => Captured {
                code: *code,
                stdout: String::new(),
                stderr: (*said).to_owned(),
            },
        })
    }

    fn pause(&mut self, _: std::time::Duration) {}
}

fn words(line: &str) -> Vec<OsString> {
    line.split_whitespace().map(OsString::from).collect()
}

fn read(system: &mut Script, options: &Options) -> (Result<bool, Error>, String) {
    let mut out = Vec::new();
    let outcome = check(system, Path::new("."), CI, options, &mut out);
    (outcome, String::from_utf8(out).unwrap())
}

#[test]
fn a_job_and_a_step_are_found_by_the_name_github_shows_them_under() {
    assert_eq!(
        limits(WORKFLOW),
        vec![
            Limit {
                held: Held::Job("gate".into()),
                minutes: 120
            },
            Limit {
                held: Held::Step("Run npm run test:daemons".into()),
                minutes: 50
            },
            // A step named is shown by its name, and a comment after a value is not part of it.
            Limit {
                held: Held::Step("The app's own tests".into()),
                minutes: 15
            },
            // A literal block is named by its first line, and its other lines are no keys.
            Limit {
                held: Held::Step("Run cargo test --workspace".into()),
                minutes: 40
            },
            Limit {
                held: Held::Job("other".into()),
                minutes: 30
            },
        ]
    );
}

#[test]
fn the_workspace_tests_and_their_job_have_limits_in_the_ci_this_checkout_runs() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(".github/workflows/ci.yml");
    let limits = limits(&std::fs::read_to_string(file).unwrap());
    for held in [Held::Job("gate".into()), Held::Step(WORKSPACE_TESTS.into())] {
        assert!(
            limits.iter().any(|limit| limit.held == held),
            "ci.yml gives {held:?} no timeout-minutes: renamed, or ci-times reads it wrong"
        );
    }
}

#[test]
fn a_time_over_its_share_of_the_limit_is_over_and_one_at_the_share_is_not() {
    let reading = |seconds| Reading {
        run: 1,
        job_id: 2,
        job: "gate (windows-latest)".into(),
        what: WORKSPACE_TESTS.into(),
        seconds,
        limit_minutes: 40,
    };
    // 80 % of 40 minutes is 32 minutes.
    assert!(!reading(32 * 60).over(80));
    assert!(reading(32 * 60 + 1).over(80));
    assert!(!reading(32 * 60 + 1).over(81));
    assert_eq!(reading(32 * 60).percent(), 80);
}

#[test]
fn what_main_took_at_04bc9d66_reads_as_a_step_at_85_percent_and_a_job_at_72() {
    let found = readings(39_671_630_162, &limits_of_ci(), &main_at_04bc9d66());
    let shown: Vec<_> = found
        .iter()
        .map(|reading| {
            (
                reading.job.as_str(),
                reading.what.as_str(),
                reading.percent(),
            )
        })
        .collect();
    assert_eq!(
        shown,
        vec![
            ("gate (macos-latest)", "the job", 54),
            ("gate (macos-latest)", WORKSPACE_TESTS, 20),
            ("gate (windows-latest)", "the job", 71),
            ("gate (windows-latest)", WORKSPACE_TESTS, 84),
        ]
    );
    let over: Vec<_> = found.iter().filter(|reading| reading.over(80)).collect();
    assert_eq!(over.len(), 1, "{over:?}");
    assert_eq!(over[0].job, "gate (windows-latest)");
}

#[test]
fn a_step_skipped_or_never_started_has_no_time_and_a_step_cancelled_at_its_limit_has_its_own() {
    let answer = jobs(&[(
        "gate (windows-latest)",
        7_000,
        vec![
            step(WORKSPACE_TESTS, 100, 100, "skipped"),
            step(WORKSPACE_TESTS, 100, 2_500, "cancelled"),
            json!({"name": WORKSPACE_TESTS, "conclusion": null}),
        ],
    )]);
    let found = readings(2, &limits_of_ci(), &answer);
    let steps: Vec<_> = found
        .iter()
        .filter(|reading| reading.what == WORKSPACE_TESTS)
        .map(|reading| (reading.seconds, reading.over(80)))
        .collect();
    assert_eq!(steps, vec![(2_400, true)]);
}

#[test]
fn a_job_of_a_run_that_called_the_workflow_is_found_by_its_id_as_the_runs_of_ci_are() {
    let answer = jobs(&[
        ("gate / gate (windows-latest)", 6_000, vec![]),
        ("gatekeeper", 6_000, vec![]),
        ("deploy (windows-latest)", 6_000, vec![]),
    ]);
    let found = readings(3, &limits_of_ci(), &answer);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].job, "gate / gate (windows-latest)");
    assert_eq!(found[0].seconds, 6_000);
}

#[test]
fn the_last_runs_of_main_are_listed_and_each_is_read_and_a_time_over_its_share_fails_the_check() {
    let mut system = Script {
        answers: vec![
            (
                "run list",
                Ok(r#"[{"databaseId": 39671630162, "headSha": "04bc9d66593fffe9a637c4d102fe5a6f4edef9a5"}]"#.to_owned()),
            ),
            ("runs/39671630162/jobs", Ok(main_at_04bc9d66().to_string())),
        ],
        asked: Vec::new(),
    };
    let options = Options {
        runs: Vec::new(),
        last: 3,
        share: 80,
        only: Vec::new(),
        binaries: false,
    };
    let (outcome, said) = read(&mut system, &options);
    assert!(!outcome.unwrap(), "{said}");
    assert!(said.contains("run 39671630162 04bc9d66"), "{said}");
    assert!(
        said.contains(&format!(
            "gate (windows-latest): {WORKSPACE_TESTS}: 33.9 of 40 minutes, 84 %  OVER 80 %"
        )),
        "{said}"
    );
    assert!(
        said.contains("1 of 4 times over 80 % of their limit, in 1 runs"),
        "{said}"
    );
    assert_eq!(system.asked.len(), 2);
    assert!(
        system.asked[0].contains("--workflow ci.yml --branch main --status completed --limit 3"),
        "{}",
        system.asked[0]
    );
    // The same times against a share of 90 % are all well.
    let (outcome, said) = read(
        &mut system,
        &Options {
            share: 90,
            ..options
        },
    );
    assert!(outcome.unwrap(), "{said}");
}

#[test]
fn only_the_steps_named_are_judged_and_a_step_over_its_share_beside_them_does_not_fail_the_check() {
    let answer = jobs(&[(
        "gate (windows-latest)",
        7_000,
        vec![
            step(WORKSPACE_TESTS, 100, 1_000, "success"),
            step("Run npm run test:daemons", 1_000, 4_000, "success"),
        ],
    )]);
    let workflow = format!("{CI}      - run: npm run test:daemons\n        timeout-minutes: 50\n");
    let ask = |only: &[&str]| {
        let mut system = Script {
            answers: vec![("runs/5/jobs", Ok(answer.to_string()))],
            asked: Vec::new(),
        };
        let options = Options {
            runs: vec![5],
            last: 5,
            share: 80,
            only: only.iter().map(|name| (*name).to_owned()).collect(),
            binaries: false,
        };
        let mut out = Vec::new();
        let outcome = check(&mut system, Path::new("."), &workflow, &options, &mut out);
        (outcome.unwrap(), String::from_utf8(out).unwrap())
    };
    // The daemons' 50 minutes were 50 of 50, and the job 97 % of its 120.
    assert!(!ask(&[]).0);
    let (within, said) = ask(&["cargo test --workspace"]);
    assert!(within, "{said}");
    assert!(!said.contains("test:daemons"), "{said}");
    assert!(!ask(&["test:daemons"]).0);
}

#[test]
fn runs_named_are_read_without_listing_any() {
    let mut system = Script {
        answers: vec![("runs/7/jobs", Ok(main_at_04bc9d66().to_string()))],
        asked: Vec::new(),
    };
    let options = Options {
        runs: vec![7],
        last: 5,
        share: 90,
        only: Vec::new(),
        binaries: false,
    };
    let (outcome, _) = read(&mut system, &options);
    assert!(outcome.unwrap());
    assert_eq!(system.asked.len(), 1, "{:?}", system.asked);
}

#[test]
fn a_check_that_read_no_time_is_refused_it_is_never_all_well() {
    let nothing = jobs(&[("build", 600, vec![step("Run make", 10, 500, "success")])]);
    let mut system = Script {
        answers: vec![("runs/9/jobs", Ok(nothing.to_string()))],
        asked: Vec::new(),
    };
    let options = Options {
        runs: vec![9],
        last: 5,
        share: 80,
        only: Vec::new(),
        binaries: false,
    };
    let (outcome, _) = read(&mut system, &options);
    let Err(Error::Refused(why)) = outcome else {
        panic!("it read a time where there was none");
    };
    assert!(why.starts_with("no time was read"), "{why}");
}

#[test]
fn a_gh_that_fails_or_answers_no_json_is_an_error_that_names_it() {
    let options = Options {
        runs: vec![9],
        last: 5,
        share: 80,
        only: Vec::new(),
        binaries: false,
    };
    let mut failing = Script {
        answers: vec![("runs/9/jobs", Err((1, "gh: HTTP 404 \n")))],
        asked: Vec::new(),
    };
    let (outcome, _) = read(&mut failing, &options);
    match outcome {
        Err(Error::Program {
            command,
            code,
            said,
        }) => {
            assert!(command.starts_with("gh api repos/ngvoicu/consensflow/actions/runs/9/jobs"));
            assert_eq!((code, said.as_str()), (1, "gh: HTTP 404"));
        }
        other => panic!("{other:?}"),
    }
    let mut silly = Script {
        answers: vec![("runs/9/jobs", Ok("not json".to_owned()))],
        asked: Vec::new(),
    };
    assert!(matches!(
        read(&mut silly, &options).0,
        Err(Error::Answer { .. })
    ));
}

#[test]
fn the_runs_the_share_and_how_many_are_read_are_taken_from_the_command_line() {
    assert_eq!(
        options(&words("")).unwrap(),
        Options {
            runs: Vec::new(),
            last: 5,
            share: 80,
            only: Vec::new(),
            binaries: false
        }
    );
    assert_eq!(
        options(&words(
            "39671630162 7 --last 9 --share 75 --step cargo --step npm --binaries"
        ))
        .unwrap(),
        Options {
            runs: vec![39_671_630_162, 7],
            last: 9,
            share: 75,
            only: vec!["cargo".into(), "npm".into()],
            binaries: true
        }
    );
    for (line, said) in [
        ("--step", "--step takes a name"),
        ("--share", "--share takes a number above 0"),
        ("--share 0", "--share takes a number above 0"),
        ("--share 101", "--share takes a percent, 1 to 100"),
        ("--last x", "--last takes a number above 0"),
        ("--force", "ci-times does not take --force"),
        ("main", "ci-times does not take main"),
    ] {
        let Err(Failure::Usage(message)) = options(&words(line)) else {
            panic!("{line}: taken");
        };
        assert_eq!(message, said, "{line}");
    }
}

#[test]
fn a_cargo_step_is_read_binary_by_binary_from_its_log_each_under_the_crate_whose_tests_came_first()
{
    let timing = binaries::timing(LOG, WORKSPACE_TESTS).unwrap();
    assert_eq!(timing.build, Some(213.0));
    assert_eq!(
        timing.binaries,
        vec![
            ("cf_daemon src/lib.rs".to_owned(), 261.55),
            ("cf_daemon tests/api/main.rs".to_owned(), 58.11),
            ("cf_ledger src/lib.rs".to_owned(), 0.27),
            ("cf_ledger tests/replay.rs".to_owned(), 279.85),
            ("cf_base doc-tests".to_owned(), 0.0),
        ]
    );
    // The next step's binaries are not this step's, and a step the log lacks is none.
    let next = binaries::timing(LOG, "Run cargo test -p cf-e2e --lib --bins").unwrap();
    assert_eq!(next.binaries, vec![("cf_e2e src/lib.rs".to_owned(), 99.0)]);
    assert!(binaries::timing(LOG, "Run make").is_none());
}

#[test]
fn a_time_cargo_gives_in_minutes_and_seconds_is_read_whole() {
    assert_eq!(binaries::timing("##[group]Run cargo test\n 2026-10-21T21:29:13.8Z Finished `test` profile in 1h 2m 3s\n", "Run cargo test").map(|timing| timing.build), Some(Some(3_723.0)));
}

#[test]
fn the_binaries_of_a_step_are_said_under_its_time_the_longest_first_when_they_are_asked_for() {
    let mut system = Script {
        answers: vec![
            ("runs/5/jobs", Ok(main_at_04bc9d66().to_string())),
            ("jobs/1000/logs", Ok(LOG.to_owned())),
            ("jobs/1001/logs", Ok(LOG.to_owned())),
        ],
        asked: Vec::new(),
    };
    let options = Options {
        runs: vec![5],
        binaries: true,
        ..Options::default()
    };
    let (outcome, said) = read(&mut system, &options);
    assert!(!outcome.unwrap(), "{said}");
    let lines: Vec<&str> = said.lines().collect();
    let at = lines
        .iter()
        .position(|line| line.contains("gate (windows-latest): Run cargo test --workspace"))
        .unwrap();
    assert_eq!(lines[at + 1], "    build 213 s");
    assert!(
        lines[at + 2].contains("279.9 s  14 %  cf_ledger tests/replay.rs"),
        "{said}"
    );
    assert!(
        lines[at + 3].contains("261.6 s  13 %  cf_daemon src/lib.rs"),
        "{said}"
    );
    assert!(
        lines.iter().any(|line| line.contains("in all 5 binaries")),
        "{said}"
    );
    // The log is asked for once for each step read, by the job's id and with the escapes allowed.
    assert!(
        system.asked.iter().any(|line| line.contains(
            "gh api --allow-escape-sequences repos/ngvoicu/consensflow/actions/jobs/1001/logs"
        )),
        "{:?}",
        system.asked
    );
}

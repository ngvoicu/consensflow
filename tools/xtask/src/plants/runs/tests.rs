use std::time::{Duration, Instant};

use super::results::Test;
use super::*;

/// A run of `command` that ended with `code` having written `output`, its report `report`.
fn ran_by(
    command: &[&str],
    code: Option<i32>,
    output: &str,
    report: Option<Result<Results, String>>,
) -> Ran {
    Ran::new(command, code, output, report)
}

/// A report that was read, of `results`.
fn read(results: Results) -> Option<Result<Results, String>> {
    Some(Ok(results))
}

/// A report that could not be read.
fn unread() -> Option<Result<Results, String>> {
    Some(Err(
        "/run/junit.xml could not be read: No such file or directory".to_owned(),
    ))
}

/// A report of `tests`, each a name and whether it failed, all of one test binary.
fn results(tests: &[(&str, bool)]) -> Results {
    Results {
        tests: tests
            .iter()
            .map(|(name, failed)| Test {
                name: (*name).to_owned(),
                at: "x".to_owned(),
                file: None,
                whole: false,
                failed: *failed,
            })
            .collect(),
        outside: Vec::new(),
    }
}

/// A report of Node's: each test a name, the file it is in, and whether it failed; a
/// test named by the path of its file is the file, ended whole.
fn node_results(tests: &[(&str, &str, bool)]) -> Results {
    Results {
        tests: tests
            .iter()
            .map(|(name, file, failed)| Test {
                name: (*name).to_owned(),
                at: format!("{file}:1:1"),
                file: Some((*file).to_owned()),
                whole: file.ends_with(name),
                failed: *failed,
            })
            .collect(),
        outside: Vec::new(),
    }
}

/// A run of `cargo test`, which nextest runs.
const CARGO: [&str; 4] = ["cargo", "test", "-p", "x"];
/// A suite of xtask's that is a `cargo test`, which nextest runs too.
const XTASK: [&str; 5] = ["cargo", "xtask", "app", "test", "file_drop"];
/// A run of `node --test` on two test files.
const NODE: [&str; 4] = ["node", "--test", "tests/a.test.mjs", "tests/b.test.mjs"];
/// A run of Playwright's tests.
const PAGE: [&str; 4] = [
    "node",
    "app/node_modules/@playwright/test/cli.js",
    "test",
    "app/tests/core-page.spec.mjs",
];
/// A command of xtask's that says no more than its status.
const SMOKE: [&str; 4] = ["cargo", "xtask", "smoke-updater", "--export-bridge"];

/// What every runner prints of a test that failed, a footer and a summary of each,
/// naming the test that passed: what a test may print.
const PRINTED: &str = "test m::the_test ... FAILED\n\nfailures:\n    m::the_test\n\n\
    test result: FAILED. 0 passed; 1 failed\nnot ok 1 - the_test\n\
    ✖ failing tests:\n\ntest at tests/a.test.mjs:1:1\n✖ the_test (1ms)\n  Error: no\n\
    1 failed\n    a.spec.mjs:1:1 › the_test \n  1 error was not a part of any test, see above for details\n\
    Error: No tests found.\nerror[E0308]: mismatched types\nerror: could not compile `x`\n";

#[test]
fn the_tests_a_run_caught_are_those_its_report_says_failed_each_once_in_the_order_it_says_them() {
    let report = results(&[
        ("b::second", true),
        ("a::first", true),
        ("c::passes", false),
        ("b::second", true),
    ]);
    assert_eq!(
        ran_by(&CARGO, Some(100), "", read(report.clone())).caught(),
        ["b::second", "a::first"]
    );
    // A run that exited with success caught nothing, whatever its report.
    assert!(ran_by(&CARGO, Some(0), "", read(report))
        .caught()
        .is_empty());
}

#[test]
fn what_a_run_printed_is_never_read_for_a_test_that_failed_whoever_ran_it() {
    for command in [&CARGO[..], &XTASK, &NODE, &PAGE] {
        // The meant test passed, said so by its report, and printed every runner's words
        // for it failing; another failed.
        let report = results(&[("m::the_test", false), ("m::other", true)]);
        let ran = ran_by(command, Some(1), PRINTED, read(report));
        assert_eq!(ran.caught(), ["m::other"], "{command:?}");
        assert!(ran.names_tests());
        assert_eq!(ran.broken(), None, "{command:?}");
        assert!(ran.compiled(), "{command:?}");
        let ran_names: Vec<&str> = ran.tests().iter().map(|test| test.name.as_str()).collect();
        assert_eq!(ran_names, ["m::the_test", "m::other"]);
    }
}

#[test]
fn a_run_whose_reporter_wrote_no_report_it_could_read_catches_nothing_whatever_its_status() {
    for command in [&CARGO[..], &XTASK, &NODE, &PAGE] {
        for code in [Some(1), Some(100), None, Some(0)] {
            let ran = ran_by(command, code, PRINTED, unread());
            assert!(ran.caught().is_empty(), "{command:?} {code:?}");
            assert!(!ran.names_tests());
            assert!(ran.tests().is_empty());
            assert_eq!(
                ran.broken().as_deref(),
                Some("no report of its tests could be read: /run/junit.xml could not be read: No such file or directory"),
                "{command:?} {code:?}"
            );
        }
    }
}

#[test]
fn a_run_whose_report_names_no_failure_or_that_has_no_reporter_is_the_catch_itself() {
    assert_eq!(
        ran_by(&SMOKE, Some(3), PRINTED, None).caught(),
        ["xtask smoke-updater --export-bridge: ended with 3"]
    );
    assert_eq!(
        ran_by(&SMOKE, None, "", None).caught(),
        ["xtask smoke-updater --export-bridge: was ended by a signal"]
    );
    let silent = ran_by(
        &XTASK,
        Some(100),
        "",
        read(results(&[("a::passes", false)])),
    );
    assert!(!silent.names_tests());
    assert_eq!(
        silent.caught(),
        ["xtask app test file_drop: ended with 100"]
    );
}

#[test]
fn a_run_that_passed_hung_overflowed_or_did_not_build_catches_nothing() {
    let failing = || read(results(&[("a::fails", true)]));
    assert!(ran_by(&CARGO, Some(0), "", failing()).caught().is_empty());
    let hung = Ran {
        hung: true,
        ..ran_by(&SMOKE, None, "", None)
    };
    assert!(hung.caught().is_empty());
    let overflowed = Ran {
        overflowed: true,
        ..ran_by(&SMOKE, None, "", None)
    };
    assert!(overflowed.caught().is_empty());
    let unbuilt = ran_by(&CARGO, Some(NEXTEST_BUILD_FAILED), "", unread());
    assert!(!unbuilt.compiled());
    assert!(unbuilt.caught().is_empty());
}

#[test]
fn nextest_says_a_build_that_failed_by_its_status_whatever_a_test_printed() {
    let quoted = "thread 'a::fails' panicked at src/lib.rs:11:9:\nerror[E0308]: mismatched types\n\
                  error: could not compile `x`\n";
    for command in [&CARGO[..], &XTASK] {
        let failed_test = ran_by(
            command,
            Some(100),
            quoted,
            read(results(&[("a::fails", true)])),
        );
        assert!(failed_test.compiled());
        assert_eq!(failed_test.caught(), ["a::fails"]);
        // The build failed: nextest said so by its status, whatever it printed.
        assert!(!ran_by(command, Some(101), "", unread()).compiled());
        assert!(ran_by(command, Some(1), quoted, unread()).compiled());
    }
}

#[test]
fn node_and_playwright_build_nothing_whatever_their_tests_say_of_a_build() {
    for said in [
        "Error: could not compile the response\n",
        "Error: error[E0308]: mismatched types\n",
    ] {
        for command in [&NODE[..], &PAGE, &SMOKE] {
            let ran = ran_by(command, Some(1), said, read(results(&[("the_test", true)])));
            assert!(ran.compiled(), "{command:?} {said}");
            assert_eq!(ran.caught(), ["the_test"]);
            // A run of theirs with no report is still no build of its own.
            assert!(ran_by(command, Some(1), said, unread()).compiled());
        }
    }
}

#[test]
fn a_node_test_file_that_ran_no_test_is_broken_whatever_the_other_files_ran() {
    // As Node says two files, one whose test passed and one that threw after a top-level
    // `await`, before it had a test: two tests in all, none of the second file's.
    let threw = ran_by(
        &NODE,
        Some(1),
        "ℹ tests 2\nℹ pass 1\nℹ fail 1\n",
        read(node_results(&[
            ("passes", "/checkout/tests/a.test.mjs", false),
            ("tests/b.test.mjs", "/checkout/tests/b.test.mjs", true),
        ])),
    );
    assert_eq!(
        threw.broken().as_deref(),
        Some("the test file tests/b.test.mjs ran no test")
    );
    assert!(!threw.names_tests());
    // One whose tests ran before it ended whole: no load failure, and no test named.
    let late = ran_by(
        &NODE,
        Some(1),
        "",
        read(node_results(&[
            ("passes first", "/checkout/tests/b.test.mjs", false),
            ("tests/b.test.mjs", "/checkout/tests/b.test.mjs", true),
        ])),
    );
    assert_eq!(late.broken(), None);
    assert!(!late.names_tests());
    assert_eq!(late.caught(), ["tests/b.test.mjs"]);
    // No test at all.
    let none = ran_by(&NODE, Some(1), "", read(Results::default()));
    assert_eq!(none.broken().as_deref(), Some("Node ran no test"));
}

#[test]
fn a_node_test_file_that_loaded_and_holds_no_test_is_no_load_failure() {
    // As Node says a file that holds no test beside one whose test failed.
    let ran = ran_by(
        &NODE,
        Some(1),
        "",
        read(node_results(&[
            (
                "tests/empty.test.mjs",
                "/checkout/tests/empty.test.mjs",
                false,
            ),
            ("the_test", "/checkout/tests/a.test.mjs", true),
        ])),
    );
    assert_eq!(ran.broken(), None);
    assert!(ran.names_tests());
    assert_eq!(ran.caught(), ["the_test"]);
}

#[test]
fn playwright_is_broken_by_an_error_outside_every_test_or_no_test_and_not_by_what_a_test_says() {
    let outside = Results {
        tests: Vec::new(),
        outside: vec![
            "Error: Cannot find module './a.mjs'".to_owned(),
            "Error: No tests found.".to_owned(),
        ],
    };
    assert_eq!(
        ran_by(&PAGE, Some(1), "", read(outside))
            .broken()
            .as_deref(),
        Some("Playwright: an error outside every test: Error: Cannot find module './a.mjs'")
    );
    assert_eq!(
        ran_by(&PAGE, Some(1), "", read(Results::default()))
            .broken()
            .as_deref(),
        Some("Playwright found no test")
    );
    // A test whose expectation quotes Playwright's words for an error outside every test.
    let quoted = ran_by(
        &PAGE,
        Some(1),
        "  1 error was not a part of any test, see above for details\nError: No tests found.\n",
        read(results(&[("g › the_test", true)])),
    );
    assert_eq!(quoted.broken(), None);
    assert_eq!(quoted.caught(), ["g › the_test"]);
}

#[test]
fn a_run_of_rust_tests_is_broken_where_nextest_found_none_or_a_binary_could_not_say_its_own() {
    assert_eq!(
        ran_by(&CARGO, Some(NEXTEST_NO_TESTS), "", read(Results::default()))
            .broken()
            .as_deref(),
        Some("nextest found no test to run")
    );
    assert_eq!(
        ran_by(&XTASK, Some(NEXTEST_UNLISTED), "", unread())
            .broken()
            .as_deref(),
        Some("nextest: a test binary could not say its tests")
    );
    assert_eq!(
        ran_by(&CARGO, Some(100), "", read(Results::default()))
            .broken()
            .as_deref(),
        Some("it ran no test")
    );
    assert_eq!(
        ran_by(&CARGO, Some(0), "", read(Results::default())).broken(),
        None
    );
    // A command that has no reporter says nothing of its tests.
    assert_eq!(ran_by(&SMOKE, Some(1), PRINTED, None).broken(), None);
}

#[test]
fn what_cf_process_answered_is_what_the_run_did() {
    let command = ["cargo".to_owned()];
    let ended = Ran::of(
        &command,
        Ok(Captured {
            stdout: "out\n".into(),
            stderr: "err\n".into(),
        }),
    )
    .unwrap();
    assert_eq!(
        (ended.code, ended.hung, ended.overflowed),
        (Some(0), false, false)
    );
    assert_eq!(ended.output, "out\nerr\n");
    assert_eq!(ended.report, None);

    let failed = |message: &str, code, killed| CaptureFailed {
        message: message.into(),
        code,
        killed,
        stdout: "out\n".into(),
        stderr: "err\n".into(),
    };
    let three = Ran::of(
        &command,
        Err(failed("Command failed: cargo", Some(3), false)),
    )
    .unwrap();
    assert_eq!(
        (three.code, three.hung, three.overflowed),
        (Some(3), false, false)
    );
    let out_of_time = Ran::of(&command, Err(failed("Command failed: cargo", None, true))).unwrap();
    assert_eq!((out_of_time.code, out_of_time.hung), (None, true));
    // A stream that said more than a run may: cf-process's own words for it, either stream.
    for message in [
        "stdout maxBuffer length exceeded",
        "stderr maxBuffer length exceeded",
    ] {
        let flooded = Ran::of(&command, Err(failed(message, None, false))).unwrap();
        assert_eq!(
            (flooded.code, flooded.hung, flooded.overflowed),
            (None, false, true),
            "{message}"
        );
        assert!(flooded.caught().is_empty(), "{message}");
    }

    // A program that could not be started is no outcome of a plant.
    let unstarted = CaptureFailed::unstarted("spawn cargo ENOENT".into());
    assert!(matches!(
        Ran::of(&command, Err(unstarted)),
        Err(Error::Process(process::Failure::Refused { .. }))
    ));
}

#[test]
fn what_a_run_wrote_is_read_with_the_terminal_formatting_taken_out() {
    let coloured =
        "\u{1b}[31m  1 failed\u{1b}[39m\n\u{1b}[32m  1 passed\u{1b}[39m (2s)\n\u{1b}]8;;\u{1b}\\";
    let ended = Ran::of(
        &PAGE.map(String::from),
        Err(CaptureFailed {
            message: "Command failed: node".into(),
            code: Some(1),
            killed: false,
            stdout: coloured.into(),
            stderr: String::new(),
        }),
    )
    .unwrap();
    assert!(!ended.output.contains('\u{1b}'), "{:?}", ended.output);
    assert!(
        ended.output.starts_with("  1 failed\n  1 passed (2s)\n"),
        "{:?}",
        ended.output
    );
}

/// The builds before the rig and before a run of cf-e2e's tests (`CF_E2E_PROGRAMS`)
/// say whether they built in cargo's structured words.
#[test]
fn a_build_is_judged_by_what_cargo_says_of_it_in_its_structured_words_where_it_said_any() {
    let build = |output: &str| ran_by(&["cargo", "build"], Some(101), output, None);
    let failed =
        "{\"reason\":\"compiler-artifact\"}\n{\"reason\":\"build-finished\",\"success\":false}\n";
    assert_eq!(build(failed).build_finished(), Some(false));
    let built = "{\"reason\":\"build-finished\",\"success\":true}\nerror: failed to sign\n";
    assert_eq!(build(built).build_finished(), Some(true));
    // Two builds, one failed: it did not build.
    let one_failed = "{\"reason\":\"build-finished\",\"success\":true}\n\
                      {\"reason\":\"build-finished\",\"success\":false}\n";
    assert_eq!(build(one_failed).build_finished(), Some(false));
    assert_eq!(
        build("error: could not compile `x`\n").build_finished(),
        None
    );
}

// The commands below are real. Where the program is the one running these tests, each of
// the tests it is asked for is one of the ignored ones below.

fn this_binary() -> PathBuf {
    std::env::current_exe().unwrap()
}

/// This test binary asked for the one test `name` of the ignored ones below.
fn helper(name: &str) -> Vec<String> {
    vec![
        this_binary().to_string_lossy().into_owned(),
        "--ignored".into(),
        "--exact".into(),
        format!("plants::runs::tests::{name}"),
    ]
}

#[test]
#[ignore = "a program the tests of runs start"]
fn helper_fails() {
    panic!("the planted bug is here");
}

#[test]
#[ignore = "a program the tests of runs start"]
fn helper_passes() {}

#[test]
#[ignore = "a program the tests of runs start"]
fn helper_exits_with_three() {
    std::process::exit(3);
}

#[test]
#[ignore = "a program the tests of runs start"]
fn helper_waits() {
    std::thread::sleep(Duration::from_secs(120));
}

#[test]
#[ignore = "a program the tests of runs start"]
fn helper_floods() {
    let line = "flood ".repeat(1000);
    for _ in 0..1000 {
        println!("{line}");
    }
}

#[test]
#[ignore = "a program the tests of runs start"]
#[allow(clippy::disallowed_methods)] // The test is of what the environment it is started with holds.
fn helper_has_no_node_test_context() {
    assert!(std::env::var_os("NODE_TEST_CONTEXT").is_none());
}

#[test]
#[ignore = "a program the tests of runs start"]
#[allow(clippy::disallowed_methods)] // The test is of what the environment it is started with holds.
fn helper_has_no_colour() {
    assert_eq!(std::env::var("NO_COLOR").as_deref(), Ok("1"));
    assert_eq!(std::env::var("FORCE_COLOR").as_deref(), Ok("0"));
}

fn context() -> Context {
    Context::new(&Env::from_process()).unwrap()
}

/// A runner that no stop reaches.
fn runner_with(context: &Context, limit: Duration, stops: impl FnOnce() -> Stops) -> Execute {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let stops = {
        let _inside = runtime.enter();
        stops()
    };
    Execute::with_stops(context, this_binary(), limit, runtime, stops)
}

fn runner(limit: Duration) -> Execute {
    runner_with(&context(), limit, Stops::never)
}

/// A runner in the checkout `root`, with this one's environment.
fn runner_in(root: &Path) -> Execute {
    let checkout = Context {
        root: root.to_path_buf(),
        env: context().env,
    };
    runner_with(&checkout, Duration::from_secs(300), Stops::never)
}

fn ended(execution: Execution) -> Ran {
    match execution {
        Execution::Ended(ran) => ran,
        Execution::Stopped => panic!("the command was stopped"),
    }
}

fn words(command: &[&str]) -> Vec<String> {
    command.iter().map(|word| (*word).to_owned()).collect()
}

#[test]
fn a_command_that_says_no_more_than_its_status_is_the_catch_itself_whatever_it_printed() {
    let ran = ended(
        runner(Duration::from_secs(120))
            .run(&helper("helper_fails"))
            .unwrap(),
    );
    assert_eq!(ran.code, Some(101));
    assert!(!ran.hung);
    assert_eq!(ran.report, None);
    let caught = ran.caught();
    assert_eq!(caught.len(), 1);
    assert!(caught[0].ends_with(": ended with 101"), "{}", caught[0]);
    assert!(
        ran.output.contains("the planted bug is here"),
        "{}",
        ran.output
    );
    let passed = ended(
        runner(Duration::from_secs(120))
            .run(&helper("helper_passes"))
            .unwrap(),
    );
    assert_eq!(passed.code, Some(0));
    assert!(passed.caught().is_empty());
    let three = ended(
        runner(Duration::from_secs(120))
            .run(&helper("helper_exits_with_three"))
            .unwrap(),
    );
    assert!(
        three.caught()[0].ends_with(": ended with 3"),
        "{:?}",
        three.caught()
    );
}

#[test]
fn a_command_that_does_not_end_in_its_time_is_ended_and_said_to_have_hung() {
    let started = Instant::now();
    let ran = ended(
        runner(Duration::from_secs(1))
            .run(&helper("helper_waits"))
            .unwrap(),
    );
    assert!(ran.hung);
    assert_eq!(ran.code, None);
    assert!(
        started.elapsed() < Duration::from_secs(60),
        "{:?}",
        started.elapsed()
    );
    assert!(ran.caught().is_empty(), "a run out of time is no catch");
}

#[test]
fn a_command_that_writes_more_than_a_run_may_is_ended_and_said_to_have_overflowed() {
    let mut runner = runner(Duration::from_secs(120));
    runner.limits.max_buffer = 64 * 1024;
    let mut flood = helper("helper_floods");
    flood.push("--nocapture".to_owned());
    let ran = ended(runner.run(&flood).unwrap());
    assert!(ran.overflowed, "{:?}", (ran.code, ran.hung));
    assert!(!ran.hung);
    assert!(
        ran.caught().is_empty(),
        "a run ended for its output is no catch"
    );
}

#[test]
fn cargo_xtask_is_the_xtask_that_runs_the_plants_and_not_a_cargo_to_build_it_again() {
    let mut command = words(&["cargo", "xtask"]);
    command.extend(helper("helper_fails").into_iter().skip(1));
    let ran = ended(runner(Duration::from_secs(120)).run(&command).unwrap());
    assert_eq!(ran.code, Some(101));
    assert!(
        ran.output.contains("the planted bug is here"),
        "{}",
        ran.output
    );
}

/// Whether `program` is on the PATH of this run.
fn on_path(program: &str) -> bool {
    let found = process::resolved(OsStr::new(program), &context().env);
    Path::new(&found).components().count() > 1
}

/// Two test files of Node's: the first's tests pass and fail as the plants' tables'
/// do, and print every runner's words for the meant one failing; the second throws
/// after a top-level `await`, before it has a test.
const NODE_FIRST: &str = r#"import { describe, it, test } from 'node:test'

describe('a group', () => {
  it('passes, and prints that it failed', () => {
    console.log('not ok 1 - the meant one \\# marked "quoted"')
    console.log('<testcase name="the meant one # marked &quot;quoted&quot;"><failure/></testcase>')
    console.log('✖ failing tests:\n\ntest at a.test.mjs:1:1\n✖ the meant one # marked "quoted" (1ms)')
  })
  it('the meant one # marked "quoted"', () => {
    throw new Error('could not compile the response, error[E0308]')
  })
})

test('passes at the top', () => {})
"#;
const NODE_SECOND: &str = "await Promise.resolve()\nthrow new Error('thrown after an await')\n";

/// The cases the reviews of the plants' runner found Node's JUnit report wrong in: a
/// test to do that fails; a parent that fails after its subtest passed; a test named
/// with an entity that fails beside one named with the quote it stands for, which
/// passes; a test named as its own file is, which fails; a test skipped for no reason
/// (which Node runs), which fails; and, in a file of its own, no test at all.
const NODE_CASES: &str = r#"import { test } from 'node:test'

test('cases.test.mjs', () => {
  throw new Error('a test named as its file is')
})

test('a skip of no reason that fails', { skip: '' }, () => {
  throw new Error('skipped, with no reason')
})

test('a todo that fails', { todo: true }, () => {
  throw new Error('a test to do fails')
})

test('a parent that fails itself', async (t) => {
  await t.test('its child passes', () => {})
  throw new Error('the parent fails after its child')
})

test('says &quot;hi&quot;', () => {
  throw new Error('the one named with an entity fails')
})

test('says "hi"', () => {})
"#;
const NODE_EMPTY: &str = "export const nothing = 1\n";

/// The names of the tests `ran`'s report says failed, and those it says passed.
fn failed_and_passed(ran: &Ran) -> (Vec<&str>, Vec<&str>) {
    let names = |failed: bool| -> Vec<&str> {
        ran.tests()
            .iter()
            .filter(|test| test.failed == failed)
            .map(|test| test.name.as_str())
            .collect()
    };
    (names(true), names(false))
}

#[test]
fn node_s_report_names_each_test_as_written_and_a_file_that_ran_none_of_its_own() {
    assert!(on_path("node"), "node is not on the PATH");
    let checkout = tempfile::tempdir().unwrap();
    fs::write(checkout.path().join("a.test.mjs"), NODE_FIRST).unwrap();
    fs::write(checkout.path().join("b.test.mjs"), NODE_SECOND).unwrap();
    let mut runner = runner_in(checkout.path());

    let first = ended(
        runner
            .run(&words(&["node", "--test", "a.test.mjs"]))
            .unwrap(),
    );
    assert_eq!(first.code, Some(1), "{}", first.output);
    assert!(first.compiled());
    assert_eq!(first.broken(), None);
    assert_eq!(
        first.caught(),
        [r#"a group › the meant one # marked "quoted""#],
        "{}",
        first.output
    );
    assert!(first
        .tests()
        .iter()
        .any(|test| test.name == "a group › passes, and prints that it failed"));
    // The report the table asked for is the one printed; what a test prints, in Node's
    // process, is printed as it prints it, before the report.
    let tap = ended(
        runner
            .run(&words(&[
                "node",
                "--test",
                "--test-reporter=tap",
                "a.test.mjs",
            ]))
            .unwrap(),
    );
    assert!(
        tap.output
            .contains("\nTAP version 13\n# Subtest: a group\n"),
        "{}",
        tap.output
    );
    assert_eq!(tap.caught(), first.caught());

    // Node counts the tests of both files together; the second ran none.
    let both = ended(
        runner
            .run(&words(&["node", "--test", "a.test.mjs", "b.test.mjs"]))
            .unwrap(),
    );
    assert_eq!(
        both.broken().as_deref(),
        Some("the test file b.test.mjs ran no test"),
        "{}",
        both.output
    );
}

#[test]
fn node_s_report_holds_a_parent_s_own_failure_neither_fails_nor_passes_a_test_to_do_and_tells_an_entity_from_its_character(
) {
    assert!(on_path("node"), "node is not on the PATH");
    let checkout = tempfile::tempdir().unwrap();
    fs::write(checkout.path().join("cases.test.mjs"), NODE_CASES).unwrap();
    fs::write(checkout.path().join("empty.test.mjs"), NODE_EMPTY).unwrap();
    let mut runner = runner_in(checkout.path());
    let ran = ended(
        runner
            .run(&words(&[
                "node",
                "--test",
                "cases.test.mjs",
                "empty.test.mjs",
            ]))
            .unwrap(),
    );
    assert_eq!(ran.code, Some(1), "{}", ran.output);
    let (failed, passed) = failed_and_passed(&ran);
    assert_eq!(
        failed,
        [
            "cases.test.mjs",
            "a parent that fails itself",
            "says &quot;hi&quot;"
        ],
        "{}",
        ran.output
    );
    // A test named as its file is, is a test; the file that holds none is the file.
    let whole: Vec<&str> = ran
        .tests()
        .iter()
        .filter(|test| test.whole)
        .map(|test| test.name.as_str())
        .collect();
    assert_eq!(whole, ["empty.test.mjs"]);
    assert_eq!(
        passed,
        [
            "a parent that fails itself › its child passes",
            r#"says "hi""#,
            "empty.test.mjs",
        ]
    );
    // A file that loaded and holds no test did not fail, and broke nothing.
    assert_eq!(ran.broken(), None);
    assert!(ran.names_tests());
}

/// A spec of Playwright's whose test in a group passes, writing Playwright's summary of
/// a failure of it to its standard error and its words for an error outside every test to
/// its output, and whose test at the top fails on an expectation that quotes them.
const PLAYWRIGHT_SPEC: &str = r#"import { expect, test } from '@playwright/test'

test.describe('a group', () => {
  test('the meant one', () => {
    console.error('  1 failed\n    prints.spec.mjs:4:3 › a group › the meant one ')
    console.log('  1 error was not a part of any test, see above for details')
    expect(1).toBe(1)
  })
})

test('another fails', () => {
  expect('1 error was not a part of any test').toBe('Error: No tests found.')
})
"#;

#[test]
fn playwright_s_json_report_names_each_test_and_the_errors_outside_every_test() {
    let root = context().root;
    let cli = "app/node_modules/@playwright/test/cli.js";
    assert!(
        root.join(cli).is_file(),
        "Playwright is not installed: npm ci --prefix app"
    );
    // In the checkout, where the specs find `@playwright/test`.
    let fixtures = root.join("app/src-tauri/target/plants-fixtures");
    fs::create_dir_all(&fixtures).unwrap();
    let folder = tempfile::tempdir_in(&fixtures).unwrap();
    fs::write(
        folder.path().join("plants.config.mjs"),
        "export default { testDir: '.', outputDir: './test-results', reporter: 'dot' }\n",
    )
    .unwrap();
    fs::write(folder.path().join("prints.spec.mjs"), PLAYWRIGHT_SPEC).unwrap();
    fs::write(
        folder.path().join("missing.spec.mjs"),
        "import './not-there.mjs'\n",
    )
    .unwrap();
    let relative = folder.path().strip_prefix(&root).unwrap();
    let at = |file: &str| relative.join(file).to_string_lossy().replace('\\', "/");
    let config = at("plants.config.mjs");
    let mut runner = runner_in(&root);

    let printed = ended(
        runner
            .run(&words(&[
                "node",
                cli,
                "test",
                "--config",
                &config,
                &at("prints.spec.mjs"),
            ]))
            .unwrap(),
    );
    assert_eq!(printed.code, Some(1), "{}", printed.output);
    assert_eq!(printed.broken(), None, "{}", printed.output);
    assert_eq!(printed.caught(), ["another fails"], "{}", printed.output);
    let meant = printed
        .tests()
        .iter()
        .find(|test| test.name == "a group › the meant one")
        .unwrap();
    // Told from another test of its name by where it is written.
    assert_eq!(meant.at, "prints.spec.mjs:4:3");
    // The list is printed, whatever reporter the configuration names.
    assert!(
        printed.output.contains("a group › the meant one"),
        "{}",
        printed.output
    );

    let missing = ended(
        runner
            .run(&words(&[
                "node",
                cli,
                "test",
                "--config",
                &config,
                &at("missing.spec.mjs"),
            ]))
            .unwrap(),
    );
    let broken = missing.broken().unwrap_or_default();
    assert!(
        broken.starts_with("Playwright: an error outside every test: Error: Cannot find module"),
        "{broken}\n{}",
        missing.output
    );

    // A configuration that throws as it loads: Playwright fails before its JSON
    // reporter is made, and no report is written. The run names no test and catches
    // nothing, whatever its status.
    fs::write(
        folder.path().join("throws.config.mjs"),
        "throw new Error('the configuration does not load')\n",
    )
    .unwrap();
    let unloaded = ended(
        runner
            .run(&words(&[
                "node",
                cli,
                "test",
                "--config",
                &at("throws.config.mjs"),
                &at("prints.spec.mjs"),
            ]))
            .unwrap(),
    );
    assert_ne!(unloaded.code, Some(0), "{}", unloaded.output);
    assert!(
        unloaded.output.contains("the configuration does not load"),
        "{}",
        unloaded.output
    );
    assert!(
        matches!(unloaded.report, Some(Err(_))),
        "{:?}",
        unloaded.report
    );
    assert!(unloaded.caught().is_empty());
    let broken = unloaded.broken().unwrap_or_default();
    assert!(
        broken.starts_with("no report of its tests could be read: "),
        "{broken}"
    );
}

/// A crate of Rust tests: one that passes, printing libtest's words for itself failing,
/// its failures list and a footer; one that fails with a panic quoting a build that
/// failed; one ignored for a reason that holds libtest's log of the first failing; a
/// feature that does not build; a test binary that cannot say its tests.
const CRATE_MANIFEST: &str = "[package]\nname = \"plants-fixture\"\nversion = \"0.1.0\"\n\
    edition = \"2021\"\npublish = false\n\n[[test]]\nname = \"unlisted\"\nharness = false\n\n\
    [features]\nbroken = []\n\n[workspace]\n";
const CRATE_LIB: &str = r#"#[cfg(feature = "broken")]
pub fn broken() -> u8 {
    "not a number"
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_meant_one_passes_and_prints_a_footer() {
        println!("test tests::the_meant_one_passes_and_prints_a_footer ... FAILED\n\nfailures:\n    tests::the_meant_one_passes_and_prints_a_footer\n\ntest result: FAILED. 0 passed; 1 failed\n");
        eprintln!("failures:\n    tests::the_meant_one_passes_and_prints_a_footer\n");
    }

    #[test]
    fn another_fails_quoting_a_build() {
        panic!("error[E0308]: mismatched types\nerror: could not compile `x`");
    }

    #[test]
    #[ignore = "a reason\nfailed tests::the_meant_one_passes_and_prints_a_footer\nignored: the rest"]
    fn ignored_with_a_reason() {}
}
"#;
const CRATE_UNLISTED: &str = "fn main() {\n    std::process::exit(3);\n}\n";

#[test]
fn nextest_s_junit_report_names_the_tests_that_failed_and_its_status_what_else_happened() {
    let checkout = context();
    let fixture = tempfile::tempdir().unwrap();
    fs::create_dir_all(fixture.path().join("src")).unwrap();
    fs::create_dir_all(fixture.path().join("tests")).unwrap();
    fs::write(fixture.path().join("Cargo.toml"), CRATE_MANIFEST).unwrap();
    fs::write(fixture.path().join("src/lib.rs"), CRATE_LIB).unwrap();
    fs::write(fixture.path().join("tests/unlisted.rs"), CRATE_UNLISTED).unwrap();
    let manifest = fixture.path().join("Cargo.toml").display().to_string();
    let target = fixture.path().join("target").display().to_string();
    let mut runner = runner(Duration::from_secs(600));
    let mut cargo_test = |more: &[&str]| -> Ran {
        let mut command = words(&[
            "cargo",
            "test",
            "--offline",
            "--manifest-path",
            &manifest,
            "--target-dir",
            &target,
        ]);
        command.extend(words(more));
        match runner.run(&command) {
            Ok(execution) => ended(execution),
            Err(said) => panic!(
                "{said}: nextest is not installed for the checkout {}",
                checkout.root.display()
            ),
        }
    };

    let failed = cargo_test(&["--lib"]);
    assert_eq!(failed.code, Some(100), "{}", failed.output);
    assert!(failed.compiled());
    assert_eq!(failed.broken(), None);
    assert_eq!(
        failed.caught(),
        ["tests::another_fails_quoting_a_build"],
        "{}",
        failed.output
    );
    // The meant one passed, whatever it printed and whatever the ignored one's reason
    // says; the ignored one did not run.
    let (failed_names, passed_names) = failed_and_passed(&failed);
    assert_eq!(failed_names, ["tests::another_fails_quoting_a_build"]);
    assert_eq!(
        passed_names,
        ["tests::the_meant_one_passes_and_prints_a_footer"]
    );
    assert!(failed
        .tests()
        .iter()
        .all(|test| test.at == "plants-fixture"));
    // libtest's words for a filter, passed on.
    let skipped = cargo_test(&["--lib", "--", "--skip", "another_fails"]);
    assert_eq!(skipped.code, Some(0), "{}", skipped.output);
    assert_eq!(
        failed_and_passed(&skipped),
        (
            Vec::new(),
            vec!["tests::the_meant_one_passes_and_prints_a_footer"]
        )
    );
    // The ignored one, asked for, ran, and passed.
    let ignored = cargo_test(&["--lib", "--", "--ignored"]);
    assert_eq!(
        failed_and_passed(&ignored),
        (Vec::new(), vec!["tests::ignored_with_a_reason"]),
        "{}",
        ignored.output
    );

    let unbuilt = cargo_test(&["--lib", "--features", "broken"]);
    assert!(!unbuilt.compiled(), "{}", unbuilt.output);
    let none = cargo_test(&["--lib", "no_test_is_named_so"]);
    assert_eq!(
        none.broken().as_deref(),
        Some("nextest found no test to run")
    );
    let unlisted = cargo_test(&["--test", "unlisted"]);
    assert_eq!(
        unlisted.broken().as_deref(),
        Some("nextest: a test binary could not say its tests"),
        "{}",
        unlisted.output
    );
}

/// Two tests that each leave a mark in their crate's folder while they run, and wait
/// a while for the other's: one that sees it ran beside the other, and fails. Run one
/// at a time, neither sees the other's; run beside each other, both do.
const ALONE_LIB: &str = r#"#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    fn alone(own: &str, other: &str) {
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        std::fs::write(folder.join(own), "").unwrap();
        let started = Instant::now();
        let mut beside = false;
        while !beside && started.elapsed() < Duration::from_secs(5) {
            beside = folder.join(other).exists();
            std::thread::sleep(Duration::from_millis(50));
        }
        // Seen, the other's mark stays for it to see this one's.
        std::thread::sleep(Duration::from_millis(500));
        std::fs::remove_file(folder.join(own)).unwrap();
        assert!(!beside, "the other test ran beside this one");
    }

    #[test]
    fn first() {
        alone("first", "second")
    }

    #[test]
    fn second() {
        alone("second", "first")
    }
}
"#;

/// A crate of the name `package` in a folder of its own, whose library is `lib`, and
/// the words of `cargo test` that test it, building it in that folder.
fn fixture_crate(package: &str, lib: &str) -> (tempfile::TempDir, Vec<String>) {
    let fixture = tempfile::tempdir().unwrap();
    fs::create_dir_all(fixture.path().join("src")).unwrap();
    fs::write(
        fixture.path().join("Cargo.toml"),
        format!(
            "[package]\nname = \"{package}\"\nversion = \"0.1.0\"\nedition = \"2021\"\npublish = false\n\n[workspace]\n"
        ),
    )
    .unwrap();
    fs::write(fixture.path().join("src/lib.rs"), lib).unwrap();
    let manifest = fixture.path().join("Cargo.toml").display().to_string();
    let target = fixture.path().join("target").display().to_string();
    let command = words(&[
        "cargo",
        "test",
        "--offline",
        "--manifest-path",
        &manifest,
        "--target-dir",
        &target,
        "-p",
        package,
    ]);
    (fixture, command)
}

#[test]
fn nextest_runs_tests_beside_each_other_and_is_given_cargo_s_jobs_and_profile_as_cargo_means_them()
{
    let mut runner = runner(Duration::from_secs(600));
    // Cargo's `-j` is the jobs that build, nextest's the tests run at a time; cargo's
    // `--profile` the profile of the build, nextest's its own.
    let (_fixture, mut command) = fixture_crate("plants-beside", ALONE_LIB);
    command.extend(words(&["-j", "1", "--profile", "release"]));
    let beside = ended(runner.run(&command).unwrap());
    assert_eq!(beside.code, Some(100), "{}", beside.output);
    // In the order they ended, which is not known: they met.
    let (mut failed, passed) = failed_and_passed(&beside);
    failed.sort_unstable();
    assert_eq!(
        (failed, passed),
        (vec!["tests::first", "tests::second"], Vec::new()),
        "{}",
        beside.output
    );
    assert!(
        beside.output.contains("release"),
        "built as the release profile: {}",
        beside.output
    );
    // An option nextest has no word for is no run.
    let (_fixture, mut doc) = fixture_crate("plants-doc", ALONE_LIB);
    doc.push("--doc".to_owned());
    // Refused as a command that cannot be started is, which the plant's next run follows;
    // not an error of the run of the plants, which would end it.
    let refused = runner.run(&doc).unwrap_err();
    assert!(
        matches!(&refused, Error::Process(process::Failure::Refused { cause, .. })
            if cause.to_string() == "cargo test --doc: nextest runs no doctest"),
        "{refused}"
    );
}

/// The tests of a crate named `cf-e2e`, which see where the programs they run were built
/// (`CF_E2E_BUILT`): that it is the release folder of the checkout `RELEASED`, and that
/// the build before them left `MARK`; and the mark that they ran.
#[cfg(unix)]
const CF_E2E_LIB: &str = r#"#[cfg(test)]
mod tests {
    #[test]
    fn given_the_programs_built_before_the_run() {
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        std::fs::write(folder.join("ran"), "").unwrap();
        assert_eq!(std::env::var("CF_E2E_BUILT").as_deref(), Ok("RELEASED"));
        assert!(folder.join("built").exists(), "built before");
    }
}
"#;

/// The tests of another crate, which see no programs built for them.
#[cfg(unix)]
const OTHER_LIB: &str = r#"#[cfg(test)]
mod tests {
    #[test]
    fn given_nothing_built() {
        assert!(std::env::var_os("CF_E2E_BUILT").is_none());
    }
}
"#;

#[cfg(unix)]
#[test]
fn a_run_of_cf_e2e_s_tests_is_given_the_programs_built_before_it_and_one_whose_build_failed_runs_no_test(
) {
    let mut runner = runner(Duration::from_secs(600));
    let released = context().root.join("app/src-tauri/target/release");
    let lib = CF_E2E_LIB.replace("RELEASED", &released.display().to_string());
    let (crate_folder, command) = fixture_crate("cf-e2e", &lib);
    let marked = |name: &str| crate_folder.path().join(name).exists();
    // Builds that stand for cf's and the pane host's, the first leaving its mark.
    let builds = |first: &str| vec![words(&["sh", "-c", first]), words(&["sh", "-c", "exit 0"])];
    let mark = format!(": > '{}'", crate_folder.path().join("built").display());
    runner.programs = builds(&mark);
    let given = ended(runner.run(&command).unwrap());
    assert_eq!(given.code, Some(0), "{}", given.output);
    assert_eq!(
        failed_and_passed(&given),
        (
            Vec::new(),
            vec!["tests::given_the_programs_built_before_the_run"]
        )
    );
    // Another crate's tests are given nothing, and nothing is built for them.
    fs::remove_file(crate_folder.path().join("built")).unwrap();
    let (_other, other) = fixture_crate("plants-other", OTHER_LIB);
    let alone = ended(runner.run(&other).unwrap());
    assert_eq!(alone.code, Some(0), "{}", alone.output);
    assert!(!marked("built"));
    // A build cargo says failed: the run did not build, and no test of it ran.
    fs::remove_file(crate_folder.path().join("ran")).unwrap();
    runner.programs = builds(r#"echo '{"reason":"build-finished","success":false}'; exit 101"#);
    let unbuilt = ended(runner.run(&command).unwrap());
    assert!(!unbuilt.compiled(), "{}", unbuilt.output);
    assert!(unbuilt.caught().is_empty());
    assert!(!marked("ran"), "no test ran");
    // A build that failed otherwise: no test ran, which nothing can be caught by.
    runner.programs = builds("echo 'error: the lock is held'; exit 3");
    let failed = ended(runner.run(&command).unwrap());
    assert!(failed.compiled());
    assert_eq!(failed.code, Some(3));
    assert!(failed.caught().is_empty());
    assert_eq!(
        failed.broken().as_deref(),
        Some(
            "no report of its tests could be read: sh -c echo 'error: the lock is held'; exit 3 \
             did not succeed, and no test was run"
        )
    );
    assert!(failed.output.contains("error: the lock is held"));
    assert!(!marked("ran"), "no test ran");
    // A build out of its time is the build that hung.
    let mut hurried = runner_with(&context(), Duration::from_secs(1), Stops::never);
    hurried.programs = vec![words(&["sh", "-c", "exec sleep 30"])];
    let hung = ended(hurried.run(&command).unwrap());
    assert!(hung.hung);
    assert_eq!(hung.command, ["sh", "-c", "exec sleep 30"]);
    assert!(!marked("ran"), "no test ran");
}

/// A plant of one file in a checkout of its own, tried by `command`.
fn tried_by(command: Vec<String>) -> (tempfile::TempDir, Context, crate::plants::kit::Plant) {
    use crate::plants::kit::{Edit, Plant, Run as PlantRun};
    let folder = tempfile::tempdir().unwrap();
    let checkout = Context {
        root: folder.path().to_path_buf(),
        env: Env::default(),
    };
    std::fs::write(checkout.path("a.rs"), "one\n").unwrap();
    let plant = Plant {
        table: "t".into(),
        name: "p: one".into(),
        meant: "the_test".into(),
        runs: vec![PlantRun {
            command,
            builds: false,
        }],
        builds: false,
        prepare: Vec::new(),
        requires: Vec::new(),
        platforms: Vec::new(),
        edits: vec![Edit {
            file: "a.rs".into(),
            find: "one".into(),
            replace: "ONE".into(),
        }],
    };
    (folder, checkout, plant)
}

/// The status the driver ends `plants` with in `checkout`, their commands run as
/// processes, and what it says.
fn driven(checkout: &Context, plants: &[crate::plants::kit::Plant]) -> (i32, String) {
    let family = crate::plants::kit::Family {
        name: "demo".into(),
        limit: Duration::from_secs(600),
        rebuild: None,
        plants: plants.to_vec(),
    };
    let chosen: Vec<&crate::plants::kit::Plant> = family.plants.iter().collect();
    let mut out = Vec::new();
    let status = crate::plants::driver::run(
        checkout,
        &family,
        &chosen,
        &mut runner(Duration::from_secs(600)),
        &mut out,
    )
    .unwrap();
    (status, String::from_utf8(out).unwrap())
}

/// What the driver says of `plant` in `checkout`, its commands run as processes.
fn verdict_of(checkout: &Context, plant: &crate::plants::kit::Plant) -> String {
    let (status, said) = driven(checkout, std::slice::from_ref(plant));
    assert_eq!(status, 1);
    assert_eq!(
        std::fs::read_to_string(checkout.path("a.rs")).unwrap(),
        "one\n"
    );
    said
}

/// A crate whose one test holds a value to one, which a plant makes two.
const VALUED_MANIFEST: &str =
    "[package]\nname = \"plants-valued\"\nversion = \"0.1.0\"\nedition = \"2021\"\npublish = false\n\n[workspace]\n";
const VALUED_LIB: &str = "pub fn value() -> u8 {\n    1\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn the_value_is_one() {\n        assert_eq!(super::value(), 1);\n    }\n}\n";

#[test]
fn every_command_of_a_plant_runs_and_one_whose_test_target_is_gone_is_said_for_itself() {
    use crate::plants::kit::{Edit, Plant, Run as PlantRun};
    let folder = tempfile::tempdir().unwrap();
    let checkout = Context {
        root: folder.path().to_path_buf(),
        env: Env::default(),
    };
    fs::create_dir_all(checkout.path("src")).unwrap();
    fs::write(checkout.path("Cargo.toml"), VALUED_MANIFEST).unwrap();
    fs::write(checkout.path("src/lib.rs"), VALUED_LIB).unwrap();
    let manifest = checkout.path("Cargo.toml").display().to_string();
    let target = checkout.path("target").display().to_string();
    let cargo_test = |more: &[&str]| -> PlantRun {
        let mut command = words(&[
            "cargo",
            "test",
            "--offline",
            "--manifest-path",
            &manifest,
            "--target-dir",
            &target,
        ]);
        command.extend(words(more));
        PlantRun {
            command,
            builds: false,
        }
    };
    let plant = |name: &str, meant: &str, runs: Vec<PlantRun>| Plant {
        table: "t".into(),
        name: name.into(),
        meant: meant.into(),
        runs,
        builds: false,
        prepare: Vec::new(),
        requires: Vec::new(),
        platforms: Vec::new(),
        edits: vec![Edit {
            file: "src/lib.rs".into(),
            find: "    1\n}\n\n#[cfg".into(),
            replace: "    2\n}\n\n#[cfg".into(),
        }],
    };
    let gone = || cargo_test(&["--test", "a_test_target_that_is_gone"]);
    let (status, said) = driven(
        &checkout,
        &[
            // As the obligations' plants are: a run whose test target is gone, then a
            // run that has the meant test.
            plant(
                "p: the target is gone, then the meant test fails",
                "the_value_is_one",
                vec![gone(), cargo_test(&["--lib"])],
            ),
            // A program that is not there, then the meant test.
            plant(
                "p: a program that is not there, then the meant test fails",
                "the_value_is_one",
                vec![
                    PlantRun {
                        command: words(&["no-such-program-for-the-plants"]),
                        builds: false,
                    },
                    cargo_test(&["--lib"]),
                ],
            ),
            // As the obligations' plants are once their meant test is gone too.
            plant(
                "p: the target is gone, and the meant test",
                "a_test_that_is_gone",
                vec![gone(), cargo_test(&["--lib"])],
            ),
        ],
    );
    assert_eq!(
        fs::read_to_string(checkout.path("src/lib.rs")).unwrap(),
        VALUED_LIB
    );
    let gone_words = gone().command.join(" ");
    let first = said
        .find("CAUGHT           p: the target is gone, then the meant test fails (")
        .unwrap_or_else(|| panic!("{said}"));
    let second = said
        .find("CAUGHT           p: a program that is not there, then the meant test fails (")
        .unwrap_or_else(|| panic!("{said}"));
    let third = said
        .find("NOT RUN          p: the target is gone, and the meant test (")
        .unwrap_or_else(|| panic!("{said}"));
    let (first, second, third) = (&said[first..second], &said[second..third], &said[third..]);
    assert!(first.contains(") by tests::the_value_is_one\n"), "{first}");
    // Gone without the plant too: it tells nothing of any plant, and is said so.
    assert!(
        first.contains(&format!(
            "    and NOT RUN: {gone_words} fails without the plant too, twice, so it tells nothing of it:"
        )),
        "{first}"
    );
    assert!(
        second.contains("    and NOT RUN: no-such-program-for-the-plants could not be started: "),
        "{second}"
    );
    // Known to fail by then, the gone one is not run again; the other run names another
    // test, and no run ran the meant one.
    assert!(
        third.contains(&format!(
            "    {gone_words} fails without the plant too, twice, so it tells nothing of it:"
        )),
        "{third}"
    );
    assert!(third.contains("    and MISSED-BY-MEANT: "), "{third}");
    assert!(
        third.contains("    failed: tests::the_value_is_one\n"),
        "{third}"
    );
    assert!(
        third.contains("    no test named a_test_that_is_gone ran in its runs\n"),
        "{third}"
    );
    assert_eq!(status, 1);
    assert!(said.ends_with("2 of 3 plants caught\n"), "{said}");
}

/// A plant of `checkout`'s `flag.mjs`, which says `planted` is `true`, tried by `command`
/// for the test `meant`.
fn flagged(name: &str, meant: &str, command: Vec<String>) -> crate::plants::kit::Plant {
    use crate::plants::kit::{Edit, Plant, Run as PlantRun};
    Plant {
        table: "t".into(),
        name: name.into(),
        meant: meant.into(),
        runs: vec![PlantRun {
            command,
            builds: false,
        }],
        builds: false,
        prepare: Vec::new(),
        requires: Vec::new(),
        platforms: Vec::new(),
        edits: vec![Edit {
            file: "flag.mjs".into(),
            find: "planted = false".into(),
            replace: "planted = true".into(),
        }],
    }
}

const FLAG: &str = "export const planted = false\n";

/// Node's tests of the plants' cases: a test to do that always fails, a test the plant
/// fails, a parent that the plant fails after its subtest passed, and one title in two
/// groups, the plant failing the second.
const NODE_PLANTED: &str = r#"import assert from 'node:assert'
import { describe, it, test } from 'node:test'
import { planted } from './flag.mjs'

test('a todo that always fails', { todo: true }, () => {
  throw new Error('a test to do that always fails')
})

test('fails only with the plant', () => {
  assert.equal(planted, false)
})

test('a parent', async (t) => {
  await t.test('its child passes', () => {})
  assert.equal(planted, false)
})

describe('a group', () => {
  it('the same title', () => {})
})

describe('another', () => {
  it('the same title', () => {
    assert.equal(planted, false)
  })
})
"#;

#[test]
fn node_s_tests_catch_a_plant_by_a_parent_s_own_failure_and_never_by_a_test_to_do_or_a_title_of_two(
) {
    assert!(on_path("node"), "node is not on the PATH");
    let folder = tempfile::tempdir().unwrap();
    let checkout = Context {
        root: folder.path().to_path_buf(),
        env: Env::default(),
    };
    fs::write(checkout.path("flag.mjs"), FLAG).unwrap();
    fs::write(checkout.path("planted.test.mjs"), NODE_PLANTED).unwrap();
    let file = checkout.path("planted.test.mjs").display().to_string();
    let command = || words(&["node", "--test", &file]);
    let (status, said) = driven(
        &checkout,
        &[
            flagged("p: a todo", "a todo that always fails", command()),
            flagged("p: a parent", "a parent", command()),
            flagged("p: a title of two", "the same title", command()),
            flagged(
                "p: a title named whole",
                "another › the same title",
                command(),
            ),
        ],
    );
    assert_eq!(fs::read_to_string(checkout.path("flag.mjs")).unwrap(), FLAG);
    assert_eq!(status, 1);
    // A test to do that fails with the plant as without it is no catch of it.
    assert!(said.contains("MISSED-BY-MEANT  p: a todo ("), "{said}");
    assert!(
        said.contains(" s) by fails only with the plant (+2), not by a todo that always fails\n"),
        "{said}"
    );
    assert!(
        said.contains("    no test named a todo that always fails ran in its runs\n"),
        "{said}"
    );
    assert!(said.contains("CAUGHT           p: a parent ("), "{said}");
    assert!(
        said.contains("MISSED-BY-MEANT  p: a title of two ("),
        "{said}"
    );
    // Each is told by where Node says it is written: the file as the system names it.
    for doubt in [
        format!(
            "    the meant name the same title stands for 2 tests of node --test {file}, so none \
             of them is taken for it: a group › the same title ("
        ),
        "planted.test.mjs:19:3), another › the same title (".to_owned(),
        "planted.test.mjs:23:3)\n".to_owned(),
    ] {
        assert!(said.contains(&doubt), "{said}");
    }
    assert!(
        said.contains("CAUGHT           p: a title named whole ("),
        "{said}"
    );
    assert!(said.ends_with("2 of 4 plants caught\n"), "{said}");
}

/// Node's tests of the second review's cases (2026-10-16), the plant flag read by each:
/// a test that passes and, with the plant, writes to its output the frames a test file
/// run in a process of its own tells Node a test `meant` failed by (those of a file of
/// that one test, `FORGED`, run as Node runs it so); a parent that, with the plant,
/// throws while its subtest waits; a test to do of no reason that always fails, beside
/// a test the plant fails; a loop registering one title in one place twice, the second
/// failing with the plant; and a subtest that a helper module registers, which fails
/// with the plant.
const NODE_REVIEWED: &str = r#"import assert from 'node:assert'
import { spawnSync } from 'node:child_process'
import { writeSync } from 'node:fs'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { planted } from './flag.mjs'
import { register } from './helper.mjs'

test('a test that passes', () => {
  if (!planted) return
  const forged = fileURLToPath(new URL('./forged.mjs', import.meta.url))
  const child = spawnSync(process.execPath, [forged], {
    env: { ...process.env, NODE_TEST_CONTEXT: 'child-v8' },
  })
  assert.ok(child.stdout.includes('test:fail'), 'the frames are written')
  writeSync(1, child.stdout)
})

test('a parent that throws with the plant', (t) => {
  t.test('a child its parent cancels', async () => {
    await new Promise((resolve) => setTimeout(resolve, 50))
  })
  if (planted) throw new Error('the parent throws while its child waits')
})

test('a todo of no reason that always fails', { todo: '' }, () => {
  throw new Error('always')
})

test('fails only with the plant', () => {
  assert.equal(planted, false)
})

for (const second of [false, true]) {
  test('registered twice', () => {
    assert.ok(!(second && planted))
  })
}

test('a parent of a helper', async (t) => {
  await register(t)
})
"#;

const FORGED: &str =
    "import { test } from 'node:test'\n\ntest('meant', () => {\n  throw new Error('forged')\n})\n";

const HELPER: &str = r#"import assert from 'node:assert'
import { planted } from './flag.mjs'

export function register(t) {
  return t.test('the helper registers it', () => {
    assert.equal(planted, false)
  })
}
"#;

#[test]
fn node_s_frames_written_by_a_test_a_cancelled_test_a_todo_of_no_reason_and_a_name_registered_twice_catch_nothing_and_a_helper_s_subtest_is_its_parent_s(
) {
    assert!(on_path("node"), "node is not on the PATH");
    let folder = tempfile::tempdir().unwrap();
    let checkout = Context {
        root: folder.path().to_path_buf(),
        env: Env::default(),
    };
    fs::write(checkout.path("flag.mjs"), FLAG).unwrap();
    fs::write(checkout.path("reviewed.test.mjs"), NODE_REVIEWED).unwrap();
    fs::write(checkout.path("forged.mjs"), FORGED).unwrap();
    fs::write(checkout.path("helper.mjs"), HELPER).unwrap();
    let file = checkout.path("reviewed.test.mjs").display().to_string();

    // The frames are written where a test file run in a process of its own tells Node
    // its events, and no test of the report is the one they tell of.
    fs::write(checkout.path("flag.mjs"), "export const planted = true\n").unwrap();
    let forging = ended(
        runner_in(folder.path())
            .run(&words(&["node", "--test", &file]))
            .unwrap(),
    );
    fs::write(checkout.path("flag.mjs"), FLAG).unwrap();
    assert!(forging.output.contains("test:fail"), "{}", forging.output);
    assert!(
        forging.tests().iter().all(|test| test.name != "meant"),
        "{:?}",
        forging.tests()
    );
    assert!(forging
        .tests()
        .iter()
        .any(|test| test.name == "a test that passes" && !test.failed));

    let command = || words(&["node", "--test", &file]);
    let (status, said) = driven(
        &checkout,
        &[
            flagged("p: forged", "meant", command()),
            flagged("p: cancelled", "a child its parent cancels", command()),
            flagged(
                "p: a todo of no reason",
                "a todo of no reason that always fails",
                command(),
            ),
            flagged("p: registered twice", "registered twice", command()),
            flagged(
                "p: a helper's subtest",
                "a parent of a helper › the helper registers it",
                command(),
            ),
        ],
    );
    assert_eq!(fs::read_to_string(checkout.path("flag.mjs")).unwrap(), FLAG);
    assert_eq!(status, 1);
    // What a test wrote is no test: the meant one was never registered.
    assert!(said.contains("MISSED-BY-MEANT  p: forged ("), "{said}");
    assert!(
        said.contains("    no test named meant ran in its runs\n"),
        "{said}"
    );
    // A test its parent cancelled ended for what is not its own code: neither failed
    // nor passed.
    assert!(said.contains("MISSED-BY-MEANT  p: cancelled ("), "{said}");
    assert!(
        said.contains("    no test named a child its parent cancels ran in its runs\n"),
        "{said}"
    );
    assert!(
        said.contains("MISSED-BY-MEANT  p: a todo of no reason ("),
        "{said}"
    );
    assert!(
        said.contains("    no test named a todo of no reason that always fails ran in its runs\n"),
        "{said}"
    );
    // One title registered twice in one place is two tests.
    assert!(
        said.contains("MISSED-BY-MEANT  p: registered twice ("),
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "    the meant name registered twice stands for 2 tests of node --test {file}, so none \
             of them is taken for it: registered twice ("
        )),
        "{said}"
    );
    assert!(
        said.contains("CAUGHT           p: a helper's subtest ("),
        "{said}"
    );
    assert!(said.ends_with("1 of 5 plants caught\n"), "{said}");
}

#[test]
fn playwright_s_meant_test_failing_beside_a_global_teardown_that_threw_is_caught_and_the_error_said(
) {
    let root = context().root;
    let cli = "app/node_modules/@playwright/test/cli.js";
    assert!(
        root.join(cli).is_file(),
        "Playwright is not installed: npm ci --prefix app"
    );
    // In the checkout, where the specs find `@playwright/test`.
    let fixtures = root.join("app/src-tauri/target/plants-fixtures");
    fs::create_dir_all(&fixtures).unwrap();
    let folder = tempfile::tempdir_in(&fixtures).unwrap();
    let checkout = Context {
        root: folder.path().to_path_buf(),
        env: Env::default(),
    };
    fs::write(checkout.path("flag.mjs"), FLAG).unwrap();
    fs::write(
        checkout.path("teardown.mjs"),
        "import { planted } from './flag.mjs'\n\nexport default function teardown() {\n  \
         if (planted) throw new Error('the teardown fails with the plant')\n}\n",
    )
    .unwrap();
    fs::write(
        checkout.path("plants.config.mjs"),
        "export default { testDir: '.', outputDir: './test-results', globalTeardown: './teardown.mjs' }\n",
    )
    .unwrap();
    fs::write(
        checkout.path("meant.spec.mjs"),
        "import { expect, test } from '@playwright/test'\nimport { planted } from './flag.mjs'\n\n\
         test('the meant one', () => {\n  expect(planted).toBe(false)\n})\n",
    )
    .unwrap();
    let relative = folder.path().strip_prefix(&root).unwrap();
    let at = |file: &str| relative.join(file).to_string_lossy().replace('\\', "/");
    let command = words(&[
        "node",
        cli,
        "test",
        "--config",
        &at("plants.config.mjs"),
        &at("meant.spec.mjs"),
    ]);
    let (status, said) = driven(
        &checkout,
        &[flagged("p: teardown", "the meant one", command)],
    );
    assert_eq!(fs::read_to_string(checkout.path("flag.mjs")).unwrap(), FLAG);
    assert_eq!(status, 0, "{said}");
    assert!(said.contains("CAUGHT           p: teardown ("), "{said}");
    assert!(said.contains(") by the meant one\n"), "{said}");
    assert!(
        said.contains("    beside it: Playwright: an error outside every test: ")
            && said.contains("the teardown fails with the plant"),
        "{said}"
    );
}

#[test]
fn a_plant_whose_command_cannot_be_started_or_fails_without_it_is_not_run_not_caught() {
    // A program that is not there.
    let (_folder, checkout, plant) = tried_by(vec!["no-such-program-for-the-plants".into()]);
    let said = verdict_of(&checkout, &plant);
    assert!(said.contains("NOT RUN          p: one ("), "{said}");
    assert!(
        said.contains("    no-such-program-for-the-plants could not be started: "),
        "{said}"
    );
    // One that starts and fails before any test says so, whatever the plant (a module its
    // script cannot load, a filter that finds no test): its failure with the plant is none.
    let (_folder, checkout, plant) = tried_by(helper("helper_exits_with_three"));
    let said = verdict_of(&checkout, &plant);
    assert!(said.contains("NOT RUN          p: one ("), "{said}");
    assert!(
        said.contains("fails without the plant too, twice, so it tells nothing of it:"),
        "{said}"
    );
    assert!(said.ends_with("0 of 1 plants caught\n"), "{said}");
}

#[test]
fn the_commands_run_with_no_colour_whatever_this_one_was_given() {
    let this = context();
    let with = Env::from_vars(
        this.env
            .iter()
            .filter(|(name, _)| *name != "NO_COLOR" && *name != "FORCE_COLOR")
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .chain([(OsString::from("FORCE_COLOR"), OsString::from("1"))]),
    );
    let with = Context {
        root: this.root,
        env: with,
    };
    let ran = ended(
        runner_with(&with, Duration::from_secs(120), Stops::never)
            .run(&helper("helper_has_no_colour"))
            .unwrap(),
    );
    assert_eq!(ran.code, Some(0), "{}", ran.output);
}

#[test]
fn a_program_that_is_not_there_is_an_error_to_mend_not_the_outcome_of_a_plant() {
    let said = runner(Duration::from_secs(120))
        .run(&["no-such-program-for-the-plants".to_owned()])
        .unwrap_err();
    assert!(
        matches!(&said, Error::Process(process::Failure::NotFound { program }) if program == "no-such-program-for-the-plants"),
        "{said}"
    );
}

#[test]
fn node_test_context_is_not_handed_to_the_commands() {
    let this = context();
    let with = Env::from_vars(
        this.env
            .iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .chain([(
                OsString::from("NODE_TEST_CONTEXT"),
                OsString::from("child-v8"),
            )]),
    );
    assert!(with.text("NODE_TEST_CONTEXT").is_some());
    let with = Context {
        root: this.root,
        env: with,
    };
    let ran = ended(
        runner_with(&with, Duration::from_secs(120), Stops::never)
            .run(&helper("helper_has_no_node_test_context"))
            .unwrap(),
    );
    assert_eq!(ran.code, Some(0), "{}", ran.output);
}

/// `git` with `words` in the checkout `runner` runs in, which must succeed: as no one's,
/// with no hook and no signature of the machine's.
fn git(runner: &mut Execute, words: &[&str]) {
    let command: Vec<String> = [
        "git",
        "-c",
        "user.name=plants",
        "-c",
        "user.email=plants@example.com",
        "-c",
        "commit.gpgsign=false",
        "-c",
        "core.hooksPath=/dev/null",
    ]
    .iter()
    .chain(words)
    .map(|word| (*word).to_owned())
    .collect();
    let ran = ended(runner.run(&command).unwrap());
    assert_eq!(ran.code, Some(0), "{command:?}: {}", ran.output);
}

#[test]
fn the_state_of_a_checkout_is_its_commit_and_the_bytes_of_what_changed_and_nothing_else() {
    let folder = tempfile::tempdir().unwrap();
    let checkout = Context {
        root: folder.path().to_path_buf(),
        env: context().env,
    };
    let mut runner = runner_with(&checkout, Duration::from_secs(120), Stops::never);
    std::fs::write(checkout.path("a.rs"), "one\n").unwrap();
    std::fs::write(checkout.path("fixture.txt"), "kept\n").unwrap();
    git(&mut runner, &["init", "-q"]);
    git(&mut runner, &["add", "."]);
    git(&mut runner, &["commit", "-q", "-m", "first"]);
    let first = runner.state();
    assert_eq!(runner.state(), first, "nothing changed");
    // A plant, and its file put back by its bytes: the state it was, whatever its times.
    std::fs::write(checkout.path("a.rs"), "ONE\n").unwrap();
    assert_ne!(runner.state(), first);
    std::fs::write(checkout.path("a.rs"), "one\n").unwrap();
    assert_eq!(runner.state(), first);
    // A fixture a run changed, and changed again while git calls it changed all the same.
    std::fs::write(checkout.path("fixture.txt"), "changed\n").unwrap();
    let changed = runner.state();
    assert_ne!(changed, first);
    std::fs::write(checkout.path("fixture.txt"), "changed again\n").unwrap();
    assert_ne!(runner.state(), changed);
    std::fs::write(checkout.path("fixture.txt"), "kept\n").unwrap();
    assert_eq!(runner.state(), first);
    // A file git was never told of, and a commit.
    std::fs::write(checkout.path("new.txt"), "new\n").unwrap();
    let added = runner.state();
    assert_ne!(added, first);
    git(&mut runner, &["add", "."]);
    git(&mut runner, &["commit", "-q", "-m", "second"]);
    assert_ne!(runner.state(), added);
    // Where git says nothing of the folder, each state is one of its own.
    let elsewhere = tempfile::tempdir().unwrap();
    let mut untold = runner_with(
        &Context {
            root: elsewhere.path().to_path_buf(),
            env: context().env,
        },
        Duration::from_secs(120),
        Stops::never,
    );
    assert_ne!(untold.state(), untold.state());
}

#[cfg(unix)]
mod stops {
    use nix::sys::signal::{raise, Signal};
    use tokio::signal::unix::SignalKind;

    use super::*;

    #[test]
    fn a_stop_ends_the_command_in_hand_and_is_told_with_nothing_of_it_read() {
        let mut runner = runner_with(&context(), Duration::from_secs(120), || {
            Stops::on(&[SignalKind::user_defined2()]).unwrap()
        });
        let sender = std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(500));
            raise(Signal::SIGUSR2).unwrap();
        });
        let started = Instant::now();
        assert_eq!(
            runner.run(&helper("helper_waits")).unwrap(),
            Execution::Stopped
        );
        sender.join().unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_stop_heard_while_git_is_asked_the_state_of_the_checkout_is_a_stop_and_not_lost() {
        use std::os::unix::fs::PermissionsExt;
        // A `git` that waits, first on the PATH: the stop comes while it is asked.
        let bin = tempfile::tempdir().unwrap();
        let git = bin.path().join("git");
        std::fs::write(&git, "#!/bin/sh\nexec sleep 30\n").unwrap();
        std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
        let this = context();
        let path = std::env::join_paths(std::iter::once(bin.path().to_path_buf()).chain(
            std::env::split_paths(this.env.os("PATH").unwrap_or_default()),
        ))
        .unwrap();
        let env = Env::from_vars(
            this.env
                .iter()
                .filter(|(name, _)| *name != OsStr::new("PATH"))
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .chain([(OsString::from("PATH"), path)]),
        );
        let checkout = Context {
            root: this.root,
            env,
        };
        let mut runner = runner_with(&checkout, Duration::from_secs(120), || {
            Stops::on(&[SignalKind::window_change()]).unwrap()
        });
        let sender = std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(500));
            raise(Signal::SIGWINCH).unwrap();
        });
        let started = Instant::now();
        assert_eq!(runner.state(), None, "the stop is the driver's to hear");
        sender.join().unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "{:?}",
            started.elapsed()
        );
    }

    /// The stop is told once the runtime has had its turn, which a loaded machine can put
    /// off past the first question after it (CI's macOS, 2026-10-15): it is not lost, and is
    /// found once.
    #[test]
    fn a_stop_that_came_with_no_command_in_hand_is_not_lost_and_is_found_once() {
        let mut runner = runner_with(&context(), Duration::from_secs(120), || {
            Stops::on(&[SignalKind::from_raw(Signal::SIGURG as i32)]).unwrap()
        });
        assert!(!runner.stopped());
        raise(Signal::SIGURG).unwrap();
        let asked = Instant::now();
        while !runner.stopped() {
            assert!(
                asked.elapsed() < Duration::from_secs(10),
                "the stop was lost"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!runner.stopped(), "a stop is asked for once");
    }
}

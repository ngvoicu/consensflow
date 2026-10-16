use std::time::{Duration, Instant};

use super::results::Test;
use super::*;

/// A run of `command` that ended with `code` having written `output`, its report `results`.
fn ran_by(command: &[&str], code: Option<i32>, output: &str, results: Option<Results>) -> Ran {
    Ran::new(command, code, output, results)
}

/// A report of `tests`, each a name and whether it failed.
fn results(tests: &[(&str, bool)]) -> Results {
    Results {
        tests: tests
            .iter()
            .map(|(name, failed)| Test {
                name: (*name).to_owned(),
                file: None,
                failed: *failed,
            })
            .collect(),
        outside: Vec::new(),
    }
}

/// A report of Node's: each test a name, the file it is in, and whether it failed.
fn node_results(tests: &[(&str, &str, bool)]) -> Results {
    Results {
        tests: tests
            .iter()
            .map(|(name, file, failed)| Test {
                name: (*name).to_owned(),
                file: Some((*file).to_owned()),
                failed: *failed,
            })
            .collect(),
        outside: Vec::new(),
    }
}

/// A run of `cargo test`, which nextest runs.
const CARGO: [&str; 4] = ["cargo", "test", "-p", "x"];
/// A suite of xtask's, whose test binaries log their tests.
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

/// What every runner prints of a test that failed, a footer and a summary of each,
/// naming the test that passed: what a test may print.
const PRINTED: &str = "test m::the_test ... FAILED\n\nfailures:\n    m::the_test\n\n\
    test result: FAILED. 0 passed; 1 failed\nnot ok 1 - the_test\n\
    ✖ failing tests:\n\ntest at tests/a.test.mjs:1:1\n✖ the_test (1ms)\n  Error: no\n\
    1 failed\n    a.spec.mjs:1:1 › the_test \n  1 error was not a part of any test, see above for details\n\
    Error: No tests found.\n";

#[test]
fn the_tests_a_run_caught_are_those_its_report_says_failed_each_once_in_the_order_it_says_them() {
    let report = results(&[
        ("b::second", true),
        ("a::first", true),
        ("c::passes", false),
        ("b::second", true),
    ]);
    assert_eq!(
        ran_by(&CARGO, Some(100), "", Some(report.clone())).caught(),
        ["b::second", "a::first"]
    );
    // A run that exited with success caught nothing, whatever its report.
    assert!(ran_by(&CARGO, Some(0), "", Some(report))
        .caught()
        .is_empty());
}

#[test]
fn what_a_run_printed_is_never_read_for_a_test_that_failed_whoever_ran_it() {
    for command in [&CARGO[..], &XTASK, &NODE, &PAGE] {
        // The meant test passed, said so by its report, and printed every runner's words
        // for it failing; another failed.
        let report = results(&[("m::the_test", false), ("m::other", true)]);
        let ran = ran_by(command, Some(1), PRINTED, Some(report));
        assert_eq!(ran.caught(), ["m::other"], "{command:?}");
        assert!(ran.names_tests());
        assert_eq!(ran.broken(), None, "{command:?}");
        assert!(ran.compiled(), "{command:?}");
        assert_eq!(ran.tests_ran(), ["m::the_test", "m::other"]);
    }
}

#[test]
fn a_run_that_failed_with_no_report_or_none_failed_in_it_is_the_catch_itself() {
    assert_eq!(
        ran_by(&CARGO, Some(3), PRINTED, None).caught(),
        ["test -p x: ended with 3"]
    );
    assert_eq!(
        ran_by(&CARGO, None, "", None).caught(),
        ["test -p x: was ended by a signal"]
    );
    let died = "error: test failed, to rerun pass `-p x --test t`\n\nCaused by:\n  \
                process didn't exit successfully: `/target/t-1a --nocapture` (signal: 11, SIGSEGV: invalid memory reference)\n";
    assert_eq!(
        ran_by(&XTASK, Some(101), died, Some(results(&[("a::passes", false)]))).caught(),
        ["xtask app test file_drop: the test binary died (signal: 11, SIGSEGV: invalid memory reference)"]
    );
    let silent = ran_by(
        &XTASK,
        Some(101),
        "",
        Some(results(&[("a::passes", false)])),
    );
    assert!(!silent.names_tests());
    assert_eq!(
        silent.caught(),
        ["xtask app test file_drop: ended with 101"]
    );
}

#[test]
fn a_run_that_passed_hung_overflowed_or_did_not_build_catches_nothing() {
    let failing = || Some(results(&[("a::fails", true)]));
    assert!(ran_by(&CARGO, Some(0), "", failing()).caught().is_empty());
    let hung = Ran {
        hung: true,
        ..ran_by(&CARGO, None, "", None)
    };
    assert!(hung.caught().is_empty());
    let overflowed = Ran {
        overflowed: true,
        ..ran_by(&CARGO, None, "", None)
    };
    assert!(overflowed.caught().is_empty());
    let unbuilt = ran_by(&CARGO, Some(NEXTEST_BUILD_FAILED), "", None);
    assert!(!unbuilt.compiled());
    assert!(unbuilt.caught().is_empty());
}

#[test]
fn nextest_says_a_build_that_failed_by_its_status_whatever_a_test_printed() {
    let quoted = "thread 'a::fails' panicked at src/lib.rs:11:9:\nerror[E0308]: mismatched types\n\
                  error: could not compile `x`\n";
    let failed_test = ran_by(
        &CARGO,
        Some(100),
        quoted,
        Some(results(&[("a::fails", true)])),
    );
    assert!(failed_test.compiled());
    assert_eq!(failed_test.caught(), ["a::fails"]);
    // The build failed: nextest said so by its status, whatever it printed.
    assert!(!ran_by(&CARGO, Some(101), "", None).compiled());
}

#[test]
fn node_and_playwright_build_nothing_whatever_their_tests_say_of_a_build() {
    for said in [
        "Error: could not compile the response\n",
        "Error: error[E0308]: mismatched types\n",
    ] {
        for command in [&NODE[..], &PAGE] {
            let ran = ran_by(command, Some(1), said, Some(results(&[("the_test", true)])));
            assert!(ran.compiled(), "{command:?} {said}");
            assert_eq!(ran.caught(), ["the_test"]);
            // A run of theirs with no report is still no build of its own.
            assert!(ran_by(command, Some(1), said, None).compiled());
        }
    }
}

#[test]
fn a_suite_of_xtask_s_did_not_build_where_cargo_said_so_before_any_test_binary_logged() {
    let unbuilt =
        "   Compiling app v1\nerror[E0308]: mismatched types\nerror: could not compile `app`\n";
    assert!(!ran_by(&XTASK, Some(101), unbuilt, None).compiled());
    assert!(!ran_by(
        &XTASK,
        Some(101),
        "error: could not compile `app` (lib)\n",
        None
    )
    .compiled());
    // A binary logged its tests: the build was made, and a test printed the words.
    let logged = ran_by(
        &XTASK,
        Some(101),
        unbuilt,
        Some(results(&[("a::fails", true)])),
    );
    assert!(logged.compiled());
    assert_eq!(logged.caught(), ["a::fails"]);
    // An error code is `E` and digits and `]`.
    assert!(ran_by(&XTASK, Some(1), "error[E]: no\nerror[E12: no\n", None).compiled());
}

#[test]
fn a_node_test_file_that_ran_no_test_is_broken_whatever_the_other_files_ran() {
    // As Node says two files, one whose test passed and one that threw after a top-level
    // `await`, before it had a test: two tests in all, none of the second file's.
    let threw = ran_by(
        &NODE,
        Some(1),
        "ℹ tests 2\nℹ pass 1\nℹ fail 1\n",
        Some(node_results(&[
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
        Some(node_results(&[
            ("passes first", "/checkout/tests/b.test.mjs", false),
            ("tests/b.test.mjs", "/checkout/tests/b.test.mjs", true),
        ])),
    );
    assert_eq!(late.broken(), None);
    assert!(!late.names_tests());
    assert_eq!(late.caught(), ["tests/b.test.mjs"]);
    // No test at all.
    let none = ran_by(&NODE, Some(1), "", Some(Results::default()));
    assert_eq!(none.broken().as_deref(), Some("Node ran no test"));
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
        ran_by(&PAGE, Some(1), "", Some(outside))
            .broken()
            .as_deref(),
        Some("Playwright: an error outside every test: Error: Cannot find module './a.mjs'")
    );
    assert_eq!(
        ran_by(&PAGE, Some(1), "", Some(Results::default()))
            .broken()
            .as_deref(),
        Some("Playwright found no test")
    );
    // A test whose expectation quotes Playwright's words for an error outside every test.
    let quoted = ran_by(
        &PAGE,
        Some(1),
        "  1 error was not a part of any test, see above for details\nError: No tests found.\n",
        Some(results(&[("g › the_test", true)])),
    );
    assert_eq!(quoted.broken(), None);
    assert_eq!(quoted.caught(), ["g › the_test"]);
}

#[test]
fn a_run_of_rust_tests_is_broken_where_nextest_found_none_or_a_binary_could_not_say_its_own() {
    assert_eq!(
        ran_by(&CARGO, Some(NEXTEST_NO_TESTS), "", Some(Results::default()))
            .broken()
            .as_deref(),
        Some("nextest found no test to run")
    );
    assert_eq!(
        ran_by(&CARGO, Some(NEXTEST_UNLISTED), "", None)
            .broken()
            .as_deref(),
        Some("nextest: a test binary could not say its tests")
    );
    assert_eq!(
        ran_by(&XTASK, Some(101), "", Some(Results::default()))
            .broken()
            .as_deref(),
        Some("it ran no test")
    );
    // No log at all is no word on its tests.
    assert_eq!(ran_by(&XTASK, Some(101), "", None).broken(), None);
    assert_eq!(
        ran_by(&CARGO, Some(0), "", Some(Results::default())).broken(),
        None
    );
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
    assert_eq!(ended.results, None);

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

/// The build before the rig (`driver`'s `PREFLIGHT`) says whether it built in cargo's
/// structured words.
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

#[test]
fn the_target_runner_is_set_by_the_variable_cargo_reads_for_this_machine_s_target() {
    let variable = runner(Duration::from_secs(120)).runner_variable().unwrap();
    let host = variable
        .strip_prefix("CARGO_TARGET_")
        .and_then(|rest| rest.strip_suffix("_RUNNER"))
        .unwrap_or_default();
    assert!(!host.is_empty(), "{variable}");
    assert!(
        host.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
        "{variable}"
    );
    let arch = if cfg!(target_arch = "aarch64") {
        "AARCH64_"
    } else {
        "X86_64_"
    };
    assert!(host.starts_with(arch), "{variable}");
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

/// Passes, printing libtest's log of itself failing, and libtest's words for it.
#[test]
#[ignore = "a program the tests of runs start"]
fn helper_named_meant() {
    println!("failed plants::runs::tests::helper_named_meant");
    println!("test plants::runs::tests::helper_named_meant ... FAILED");
}

/// Fails, printing libtest's log of the other failing.
#[test]
#[ignore = "a program the tests of runs start"]
fn helper_named_other() {
    println!("failed plants::runs::tests::helper_named_meant");
    panic!("the other test failed");
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
    assert_eq!(ran.results, None);
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

#[test]
fn plants_log_runs_a_test_binary_with_libtest_s_log_which_nothing_a_test_prints_reaches() {
    for nocapture in [false, true] {
        let folder = tempfile::tempdir().unwrap();
        let mut words: Vec<OsString> = vec![
            "--ignored".into(),
            "plants::runs::tests::helper_named_".into(),
        ];
        if nocapture {
            words.push("--nocapture".into());
        }
        let binary = this_binary().into_os_string();
        let invocation = logged(folder.path(), &binary, &words, context().root);
        let ran = process::capture(&invocation, &context().env).unwrap();
        assert_eq!(ran.code, 101, "the binary's own status");
        assert!(
            ran.stdout
                .contains("failed plants::runs::tests::helper_named_meant"),
            "{}",
            ran.stdout
        );
        let read = Reporter::Libtest.results(folder.path()).unwrap().unwrap();
        // In the order the tests ended, which their threads decide.
        let mut said: Vec<(&str, bool)> = read
            .tests
            .iter()
            .map(|test| (test.name.as_str(), test.failed))
            .collect();
        said.sort_unstable();
        assert_eq!(
            said,
            [
                ("plants::runs::tests::helper_named_meant", false),
                ("plants::runs::tests::helper_named_other", true),
            ],
            "nocapture: {nocapture}"
        );
        // A log for each binary started, named by the binary's, in turn.
        let logs: Vec<String> = fs::read_dir(folder.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(logs.len(), 1);
        assert!(logs[0].starts_with("0000-"), "{logs:?}");
        let next = logged(folder.path(), &binary, &[], context().root);
        let last = next.args.last().unwrap().to_string_lossy().into_owned();
        assert!(last.contains("0001-"), "{last}");
    }
}

#[test]
fn plants_log_takes_a_folder_a_test_binary_and_its_words() {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut console = Console {
        out: &mut out,
        err: &mut err,
    };
    let said = log(&context(), &[OsString::from("folder")], &mut console).unwrap_err();
    assert!(matches!(said, Failure::Usage(_)), "{said}");
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

#[test]
fn node_s_junit_report_names_each_test_as_written_and_a_file_that_ran_none_of_its_own() {
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
        [r#"the meant one # marked "quoted""#],
        "{}",
        first.output
    );
    assert!(first
        .tests_ran()
        .contains(&"passes, and prints that it failed"));
    // The report the table asked for is the one printed.
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
    assert!(tap.output.starts_with("TAP version 13"), "{}", tap.output);
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
    assert!(printed.tests_ran().contains(&"a group › the meant one"));
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
}

/// A crate of Rust tests: one that passes, printing libtest's words for itself failing,
/// its failures list and a footer, and one that fails with a panic quoting a build that
/// failed; a feature that does not build; a test binary that cannot say its tests.
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
    assert!(failed
        .tests_ran()
        .contains(&"tests::the_meant_one_passes_and_prints_a_footer"));
    // libtest's words for a filter, passed on.
    let skipped = cargo_test(&["--lib", "--", "--skip", "another_fails"]);
    assert_eq!(skipped.code, Some(0), "{}", skipped.output);
    assert_eq!(
        skipped.tests_ran(),
        ["tests::the_meant_one_passes_and_prints_a_footer"]
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
        edits: vec![Edit {
            file: "a.rs".into(),
            find: "one".into(),
            replace: "ONE".into(),
        }],
    };
    (folder, checkout, plant)
}

/// What the driver says of `plant` in `checkout`, its commands run as processes.
fn verdict_of(checkout: &Context, plant: &crate::plants::kit::Plant) -> String {
    let family = crate::plants::kit::Family {
        name: "demo".into(),
        limit: Duration::from_secs(120),
        rebuild: None,
        plants: vec![plant.clone()],
    };
    let mut out = Vec::new();
    let status = crate::plants::driver::run(
        checkout,
        &family,
        &[plant],
        &mut runner(Duration::from_secs(120)),
        &mut out,
    )
    .unwrap();
    assert_eq!(status, 1);
    assert_eq!(
        std::fs::read_to_string(checkout.path("a.rs")).unwrap(),
        "one\n"
    );
    String::from_utf8(out).unwrap()
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

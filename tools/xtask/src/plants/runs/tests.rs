use std::time::{Duration, Instant};

use super::*;

/// A run of `cargo test -p x` that ended with `code` having written `output`.
fn ran(code: Option<i32>, output: &str) -> Ran {
    ran_by(&["cargo", "test", "-p", "x"], code, output)
}

/// A run of `command` that ended with `code` having written `output`.
fn ran_by(command: &[&str], code: Option<i32>, output: &str) -> Ran {
    Ran {
        command: command.iter().map(|word| (*word).to_owned()).collect(),
        code,
        hung: false,
        output: output.to_owned(),
    }
}

/// A run of `node --test` on a test file.
const NODE: [&str; 3] = ["node", "--test", "tests/a.test.mjs"];
/// A run of Playwright's tests.
const PAGE: [&str; 4] = [
    "node",
    "app/node_modules/@playwright/test/cli.js",
    "test",
    "app/tests/core-page.spec.mjs",
];

fn names(ran: &Ran) -> Vec<String> {
    ran.caught()
}

#[test]
fn a_test_failed_where_libtest_lists_it_at_the_end_of_its_binary_and_nowhere_else() {
    let output = "running 3 tests\ntest a::passes ... ok\ntest a::fails ... FAILED\n\
                  test a::printed ... FAILED\ntest b::ignored ... ignored\n\n\
                  failures:\n\n---- a::fails stdout ----\ntest a::printed ... FAILED\n\n\
                  failures:\n    a::fails\n\ntest result: FAILED. 1 passed; 1 failed\n";
    assert_eq!(names(&ran(Some(101), output)), ["a::fails"]);
    // A list that ends no binary's run, as a test may print one, is none.
    let printed = "running 1 test\ntest a::b ... FAILED\n\nfailures:\n    a::meant\n\nthe end\n";
    assert!(!ran(Some(101), printed).names_tests());
}

#[test]
fn a_test_failed_where_node_says_so_in_its_report_and_in_tap() {
    let spec = "✖ printed (1ms)\n✔ passes (0.5ms)\n▶ g\n  ✖ nested fails (1.25ms)\n✖ g (2ms)\n\
                ✖ top fails (3ms)\nℹ tests 3\n\n✖ failing tests:\n\n\
                test at tests/a.test.mjs:3:3\n✖ nested fails (1.25ms)\n  Error: no\n\n\
                test at tests/a.test.mjs:9:1\n✖ top fails (3ms)\n  Error: no\n";
    assert_eq!(
        names(&ran_by(&NODE, Some(1), spec)),
        ["nested fails", "top fails"],
        "the failed tests of the list the report ends with"
    );
    let tap =
        "TAP version 13\n# not ok 9 - printed\n# Subtest: g\n    # Subtest: nested fails\n    \
               not ok 1 - nested fails\n      ---\n      error: |-\n        no\n        \
               not ok 7 - said in an error\n      ...\n    1..1\nnot ok 1 - g\n  ---\n  ...\n\
               ok 2 - passes\nnot ok 3 - top fails\n  ok 4 - fine\nnot ok - no number\n";
    assert_eq!(
        names(&ran_by(&NODE, Some(1), tap)),
        ["nested fails", "g", "top fails"]
    );
}

/// What Playwright's list reporter writes when it does not write to a terminal, as it
/// did for a spec of two tests, one failing (Playwright 1.63, 2026-10-15).
const PLAYWRIGHT: &str = "Running 2 tests using 1 worker\n\n  \
    ✓  1 app/tests/probe.spec.mjs:3:1 › a probe that passes (2ms)\n  \
    ✘  2 app/tests/probe.spec.mjs:8:3 › a group › a probe that fails, with › in its title (1ms)\n\n\n  \
    1) app/tests/probe.spec.mjs:8:3 › a group › a probe that fails, with › in its title \n\n    \
    Error: expect(received).toBe(expected) // Object.is equality\n\n    Expected: 2\n\n  \
    1 failed\n    \
    app/tests/probe.spec.mjs:8:3 › a group › a probe that fails, with › in its title \n  \
    1 passed (214ms)\n";

#[test]
fn a_test_failed_where_the_summary_of_playwright_lists_it_named_from_its_groups_on() {
    assert_eq!(
        names(&ran_by(&PAGE, Some(1), PLAYWRIGHT)),
        ["a group › a probe that fails, with › in its title"]
    );
    // Before the file, a project; under the heading, every test until a line that is none.
    let two = "  2 failed\n    [chromium] › a.spec.mjs:1:1 › one \n    a.spec.mjs:9:3 › g › two ───\n  3 passed (1s)\n    a.spec.mjs:20:1 › not under the heading \n";
    assert_eq!(names(&ran_by(&PAGE, Some(1), two)), ["one", "g › two"]);
    // A line of the list that names no place is no test, nor is a line that counts nothing.
    let none = "  1 failed\n    no place › here\n  the run failed\n    a.spec.mjs:1:1 › x \n";
    assert!(!ran_by(&PAGE, Some(1), none).names_tests());
}

/// What Playwright 1.63 wrote (2026-10-16) for a spec whose passing test printed the
/// heading and a line of its summary, and whose other test failed; the checkout's
/// path cut to `app/tests/`.
const PLAYWRIGHT_PRINTED: &str = "\nRunning 2 tests using 1 worker\n\n  1 failed\n    \
    prints.spec.mjs:3:1 › the meant one \n  \
    ✓  1 app/tests/prints.spec.mjs:3:1 › the meant one (2ms)\n  \
    ✘  2 app/tests/prints.spec.mjs:8:1 › another fails (1ms)\n\n\n  \
    1) app/tests/prints.spec.mjs:8:1 › another fails ─────────────────\n\n    \
    Error: expect(received).toBe(expected) // Object.is equality\n\n    Expected: 2\n    Received: 1\n\n  \
    1 failed\n    app/tests/prints.spec.mjs:8:1 › another fails ──────────────────\n  1 passed (229ms)\n";

/// What Node 26 wrote (2026-10-16) for a file whose first test printed libtest's,
/// its spec report's and its TAP report's words for itself failing, and passed, and
/// whose second failed: in its spec report, and in its TAP report.
const NODE_PRINTED: &str = "test m::the_test ... FAILED\n✖ the_test (1ms)\nnot ok 1 - the_test\n\
    ✔ the_test (0.532167ms)\n✖ another fails (0.078541ms)\nℹ tests 2\nℹ suites 0\nℹ pass 1\nℹ fail 1\n\
    ℹ cancelled 0\nℹ skipped 0\nℹ todo 0\nℹ duration_ms 44.01475\n\n✖ failing tests:\n\n\
    test at tests/prints.test.mjs:9:1\n✖ another fails (0.078541ms)\n  Error: it fails\n      \
    at TestContext.<anonymous> (file:///checkout/tests/prints.test.mjs:10:9)\n";
const NODE_PRINTED_TAP: &str =
    "TAP version 13\n# test m::the_test ... FAILED\n# ✖ the_test (1ms)\n\
    # not ok 1 - the_test\n# Subtest: the_test\nok 1 - the_test\n  ---\n  duration_ms: 0.533875\n  \
    type: 'test'\n  ...\n# Subtest: another fails\nnot ok 2 - another fails\n  ---\n  \
    duration_ms: 0.078416\n  type: 'test'\n  location: '/checkout/tests/prints.test.mjs:9:1'\n  \
    failureType: 'testCodeFailure'\n  error: 'it fails'\n  code: 'ERR_TEST_FAILURE'\n  ...\n1..2\n\
    # tests 2\n# suites 0\n# pass 1\n# fail 1\n";

#[test]
fn what_a_test_printed_is_never_read_for_a_test_that_failed() {
    assert_eq!(
        names(&ran_by(&PAGE, Some(1), PLAYWRIGHT_PRINTED)),
        ["another fails"]
    );
    for output in [NODE_PRINTED, NODE_PRINTED_TAP] {
        assert_eq!(
            names(&ran_by(&NODE, Some(1), output)),
            ["another fails"],
            "{output}"
        );
    }
    // libtest's words in a run of Node's tests, Node's in a run of libtest's, are no result.
    let libtest = "running 2 tests\ntest m::other ... FAILED\n\nfailures:\n\n---- m::other stdout ----\n\
                   test m::the_test ... FAILED\n✖ failing tests:\n\ntest at a.mjs:1:1\n✖ the_test (1ms)\n\
                   not ok 1 - the_test\n\nfailures:\n    m::other\n\ntest result: FAILED. 0 passed; 1 failed\n";
    assert_eq!(names(&ran(Some(101), libtest)), ["m::other"]);
}

#[test]
fn the_tests_are_named_in_the_order_the_output_says_them_each_once() {
    let block = |names: &[&str]| {
        let listed: String = names.iter().map(|name| format!("    {name}\n")).collect();
        format!(
            "running 2 tests\n\nfailures:\n{listed}\ntest result: FAILED. 0 passed; 2 failed\n\n"
        )
    };
    let output = block(&["b::second", "a::first"]) + &block(&["c::third", "b::second"]);
    assert_eq!(
        names(&ran(Some(101), &output)),
        ["b::second", "a::first", "c::third"]
    );
}

#[test]
fn a_run_that_ended_with_no_test_to_say_so_is_the_catch_itself() {
    let silent = ran(Some(3), "running 1 test\n");
    assert_eq!(names(&silent), ["test -p x: ended with 3"]);
    assert_eq!(names(&ran(None, "")), ["test -p x: was ended by a signal"]);
    let died = "error: test failed, to rerun pass `-p x --test t`\n\nCaused by:\n  \
                process didn't exit successfully: `/target/t-1a --nocapture` (signal: 11, SIGSEGV: invalid memory reference)\n";
    assert_eq!(
        names(&ran(Some(101), died)),
        ["test -p x: the test binary died (signal: 11, SIGSEGV: invalid memory reference)"]
    );
}

#[test]
fn a_run_that_passed_or_hung_or_did_not_build_catches_nothing() {
    assert!(names(&ran(Some(0), "running 1 test\ntest a ... ok\n")).is_empty());
    let hung = Ran {
        hung: true,
        ..ran(None, "")
    };
    assert!(hung.caught().is_empty());
    let broken = ran(
        Some(101),
        "error[E0432]: unresolved import\nerror: could not compile `x`\n",
    );
    assert!(!broken.compiled());
    assert!(broken.caught().is_empty());
}

#[test]
fn a_run_in_which_a_test_failed_built_whatever_the_test_quotes() {
    // A test of the plants that fails says what its stand-in for a broken build wrote.
    let quoted = ran(
        Some(101),
        "running 2 tests\ntest a::fails ... FAILED\n\nfailures:\n\n---- a::fails stdout ----\nDOES NOT COMPILE p (0 s)\nerror[E0432]: unresolved import\nerror: could not compile `x`\n\nfailures:\n    a::fails\n\ntest result: FAILED. 1 passed; 1 failed\n",
    );
    assert!(quoted.compiled());
    assert_eq!(quoted.caught(), ["a::fails"]);
}

#[test]
fn a_source_that_does_not_build_is_told_by_cargo_and_by_the_code_of_rustc() {
    assert!(ran(Some(0), "error: something else\n").compiled());
    assert!(!ran(
        Some(101),
        "error: could not compile `x` (lib) due to 1 error\n"
    )
    .compiled());
    assert!(!ran(Some(101), "error[E0308]: mismatched types\n").compiled());
    // An error code is `E` and digits and `]`.
    assert!(ran(Some(0), "error[E]: no\nerror[E12: no\n").compiled());
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
    assert_eq!((ended.code, ended.hung), (Some(0), false));
    assert_eq!(ended.output, "out\nerr\n");

    let failed = |code, killed| CaptureFailed {
        message: "Command failed: cargo".into(),
        code,
        killed,
        stdout: "out\n".into(),
        stderr: "err\n".into(),
    };
    let failed_with_three = Ran::of(&command, Err(failed(Some(3), false))).unwrap();
    assert_eq!(
        (failed_with_three.code, failed_with_three.hung),
        (Some(3), false)
    );
    let out_of_time = Ran::of(&command, Err(failed(None, true))).unwrap();
    assert_eq!((out_of_time.code, out_of_time.hung), (None, true));

    // A program that could not be started is no outcome of a plant.
    let unstarted = CaptureFailed::unstarted("spawn cargo ENOENT".into());
    assert!(matches!(
        Ran::of(&command, Err(unstarted)),
        Err(Error::Process(process::Failure::Refused { .. }))
    ));
}

// The commands below are real, and the program they start is the one running these tests:
// each of the tests it is asked for says, as `cargo test` does, what the plants are read from.

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
#[allow(clippy::disallowed_methods)] // The test is of what the environment it is started with holds.
fn helper_has_no_node_test_context() {
    assert!(std::env::var_os("NODE_TEST_CONTEXT").is_none());
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

fn ended(execution: Execution) -> Ran {
    match execution {
        Execution::Ended(ran) => ran,
        Execution::Stopped => panic!("the command was stopped"),
    }
}

#[test]
fn a_command_that_fails_a_test_says_which_in_the_words_of_cargo() {
    let ran = ended(
        runner(Duration::from_secs(120))
            .run(&helper("helper_fails"))
            .unwrap(),
    );
    assert_eq!(ran.code, Some(101));
    assert!(!ran.hung);
    let caught = ran.caught();
    assert_eq!(caught, ["plants::runs::tests::helper_fails"]);
    assert!(
        ran.output.contains("the planted bug is here"),
        "{}",
        ran.output
    );
}

#[test]
fn a_command_that_passes_ran_its_tests_and_catches_nothing() {
    let ran = ended(
        runner(Duration::from_secs(120))
            .run(&helper("helper_passes"))
            .unwrap(),
    );
    assert_eq!(ran.code, Some(0));
    assert!(ran.caught().is_empty());
    assert!(
        ran.output.contains("test result: ok. 1 passed"),
        "{}",
        ran.output
    );
}

#[test]
fn a_command_that_exits_with_a_code_and_no_test_to_say_so_is_the_catch() {
    let ran = ended(
        runner(Duration::from_secs(120))
            .run(&helper("helper_exits_with_three"))
            .unwrap(),
    );
    assert_eq!(ran.code, Some(3));
    let caught = ran.caught();
    assert_eq!(caught.len(), 1);
    assert!(caught[0].ends_with(": ended with 3"), "{}", caught[0]);
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
fn cargo_xtask_is_the_xtask_that_runs_the_plants_and_not_a_cargo_to_build_it_again() {
    let mut command = vec!["cargo".to_owned(), "xtask".to_owned()];
    command.extend(helper("helper_fails").into_iter().skip(1));
    let ran = ended(runner(Duration::from_secs(120)).run(&command).unwrap());
    assert_eq!(ran.caught(), ["plants::runs::tests::helper_fails"]);
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
fn only_a_run_that_exited_with_a_failure_catches_whatever_it_printed() {
    let printed = "running 1 test\ntest a::fails ... FAILED\n\nfailures:\n    a::fails\n\n\
                   test result: FAILED. 0 passed; 1 failed\n";
    assert!(names(&ran(Some(0), printed)).is_empty());
    assert_eq!(names(&ran(Some(101), printed)), ["a::fails"]);
}

#[test]
fn a_build_is_judged_by_what_cargo_says_of_it_in_its_structured_words_where_it_said_any() {
    let failed = "{\"reason\":\"compiler-artifact\"}\n{\"reason\":\"build-finished\",\"success\":false}\ntest a::fails ... FAILED\n";
    assert!(!ran(Some(101), failed).compiled());
    let built = "{\"reason\":\"build-finished\",\"success\":true}\nrunning 1 test\ntest a::fails ... FAILED\nerror[E0432]: quoted\nerror: could not compile `x`\n";
    assert!(ran(Some(101), built).compiled());
    // Two builds, one failed: it did not build.
    let one_failed = "{\"reason\":\"build-finished\",\"success\":true}\n{\"reason\":\"build-finished\",\"success\":false}\n";
    assert!(!ran(Some(101), one_failed).compiled());
    // A test that fails quoting cargo's structured words, once the tests ran, is a test's output.
    let quoted = "{\"reason\":\"build-finished\",\"success\":true}\nrunning 1 test\ntest a::fails ... FAILED\n{\"reason\":\"build-finished\",\"success\":false}\n";
    assert!(ran(Some(101), quoted).compiled());
}

#[test]
fn where_cargo_said_nothing_structured_a_broken_build_counts_only_before_any_test_ran() {
    let before =
        "   Compiling app v1\nerror[E0308]: mismatched types\nerror: could not compile `app`\n";
    assert!(!ran(Some(101), before).compiled());
    let after = "running 2 tests\ntest a::fails ... FAILED\n---- a::fails stdout ----\nerror: could not compile `cf`\n";
    assert!(ran(Some(101), after).compiled());
    let playwright = "Running 3 tests using 1 worker\nerror: could not compile `x`\n";
    assert!(ran(Some(1), playwright).compiled());
}

// What `node --test` wrote (Node 26.8.1, 2026-10-16) for a test file of its own in each way
// it fails whole, the checkout's path cut to `/checkout/` and the file's folder to `tests/`.

/// A test file that does not parse, in Node's spec report.
const NODE_UNPARSED: &str = r#"file:///checkout/tests/syntax.test.mjs:5
this is not a module
     ^^

SyntaxError: Unexpected identifier 'is'
    at compileSourceTextModule (node:internal/modules/esm/utils:355:16)
    at ModuleLoader.moduleStrategy (node:internal/modules/esm/translators:93:18)
    at #translate (node:internal/modules/esm/loader:441:20)
    at afterLoad (node:internal/modules/esm/loader:509:29)
    at ModuleLoader.loadAndTranslate (node:internal/modules/esm/loader:514:12)
    at #getOrCreateModuleJobAfterResolve (node:internal/modules/esm/loader:567:36)
    at afterResolve (node:internal/modules/esm/loader:614:52)
    at ModuleLoader.getOrCreateModuleJob (node:internal/modules/esm/loader:620:12)
    at node:internal/modules/esm/loader:639:32
    at TracingChannel.tracePromise (node:diagnostics_channel:547:22)

Node.js v26.8.1
✖ tests/syntax.test.mjs (32.667166ms)
ℹ tests 1
ℹ suites 0
ℹ pass 0
ℹ fail 1
ℹ cancelled 0
ℹ skipped 0
ℹ todo 0
ℹ duration_ms 36.541334

✖ failing tests:

test at tests/syntax.test.mjs:1:1
✖ tests/syntax.test.mjs (32.667166ms)
  'test failed'
"#;

/// A test file that imports a module that is not there, in Node's spec report.
const NODE_UNRESOLVED: &str = r#"node:internal/modules/esm/resolve:272
    throw new ERR_MODULE_NOT_FOUND(
          ^

Error [ERR_MODULE_NOT_FOUND]: Cannot find module '/checkout/tests/not-there.mjs' imported from /checkout/tests/missing.test.mjs
    at finalizeResolution (node:internal/modules/esm/resolve:272:11)
    at moduleResolve (node:internal/modules/esm/resolve:879:10)
    at defaultResolve (node:internal/modules/esm/resolve:1006:11)
    at ModuleJob.link (node:internal/modules/esm/module_job:389:17) {
  code: 'ERR_MODULE_NOT_FOUND',
  url: 'file:///checkout/tests/not-there.mjs'
}

Node.js v26.8.1
✖ tests/missing.test.mjs (33.284625ms)
ℹ tests 1
ℹ pass 0
ℹ fail 1

✖ failing tests:

test at tests/missing.test.mjs:1:1
✖ tests/missing.test.mjs (33.284625ms)
  'test failed'
"#;

/// A test file that throws as it runs, before any of its tests: Node's spec report.
const NODE_THROWN_AT_LOAD: &str = r#"node:internal/test_runner/harness:124
      throw err;
      ^

Error: the module throws as it loads
    at file:///checkout/tests/throws-at-load.test.mjs:5:7
    at ModuleJob.run (node:internal/modules/esm/module_job:569:25)
    at async node:internal/modules/esm/loader:650:26
    at async asyncRunEntryPointWithESMLoader (node:internal/modules/run_main:101:5)

Node.js v26.8.1
✖ tests/throws-at-load.test.mjs (33.486708ms)
ℹ tests 1
ℹ pass 0
ℹ fail 1

✖ failing tests:

test at tests/throws-at-load.test.mjs:1:1
✖ tests/throws-at-load.test.mjs (33.486708ms)
  'test failed'
"#;

/// A test file that does not parse, in a folder with a space in its name: the TAP report.
const NODE_UNPARSED_SPACED_TAP: &str = r#"TAP version 13
# file:///checkout/tests/with%20spaces/syntax.test.mjs:5
# this is not a module
#      ^^
# SyntaxError: Unexpected identifier 'is'
#     at compileSourceTextModule (node:internal/modules/esm/utils:355:16)
#     at ModuleLoader.moduleStrategy (node:internal/modules/esm/translators:93:18)
#     at \#translate (node:internal/modules/esm/loader:441:20)
# Node.js v26.8.1
# Subtest: tests/with spaces/syntax.test.mjs
not ok 1 - tests/with spaces/syntax.test.mjs
  ---
  duration_ms: 29.812375
  type: 'test'
  location: '/checkout/tests/with spaces/syntax.test.mjs:1:1'
  failureType: 'testCodeFailure'
  exitCode: 1
  signal: ~
  error: 'test failed'
  code: 'ERR_TEST_FAILURE'
  ...
1..1
# tests 1
# pass 0
# fail 1
"#;

/// A test file whose second test ends the process (`process.exit(3)`) at once, after its
/// first passed: Node says the file failed whole, and nothing of either test.
const NODE_ENDED_IN_A_TEST: &str = r"✖ tests/crash.test.mjs (35.731584ms)
ℹ tests 1
ℹ suites 0
ℹ pass 0
ℹ fail 1
ℹ cancelled 0
ℹ skipped 0
ℹ todo 0
ℹ duration_ms 39.651

✖ failing tests:

test at tests/crash.test.mjs:1:1
✖ tests/crash.test.mjs (35.731584ms)
  'test failed'
";

/// The same, in the TAP report.
const NODE_ENDED_IN_A_TEST_TAP: &str = r"TAP version 13
# Subtest: tests/crash.test.mjs
not ok 1 - tests/crash.test.mjs
  ---
  duration_ms: 35.082375
  type: 'test'
  location: '/checkout/tests/crash.test.mjs:1:1'
  failureType: 'testCodeFailure'
  exitCode: 3
  signal: ~
  error: 'test failed'
  code: 'ERR_TEST_FAILURE'
  ...
1..1
# tests 1
# pass 0
# fail 1
";

/// A test file whose third test ends the process after a wait, its first two having
/// passed: Node says they did, and that the file then failed whole.
const NODE_ENDED_LATE: &str = r"✔ passes first (0.30575ms)
✔ passes second (0.047958ms)
✖ tests/late.test.mjs (88.840084ms)
ℹ tests 3
ℹ suites 0
ℹ pass 2
ℹ fail 1
ℹ cancelled 0
ℹ skipped 0
ℹ todo 0
ℹ duration_ms 92.622

✖ failing tests:

test at tests/late.test.mjs:1:1
✖ tests/late.test.mjs (88.840084ms)
  'test failed'
";

/// The same, in the TAP report.
const NODE_ENDED_LATE_TAP: &str = r"TAP version 13
# Subtest: passes first
ok 1 - passes first
  ---
  duration_ms: 0.299875
  type: 'test'
  ...
# Subtest: passes second
ok 2 - passes second
  ---
  duration_ms: 0.04825
  type: 'test'
  ...
# Subtest: tests/late.test.mjs
not ok 1 - tests/late.test.mjs
  ---
  duration_ms: 85.823
  type: 'test'
  location: '/checkout/tests/late.test.mjs:1:1'
  failureType: 'testCodeFailure'
  exitCode: 3
  signal: ~
  error: 'test failed'
  code: 'ERR_TEST_FAILURE'
  ...
1..3
# tests 3
# pass 2
# fail 1
";

/// A test file that throws after a top-level `await`, before it registers a test: no
/// loader's frame in its stack.
const NODE_THROWN_AFTER_AWAIT: &str = r"file:///checkout/tests/tla.test.mjs:4
throw new Error('thrown after an await')
      ^

Error: thrown after an await
    at file:///checkout/tests/tla.test.mjs:4:7

Node.js v26.8.1
✖ tests/tla.test.mjs (32.578625ms)
ℹ tests 1
ℹ suites 0
ℹ pass 0
ℹ fail 1
ℹ cancelled 0
ℹ skipped 0
ℹ todo 0
ℹ duration_ms 36.434583

✖ failing tests:

test at tests/tla.test.mjs:1:1
✖ tests/tla.test.mjs (32.578625ms)
  'test failed'
";

/// The same, in the TAP report.
const NODE_THROWN_AFTER_AWAIT_TAP: &str = r"TAP version 13
# file:///checkout/tests/tla.test.mjs:4
# throw new Error('thrown after an await')
#       ^
# Error: thrown after an await
#     at file:///checkout/tests/tla.test.mjs:4:7
# Node.js v26.8.1
# Subtest: tests/tla.test.mjs
not ok 1 - tests/tla.test.mjs
  ---
  duration_ms: 36.92525
  type: 'test'
  location: '/checkout/tests/tla.test.mjs:1:1'
  failureType: 'testCodeFailure'
  exitCode: 1
  signal: ~
  error: 'test failed'
  code: 'ERR_TEST_FAILURE'
  ...
1..1
# tests 1
# suites 0
# pass 0
# fail 1
";

// What Playwright 1.63.0 wrote (2026-10-16) when a run's file argument and filter found no
// test, and when its spec could not be imported, the checkout's path cut to `/checkout/`.

const PLAYWRIGHT_NONE_FOUND: &str = r#"Error: No tests found.
Make sure that arguments are regular expressions matching test files.
You may need to escape symbols like "$" or "*" and quote the arguments.

"#;

const PLAYWRIGHT_UNIMPORTED: &str = r#"Error: Cannot find module '/checkout/app/tests/not-there.mjs' imported from /checkout/app/tests/missing.spec.mjs
    at eval (<anonymous>:1:1)
Error: No tests found.
Make sure that arguments are regular expressions matching test files.
You may need to escape symbols like "$" or "*" and quote the arguments.

"#;

#[test]
fn a_node_test_file_that_ran_no_test_is_broken_however_it_failed_in_either_report_and_wherever_it_is(
) {
    for (output, file) in [
        (NODE_UNPARSED, "tests/syntax.test.mjs"),
        (NODE_UNRESOLVED, "tests/missing.test.mjs"),
        (NODE_THROWN_AT_LOAD, "tests/throws-at-load.test.mjs"),
        (
            NODE_UNPARSED_SPACED_TAP,
            "tests/with spaces/syntax.test.mjs",
        ),
        // No frame of the loader in its stack: Node says no test ran all the same.
        (NODE_THROWN_AFTER_AWAIT, "tests/tla.test.mjs"),
        (NODE_THROWN_AFTER_AWAIT_TAP, "tests/tla.test.mjs"),
        // Ended at once in a test that began: Node says nothing of any test of it.
        (NODE_ENDED_IN_A_TEST, "tests/crash.test.mjs"),
        (NODE_ENDED_IN_A_TEST_TAP, "tests/crash.test.mjs"),
    ] {
        let failed = ran_by(&NODE, Some(1), output);
        assert_eq!(
            failed.broken(),
            Some(format!("the test file {file} ran no test")),
            "{output}"
        );
        assert!(!failed.names_tests(), "{output}");
    }
}

#[test]
fn a_node_test_file_that_failed_whole_after_tests_of_it_ran_is_no_load_failure_and_names_no_test() {
    for output in [NODE_ENDED_LATE, NODE_ENDED_LATE_TAP] {
        let ended = ran_by(&NODE, Some(1), output);
        assert_eq!(ended.broken(), None, "{output}");
        assert_eq!(ended.caught(), ["tests/late.test.mjs"], "{output}");
        assert!(!ended.names_tests(), "the file failed whole: {output}");
    }
    // A test of its own named like a file, which Node says is written at a line of its own.
    let test = ran_by(
        &NODE,
        Some(1),
        "ℹ tests 1\n\n✖ failing tests:\n\ntest at tests/a.test.mjs:4:1\n✖ reads config.js (2ms)\n  Error: no\n",
    );
    assert!(test.names_tests());
    assert_eq!(test.broken(), None);
}

#[test]
fn a_node_test_file_that_failed_whole_with_its_loader_s_error_beside_others_tests_is_broken() {
    let beside = "✔ a test of b (1ms)\nSyntaxError: Unexpected end of input\n    \
                  at compileSourceTextModule (node:internal/modules/esm/utils:355:16)\n\
                  ✖ tests/a.test.mjs (29ms)\nℹ tests 2\n\n✖ failing tests:\n\n\
                  test at tests/a.test.mjs:1:1\n✖ tests/a.test.mjs (29ms)\n  'test failed'\n";
    assert_eq!(
        ran_by(&NODE, Some(1), beside).broken().as_deref(),
        Some("the test file tests/a.test.mjs could not be loaded")
    );
}

#[test]
fn playwright_that_found_no_test_or_an_error_outside_every_test_is_broken() {
    for (output, said) in [
        (PLAYWRIGHT_NONE_FOUND, "Playwright: Error: No tests found."),
        (PLAYWRIGHT_UNIMPORTED, "Playwright: Error: No tests found."),
        (
            "Running 2 tests\n  1 error was not a part of any test, see above for details\n",
            "Playwright: 1 error was not a part of any test, see above for details",
        ),
    ] {
        assert_eq!(
            ran_by(&PAGE, Some(1), output).broken().as_deref(),
            Some(said)
        );
    }
    // A test that throws a SyntaxError of its own failed, as a test does.
    let threw = "ℹ tests 1\n\n✖ failing tests:\n\ntest at tests/a.test.mjs:3:1\n\
                 ✖ parses the answer (0.3ms)\n  SyntaxError: Expected property name\n";
    assert_eq!(ran_by(&NODE, Some(1), threw).broken(), None);
}

/// The plants' own tests, failing, print the fixtures above: a run of libtest's is read
/// by libtest's words alone, whatever runner a test of it quotes (the tooling table,
/// 2026-10-16, took three such catches for modules that could not be loaded).
#[test]
fn what_node_or_playwright_says_quoted_by_a_test_of_cargo_test_is_no_load_failure() {
    let quoting = format!(
        "running 2 tests\ntest plants::a::b ... FAILED\n\nfailures:\n\n---- plants::a::b stdout ----\n\
         {NODE_UNPARSED}{NODE_UNPARSED_SPACED_TAP}{PLAYWRIGHT_UNIMPORTED}\n\nfailures:\n    plants::a::b\n\n\
         test result: FAILED. 1 passed; 1 failed\n"
    );
    let failed = ran(Some(101), &quoting);
    assert_eq!(failed.broken(), None);
    assert!(failed.names_tests());
    assert_eq!(failed.caught(), ["plants::a::b"]);
    // Said by Node, or by Playwright, the same words are a run's own.
    assert!(ran_by(&NODE, Some(1), NODE_UNPARSED).broken().is_some());
    assert!(ran_by(&PAGE, Some(1), PLAYWRIGHT_NONE_FOUND)
        .broken()
        .is_some());
}

#[test]
fn a_failure_names_tests_where_cargo_node_or_playwright_named_one_and_not_where_none_did() {
    let libtest =
        "running 1 test\n\nfailures:\n    a::b\n\ntest result: FAILED. 0 passed; 1 failed\n";
    assert!(ran(Some(101), libtest).names_tests());
    assert!(ran_by(&NODE, Some(1), "TAP version 13\nnot ok 2 - a test\n").names_tests());
    assert!(ran_by(&PAGE, Some(1), PLAYWRIGHT).names_tests());
    assert!(!ran(Some(3), "running 1 test\n").names_tests());
    assert!(!ran(Some(1), "the smoke: the app did not answer\n").names_tests());
}

#[test]
fn what_a_run_wrote_is_read_with_the_terminal_formatting_taken_out() {
    let coloured = "\u{1b}[31m  1 failed\u{1b}[39m\n\u{1b}[31m    a.spec.mjs:9:3 › g › two \u{1b}[39m\n\u{1b}[32m  1 passed\u{1b}[39m (2s)\n\u{1b}]8;;\u{1b}\\";
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
    assert_eq!(ended.caught(), ["g › two"]);
}

#[test]
fn a_cargo_test_is_asked_for_its_builds_structured_status_and_nothing_else_is_changed() {
    let words_of =
        |command: &[&str]| words(&command.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>());
    assert_eq!(
        words_of(&["cargo", "test", "--offline", "-p", "x", "--", "a::b"]),
        [
            "test",
            "--message-format=json-render-diagnostics",
            "--offline",
            "-p",
            "x",
            "--",
            "a::b"
        ]
    );
    assert_eq!(
        words_of(&["cargo", "build", "-p", "x"]),
        ["build", "-p", "x"]
    );
    assert_eq!(
        words_of(&["node", "--test", "tests/a.test.mjs"]),
        ["--test", "tests/a.test.mjs"]
    );
}

#[test]
#[ignore = "a program the tests of runs start"]
#[allow(clippy::disallowed_methods)] // The test is of what the environment it is started with holds.
fn helper_has_no_colour() {
    assert_eq!(std::env::var("NO_COLOR").as_deref(), Ok("1"));
    assert_eq!(std::env::var("FORCE_COLOR").as_deref(), Ok("0"));
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

#[test]
#[ignore = "a program the tests of runs start"]
fn helper_named_meant() {
    println!("test plants::runs::tests::helper_named_meant ... FAILED");
}

#[test]
#[ignore = "a program the tests of runs start"]
fn helper_named_other() {
    println!("test plants::runs::tests::helper_named_meant ... FAILED");
    panic!("the other test failed");
}

#[test]
fn a_test_that_prints_another_failed_is_the_one_that_failed_whether_libtest_captures_it_or_not() {
    for nocapture in [false, true] {
        let mut command = vec![
            this_binary().to_string_lossy().into_owned(),
            "--ignored".to_owned(),
            "plants::runs::tests::helper_named_".to_owned(),
        ];
        if nocapture {
            command.push("--nocapture".to_owned());
        }
        let ran = ended(runner(Duration::from_secs(120)).run(&command).unwrap());
        assert_eq!(ran.code, Some(101), "{}", ran.output);
        // The test meant passed and said it failed, or the other said it of it.
        assert!(
            ran.output
                .contains("test plants::runs::tests::helper_named_meant ... FAILED"),
            "{}",
            ran.output
        );
        assert_eq!(
            ran.caught(),
            ["plants::runs::tests::helper_named_other"],
            "{}",
            ran.output
        );
    }
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

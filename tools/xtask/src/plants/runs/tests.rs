use std::time::{Duration, Instant};

use super::*;

/// A run of `cargo test -p x` that ended with `code` having written `output`.
fn ran(code: Option<i32>, output: &str) -> Ran {
    Ran {
        command: ["cargo", "test", "-p", "x"].map(String::from).to_vec(),
        code,
        hung: false,
        output: output.to_owned(),
    }
}

fn names(ran: &Ran) -> Vec<String> {
    ran.caught()
}

#[test]
fn a_test_failed_where_cargo_says_so_with_a_name_that_has_no_space_in_it() {
    let output = "running 3 tests\ntest a::passes ... ok\ntest a::fails ... FAILED\n\
                  test src/lib.rs - doc (line 3) ... FAILED\ntest b::ignored ... ignored\n\n\
                  failures:\n    a::fails\n\ntest result: FAILED. 1 passed; 1 failed\n";
    assert_eq!(names(&ran(Some(101), output)), ["a::fails"]);
}

#[test]
fn a_test_failed_where_node_says_so_in_its_report_and_in_tap() {
    let spec =
        "✔ passes (0.5ms)\n  ✖ nested fails (1.25ms)\n✖ failing tests:\n\n✖ top fails (3ms)\n";
    assert_eq!(
        names(&ran(Some(1), spec)),
        ["nested fails", "top fails"],
        "`✖ failing tests:` is the heading, no test"
    );
    let tap = "ok 1 - passes\nnot ok 2 - top fails\n    not ok 3 - nested fails\n  ok 4 - fine\nnot ok - no number\n";
    assert_eq!(names(&ran(Some(1), tap)), ["top fails", "nested fails"]);
}

#[test]
fn the_tests_are_named_in_the_order_the_output_says_them_each_once() {
    let output = "test b::second ... FAILED\n✖ node one (1ms)\ntest b::second ... FAILED\ntest a::third ... FAILED\n";
    assert_eq!(
        names(&ran(Some(101), output)),
        ["b::second", "node one", "a::third"]
    );
    let reversed = "✖ node one (1ms)\ntest b::second ... FAILED\n";
    assert_eq!(names(&ran(Some(101), reversed)), ["node one", "b::second"]);
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

#[test]
#[ignore = "a program the tests of runs start"]
fn helper_has_the_temp_folder_its_run_gave_it() {
    assert!(std::env::temp_dir().ends_with("the-runs-temp"));
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
fn a_command_is_given_the_temp_folder_its_run_made_for_it() {
    let mut runner = runner(Duration::from_secs(120));
    runner.temp_in(&std::env::temp_dir().join("the-runs-temp"));
    let ran = ended(
        runner
            .run(&helper("helper_has_the_temp_folder_its_run_gave_it"))
            .unwrap(),
    );
    assert_eq!(ran.code, Some(0), "{}", ran.output);
    assert!(ran.caught().is_empty(), "{}", ran.output);
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

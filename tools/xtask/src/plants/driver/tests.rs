use std::collections::VecDeque;
use std::time::Duration;

use cf_base::env::Env;

use super::*;
use crate::plants::kit::{Edit, Run};
use crate::plants::runs::results::{Results, Test};

mod restore;

/// What a command is answered: how it ended, or that it could not be started.
type Answer = Result<Execution, Error>;

/// A runner that answers each command by `answer`, and says what it was asked.
struct Fake<A: FnMut(&[String]) -> Answer> {
    answer: A,
    ran: Vec<String>,
    stops: VecDeque<bool>,
    /// The file whose text is the state of the checkout, where a test changes it; else
    /// the state never changes.
    state_of: Option<PathBuf>,
}

impl<A: FnMut(&[String]) -> Answer> Fake<A> {
    fn new(answer: A) -> Self {
        Self {
            answer,
            ran: Vec::new(),
            stops: VecDeque::new(),
            state_of: None,
        }
    }
}

impl<A: FnMut(&[String]) -> Answer> Runner for Fake<A> {
    fn run(&mut self, command: &[String]) -> Answer {
        self.ran.push(command.join(" "));
        (self.answer)(command)
    }

    fn stopped(&mut self) -> bool {
        self.stops.pop_front().unwrap_or(false)
    }

    fn state(&mut self) -> String {
        self.state_of.as_ref().map_or_else(String::new, |file| {
            fs::read_to_string(file).unwrap_or_default()
        })
    }
}

/// A run of `cargo test -p x`, which nextest runs, that exited with `code` having
/// written `output` and no report.
fn ended(code: i32, output: &str) -> Answer {
    ended_by(&["cargo", "test", "-p", "x"], code, output, None)
}

/// A run of `command` that exited with `code` having written `output`, its report `results`.
fn ended_by(command: &[&str], code: i32, output: &str, results: Option<Results>) -> Answer {
    Ok(Execution::Ended(Ran::new(
        command,
        Some(code),
        output,
        results,
    )))
}

/// A report of `tests`, each a name and whether it failed.
fn report(tests: &[(&str, bool)]) -> Results {
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

/// A run of `cargo test -p x` that exited with `code` having written `output`, its
/// report saying `tests` ran.
fn reported(code: i32, output: &str, tests: &[(&str, bool)]) -> Answer {
    ended_by(
        &["cargo", "test", "-p", "x"],
        code,
        output,
        Some(report(tests)),
    )
}

/// What libtest writes of a run in which the tests `names` failed: each as it ended,
/// what it printed, and the list it ends with.
fn libtest_failed(names: &[&str]) -> String {
    let ended: String = names
        .iter()
        .map(|name| format!("test {name} ... FAILED\n"))
        .collect();
    let printed: String = names
        .iter()
        .map(|name| format!("---- {name} stdout ----\nthe plant is here\n\n"))
        .collect();
    let listed: String = names.iter().map(|name| format!("    {name}\n")).collect();
    format!(
        "running {} tests\n{ended}\nfailures:\n\n{printed}\nfailures:\n{listed}\n\
         test result: FAILED. 0 passed; {} failed; 0 ignored; 0 measured; 0 filtered out\n",
        names.len(),
        names.len()
    )
}

/// A run in which the tests `names` failed, and no other ran.
fn fail(names: &[&str]) -> Answer {
    let tests: Vec<(&str, bool)> = names.iter().map(|name| (*name, true)).collect();
    reported(100, &libtest_failed(names), &tests)
}

/// A run in which the test `name` failed.
fn fails(name: &str) -> Answer {
    fail(&[name])
}

/// A run in which every test passed, the meant one among them.
fn passes() -> Answer {
    reported(
        0,
        "running 2 tests\ntest m::the_test ... ok\ntest m::other ... ok\n\ntest result: ok. 2 passed; 0 failed\n",
        &[("m::the_test", false), ("m::other", false)],
    )
}

/// A run that failed and named no test, as a smoke that says no more than its status.
fn fails_unnamed() -> Answer {
    ended(1, "the smoke: the app did not answer\n")
}

/// The words of the build that comes before a run that builds the native `cf`.
const PREFLIGHT_WORDS: &str = "cargo build --release --locked --offline \
    --message-format=json-render-diagnostics -p cf --bin cf -p cf-panes --bin consensflow-bridge";

/// What cargo says of a build, in its structured words.
fn built(success: bool) -> Answer {
    ended(
        if success { 0 } else { 101 },
        &format!("{{\"reason\":\"build-finished\",\"success\":{success}}}\n"),
    )
}

/// A checkout of two files, `src/a.rs` and `src/b.rs`.
fn checkout() -> (tempfile::TempDir, Context) {
    let folder = tempfile::tempdir().unwrap();
    let context = Context {
        root: folder.path().to_path_buf(),
        env: Env::default(),
    };
    fs::create_dir_all(context.path("src")).unwrap();
    fs::write(context.path("src/a.rs"), "one\ntwo\n").unwrap();
    fs::write(context.path("src/b.rs"), "three\n").unwrap();
    (folder, context)
}

fn text(context: &Context, file: &str) -> String {
    fs::read_to_string(context.path(file)).unwrap()
}

/// Whether a plant is in the sources: a file of `checkout` is not as it wrote it.
fn planted(context: &Context) -> bool {
    text(context, "src/a.rs") != "one\ntwo\n" || text(context, "src/b.rs") != "three\n"
}

/// A run that fails the test `name` with a plant in the sources, and passes without one.
fn catches(context: &Context, name: &str) -> Answer {
    if planted(context) {
        fails(name)
    } else {
        passes()
    }
}

/// A plant that makes `edits`, tried by `runs` (each a program and whether it builds).
fn plant(name: &str, edits: &[(&str, &str, &str)], runs: &[(&str, bool)]) -> Plant {
    Plant {
        table: "t".into(),
        name: name.into(),
        meant: "the_test".into(),
        runs: runs
            .iter()
            .map(|(program, builds)| Run {
                command: vec![(*program).into()],
                builds: *builds,
            })
            .collect(),
        builds: false,
        prepare: Vec::new(),
        requires: Vec::new(),
        edits: edits
            .iter()
            .map(|(file, find, replace)| Edit {
                file: (*file).into(),
                find: (*find).into(),
                replace: (*replace).into(),
            })
            .collect(),
    }
}

/// The one plant of most tests: it puts `ONE` for `one` in `src/a.rs`.
fn upper(name: &str, runs: &[(&str, bool)]) -> Plant {
    plant(name, &[("src/a.rs", "one", "ONE")], runs)
}

fn family(plants: Vec<Plant>) -> Family {
    Family {
        name: "demo".into(),
        limit: Duration::from_secs(600),
        rebuild: Some(vec!["rebuild-cf".into()]),
        plants,
    }
}

/// What `run` answered, and what it said.
fn go(
    context: &Context,
    family: &Family,
    runner: &mut dyn Runner,
) -> (Result<i32, Failure>, String) {
    let chosen: Vec<&Plant> = family.plants.iter().collect();
    let mut out = Vec::new();
    let status = run(context, family, &chosen, runner, &mut out);
    (status, String::from_utf8(out).unwrap())
}

/// The folder the copies were kept in, from what the run said first.
fn copies_folder(said: &str) -> PathBuf {
    let first = said.lines().next().unwrap();
    PathBuf::from(
        first
            .strip_prefix("copies of what is planted are kept in ")
            .unwrap(),
    )
}

#[test]
fn a_plant_is_in_the_sources_while_its_runs_go_and_back_as_it_was_once_one_has_failed() {
    let (_folder, context) = checkout();
    let family = family(vec![upper("p: one", &[("first", false)])]);
    let mut seen = Vec::new();
    let mut runner = Fake::new(|_| {
        seen.push(text(&context, "src/a.rs"));
        catches(&context, "m::the_test")
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 0);
    // Planted once, failing the test it was meant for; then once with the plant put back, passing.
    assert_eq!(seen, ["ONE\ntwo\n", "one\ntwo\n"]);
    assert_eq!(text(&context, "src/a.rs"), "one\ntwo\n");
    let lines: Vec<_> = said.lines().collect();
    assert!(
        lines[0].starts_with("copies of what is planted are kept in "),
        "{said}"
    );
    assert!(
        lines[1].starts_with("CAUGHT           p: one (")
            && lines[1].ends_with(" s) by m::the_test"),
        "{said}"
    );
    assert_eq!(lines[2], "1 of 1 plants caught");
    assert!(
        !copies_folder(&said).exists(),
        "the copies are not wanted once all is back"
    );
}

#[test]
fn the_runs_of_a_plant_go_in_order_until_one_fails() {
    let (_folder, context) = checkout();
    let family = family(vec![
        upper(
            "p: one",
            &[("first", false), ("second", false), ("third", false)],
        ),
        upper("p: two", &[("fourth", false), ("fifth", false)]),
    ]);
    let mut runner = Fake::new(|command| match command[0].as_str() {
        "second" | "fourth" => catches(&context, "m::the_test"),
        _ => passes(),
    });
    let (status, _) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 0);
    // Each run that failed the meant test, once without its plant; the second plant's
    // command is another, and is run so too.
    assert_eq!(
        runner.ran,
        ["first", "second", "second", "fourth", "fourth"]
    );
}

#[test]
fn a_failure_that_names_only_tests_the_plant_was_not_meant_for_is_missed_by_meant() {
    let (_folder, context) = checkout();
    let family = family(vec![upper(
        "p: one",
        &[("first", false), ("second", false)],
    )]);
    let mut runner = Fake::new(|command| match command[0].as_str() {
        "first" if planted(&context) => fail(&["m::other", "m::another"]),
        _ => passes(),
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    // The next run is tried for the meant test; nothing is run without the plant.
    assert_eq!(runner.ran, ["first", "second"]);
    assert!(
        said.contains(
            "MISSED-BY-MEANT  p: one (0 s) by m::other (+1), not by the_test\n    \
             failed: m::other\n    failed: m::another\n"
        ),
        "{said}"
    );
    // The meant test ran, in the second run, and passed.
    assert!(!said.contains("no test named"), "{said}");
    assert!(!said.contains("CAUGHT"), "{said}");
    assert!(said.ends_with("0 of 1 plants caught\n"), "{said}");
    assert!(!planted(&context));
}

#[test]
fn a_test_whose_name_holds_the_meant_one_is_not_it() {
    let (_folder, context) = checkout();
    let family = family(vec![upper("p: one", &[("first", false)])]);
    let mut runner = Fake::new(|_| {
        if planted(&context) {
            fail(&["m::not_the_test", "m::the_test_too", "the_tests::x"])
        } else {
            passes()
        }
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert_eq!(runner.ran, ["first"]);
    assert!(
        said.contains("MISSED-BY-MEANT  p: one (0 s) by m::not_the_test (+2), not by the_test\n"),
        "{said}"
    );
}

#[test]
fn a_test_that_printed_the_meant_one_failed_is_no_catch_by_it() {
    let (_folder, context) = checkout();
    let family = family(vec![upper("p: one", &[("first", false)])]);
    // The meant test passed; the other failed, and printed every runner's words for the
    // meant one failing: libtest's footer, its failures list, Node's TAP.
    let printed = "running 2 tests\ntest m::the_test ... ok\ntest m::other ... FAILED\n\n\
                   failures:\n\n---- m::other stdout ----\ntest m::the_test ... FAILED\n\n\
                   failures:\n    m::the_test\n\ntest result: FAILED. 1 passed; 1 failed\n\
                   not ok 1 - m::the_test\n\nfailures:\n    m::other\n\n\
                   test result: FAILED. 1 passed; 1 failed\n";
    let mut runner = Fake::new(|_| {
        if planted(&context) {
            reported(100, printed, &[("m::the_test", false), ("m::other", true)])
        } else {
            passes()
        }
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert!(
        said.contains("MISSED-BY-MEANT  p: one (0 s) by m::other, not by the_test\n"),
        "{said}"
    );
}

#[test]
fn a_plant_whose_meant_test_fails_in_a_later_run_is_caught_there() {
    let (_folder, context) = checkout();
    let family = family(vec![upper(
        "p: one",
        &[("first", false), ("second", false)],
    )]);
    let mut runner = Fake::new(|command| match command[0].as_str() {
        "first" => catches(&context, "m::other"),
        _ => catches(&context, "m::the_test"),
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 0);
    assert_eq!(runner.ran, ["first", "second", "second"]);
    assert!(said.contains("CAUGHT           p: one ("), "{said}");
    assert!(said.contains(" s) by m::the_test\n"), "{said}");
}

#[test]
fn a_plant_meant_for_a_test_no_run_ran_says_so() {
    let (_folder, context) = checkout();
    // A part of a name, as tables held (`feeds.toml`, 2026-10-16): a test whose name ends
    // so, past no boundary of its path, is another.
    let mut fragment = upper("p: fragment", &[("first", false)]);
    fragment.meant = "and_does_not_wait_for_it".into();
    let family = family(vec![fragment]);
    let whole = "m::accepts_a_new_feed_and_does_not_wait_for_it";
    let mut runner = Fake::new(|_| {
        if planted(&context) {
            fails(whole)
        } else {
            passes()
        }
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert!(
        said.contains(&format!(
            "MISSED-BY-MEANT  p: fragment (0 s) by {whole}, not by and_does_not_wait_for_it\n    \
             failed: {whole}\n    no test named and_does_not_wait_for_it ran in its runs\n"
        )),
        "{said}"
    );
    // Every run passed, and none ran it.
    let (_folder, context) = checkout();
    let mut gone = upper("p: gone", &[("first", false)]);
    gone.meant = "a_test_that_is_gone".into();
    let mut runner = Fake::new(|_| passes());
    let (_, said) = go(&context, &family_of(gone), &mut runner);
    assert!(
        said.contains(
            "    2 tests ran, and none of them failed\n    no test named a_test_that_is_gone ran in its runs\n"
        ),
        "{said}"
    );
    // A run with no report says nothing of what ran.
    let (_folder, context) = checkout();
    let mut runner = Fake::new(|_| ended(0, "the smoke passed\n"));
    let (_, said) = go(
        &context,
        &family_of(upper("p: smoke", &[("smoke", false)])),
        &mut runner,
    );
    assert!(!said.contains("no test named"), "{said}");
}

fn family_of(plant: Plant) -> Family {
    family(vec![plant])
}

#[test]
fn a_failure_that_names_no_test_is_run_so_again_and_its_command_twice_without_the_plant() {
    let (_folder, context) = checkout();
    let family = family(vec![upper("p: one", &[("smoke", false)])]);
    let mut runner = Fake::new(|_| {
        if planted(&context) {
            fails_unnamed()
        } else {
            ended(0, "the smoke passed\n")
        }
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 0);
    assert_eq!(runner.ran, ["smoke"; 4]);
    assert!(
        said.contains(
            "CAUGHT           p: one (0 s) by test -p x: ended with 1, not by the_test\n"
        ),
        "{said}"
    );
}

#[test]
fn a_plant_that_every_run_passes_is_missed_and_what_the_last_run_ran_is_said() {
    let (_folder, context) = checkout();
    let family = family(vec![upper(
        "p: one",
        &[("first", false), ("second", false)],
    )]);
    let mut runner = Fake::new(|_| passes());
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert_eq!(runner.ran, ["first", "second"]);
    assert!(
        said.contains("MISSED           p: one (0 s)\n    2 tests ran, and none of them failed\n"),
        "{said}"
    );
    assert!(said.ends_with("0 of 1 plants caught\n"), "{said}");
    assert_eq!(text(&context, "src/a.rs"), "one\ntwo\n");
    // A run with no report: the lines of what it printed that count tests.
    let (_folder, context) = checkout();
    let mut runner =
        Fake::new(|_| ended(0, "running 2 tests\ntest result: ok. 2 passed; 0 failed\n"));
    let (_, said) = go(
        &context,
        &family_of(upper("p: two", &[("first", false)])),
        &mut runner,
    );
    assert!(
        said.contains("    running 2 tests\n    test result: ok. 2 passed; 0 failed\n"),
        "{said}"
    );
}

#[test]
fn a_plant_that_does_not_build_or_does_not_end_is_no_catch_and_says_what_happened() {
    let (_folder, context) = checkout();
    let family = family(vec![
        upper("p: broken", &[("first", false)]),
        upper("p: hung", &[("second", false)]),
    ]);
    let mut runner = Fake::new(|command| {
        if command[0] == "first" {
            return ended(
                101,
                "error[E0432]: unresolved import\nerror: could not compile `x`\n",
            );
        }
        Ok(Execution::Ended(Ran {
            hung: true,
            ..Ran::new(&["cargo", "test", "-p", "x"], None, "", None)
        }))
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert!(said.contains("DOES NOT COMPILE p: broken ("), "{said}");
    assert!(said.contains("error: could not compile `x`"), "{said}");
    assert!(said.contains("HUNG             p: hung ("), "{said}");
    assert!(
        said.contains("    cargo test -p x did not end in 600 s\n"),
        "{said}"
    );
    assert!(said.ends_with("0 of 2 plants caught\n"), "{said}");
}

#[test]
fn a_run_that_wrote_more_than_a_run_may_is_hung_and_said_so_whatever_its_status() {
    let (_folder, context) = checkout();
    let family = family(vec![upper("p: flood", &[("first", false)])]);
    // cf-process ended it for its output: no code, not out of time.
    let mut runner = Fake::new(|_| {
        Ok(Execution::Ended(Ran {
            overflowed: true,
            ..Ran::new(&["cargo", "test", "-p", "x"], None, "flood\n", None)
        }))
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert_eq!(runner.ran, ["first"], "nothing is run again");
    assert!(
        said.contains(
            "HUNG             p: flood (0 s)\n    cargo test -p x wrote more than a run may to a stream, and was ended\n"
        ),
        "{said}"
    );
    assert!(!said.contains("CAUGHT"), "{said}");
}

#[test]
fn a_run_that_fails_twice_without_the_plant_too_leaves_its_plants_not_run_and_is_known() {
    let (_folder, context) = checkout();
    let family = family(vec![
        upper("p: one", &[("page", false)]),
        plant("p: two", &[("src/b.rs", "three", "3")], &[("page", false)]),
    ]);
    // A suite whose program cannot load what it needs: it fails before any test, plant or none.
    let mut runner = Fake::new(|_| {
        ended(
            1,
            "Error: Cannot find module '@playwright/test'\n  code: 'MODULE_NOT_FOUND'\n",
        )
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert_eq!(
        runner.ran,
        ["page", "page", "page", "page"],
        "twice with the first plant, twice without it; the second plant is not planted"
    );
    for name in ["p: one", "p: two"] {
        assert!(
            said.contains(&format!("NOT RUN          {name} (")),
            "{said}"
        );
    }
    assert!(
        said.contains("    page fails without the plant too, twice, so it tells nothing of it:\n"),
        "{said}"
    );
    assert!(said.contains("code: 'MODULE_NOT_FOUND'"), "{said}");
    assert!(!said.contains("CAUGHT"), "{said}");
    assert!(said.ends_with("0 of 2 plants caught\n"), "{said}");
    assert!(!planted(&context));
}

#[test]
fn a_command_known_to_fail_without_a_plant_leaves_the_plant_s_next_run_to_decide() {
    let (_folder, context) = checkout();
    let family = family(vec![
        upper("p: one", &[("page", false)]),
        plant(
            "p: two",
            &[("src/b.rs", "three", "3")],
            &[("page", false), ("first", false)],
        ),
        plant(
            "p: three",
            &[("src/b.rs", "three", "3")],
            &[("page", false)],
        ),
    ]);
    let mut runner = Fake::new(|command| match command[0].as_str() {
        "page" => fails_unnamed(),
        _ => catches(&context, "m::the_test"),
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert_eq!(
        runner.ran,
        ["page", "page", "page", "page", "first", "first"],
        "the second plant's first run is known to fail without it, and is not run; its second is"
    );
    assert!(said.contains("NOT RUN          p: one ("), "{said}");
    assert!(said.contains("CAUGHT           p: two ("), "{said}");
    // Where no other run decides, the plant is not run, said with what the command said.
    assert!(
        said.contains(
            "NOT RUN          p: three (0 s)\n    page fails without the plant too, twice, so it tells nothing of it:\n"
        ),
        "{said}"
    );
    assert!(said.ends_with("1 of 3 plants caught\n"), "{said}");
}

#[test]
fn a_command_that_passed_without_a_plant_is_known_and_not_run_so_again() {
    let (_folder, context) = checkout();
    let family = family(vec![
        upper("p: one", &[("first", false)]),
        plant("p: two", &[("src/b.rs", "three", "3")], &[("first", false)]),
        upper("p: three", &[("smoke", false)]),
        plant(
            "p: four",
            &[("src/b.rs", "three", "3")],
            &[("smoke", false)],
        ),
    ]);
    let mut runner = Fake::new(|command| match command[0].as_str() {
        "smoke" if planted(&context) => fails_unnamed(),
        "smoke" => ended(0, "the smoke passed\n"),
        _ => catches(&context, "m::the_test"),
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 0);
    assert_eq!(
        runner.ran,
        [["first"; 3].as_slice(), &["smoke"; 6]].concat(),
        "once with each plant and once without the first, where the meant test failed; twice \
         with each plant and twice without the first, where the failure named no test"
    );
    assert!(said.ends_with("4 of 4 plants caught\n"), "{said}");
}

#[test]
fn a_command_known_to_pass_once_is_run_once_more_where_a_catch_needs_two() {
    let (_folder, context) = checkout();
    let family = family(vec![
        upper("p: named", &[("first", false)]),
        plant(
            "p: unnamed",
            &[("src/b.rs", "three", "3")],
            &[("first", false)],
        ),
    ]);
    let mut runner = Fake::new(|_| {
        if text(&context, "src/b.rs") != "three\n" {
            fails_unnamed()
        } else {
            catches(&context, "m::the_test")
        }
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 0);
    // The first plant: once with it, once without. The second: twice with it, and once more
    // without, the command having passed once.
    assert_eq!(runner.ran, ["first"; 5]);
    assert!(said.ends_with("2 of 2 plants caught\n"), "{said}");
}

#[test]
fn a_command_that_passed_without_a_plant_is_known_so_only_in_the_state_it_ran_in() {
    let (_folder, context) = checkout();
    fs::write(context.path("fixture.txt"), "kept\n").unwrap();
    let family = family(vec![
        upper("p: one", &[("first", false)]),
        plant("p: two", &[("src/b.rs", "three", "3")], &[("first", false)]),
    ]);
    // A fixture the first run without a plant changes on its way: the meant test fails
    // from then on, plant or none.
    let mut runner = Fake::new(|_| {
        if planted(&context) || text(&context, "fixture.txt") == "broken\n" {
            return fails("m::the_test");
        }
        fs::write(context.path("fixture.txt"), "broken\n").unwrap();
        passes()
    });
    runner.state_of = Some(context.path("fixture.txt"));
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    // The second plant's command is run without it again, in the state the fixture left.
    assert_eq!(runner.ran, ["first"; 5]);
    assert!(said.contains("CAUGHT           p: one ("), "{said}");
    assert!(said.contains("NOT RUN          p: two ("), "{said}");
}

#[test]
fn a_run_with_the_plant_that_changed_the_checkout_leaves_the_plant_unstable_once_it_is_put_back() {
    let (_folder, context) = checkout();
    fs::write(context.path("fixture.txt"), "stale\n").unwrap();
    let family = family(vec![
        upper("p: named", &[("first", false)]),
        plant(
            "p: unnamed",
            &[("src/b.rs", "three", "3")],
            &[("smoke", false)],
        ),
    ]);
    // A test that mends a stale fixture it reads, writing it each time it runs with the
    // plant in, and fails then; the checkout it leaves passes without the plant, and the
    // one before did not.
    let mut mends = 0;
    let mut runner = Fake::new(|command| {
        let mended = text(&context, "fixture.txt").starts_with("mended");
        if planted(&context) {
            mends += 1;
            fs::write(context.path("fixture.txt"), format!("mended {mends}\n")).unwrap();
            return match command[0].as_str() {
                "smoke" => fails_unnamed(),
                _ => fails("m::the_test"),
            };
        }
        if mended {
            passes()
        } else {
            fails("m::the_test")
        }
    });
    runner.state_of = Some(context.path("fixture.txt"));
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert_eq!(
        runner.ran,
        ["first", "smoke", "smoke"],
        "nothing is run without the plant in a checkout it changed"
    );
    assert!(
        said.contains(
            "UNSTABLE         p: named (0 s)\n    first changed the checkout: once the plant is put back, it is not as it was before it\n"
        ),
        "{said}"
    );
    assert!(
        said.contains("UNSTABLE         p: unnamed (0 s)\n    smoke changed the checkout"),
        "{said}"
    );
    assert!(
        said.ends_with("0 of 2 plants caught, 2 unstable\n"),
        "{said}"
    );
    assert!(!planted(&context));
}

#[test]
fn what_a_plant_needs_first_or_a_build_again_of_cf_has_every_command_run_again_without_a_plant() {
    let (_folder, context) = checkout();
    let mut needs = plant(
        "p: needs",
        &[("src/b.rs", "three", "3")],
        &[("first", false)],
    );
    needs.prepare = vec![vec!["prepare".into()]];
    let family = family(vec![
        upper("p: one", &[("first", false)]),
        needs,
        upper("p: builds", &[("first", true)]),
        plant(
            "p: after",
            &[("src/b.rs", "three", "3")],
            &[("first", false)],
        ),
    ]);
    let mut runner = Fake::new(|command| match command[0].as_str() {
        "prepare" | "rebuild-cf" => ended(0, ""),
        "cargo" => built(true),
        _ => catches(&context, "m::the_test"),
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 0, "{said}");
    assert_eq!(
        runner.ran,
        [
            "first",
            "first",
            // What the plant needs first is done: the command is run without it again.
            "prepare",
            "first",
            "first",
            // Known to pass, until the native cf is built again.
            PREFLIGHT_WORDS,
            "first",
            "rebuild-cf",
            "first",
            "first",
        ]
    );
}

#[test]
fn what_a_plant_needs_first_has_every_command_run_again_without_a_plant_though_it_failed() {
    let (_folder, context) = checkout();
    let mut needs = plant(
        "p: needs",
        &[("src/b.rs", "three", "3")],
        &[("first", false)],
    );
    needs.prepare = vec![vec!["prepare".into()]];
    let family = family(vec![
        upper("p: one", &[("first", false)]),
        needs,
        plant(
            "p: after",
            &[("src/b.rs", "three", "3")],
            &[("first", false)],
        ),
    ]);
    // What a plant needs first may change what the commands run on, and fail after it did.
    let mut runner = Fake::new(|command| match command[0].as_str() {
        "prepare" => ended(2, "export failed half way\n"),
        _ => catches(&context, "m::the_test"),
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1, "{said}");
    assert_eq!(
        runner.ran,
        ["first", "first", "prepare", "first", "first"],
        "the command is run without a plant again after the prepare, which failed"
    );
    assert!(said.contains("DOES NOT APPLY   p: needs ("), "{said}");
    assert!(said.ends_with("2 of 3 plants caught\n"), "{said}");
}

/// A runner that answers the runs of `command` in turn by `answers`: `true` fails naming
/// no test, `false` passes.
fn in_turn(answers: &[bool]) -> impl FnMut(&[String]) -> Answer + '_ {
    let mut next = answers.iter();
    move |_| match next.next() {
        Some(true) => fails_unnamed(),
        Some(false) => passes(),
        None => panic!("asked once more than the test answers"),
    }
}

#[test]
fn a_run_that_fails_the_meant_test_and_whose_command_fails_once_without_the_plant_is_unstable() {
    let (_folder, context) = checkout();
    let family = family(vec![upper("p: one", &[("first", false)])]);
    // With the plant, the meant test failed; without it, the command failed, then passed.
    let mut answers = [fails("m::the_test"), fails("m::the_test"), passes()].into_iter();
    let mut runner = Fake::new(|_| answers.next().expect("one answer more"));
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert_eq!(runner.ran, ["first"; 3]);
    assert!(
        said.contains(
            "UNSTABLE         p: one (0 s)\n    first failed without the plant, then passed:\n"
        ),
        "{said}"
    );
}

#[test]
fn a_run_that_fails_with_the_plant_only_once_in_two_is_unstable_neither_caught_nor_missed() {
    let (_folder, context) = checkout();
    let family = family(vec![upper("p: one", &[("first", false)])]);
    let mut runner = Fake::new(in_turn(&[true, false]));
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert_eq!(runner.ran, ["first", "first"]);
    assert!(
        said.contains(
            "UNSTABLE         p: one (0 s)\n    first failed with the plant, then passed\n"
        ),
        "{said}"
    );
    assert!(
        said.ends_with("0 of 1 plants caught, 1 unstable\n"),
        "{said}"
    );
    assert!(!planted(&context));
}

#[test]
fn a_run_that_passes_without_the_plant_once_in_two_is_unstable_and_measured_again_for_the_next() {
    let (_folder, context) = checkout();
    let family = family(vec![
        upper("p: one", &[("first", false)]),
        upper("p: two", &[("first", false)]),
    ]);
    // Twice with the first plant, then without it: passed, then failed. The second plant's
    // command is not known, and is run without it twice more: passed both times.
    let answers = [true, true, false, true, true, true, false, false];
    let mut runner = Fake::new(in_turn(&answers));
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert_eq!(runner.ran.len(), answers.len());
    assert!(
        said.contains(
            "UNSTABLE         p: one (0 s)\n    first passed without the plant, then failed:\n"
        ),
        "{said}"
    );
    assert!(said.contains("CAUGHT           p: two ("), "{said}");
    assert!(
        said.ends_with("1 of 2 plants caught, 1 unstable\n"),
        "{said}"
    );
}

#[test]
fn a_run_that_fails_without_the_plant_is_run_so_again_before_it_is_known_to_fail() {
    let (_folder, context) = checkout();
    let family = family(vec![upper("p: one", &[("first", false)])]);
    // Twice with the plant; without it, failed, then passed: unstable, not NOT RUN.
    let mut runner = Fake::new(in_turn(&[true, true, true, false]));
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert!(
        said.contains(
            "UNSTABLE         p: one (0 s)\n    first failed without the plant, then passed:\n"
        ),
        "{said}"
    );
}

#[test]
fn a_failed_test_said_by_a_run_that_exited_with_success_is_no_catch() {
    let (_folder, context) = checkout();
    let family = family(vec![upper("p: one", &[("first", false)])]);
    let mut runner = Fake::new(|_| {
        reported(
            0,
            "running 1 test\ntest a::prints ... ok\n  1 failed\n    a.spec.mjs:1:1 › example \ntest b::c ... FAILED\n",
            &[("a::prints", false), ("b::c", true)],
        )
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert!(said.contains("MISSED           p: one ("), "{said}");
}

/// A test of Node's: its name, the file it is in, and whether it failed.
fn node_test(name: &str, file: &str, failed: bool) -> Test {
    Test {
        name: name.to_owned(),
        file: Some(file.to_owned()),
        failed,
    }
}

#[test]
fn a_run_that_ran_no_test_that_could_see_the_plant_is_broken_whatever_test_failed_beside_it() {
    let (_folder, context) = checkout();
    let family = family(vec![
        upper("p: node", &[("node", false)]),
        upper("p: page", &[("page", false)]),
    ]);
    let node = ["node", "--test", "tests/a.test.mjs", "tests/b.test.mjs"];
    let page = [
        "node",
        "app/node_modules/@playwright/test/cli.js",
        "test",
        "a.spec.mjs",
    ];
    let mut runner = Fake::new(|command| match command[0].as_str() {
        // The test the plant was meant for failed, in one file; the other failed whole
        // and ran none of its own.
        "node" => ended_by(
            &node,
            1,
            "ℹ tests 2\nℹ pass 0\nℹ fail 2\n",
            Some(Results {
                tests: vec![
                    node_test("tests/a.test.mjs", "/checkout/tests/a.test.mjs", true),
                    node_test("the_test", "/checkout/tests/b.test.mjs", true),
                ],
                outside: Vec::new(),
            }),
        ),
        // As Playwright 1.63 says a spec it could not import.
        _ => ended_by(
            &page,
            1,
            "",
            Some(Results {
                tests: Vec::new(),
                outside: vec![
                    "Error: Cannot find module './a.mjs'".to_owned(),
                    "Error: No tests found.".to_owned(),
                ],
            }),
        ),
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert_eq!(runner.ran, ["node", "page"], "none is run again");
    assert!(
        said.contains(
            "BROKEN           p: node (0 s)\n    the test file tests/a.test.mjs ran no test\n"
        ),
        "{said}"
    );
    assert!(
        said.contains(
            "BROKEN           p: page (0 s)\n    Playwright: an error outside every test: Error: Cannot find module './a.mjs'\n"
        ),
        "{said}"
    );
    assert!(said.ends_with("0 of 2 plants caught\n"), "{said}");
}

#[test]
fn a_node_test_file_the_plant_ended_after_tests_of_it_ran_is_a_catch_confirmed_as_one_that_names_no_test(
) {
    let (_folder, context) = checkout();
    let family = family(vec![upper("p: crash", &[("node", false)])]);
    // As Node 26 says a file whose second test called `process.exit(3)` after a wait: the
    // one before it passed, then the file failed whole.
    let node = ["node", "--test", "tests/late.test.mjs"];
    let file = "/checkout/tests/late.test.mjs";
    let mut runner = Fake::new(|_| {
        let crashed = planted(&context);
        let mut tests = vec![node_test("passes first", file, false)];
        if crashed {
            tests.push(node_test("tests/late.test.mjs", file, true));
        }
        ended_by(
            &node,
            i32::from(crashed),
            "",
            Some(Results {
                tests,
                outside: Vec::new(),
            }),
        )
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 0);
    assert_eq!(runner.ran, ["node"; 4]);
    assert!(
        said.contains("CAUGHT           p: crash (0 s) by tests/late.test.mjs, not by the_test\n"),
        "{said}"
    );
}

#[test]
fn a_build_nextest_says_failed_does_not_compile_and_a_test_that_quotes_one_is_caught() {
    let (_folder, context) = checkout();
    let family = family(vec![
        upper("p: broken", &[("broken", false)]),
        upper("p: quoted", &[("quoted", false)]),
        upper("p: node", &[("node", false)]),
        upper("p: page", &[("page", false)]),
    ]);
    let quoted = "Error: could not compile the response\nerror[E0308]: mismatched types\n\
                  error: could not compile `x`\n  1 error was not a part of any test, see above for details\n";
    let node = ["node", "--test", "tests/a.test.mjs"];
    let page = [
        "node",
        "app/node_modules/@playwright/test/cli.js",
        "test",
        "a.spec.mjs",
    ];
    let mut runner = Fake::new(|command| {
        let failing = |name: &str| Some(report(&[(name, planted(&context))]));
        let code = i32::from(planted(&context));
        match command[0].as_str() {
            // nextest's status for a build that failed, whatever else it printed.
            "broken" => reported(101, "test a::fails ... FAILED\n", &[]),
            // A test that fails quoting a broken build, where nextest says a test failed.
            "quoted" => ended_by(
                &["cargo", "test", "-p", "x"],
                code * 100,
                quoted,
                failing("m::the_test"),
            ),
            // As much said by Node's test, and by Playwright's.
            "node" => ended_by(&node, code, quoted, failing("the_test")),
            _ => ended_by(&page, code, quoted, failing("a group › the_test")),
        }
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert!(said.contains("DOES NOT COMPILE p: broken ("), "{said}");
    for name in ["quoted", "node", "page"] {
        assert!(
            said.contains(&format!("CAUGHT           p: {name} (")),
            "{said}"
        );
    }
    assert!(said.ends_with("3 of 4 plants caught\n"), "{said}");
}

#[test]
fn a_run_whose_tests_build_cf_is_built_from_the_plant_first_and_a_build_cargo_says_failed_does_not_compile(
) {
    let (_folder, context) = checkout();
    let family = family(vec![upper("p: one", &[("rig", true)])]);
    let mut runner = Fake::new(|command| match command[0].as_str() {
        "cargo" => built(!planted(&context)),
        "rebuild-cf" => ended(0, ""),
        // The rig's tests, which would fail with the build, and name the meant test.
        _ => fails("rig::the_test"),
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert_eq!(
        runner.ran,
        [PREFLIGHT_WORDS, "rebuild-cf"],
        "built from the plant, which puts nothing in bin/, and the native cf built again once \
         it is put back; the rig is never run"
    );
    assert!(said.contains("DOES NOT COMPILE p: one ("), "{said}");
}

#[test]
fn a_build_before_the_rig_that_ends_otherwise_than_cargo_saying_it_failed_is_not_built_or_hung() {
    let (_folder, context) = checkout();
    let family = family(vec![
        upper("p: locked", &[("rig", true)]),
        upper("p: slow", &[("rig", true)]),
    ]);
    let mut builds = 0;
    let mut runner = Fake::new(|command| match command[0].as_str() {
        "cargo" => {
            builds += 1;
            if builds == 1 {
                // Built, then something after the build failed, which says nothing structured.
                return ended(
                    101,
                    "{\"reason\":\"build-finished\",\"success\":true}\nerror: failed to sign\n",
                );
            }
            let words: Vec<&str> = PREFLIGHT.to_vec();
            Ok(Execution::Ended(Ran {
                hung: true,
                ..Ran::new(&words, None, "", None)
            }))
        }
        "rebuild-cf" => ended(0, ""),
        _ => fails("rig::the_test"),
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert!(said.contains("NOT BUILT        p: locked ("), "{said}");
    assert!(said.contains("error: failed to sign"), "{said}");
    assert!(said.contains("HUNG             p: slow ("), "{said}");
    assert!(
        said.contains(&format!("    {PREFLIGHT_WORDS} did not end in 600 s\n")),
        "{said}"
    );
    assert!(!said.contains("CAUGHT"), "{said}");
    assert!(said.ends_with("0 of 2 plants caught\n"), "{said}");
}

#[test]
fn a_stop_between_the_build_before_the_rig_and_the_rig_puts_everything_back_and_installs_nothing() {
    let (_folder, context) = checkout();
    let family = family(vec![upper("p: one", &[("rig", true)])]);
    let mut runner = Fake::new(|command| match command[0].as_str() {
        "cargo" => built(true),
        _ => Ok(Execution::Stopped),
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 130);
    // The build from the plant is cargo's own, into its build folder: no `build-cf`, so
    // nothing of the plant is in bin/ to be built again.
    assert_eq!(runner.ran, [PREFLIGHT_WORDS, "rig"]);
    assert!(
        said.ends_with("stopped: every file is as it was\n"),
        "{said}"
    );
    assert!(!planted(&context));
}

#[test]
fn a_command_that_cannot_be_started_leaves_its_plant_not_run_and_the_others_go_on() {
    let (_folder, context) = checkout();
    let family = family(vec![
        upper("p: one", &[("missing", false)]),
        upper("p: two", &[("second", false)]),
    ]);
    let mut runner = Fake::new(|command| match command[0].as_str() {
        "missing" => Err(Error::Process(crate::process::Failure::NotFound {
            program: "missing".into(),
        })),
        _ => catches(&context, "m::the_test"),
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert_eq!(runner.ran, ["missing", "second", "second"]);
    assert!(
        said.contains(
            "NOT RUN          p: one (0 s)\n    missing could not be started: `missing` was not found"
        ),
        "{said}"
    );
    assert!(said.contains("CAUGHT           p: two ("), "{said}");
    assert!(said.ends_with("1 of 2 plants caught\n"), "{said}");
    assert!(!planted(&context));
}

#[test]
fn a_plant_that_does_not_apply_is_named_and_counted_and_the_others_go_on() {
    let (_folder, context) = checkout();
    let family = family(vec![
        plant(
            "p: gone",
            &[("src/a.rs", "absent", "x")],
            &[("first", false)],
        ),
        plant("p: twice", &[("src/a.rs", "o", "0")], &[("first", false)]),
        upper("p: fine", &[("second", false)]),
    ]);
    let mut runner = Fake::new(|_| catches(&context, "m::the_test"));
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert_eq!(
        runner.ran, ["second"; 2],
        "a plant that does not apply runs nothing"
    );
    assert!(
        said.contains(
            "DOES NOT APPLY   p: gone (0 s)\n    p: gone: src/a.rs holds \"absent\" 0 times, not once\n"
        ),
        "{said}"
    );
    assert!(
        said.contains("    p: twice: src/a.rs holds \"o\" 2 times, not once\n"),
        "{said}"
    );
    assert!(said.ends_with("1 of 3 plants caught\n"), "{said}");
}

#[test]
fn what_a_plant_takes_as_given_is_looked_for_before_anything_is_run() {
    let (_folder, context) = checkout();
    let mut needs = upper("p: needs", &[("first", false)]);
    needs.requires = vec!["target/built.app".into()];
    needs.prepare = vec![vec!["prepare".into()]];
    let family = family(vec![needs.clone()]);
    let mut runner = Fake::new(|_| fails("m::the_test"));
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert!(runner.ran.is_empty(), "{:?}", runner.ran);
    assert!(
        said.contains("p: needs: target/built.app is not there"),
        "{said}"
    );

    fs::create_dir_all(context.path("target")).unwrap();
    fs::write(context.path("target/built.app"), "").unwrap();
    let mut runner = Fake::new(|command| match command[0].as_str() {
        "prepare" => ended(0, ""),
        _ => catches(&context, "m::the_test"),
    });
    let (status, _) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 0);
    assert_eq!(
        runner.ran,
        ["prepare", "first", "first"],
        "what is to be done first is done first"
    );
}

#[test]
fn a_command_a_plant_needs_first_must_succeed_or_the_plant_is_not_planted() {
    let (_folder, context) = checkout();
    let mut needs = upper("p: needs", &[("first", false)]);
    needs.prepare = vec![vec!["prepare".into()]];
    let family = family(vec![needs]);
    let mut runner = Fake::new(|_| ended(2, "export failed: no tag\n"));
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert_eq!(runner.ran, ["prepare"]);
    assert!(
        said.contains("p: needs: prepare failed\nexport failed: no tag"),
        "{said}"
    );
    assert_eq!(text(&context, "src/a.rs"), "one\ntwo\n");
}

#[test]
fn the_native_cf_is_built_again_after_a_plant_whose_runs_built_it_from_planted_sources_only() {
    let (_folder, context) = checkout();
    let mut by_plant = upper("p: three", &[("fourth", false)]);
    by_plant.builds = true;
    let family = family(vec![
        // Caught by its first run, which builds nothing: the second never ran.
        upper("p: one", &[("first", false), ("second", true)]),
        upper("p: two", &[("third", true)]),
        by_plant,
        upper("p: four", &[("fifth", false)]),
    ]);
    let mut seen = Vec::new();
    let mut runner = Fake::new(|command| {
        if command[0] == "rebuild-cf" || command[0] == "cargo" {
            // What it builds from is what the sources are.
            seen.push((command[0].clone(), text(&context, "src/a.rs")));
            return built(true);
        }
        catches(&context, "m::the_test")
    });
    let (status, _) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 0);
    // A run that builds is built from the plant first, by cargo alone, and the native cf
    // built again once it is back.
    let two = |run: &str| [run; 2].map(String::from).to_vec();
    let builds = |run: &str| {
        let mut runs = vec![PREFLIGHT_WORDS.to_owned()];
        runs.extend(two(run));
        runs.push("rebuild-cf".to_owned());
        runs
    };
    let expected: Vec<String> = [
        two("first"),
        builds("third"),
        builds("fourth"),
        two("fifth"),
    ]
    .concat();
    assert_eq!(runner.ran, expected);
    let planted_then_back = [("cargo", "ONE\ntwo\n"), ("rebuild-cf", "one\ntwo\n")];
    let expected_seen: Vec<(String, String)> = [planted_then_back, planted_then_back]
        .concat()
        .into_iter()
        .map(|(program, source)| (program.to_owned(), source.to_owned()))
        .collect();
    assert_eq!(seen, expected_seen);
}

#[test]
fn a_build_that_cannot_be_made_again_is_said_and_counted() {
    let (_folder, context) = checkout();
    let family = family(vec![upper("p: one", &[("first", true)])]);
    let mut runner = Fake::new(|command| match command[0].as_str() {
        // Built from the plant; not again once it is back.
        "cargo" => built(true),
        "rebuild-cf" => ended(1, "error: the build is broken\n"),
        _ => catches(&context, "m::the_test"),
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert!(
        said.contains("the native cf of bin/ is not built again:\nerror: the build is broken\n"),
        "{said}"
    );
    assert!(said.ends_with("0 of 1 plants caught\n"), "{said}");
}

/// What `check` answered, and what it said.
fn checked(context: &Context, plants: &[Plant], runner: &mut dyn Runner) -> (i32, String) {
    let chosen: Vec<&Plant> = plants.iter().collect();
    let mut out = Vec::new();
    let status = check(context, &chosen, runner, &mut out).unwrap();
    (status, String::from_utf8(out).unwrap())
}

#[test]
fn to_check_is_to_apply_every_plant_without_planting_any_and_to_count_them() {
    let (_folder, context) = checkout();
    let plants = [
        upper("p: one", &[("first", false)]),
        plant(
            "p: two",
            &[("src/b.rs", "three", "3")],
            &[("second", false)],
        ),
    ];
    let mut runner = Fake::new(|_| fails("m::the_test"));
    let (status, said) = checked(&context, &plants, &mut runner);
    assert_eq!((status, said.as_str()), (0, "2 plants apply\n"));
    assert!(runner.ran.is_empty());
    assert_eq!(text(&context, "src/a.rs"), "one\ntwo\n");
}

#[test]
fn to_check_names_every_plant_that_does_not_apply_not_the_first_alone() {
    let (_folder, context) = checkout();
    let mut needs = upper("p: needs", &[("first", false)]);
    needs.prepare = vec![vec!["prepare".into()]];
    let plants = [
        plant(
            "p: gone",
            &[("src/a.rs", "absent", "x")],
            &[("first", false)],
        ),
        upper("p: fine", &[("first", false)]),
        plant(
            "p: also gone",
            &[("src/b.rs", "absent", "x")],
            &[("first", false)],
        ),
        needs,
    ];
    let mut runner = Fake::new(|_| passes());
    let (status, said) = checked(&context, &plants, &mut runner);
    assert_eq!(status, 1);
    assert_eq!(
        said,
        "p: gone: src/a.rs holds \"absent\" 0 times, not once\n\
         p: also gone: src/b.rs holds \"absent\" 0 times, not once\n\
         2 of 4 plants apply\n"
    );
    // A plant that needs a command first has it run, here too: its text may not be there until it is.
    assert_eq!(runner.ran, ["prepare"]);
}

#[test]
fn to_check_is_stopped_where_a_command_is() {
    let (_folder, context) = checkout();
    let mut needs = upper("p: needs", &[("first", false)]);
    needs.prepare = vec![vec!["prepare".into()]];
    let mut runner = Fake::new(|_| Ok(Execution::Stopped));
    assert_eq!(checked(&context, &[needs], &mut runner).0, 130);
}

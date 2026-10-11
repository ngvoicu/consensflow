use std::collections::VecDeque;
use std::time::Duration;

use cf_base::env::Env;

use super::*;
use crate::plants::kit::{Edit, Run};

mod restore;

/// What a command is answered: how it ended, or that it could not be started.
type Answer = Result<Execution, Error>;

/// A runner that answers each command by `answer`, and says what it was asked.
struct Fake<A: FnMut(&[String]) -> Answer> {
    answer: A,
    ran: Vec<String>,
    stops: VecDeque<bool>,
}

impl<A: FnMut(&[String]) -> Answer> Fake<A> {
    fn new(answer: A) -> Self {
        Self {
            answer,
            ran: Vec::new(),
            stops: VecDeque::new(),
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
}

fn ended(code: i32, output: &str) -> Answer {
    Ok(Execution::Ended(Ran {
        command: vec!["cargo".into(), "test".into(), "-p".into(), "x".into()],
        code: Some(code),
        hung: false,
        output: output.to_owned(),
    }))
}

/// A run in which the test `name` failed.
fn fails(name: &str) -> Answer {
    ended(101, &format!("running 1 test\ntest {name} ... FAILED\n"))
}

/// A run in which every test passed.
fn passes() -> Answer {
    ended(
        0,
        "running 2 tests\ntest a ... ok\ntest b ... ok\n\ntest result: ok. 2 passed; 0 failed\n",
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
        fails("m::the_test")
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 0);
    assert_eq!(seen, ["ONE\ntwo\n"]);
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
        "second" | "fourth" => fails("m::the_test"),
        _ => passes(),
    });
    let (status, _) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 0);
    assert_eq!(runner.ran, ["first", "second", "fourth"]);
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
    assert!(said.contains("MISSED           p: one ("), "{said}");
    assert!(
        said.contains("    running 2 tests\n    test result: ok. 2 passed; 0 failed\n"),
        "{said}"
    );
    assert!(said.ends_with("0 of 1 plants caught\n"), "{said}");
    assert_eq!(text(&context, "src/a.rs"), "one\ntwo\n");
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
            command: vec!["cargo".into(), "test".into(), "-p".into(), "x".into()],
            code: None,
            hung: true,
            output: String::new(),
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
    let mut runner = Fake::new(|_| fails("m::the_test"));
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 1);
    assert_eq!(
        runner.ran,
        ["second"],
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
        _ => fails("m::the_test"),
    });
    let (status, _) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 0);
    assert_eq!(
        runner.ran,
        ["prepare", "first"],
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
        if command[0] == "rebuild-cf" {
            // What it builds from is what the sources are.
            seen.push(text(&context, "src/a.rs"));
            return ended(0, "");
        }
        fails("m::the_test")
    });
    let (status, _) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 0);
    assert_eq!(
        runner.ran,
        [
            "first",
            "third",
            "rebuild-cf",
            "fourth",
            "rebuild-cf",
            "fifth"
        ]
    );
    assert_eq!(seen, ["one\ntwo\n", "one\ntwo\n"]);
}

#[test]
fn a_build_that_cannot_be_made_again_is_said_and_counted() {
    let (_folder, context) = checkout();
    let family = family(vec![upper("p: one", &[("first", true)])]);
    let mut runner = Fake::new(|command| {
        if command[0] == "rebuild-cf" {
            return ended(1, "error: the build is broken\n");
        }
        fails("m::the_test")
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

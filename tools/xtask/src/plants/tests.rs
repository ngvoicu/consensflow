use std::fs;
use std::time::Duration;

use cf_base::env::Env;

use super::kit::Plant;
use super::runs::Execution;
use super::*;

fn line(words: &str) -> Vec<OsString> {
    words.split_whitespace().map(OsString::from).collect()
}

fn read(words: &str) -> Result<Request, Failure> {
    Request::read(&line(words))
}

#[test]
fn the_command_line_is_a_family_the_words_of_names_and_where_it_is_given_a_check() {
    let request = read("cli parser --check dispatch").unwrap();
    assert_eq!(request.family.as_deref(), Some("cli"));
    assert_eq!(request.words, ["parser", "dispatch"]);
    assert!(request.check);

    let plain = read("daemon").unwrap();
    assert_eq!(plain.family.as_deref(), Some("daemon"));
    assert!(plain.words.is_empty() && !plain.check);
    assert_eq!(read("--check").unwrap().family, None);
}

#[test]
fn an_option_that_is_not_the_check_is_refused_for_a_typo_would_run_every_plant() {
    let refused = read("cli --chek").unwrap_err();
    assert!(matches!(&refused, Failure::Usage(said) if said == "plants takes --check, not --chek"));
    assert!(matches!(
        read("cli parser --").unwrap_err(),
        Failure::Usage(_)
    ));
    // A single dash is a word of a name.
    assert_eq!(read("cli -h").unwrap().words, ["-h"]);
}

#[test]
fn a_help_is_asked_wherever_it_is_because_a_family_comes_before_what_xtask_answers_help_for() {
    // `npm run plants:cli -- --help` and the test that every npm script names a command.
    for words in ["--help", "cli --help", "cli parser --check --help"] {
        assert!(read(words).unwrap().help, "{words}");
    }
    assert!(!read("cli parser --check").unwrap().help);
}

#[cfg(unix)]
#[test]
fn words_that_are_not_text_are_refused() {
    use std::os::unix::ffi::OsStringExt;

    let refused = Request::read(&[OsString::from_vec(vec![0xff, 0xfe])]).unwrap_err();
    assert!(matches!(refused, Failure::Usage(_)));
}

/// A runner for what runs nothing.
struct Nothing;

impl Runner for Nothing {
    fn run(&mut self, command: &[String]) -> Result<Execution, Error> {
        panic!("nothing was to be run, and {command:?} was");
    }

    fn stopped(&mut self) -> bool {
        false
    }

    fn state(&mut self) -> Option<String> {
        panic!("nothing was to be run, and the state was asked for");
    }
}

/// A checkout with the family `demo` of two plants and the file they plant, whose kit
/// says `extra` as well.
fn checkout_with(extra: &str) -> (tempfile::TempDir, Context) {
    let folder = tempfile::tempdir().unwrap();
    let context = Context {
        root: folder.path().to_path_buf(),
        env: Env::default(),
    };
    let write = |relative: &str, text: &str| {
        let path = context.path(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    };
    write("src/a.rs", "one\ntwo\n");
    write(
        "tests/plants/demo/kit.toml",
        &format!("run_limit_minutes = 1\ntables = [\"all\"]\n{extra}"),
    );
    write(
        "tests/plants/demo/all.toml",
        "[[plant]]\nname = \"demo: one\"\nmeant = \"m\"\nruns = [[\"cargo\", \"test\"]]\n\n\
         [[plant.edit]]\nfile = \"src/a.rs\"\nfind = \"one\"\nreplace = \"1\"\n\n\
         [[plant]]\nname = \"demo: two\"\nmeant = \"m\"\nruns = [[\"cargo\", \"test\"]]\n\n\
         [[plant.edit]]\nfile = \"src/a.rs\"\nfind = \"gone\"\nreplace = \"2\"\n",
    );
    (folder, context)
}

/// A checkout with the family `demo` as [`checkout_with`] makes it, with nothing more in its kit.
fn checkout() -> (tempfile::TempDir, Context) {
    checkout_with("")
}

/// What the command answered to `words` in `context`, and what it said.
fn command(context: &Context, words: &str) -> (Result<i32, Failure>, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut console = Console {
        out: &mut out,
        err: &mut err,
    };
    let status = plant(context, &line(words), &mut console, |_| Ok(Nothing));
    (status, String::from_utf8(out).unwrap())
}

#[test]
fn a_family_the_data_does_not_have_is_refused_with_the_ones_it_has() {
    let (_folder, context) = checkout();
    for words in ["", "elsewhere", "--check"] {
        let (status, _) = command(&context, words);
        assert!(
            matches!(&status, Err(Failure::Usage(said)) if said == "plants takes a family: demo"),
            "{words}: {status:?}"
        );
    }
}

#[test]
fn the_help_says_how_the_command_is_used_and_the_families_the_data_has_and_runs_nothing() {
    let (_folder, context) = checkout();
    let expected = format!("Usage: cargo xtask plants {USAGE}\n\n{ABOUT}\n\nThe families: demo.\n");
    for words in [
        "--help",
        "demo --help",
        "demo one --check --help",
        "elsewhere --help",
    ] {
        let (status, said) = command(&context, words);
        assert_eq!(status.unwrap(), 0, "{words}");
        assert_eq!(said, expected, "{words}");
    }
}

#[test]
fn a_word_that_no_plant_has_in_its_name_is_refused_as_the_words_of_a_command_are() {
    let (_folder, context) = checkout();
    let (status, said) = command(&context, "demo one nonsense --check");
    assert!(
        matches!(&status, Err(Failure::Usage(said)) if said == "no plant of demo has a name that holds `nonsense`"),
        "{status:?}"
    );
    assert!(said.is_empty());
}

#[test]
fn to_check_the_chosen_plants_is_to_say_which_do_not_apply_and_to_fail_for_them() {
    let (_folder, context) = checkout();
    let (status, said) = command(&context, "demo one --check");
    assert_eq!((status.unwrap(), said.as_str()), (0, "1 plants apply\n"));

    let (status, said) = command(&context, "demo --check");
    assert_eq!(status.unwrap(), 1);
    assert_eq!(
        said,
        "demo: two: src/a.rs holds \"gone\" 0 times, not once\n1 of 2 plants apply\n"
    );
    assert_eq!(
        fs::read_to_string(context.path("src/a.rs")).unwrap(),
        "one\ntwo\n"
    );
}

#[test]
fn data_that_cannot_be_read_is_an_error_that_names_the_file() {
    let (_folder, context) = checkout();
    fs::write(context.path("tests/plants/demo/all.toml"), "[[plant]\n").unwrap();
    let (status, _) = command(&context, "demo --check");
    let Err(Failure::Plants(error)) = status else {
        panic!("{status:?}");
    };
    assert!(
        error.to_string().starts_with(&format!(
            "{}",
            context.path("tests/plants/demo/all.toml").display()
        )),
        "{error}"
    );
}

#[test]
fn a_plant_the_kit_lists_as_not_applying_is_known_and_does_not_fail_the_check() {
    let (_folder, context) = checkout_with("known_unapplied = [\"demo: two\"]\n");
    let (status, said) = command(&context, "demo --check");
    assert_eq!(status.unwrap(), 0);
    assert_eq!(
        said,
        "known not to apply: demo: two: src/a.rs holds \"gone\" 0 times, not once\n\
         1 of 2 plants apply, 1 known not to apply\n"
    );
}

#[test]
fn a_plant_the_kit_lists_that_applies_again_fails_the_check_until_the_list_is_emptied() {
    let (_folder, context) = checkout_with("known_unapplied = [\"demo: one\"]\n");
    let (status, said) = command(&context, "demo --check");
    assert_eq!(status.unwrap(), 1);
    assert_eq!(
        said,
        "demo: one: applies, so it is not known not to apply: take it off known_unapplied in kit.toml\n\
         demo: two: src/a.rs holds \"gone\" 0 times, not once\n\
         1 of 2 plants apply\n"
    );
}

#[test]
fn a_name_the_kit_lists_that_no_plant_has_is_a_data_error_that_names_it() {
    let (_folder, context) = checkout_with("known_unapplied = [\"demo: three\"]\n");
    let (status, _) = command(&context, "demo --check");
    let Err(Failure::Plants(error)) = status else {
        panic!("{status:?}");
    };
    assert!(
        error
            .to_string()
            .ends_with("known_unapplied names `demo: three`, which no plant of the family has"),
        "{error}"
    );
}

// What the repository's own plants are held to.

/// The families of the repository, as the driver reads them.
fn repository() -> Vec<kit::Family> {
    let context = Context::new(&Env::default()).unwrap();
    let folder = context.path("tests/plants");
    let found = kit::families(&folder).unwrap();
    found
        .iter()
        .map(|name| Family::load(&folder, name).unwrap())
        .collect()
}

fn plants(families: &[kit::Family]) -> impl Iterator<Item = &Plant> {
    families.iter().flat_map(|family| family.plants.iter())
}

fn runs_of(plant: &Plant) -> impl Iterator<Item = &Vec<String>> {
    plant
        .runs
        .iter()
        .map(|run| &run.command)
        .chain(plant.prepare.iter())
}

#[test]
fn the_repository_has_the_plants_of_three_families_each_read_and_each_a_plant_of_its_own() {
    let families = repository();
    let names: Vec<_> = families.iter().map(|family| family.name.as_str()).collect();
    assert_eq!(names, ["cli", "daemon", "release"]);
    for family in &families {
        assert!(
            family.plants.len() > 100,
            "{}: {}",
            family.name,
            family.plants.len()
        );
        assert!(family.limit >= Duration::from_secs(300), "{}", family.name);
    }
}

#[test]
fn every_command_of_a_plant_starts_a_program_the_driver_has() {
    for plant in plants(&repository()) {
        for command in runs_of(plant) {
            let program = command[0].as_str();
            assert!(
                ["cargo", "node"].contains(&program),
                "{}: {command:?}",
                plant.name
            );
        }
    }
}

#[test]
fn a_test_of_the_xtask_package_whole_is_built_in_the_folder_xtask_is_tested_in() {
    // The xtask that runs the plants is `target/debug/xtask`, and a test of the whole
    // package builds xtask again: it is not to be replaced while it runs.
    let folder = crate::check::XTASK_TARGET;
    for plant in plants(&repository()) {
        for command in runs_of(plant) {
            let words: Vec<_> = command.iter().map(String::as_str).collect();
            let of_xtask = words.windows(2).any(|pair| pair == ["-p", "xtask"]);
            if !of_xtask || words.contains(&"--lib") || words[1] != "test" {
                continue;
            }
            assert!(
                words
                    .windows(2)
                    .any(|pair| pair == ["--target-dir", folder]),
                "{}: {command:?} is built in the folder xtask runs from",
                plant.name
            );
        }
    }
}

#[test]
fn a_plant_that_runs_xtask_plants_nothing_that_xtask_is_built_from() {
    // `cargo xtask …` is the xtask that is running: a plant in what it is built from would
    // be tried against the one that does not have it.
    let closure = [
        "tools/xtask/",
        "tools/cf-release/",
        "crates/cf-base/",
        "crates/cf-process/",
        "crates/cf-portable/",
    ];
    for plant in plants(&repository()) {
        let runs_xtask = runs_of(plant).any(|command| command[..2] == ["cargo", "xtask"]);
        let in_closure = plant
            .edits
            .iter()
            .find(|edit| closure.iter().any(|folder| edit.file.starts_with(folder)));
        if let (true, Some(edit)) = (runs_xtask, in_closure) {
            panic!("{} plants {} and runs cargo xtask", plant.name, edit.file);
        }
    }
}

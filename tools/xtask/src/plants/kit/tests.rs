use std::fs;

use cf_base::env::Env;

use super::*;

/// A checkout of a few files, in a folder that goes with the test.
fn checkout() -> (tempfile::TempDir, Context) {
    let folder = tempfile::tempdir().unwrap();
    let context = Context {
        root: folder.path().to_path_buf(),
        env: Env::default(),
    };
    (folder, context)
}

/// `text` as the file `relative` of the checkout, made folder by folder.
fn put(context: &Context, relative: &str, text: &str) {
    let path = context.path(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

const KIT: &str = r#"
# A family.
run_limit_minutes = 5
rebuild = ["cargo", "xtask", "build-cf", "--offline"]
tables = ["first", "second"]

[run]
goldens = ["cargo", "test", "--offline", "-p", "cf", "--test", "cli_goldens"]
clis = { command = ["cargo", "xtask", "test", "clis", "--offline"], builds = true }
"#;

const FIRST: &str = r#"
[run]
own = ["node", "--test", "tests/x.test.mjs"]

# The first.
[[plant]]
name = "first: one"
meant = "the_test"
runs = ["goldens", "own", ["cargo", "test", "-p", "y"]]
builds = true
prepare = [["cargo", "xtask", "smoke-updater", "--export-bridge"]]
requires = ["app/x"]
platforms = ["macos", "linux"]

[[plant.edit]]
file = "src/a.rs"
find = '''
    if a {
        b
    }'''
replace = "    if a {}\n"

[[plant.edit]]
file = "src/b.rs"
find = "c"
replace = "d"

[[plant]]
name = "first: two"
meant = "another"
runs = ["clis"]

[[plant.edit]]
file = "src/a.rs"
find = "x"
replace = "y"
"#;

const SECOND: &str = r#"
[[plant]]
name = "second: one"
meant = "m"
runs = ["goldens"]

[[plant.edit]]
file = "src/a.rs"
find = "x"
replace = "y"
"#;

/// The family `demo` of a checkout, as `kit`, `first` and `second` write it.
fn demo(kit: &str, first: &str, second: &str) -> (tempfile::TempDir, Context) {
    let (folder, context) = checkout();
    put(&context, "tests/plants/demo/kit.toml", kit);
    put(&context, "tests/plants/demo/first.toml", first);
    put(&context, "tests/plants/demo/second.toml", second);
    (folder, context)
}

fn load(context: &Context) -> Result<Family, Error> {
    Family::load(&context.path("tests/plants"), "demo")
}

fn error_of(result: Result<Family, Error>) -> String {
    result.unwrap_err().to_string()
}

#[test]
fn a_family_is_what_its_folder_says_its_plants_in_the_order_of_its_tables() {
    let (_folder, context) = demo(KIT, FIRST, SECOND);
    let family = load(&context).unwrap();
    assert_eq!(family.name, "demo");
    assert_eq!(family.limit, Duration::from_secs(300));
    assert_eq!(
        family.rebuild,
        Some(
            ["cargo", "xtask", "build-cf", "--offline"]
                .map(String::from)
                .to_vec()
        )
    );
    let found: Vec<_> = family
        .plants
        .iter()
        .map(|plant| (plant.table.as_str(), plant.name.as_str()))
        .collect();
    assert_eq!(
        found,
        [
            ("first", "first: one"),
            ("first", "first: two"),
            ("second", "second: one")
        ]
    );
}

#[test]
fn a_plant_has_what_it_is_meant_by_the_runs_it_names_and_what_it_asks_for_first() {
    let (_folder, context) = demo(KIT, FIRST, SECOND);
    let family = load(&context).unwrap();
    let one = &family.plants[0];
    assert_eq!(one.meant, "the_test");
    assert!(one.builds);
    assert_eq!(
        one.prepare,
        [["cargo", "xtask", "smoke-updater", "--export-bridge"]
            .map(String::from)
            .to_vec()]
    );
    assert_eq!(one.requires, ["app/x"]);
    // The kit's run, the table's own, and one in words; only the kit's second builds.
    let commands: Vec<_> = one.runs.iter().map(|run| run.command.join(" ")).collect();
    assert_eq!(
        commands,
        [
            "cargo test --offline -p cf --test cli_goldens",
            "node --test tests/x.test.mjs",
            "cargo test -p y"
        ]
    );
    assert!(one.runs.iter().all(|run| !run.builds));
    let two = &family.plants[1];
    assert_eq!(two.runs.len(), 1);
    assert!(two.runs[0].builds && !two.builds);
    assert!(two.prepare.is_empty() && two.requires.is_empty());
    // One is for the systems it names, another, naming none, for every one.
    assert_eq!(one.platforms, ["macos", "linux"]);
    assert!(one.runs_on("macos") && one.runs_on("linux") && !one.runs_on("windows"));
    assert!(two.platforms.is_empty());
    assert!(two.runs_on("windows") && two.runs_on("macos"));
}

#[test]
fn a_plant_for_a_system_there_is_none_of_or_for_none_is_an_error_of_its_table() {
    for (platforms, said) in [
        (
            r#"["macos", "darwin"]"#,
            "the plant `second: one` is for `darwin`, which is no system a plant is for: \
             macos, windows, linux",
        ),
        (
            "[]",
            "the plant `second: one` is for no system: name one, or leave its platforms out",
        ),
    ] {
        let second = SECOND.replace(
            "runs = [\"goldens\"]",
            &format!("runs = [\"goldens\"]\nplatforms = {platforms}"),
        );
        let (_folder, context) = demo(KIT, FIRST, &second);
        let error = error_of(load(&context));
        assert!(error.ends_with(said), "{error}");
        assert!(error.contains("second.toml"), "{error}");
    }
}

#[test]
fn text_is_held_as_it_is_written_between_triple_quotes_and_between_double() {
    let first = r#"
[[plant]]
name = "text"
meant = "m"
runs = [["cargo", "test"]]

[[plant.edit]]
file = "f"
find = '''
a\n"quoted"
'''
replace = '''

lead'''

[[plant.edit]]
file = "f"
find = '''one line with 'quotes' and \backslashes'''
replace = "a tab\there"
"#;
    let (_folder, context) = demo(KIT, first, SECOND);
    let edits = &load(&context).unwrap().plants[0].edits;
    // A newline right after the opening quotes is not part of the text; the one before the closing ones is.
    assert_eq!(edits[0].find, "a\\n\"quoted\"\n");
    assert_eq!(edits[0].replace, "\nlead");
    assert_eq!(edits[1].find, "one line with 'quotes' and \\backslashes");
    assert_eq!(edits[1].replace, "a tab\there");
}

#[test]
fn the_tables_are_the_files_of_the_folder_and_no_other() {
    let (_folder, context) = demo(KIT, FIRST, SECOND);
    put(&context, "tests/plants/demo/third.toml", SECOND);
    let said = error_of(load(&context));
    assert!(
        said.contains("third.toml is a table that is not listed"),
        "{said}"
    );

    let (_folder, context) = demo(KIT, FIRST, SECOND);
    fs::remove_file(context.path("tests/plants/demo/second.toml")).unwrap();
    let said = error_of(load(&context));
    assert!(said.contains("the table `second` is listed"), "{said}");

    // What is not data is none of its business.
    let (_folder, context) = demo(KIT, FIRST, SECOND);
    put(&context, "tests/plants/demo/notes.txt", "not a table");
    assert!(load(&context).is_ok());
}

#[test]
fn a_run_is_named_once_and_a_run_that_is_named_nowhere_is_said_with_the_plant_that_wants_it() {
    let own = FIRST.replace("own = [", "goldens = [");
    let said_of_both = error_of(load(&demo(KIT, &own, SECOND).1));
    assert!(
        said_of_both.contains("the run `goldens` is named in kit.toml too"),
        "{said_of_both}"
    );

    let unnamed = SECOND.replace("runs = [\"goldens\"]", "runs = [\"nowhere\"]");
    let said_of_missing = error_of(load(&demo(KIT, FIRST, &unnamed).1));
    assert!(
        said_of_missing.contains("the plant `second: one` runs `nowhere`"),
        "{said_of_missing}"
    );
}

#[test]
fn a_plant_is_named_once_has_a_run_and_says_nothing_the_driver_does_not_know() {
    let twice = SECOND.replace("second: one", "first: two");
    let said_twice = error_of(load(&demo(KIT, FIRST, &twice).1));
    assert!(
        said_twice.contains("the plant `first: two` is also in first.toml"),
        "{said_twice}"
    );

    let none = SECOND.replace("runs = [\"goldens\"]", "runs = []");
    let said_none = error_of(load(&demo(KIT, FIRST, &none).1));
    assert!(
        said_none.contains("`second: one` has no run"),
        "{said_none}"
    );

    let typed = SECOND.replace("meant = \"m\"", "meant = \"m\"\nbuild = true");
    let said_typed = error_of(load(&demo(KIT, FIRST, &typed).1));
    assert!(
        said_typed.contains("second.toml") && said_typed.contains("unknown field `build`"),
        "{said_typed}"
    );
}

#[test]
fn a_family_whose_plants_build_has_a_way_to_build_again() {
    let rebuild = "rebuild = [\"cargo\", \"xtask\", \"build-cf\", \"--offline\"]\n";
    let without = KIT.replace(rebuild, "");
    // A plant that builds (a run of it does, or it says so) has the kit say how to build again.
    let said = error_of(load(&demo(&without, FIRST, SECOND).1));
    assert!(
        said.contains("no `rebuild` says how to build it again"),
        "{said}"
    );

    // One that builds nothing needs none.
    let builds = "clis = { command = [\"cargo\", \"xtask\", \"test\", \"clis\", \"--offline\"], builds = true }\n";
    let plain_kit = without.replace(builds, "");
    let plain_first = FIRST
        .replace("builds = true\n", "")
        .replace("runs = [\"clis\"]", "runs = [\"goldens\"]");
    let (_folder, context) = demo(&plain_kit, &plain_first, SECOND);
    assert!(load(&context).unwrap().rebuild.is_none());
}

#[test]
fn the_families_are_the_folders_with_a_kit() {
    let (_folder, context) = demo(KIT, FIRST, SECOND);
    put(&context, "tests/plants/other/kit.toml", KIT);
    put(&context, "tests/plants/not-a-family/notes.txt", "x");
    assert_eq!(
        families(&context.path("tests/plants")).unwrap(),
        ["demo", "other"]
    );
}

#[test]
fn the_plants_whose_names_hold_a_word_are_chosen_and_a_word_no_name_holds_is_refused() {
    let (_folder, context) = demo(KIT, FIRST, SECOND);
    let family = load(&context).unwrap();
    let names = |words: &[&str]| -> Vec<String> {
        let words: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
        family
            .choose(&words)
            .unwrap()
            .iter()
            .map(|plant| plant.name.clone())
            .collect()
    };
    assert_eq!(names(&[]), ["first: one", "first: two", "second: one"]);
    assert_eq!(names(&["second"]), ["second: one"]);
    assert_eq!(names(&["two", "second"]), ["first: two", "second: one"]);
    assert_eq!(names(&["first: o"]), ["first: one"]);

    let refused = family
        .choose(&["second".to_owned(), "nonsense".to_owned()])
        .unwrap_err();
    assert_eq!(
        refused.to_string(),
        "no plant of demo has a name that holds `nonsense`"
    );
}

/// A plant that makes `edits` and runs nothing: for the text it makes.
fn plant(edits: &[(&str, &str, &str)]) -> Plant {
    Plant {
        table: "t".into(),
        name: "t: a plant".into(),
        meant: "m".into(),
        runs: vec![Run {
            command: vec!["x".into()],
            builds: false,
        }],
        builds: false,
        prepare: Vec::new(),
        requires: Vec::new(),
        platforms: Vec::new(),
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

#[test]
fn the_text_of_a_plant_is_each_file_with_its_edits_made_one_on_the_text_the_other_left() {
    let (_folder, context) = checkout();
    put(&context, "src/a.rs", "let a = 1;\nlet b = 2;\n");
    put(&context, "src/b.rs", "fn b() {}\n");
    let planted = plant(&[
        ("src/a.rs", "a = 1", "a = 10"),
        ("src/b.rs", "{}", "{ todo!() }"),
        // The second edit of a file finds what the first made.
        ("src/a.rs", "a = 10;\nlet b", "a = 10; let b"),
    ])
    .planted(&context)
    .unwrap();
    // The files in the order the plant names them first.
    assert_eq!(
        planted
            .iter()
            .map(|file| file.file.as_str())
            .collect::<Vec<_>>(),
        ["src/a.rs", "src/b.rs"]
    );
    assert_eq!(planted[0].text, "let a = 10; let b = 2;\n");
    assert_eq!(planted[0].original, b"let a = 1;\nlet b = 2;\n");
    assert_eq!(planted[0].path, context.path("src/a.rs"));
    assert_eq!(planted[1].text, "fn b() { todo!() }\n");
    // The files themselves are as they were: this is the text of the plant.
    assert_eq!(
        fs::read_to_string(context.path("src/b.rs")).unwrap(),
        "fn b() {}\n"
    );
}

#[test]
fn text_is_replaced_as_it_is_written_whatever_a_dollar_or_a_backslash_would_mean_elsewhere() {
    let (_folder, context) = checkout();
    put(&context, "f", "x\n");
    let planted = plant(&[("f", "x", "$1 $& \\n ${y}")])
        .planted(&context)
        .unwrap();
    assert_eq!(planted[0].text, "$1 $& \\n ${y}\n");
}

#[test]
fn a_plant_that_does_not_apply_is_named_with_the_file_and_the_text_it_did_not_find_once() {
    let (_folder, context) = checkout();
    put(&context, "src/a.rs", "one\ntwo\ntwo\n");
    let none = plant(&[("src/a.rs", "three \"q\"", "x")])
        .planted(&context)
        .unwrap_err();
    assert_eq!(
        none.to_string(),
        r#"t: a plant: src/a.rs holds "three \"q\"" 0 times, not once"#
    );
    let twice = plant(&[("src/a.rs", "two", "x")])
        .planted(&context)
        .unwrap_err();
    assert_eq!(
        twice.to_string(),
        r#"t: a plant: src/a.rs holds "two" 2 times, not once"#
    );
    // The text the first edit left is what the second looks in.
    let gone = plant(&[("src/a.rs", "one", "1"), ("src/a.rs", "one", "x")])
        .planted(&context)
        .unwrap_err();
    assert!(matches!(gone, Error::NotOnce { found: 0, .. }), "{gone}");
}

#[test]
fn a_file_that_is_not_there_or_is_not_text_is_not_one_a_plant_can_be_made_in() {
    let (_folder, context) = checkout();
    let missing = plant(&[("src/none.rs", "a", "b")])
        .planted(&context)
        .unwrap_err();
    assert!(
        missing.to_string().starts_with("could not read "),
        "{missing}"
    );
    fs::create_dir_all(context.path("src")).unwrap();
    fs::write(context.path("src/binary.rs"), [0xff, 0xfe, 0x00]).unwrap();
    let binary = plant(&[("src/binary.rs", "a", "b")])
        .planted(&context)
        .unwrap_err();
    assert!(
        binary
            .to_string()
            .contains("is not text a plant can be made in"),
        "{binary}"
    );
}

//! The words of `live` read as its doc says: a flag and its value, the words that
//! name harnesses, where cargo's own words and the test program's begin, what is
//! refused and nothing started, and the help a `--help` among them is.

use super::*;

use std::path::PathBuf;

use cf_base::env::Env;

fn context() -> Context {
    Context {
        root: PathBuf::from("checkout"),
        env: Env::default(),
    }
}

/// The tests there are, for the words below.
fn names() -> Vec<String> {
    ["approval", "designer", "input_box", "paste"]
        .map(str::to_owned)
        .to_vec()
}

fn words(line: &str) -> Vec<OsString> {
    line.split_whitespace().map(OsString::from).collect()
}

/// What `cargo xtask live <line>` runs, and the variables it sets.
fn runs(line: &str) -> Result<(String, Vec<(String, String)>), Failure> {
    let asked = parse(&words(line), &names())?;
    let invocation = invocation(&context(), &asked);
    let vars = invocation
        .vars
        .iter()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value
                    .as_ref()
                    .map_or_else(String::new, |value| value.to_string_lossy().into_owned()),
            )
        })
        .collect();
    Ok((invocation.display(), vars))
}

fn var(name: &str, value: &str) -> (String, String) {
    (name.to_owned(), value.to_owned())
}

#[test]
fn a_live_test_is_the_ignored_case_of_its_test_and_runs_only_when_asked_for() {
    for name in ["designer", "approval", "paste"] {
        let (line, vars) = runs(name).unwrap();
        assert_eq!(
            line,
            format!("cargo test -p cf-e2e --test live_{name} -- --ignored --nocapture")
        );
        assert!(vars.is_empty());
    }
    let command = COMMANDS.iter().find(|command| command.words == ["live"]);
    assert!(matches!(
        command.map(|command| &command.run),
        Some(Run::Native(_))
    ));
}

#[test]
fn a_test_is_named_by_its_file_with_dashes_or_underscores() {
    assert_eq!(
        runs("input-box").unwrap().0,
        "cargo test -p cf-e2e --test live_input_box -- --ignored --nocapture"
    );
    assert_eq!(runs("input_box").unwrap().0, runs("input-box").unwrap().0);
}

#[test]
fn words_with_no_dash_name_harnesses_whatever_flags_come_between_them() {
    assert_eq!(
        runs("approval claude pi").unwrap().1,
        [var("CONSENSFLOW_LIVE_HARNESS", "claude,pi")]
    );
    assert_eq!(
        runs("paste claude --harness devin pi").unwrap().1,
        [var("CONSENSFLOW_LIVE_HARNESS", "claude,devin,pi")]
    );
    assert_eq!(
        runs("paste --harness claude,devin").unwrap().1,
        [var("CONSENSFLOW_LIVE_HARNESS", "claude,devin")]
    );
}

#[test]
fn a_flag_has_its_value_and_a_flag_given_more_than_once_has_all_of_them_in_order() {
    assert_eq!(
        runs("paste --harness claude --long 8000 --harness devin --long 16000")
            .unwrap()
            .1,
        [
            var("CONSENSFLOW_LIVE_HARNESS", "claude,devin"),
            var("CONSENSFLOW_LIVE_LONG", "8000,16000"),
        ]
    );
    assert_eq!(
        runs("approval --steps worker,question").unwrap().1,
        [var("CONSENSFLOW_LIVE_STEPS", "worker,question")]
    );
    // The variable is the flag in capitals, its dashes underscores.
    assert_eq!(
        runs("input-box --strategy ctrl-c").unwrap().1,
        [var("CONSENSFLOW_LIVE_STRATEGY", "ctrl-c")]
    );
    assert_eq!(
        runs("paste --claude-model x").unwrap().1,
        [var("CONSENSFLOW_LIVE_CLAUDE_MODEL", "x")]
    );
}

#[test]
fn keep_takes_no_value_so_what_follows_it_is_a_word_of_its_own() {
    assert_eq!(
        runs("designer --keep").unwrap().1,
        [var("CONSENSFLOW_LIVE_KEEP", "1")]
    );
    let (line, vars) = runs("approval --keep claude --steps worker").unwrap();
    assert_eq!(
        vars,
        [
            var("CONSENSFLOW_LIVE_KEEP", "1"),
            var("CONSENSFLOW_LIVE_HARNESS", "claude"),
            var("CONSENSFLOW_LIVE_STEPS", "worker"),
        ]
    );
    assert_eq!(
        line,
        "cargo test -p cf-e2e --test live_approval -- --ignored --nocapture"
    );
}

#[test]
fn cargos_own_flags_end_the_words_and_they_and_what_follows_are_cargos() {
    let (line, vars) = runs("approval --steps worker devin --keep --offline claude").unwrap();
    assert_eq!(
        line,
        "cargo test -p cf-e2e --test live_approval --offline claude -- --ignored --nocapture"
    );
    assert_eq!(
        vars,
        [
            var("CONSENSFLOW_LIVE_STEPS", "worker"),
            var("CONSENSFLOW_LIVE_HARNESS", "devin"),
            var("CONSENSFLOW_LIVE_KEEP", "1"),
        ]
    );
    // After one, a word of live's is not live's.
    let (line, vars) = runs("designer --offline --keep").unwrap();
    assert!(line.contains("--offline --keep -- --ignored"), "{line}");
    assert!(vars.is_empty());
    for flag in ["--locked", "--frozen"] {
        let (line, _) = runs(&format!("paste {flag} --keep")).unwrap();
        assert!(line.contains(&format!("{flag} --keep --")), "{line}");
    }
}

#[test]
fn what_follows_a_double_dash_goes_to_the_test_program_after_what_every_run_gives_it() {
    assert_eq!(
        runs("paste -- --list").unwrap().0,
        "cargo test -p cf-e2e --test live_paste -- --ignored --nocapture --list"
    );
    assert_eq!(
        runs("paste claude -- --list --exact the_name").unwrap(),
        (
            "cargo test -p cf-e2e --test live_paste -- --ignored --nocapture --list --exact the_name"
                .to_owned(),
            vec![var("CONSENSFLOW_LIVE_HARNESS", "claude")]
        )
    );
    // Words of live's after it are the test program's, flags among them.
    let (line, vars) = runs("paste -- --keep").unwrap();
    assert!(line.ends_with("-- --ignored --nocapture --keep"), "{line}");
    assert!(vars.is_empty());
    // What every run is given is not given twice: the test program refuses a flag it is
    // given more than once (`-- --nocapture` is what the live tests have always been run with).
    assert_eq!(
        runs("paste -- --nocapture --list").unwrap().0,
        "cargo test -p cf-e2e --test live_paste -- --ignored --nocapture --list"
    );
    assert_eq!(
        runs("approval claude -- --ignored --nocapture").unwrap().0,
        "cargo test -p cf-e2e --test live_approval -- --ignored --nocapture"
    );
    assert_eq!(
        runs("paste --offline -- --ignored").unwrap().0,
        "cargo test -p cf-e2e --test live_paste --offline -- --nocapture --ignored"
    );
    assert_eq!(
        runs("paste --offline -- --list").unwrap().0,
        "cargo test -p cf-e2e --test live_paste --offline -- --ignored --nocapture --list"
    );
}

#[test]
fn a_word_that_is_none_of_these_is_refused_with_what_is_wrong_and_nothing_is_started() {
    let refused = |line: &str| match runs(line) {
        Err(Failure::Usage(message)) => message,
        other => panic!("{line}: {other:?}"),
    };
    assert_eq!(
        refused("approval --steps"),
        "--steps takes a value (a flag that takes none is --keep)"
    );
    assert_eq!(
        refused("approval --steps --keep"),
        "--steps takes a value (a flag that takes none is --keep)"
    );
    assert!(refused("paste -x").starts_with("unknown option: -x"));
    assert!(refused("paste claude -h").starts_with("unknown option: -h"));
}

#[test]
fn a_help_asked_among_the_words_of_live_is_the_commands_help_and_starts_nothing() {
    // As it is for a command that has a first word: `npm run live:designer -- --help`.
    for line in [
        "designer --help",
        "paste claude -h",
        "approval --steps worker --help",
    ] {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut console = Console {
            out: &mut out,
            err: &mut err,
        };
        let status = live(&context(), &words(line), &mut console).unwrap();
        let said = String::from_utf8(out).unwrap();
        assert_eq!(status, 0, "{line}");
        assert!(said.starts_with("Usage: cargo xtask live <name>"), "{said}");
        assert!(said.contains("It runs in Rust."), "{said}");
        assert!(err.is_empty(), "{line}");
    }
    // What follows a `--`, or a flag of cargo's, is not live's: it is the test program's.
    assert!(!asks_for_help(&words("paste -- --help")));
    assert!(!asks_for_help(&words("paste --offline --help")));
    assert!(!asks_for_help(&words("paste --harness claude")));
    assert!(!asks_for_help(&words("")));
}

#[test]
fn a_name_that_is_no_live_test_or_none_says_which_there_are() {
    let refused = |args: &[&str]| {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        match parse(&args, &names()) {
            Err(Failure::Usage(message)) => message,
            other => panic!("{args:?}: {other:?}"),
        }
    };
    assert_eq!(
        refused(&[]),
        "live takes the name of a live test: approval, designer, input_box, paste"
    );
    assert_eq!(
        refused(&["aproval"]),
        "no live test is called aproval: approval, designer, input_box, paste"
    );
    // A flag or a harness is not a name.
    assert!(refused(&["--keep"]).starts_with("no live test is called --keep"));
    assert!(refused(&["claude"]).starts_with("no live test is called claude"));
}

#[test]
fn the_live_tests_there_are_the_files_of_the_tests_folder_that_begin_with_live() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("crates").join("cf-e2e").join("tests");
    std::fs::create_dir_all(folder.join("live_paste")).unwrap();
    for file in [
        "live_paste.rs",
        "live_approval.rs",
        "live_.rs",
        "live_notes.txt",
        "load.rs",
        "rig.rs",
    ] {
        std::fs::write(folder.join(file), "").unwrap();
    }
    // The parts of a test are in a folder of its name, and no test of their own.
    assert_eq!(tests(root.path()).unwrap(), ["approval", "paste"]);
    assert!(tests(&root.path().join("elsewhere")).is_err());
}

#[test]
fn the_variables_the_words_set_are_the_ones_the_live_tests_read() {
    let live = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .join("crates/cf-e2e/src/live/mod.rs"),
    )
    .unwrap();
    let constant = |name: &str, value: &str| {
        assert!(
            live.contains(&format!("pub const {name}: &str = \"{value}\";")),
            "crates/cf-e2e/src/live/mod.rs does not name {value} as {name}"
        );
    };
    constant("PREFIX", PREFIX);
    constant("KEEP", &variable(KEEP));
    constant("HARNESS", &variable(HARNESS));
    constant("STEPS", &variable("steps"));
    // And the rule that makes a variable of a flag is the same there.
    assert!(
        live.contains("format!(\"{PREFIX}{}\", flag.to_ascii_uppercase().replace('-', \"_\"))"),
        "the rule that makes the variable of a flag is not the one xtask has"
    );
}

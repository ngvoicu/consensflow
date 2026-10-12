//! The command list and the command line, held to what they say: every command
//! is there once, in the order the help lists them, named by its words and given
//! the rest as they are.

use super::*;

fn line(words: &str) -> Vec<OsString> {
    words.split_whitespace().map(OsString::from).collect()
}

/// What `cargo xtask <words>` is asked to do, as text for a test to read.
fn parsed(words: &str) -> String {
    let args = line(words);
    match parse(&commands(), &args, Path::new("/checkout")) {
        Parsed::Run { command, args } => format!("run {} {args:?}", command.words.join(" ")),
        Parsed::Help(text) => format!("help\n{text}"),
        Parsed::Usage(message) => format!("usage {message}"),
    }
}

#[test]
fn every_command_of_the_plan_is_there_once() {
    let words: Vec<_> = commands()
        .iter()
        .map(|command| command.words.join(" "))
        .collect();
    assert_eq!(
        words,
        [
            "build-cf",
            "stage",
            "conpty",
            "portable pack",
            "portable inspect",
            "app test",
            "app clippy",
            "clippy-windows",
            "--as-archiver",
            "test daemons",
            "test integration",
            "test regressions",
            "test clis",
            "test agents",
            "test load",
            "live",
            "windows",
            "windows-matrix",
            "eval summary",
            "eval",
            "watch",
            "scrub-transcript",
            "departures",
            "bench records-memory",
            "plants",
            "smoke",
            "smoke-updater",
            "candidate",
            "check",
        ]
    );
}

#[test]
fn a_command_is_named_by_its_words_and_takes_the_rest_as_they_are() {
    assert_eq!(parsed("departures"), "run departures []");
    assert_eq!(
        parsed("app test portable::"),
        r#"run app test ["portable::"]"#
    );
    // `--` and what follows it are the command's, not xtask's.
    assert_eq!(
        parsed("test daemons --offline -- --help"),
        r#"run test daemons ["--offline", "--", "--help"]"#
    );
    // A word xtask has no meaning for is the command's too.
    assert_eq!(
        parsed("smoke --app ./x.app"),
        r#"run smoke ["--app", "./x.app"]"#
    );
    assert_eq!(parsed("check -h"), parsed("check --help"));
}

#[test]
fn the_live_tests_are_one_command_whatever_their_names_and_eval_summary_is_found_before_eval() {
    assert_eq!(
        parsed("live paste claude --long 8000 -- --list"),
        r#"run live ["paste", "claude", "--long", "8000", "--", "--list"]"#
    );
    assert_eq!(parsed("live"), "run live []");
    assert_eq!(parsed("eval summary"), "run eval summary []");
    assert_eq!(
        parsed("eval --scenario round-trip --chief claude"),
        r#"run eval ["--scenario", "round-trip", "--chief", "claude"]"#
    );
    assert_eq!(parsed("eval"), "run eval []");
    assert_eq!(
        parsed("windows --host zeewin -- cargo xtask check"),
        r#"run windows ["--host", "zeewin", "--", "cargo", "xtask", "check"]"#
    );
    assert_eq!(
        parsed("scrub-transcript a.jsonl b.jsonl"),
        r#"run scrub-transcript ["a.jsonl", "b.jsonl"]"#
    );
    // A command's own help is said for the one that is asked about.
    assert!(parsed("eval summary --help").contains("evals/RESULTS.md"));
    assert!(parsed("eval --help").contains("--scenario"));
    assert!(parsed("live --help").contains("Usage: cargo xtask live <name>"));
}

#[test]
fn a_command_that_is_not_ported_yet_says_so_and_what_to_run_and_answers_1() {
    for (line, said) in [
        (
            "windows --host zeewin",
            "xtask: windows is not ported yet: run `npm run windows` for now\n",
        ),
        (
            "windows-matrix",
            "xtask: windows-matrix is not ported yet: run `npm run eval:windows` for now\n",
        ),
        (
            "eval summary",
            "xtask: eval summary is not ported yet: run `npm run eval:summary` for now\n",
        ),
        (
            "eval --scenario round-trip",
            "xtask: eval is not ported yet: run `npm run eval` for now\n",
        ),
        (
            "watch --tokens",
            "xtask: watch is not ported yet: run `npm run watch` for now\n",
        ),
    ] {
        let (status, out, err) = answer(line);
        assert_eq!(
            (status, out.as_str(), err.as_str()),
            (1, "", said),
            "{line}"
        );
    }
}

#[test]
fn words_with_spaces_in_them_stay_one_word_each() {
    let args: Vec<OsString> = ["smoke", "--app", "A Folder/Consens Flow.app", "", "-"]
        .iter()
        .map(OsString::from)
        .collect();
    let Parsed::Run { command, args } = parse(&commands(), &args, Path::new("/checkout")) else {
        panic!("smoke is a command");
    };
    assert_eq!(command.words, ["smoke"]);
    assert_eq!(args, ["--app", "A Folder/Consens Flow.app", "", "-"]);
}

#[test]
fn a_help_asked_first_is_xtasks_and_asks_nothing_to_run() {
    let all = parsed("--help");
    assert!(all.starts_with("help\nxtask: "), "{all}");
    assert!(all.contains("checkout: /checkout\n"), "{all}");
    for command in commands() {
        assert!(all.contains(&command.words.join(" ")), "{all}");
        assert!(all.contains(command.about), "{all}");
    }
    assert_eq!(parsed("-h"), all);
    // A command's own help says what it takes and what it runs.
    let help = parsed("build-cf --help");
    assert!(
        help.starts_with("help\nUsage: cargo xtask build-cf [--offline]\n"),
        "{help}"
    );
    assert!(help.contains("It runs in Rust."), "{help}");
    assert!(parsed("check --help").contains("It runs in Rust."));
    // A help that is not the first word is the command's own argument.
    assert_eq!(
        parsed("build-cf --offline --help"),
        r#"run build-cf ["--offline", "--help"]"#
    );
}

#[test]
fn a_group_of_commands_has_its_help_and_wants_a_command_of_it() {
    let help = parsed("app --help");
    assert!(
        help.starts_with("help\nUsage: cargo xtask app <command> [arguments]\n"),
        "{help}"
    );
    assert!(
        help.contains("app test") && help.contains("app clippy"),
        "{help}"
    );
    assert!(!help.contains("departures"), "{help}");
    assert_eq!(parsed("app"), "usage app takes a command: test, clippy");
    assert_eq!(
        parsed("test"),
        "usage test takes a command: daemons, integration, regressions, clis, agents, load"
    );
    assert_eq!(parsed("app tests"), "usage unknown command: app tests");
    assert_eq!(
        parsed("portable"),
        "usage portable takes a command: pack, inspect"
    );
    assert_eq!(
        parsed("portable pack --out here"),
        r#"run portable pack ["--out", "here"]"#
    );
}

#[test]
fn words_that_name_no_command_are_refused() {
    assert_eq!(parsed(""), "usage a command is required");
    assert_eq!(parsed("deploy"), "usage unknown command: deploy");
    assert_eq!(parsed("--offline"), "usage unknown command: --offline");
    // A command's words are whole words: a longer one is another.
    assert_eq!(parsed("checks"), "usage unknown command: checks");
    assert_eq!(
        parsed("bench"),
        "usage bench takes a command: records-memory"
    );
}

/// `cargo xtask <words>` as `run` answers: its status, what it printed and what it said.
fn answer(words: &str) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let status = run(&Env::from_process(), &line(words), &mut out, &mut err);
    (
        status,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn a_help_or_a_refusal_is_answered_without_running_anything() {
    let (status, out, err) = answer("--help");
    assert_eq!((status, err.as_str()), (0, ""));
    let root = Context::new(&Env::default()).unwrap().root;
    assert!(
        out.contains(&format!("checkout: {}\n", root.display())),
        "{out}"
    );

    let (status, out, err) = answer("stage --help");
    assert_eq!((status, err.as_str()), (0, ""));
    assert!(out.starts_with("Usage: cargo xtask stage\n"), "{out}");

    let (status, out, err) = answer("nonsense");
    assert_eq!((status, out.as_str()), (2, ""));
    assert_eq!(
        err,
        "xtask: unknown command: nonsense\nsee `cargo xtask --help`\n"
    );
    assert_eq!(answer("").0, 2);
}

#[test]
fn a_closed_pipe_is_a_quiet_end_not_a_failure() {
    struct Closed;
    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut err = Vec::new();
    let status = run(&Env::from_process(), &line("--help"), &mut Closed, &mut err);
    assert_eq!((status, err.is_empty()), (0, true));
}

//! What `bench records-memory` reads from its command line, runs, and says of
//! what it ran, with no cargo and no transcript: the measures are told what to
//! have printed. The command as a process is `tests/drivers.rs`'s.

use super::*;

use cf_base::env::Env;

fn words(words: &[&str]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

fn read(line: &[&str]) -> Result<Options, Failure> {
    Options::read(&words(line))
}

fn refusal(line: &[&str]) -> String {
    match read(line) {
        Err(Failure::Usage(said)) => said,
        Err(other) => panic!("{line:?} was not refused as a usage: {other}"),
        Ok(_) => panic!("{line:?} was taken"),
    }
}

#[test]
fn with_no_option_it_is_one_run_and_nothing_is_set() {
    let options = read(&[]).unwrap();
    assert_eq!(options.runs, 1);
    assert!(options.lines.is_none() && options.transcript.is_none() && options.repo.is_none());
}

#[test]
fn an_option_is_given_as_name_and_value_or_as_name_equals_value() {
    let options = read(&[
        "--runs",
        "3",
        "--lines=125000",
        "--transcript",
        "a b/t.jsonl",
        "--repo=../before",
    ])
    .unwrap();
    assert_eq!(options.runs, 3);
    assert_eq!(options.lines, Some("125000".into()));
    assert_eq!(options.transcript, Some("a b/t.jsonl".into()));
    assert_eq!(options.repo, Some(PathBuf::from("../before")));
}

#[test]
fn an_option_given_twice_has_the_last_say_and_a_value_may_be_empty_or_hold_an_equals_sign() {
    assert_eq!(read(&["--runs", "2", "--runs=5"]).unwrap().runs, 5);
    assert_eq!(read(&["--lines", ""]).unwrap().lines, Some("".into()));
    assert_eq!(read(&["--lines="]).unwrap().lines, Some("".into()));
    assert_eq!(read(&["--lines=a=b"]).unwrap().lines, Some("a=b".into()));
    // After `=` a value may begin with a dash, as a number of lines never does but a path may.
    assert_eq!(
        read(&["--transcript=-odd"]).unwrap().transcript,
        Some("-odd".into())
    );
}

#[test]
fn a_word_that_is_no_option_it_has_is_refused_with_what_it_takes() {
    let takes =
        "bench records-memory takes [--runs N] [--lines N] [--transcript FILE] [--repo DIR]";
    for word in [
        "positional",
        "-x",
        "--",
        "--run",
        "--Runs",
        "--runs3",
        "--bogus=1",
        "=1",
    ] {
        assert_eq!(refusal(&[word]), format!("{takes}, not {word}"), "{word}");
    }
    // The one that is wrong, among ones that are right.
    assert_eq!(
        refusal(&["--runs", "2", "--ofline"]),
        format!("{takes}, not --ofline")
    );
}

#[test]
fn an_option_with_no_value_is_refused_and_so_is_one_followed_by_another_option() {
    assert_eq!(
        refusal(&["--lines"]),
        "bench records-memory: --lines needs a value"
    );
    assert_eq!(
        refusal(&["--lines", "--runs", "2"]),
        "bench records-memory: --lines needs a value"
    );
    assert_eq!(
        refusal(&["--transcript", "-5"]),
        "bench records-memory: --transcript needs a value"
    );
}

#[test]
fn the_number_of_runs_is_a_whole_number_which_may_be_none() {
    assert_eq!(read(&["--runs", "0"]).unwrap().runs, 0);
    assert_eq!(read(&["--runs=12"]).unwrap().runs, 12);
    for given in ["abc", "1.5", "", "-1", "3x"] {
        let line = format!("--runs={given}");
        assert_eq!(
            refusal(&[&line]),
            format!("bench records-memory: --runs takes a whole number, not {given}"),
            "{given}"
        );
    }
}

#[test]
fn a_measure_is_one_release_built_ignored_test_of_the_library_run_alone() {
    let options = read(&[]).unwrap();
    let invocation = measure(Path::new("repo"), "first_look", &options);
    assert_eq!(
        invocation.display(),
        "cargo test --offline --release -p cf-harness --lib \
         claude::record::tests::memory::first_look -- --ignored --nocapture --exact"
    );
    assert_eq!(invocation.cwd, PathBuf::from("repo"));
    assert!(invocation.vars.is_empty());
}

#[test]
fn the_lines_and_the_transcript_reach_the_measures_by_the_variables_they_read() {
    let options = read(&["--lines", "5000", "--transcript", "/t/a b.jsonl"]).unwrap();
    assert_eq!(
        measure(Path::new("repo"), "reread", &options).vars,
        [
            (
                OsString::from("CF_RECORDS_MEMORY_LINES"),
                Some(OsString::from("5000"))
            ),
            (
                OsString::from("CF_RECORDS_MEMORY_TRANSCRIPT"),
                Some(OsString::from("/t/a b.jsonl"))
            ),
        ]
    );
    let lines_only = read(&["--lines=7"]).unwrap();
    let vars = measure(Path::new("repo"), "reread", &lines_only).vars;
    assert_eq!(vars.len(), 1);
    assert_eq!(vars[0].0, "CF_RECORDS_MEMORY_LINES");
}

#[test]
fn a_repo_is_measured_from_where_it_is_named_from_the_roots_folder_when_relative() {
    let context = Context {
        root: PathBuf::from("checkout"),
        env: Env::default(),
    };
    let named = |line: &[&str]| read(line).unwrap().repo_in(&context);
    assert_eq!(named(&[]), PathBuf::from("checkout"));
    assert_eq!(
        named(&["--repo", "before"]),
        PathBuf::from("checkout").join("before")
    );
    let elsewhere = std::env::temp_dir().join("before");
    let absolute = elsewhere.to_str().unwrap();
    assert_eq!(named(&["--repo", absolute]), elsewhere);
}

fn printed(stdout: &str) -> Result<Captured, process::Failure> {
    Ok(Captured {
        code: 0,
        stdout: stdout.into(),
        stderr: "   Compiling cf-harness\n".into(),
    })
}

/// What `report` says for `runs`, with every measure printing a line of its own
/// name between the lines of the harness around it: the status, `out`, `err`, and
/// the measures asked for, in order.
fn said(runs: u32) -> (i32, String, String, Vec<String>) {
    let (mut out, mut err, mut asked) = (Vec::new(), Vec::new(), Vec::new());
    let status = report(
        runs,
        |name| {
            asked.push(name.to_string());
            printed(&format!(
                "\nrunning 1 test\n{name} took 1 s\n  peak 2 MB\n\ntest {name} ... ok\n\ntest result: ok. 1 passed\n"
            ))
        },
        &mut out,
        &mut err,
    )
    .unwrap();
    (
        status,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
        asked,
    )
}

#[test]
fn each_measure_has_its_heading_and_its_own_lines_indented_and_nothing_of_cargos_or_the_harness() {
    let (status, out, err, asked) = said(1);
    assert_eq!(status, 0);
    assert_eq!(
        out,
        "first look:\n  first_look took 1 s\n    peak 2 MB\n\
         first look parts:\n  first_look_parts took 1 s\n    peak 2 MB\n\
         unchanged look:\n  unchanged_look took 1 s\n    peak 2 MB\n\
         reread:\n  reread took 1 s\n    peak 2 MB\n"
    );
    assert_eq!(err, "");
    assert_eq!(
        asked,
        ["first_look", "first_look_parts", "unchanged_look", "reread"]
    );
}

#[test]
fn more_runs_repeat_the_first_look_and_the_look_in_parts_each_in_turn_and_the_others_once() {
    let (status, out, _, asked) = said(2);
    assert_eq!(status, 0);
    let headings: Vec<_> = out.lines().filter(|line| !line.starts_with(' ')).collect();
    assert_eq!(
        headings,
        [
            "first look (run 1):",
            "first look (run 2):",
            "first look parts (run 1):",
            "first look parts (run 2):",
            "unchanged look:",
            "reread:"
        ]
    );
    assert_eq!(
        asked,
        [
            "first_look",
            "first_look",
            "first_look_parts",
            "first_look_parts",
            "unchanged_look",
            "reread"
        ]
    );
}

#[test]
fn no_runs_leave_out_the_first_look_and_the_look_in_parts() {
    let (status, out, _, asked) = said(0);
    assert_eq!(status, 0);
    assert!(out.starts_with("unchanged look:\n"), "{out}");
    assert_eq!(asked, ["unchanged_look", "reread"]);
}

#[test]
fn only_a_line_that_starts_with_running_or_test_is_the_harnesss() {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let status = report(
        0,
        |_| printed("running 1 test\nrunning late\nthe test ran\ntesting 1 2\ntest x ... ok\n \n tail\r\n"),
        &mut out,
        &mut err,
    )
    .unwrap();
    assert_eq!(status, 0);
    // `unchanged look` and `reread` each print the same lines: whatever is left of them.
    let kept = "  the test ran\n  testing 1 2\n   \n   tail\r\n";
    assert_eq!(
        String::from_utf8(out).unwrap(),
        format!("unchanged look:\n{kept}reread:\n{kept}")
    );
}

#[test]
fn a_measure_that_fails_says_all_it_wrote_and_which_it_was_and_the_status_is_1_whatever_it_ended_with(
) {
    for code in [1, 101, 255] {
        let (mut out, mut err, mut asked) = (Vec::new(), Vec::new(), Vec::new());
        let status = report(
            2,
            |name| {
                asked.push(name.to_string());
                if name == "first_look_parts" {
                    return Ok(Captured {
                        code,
                        stdout: "the line it got to\n".into(),
                        stderr: "the panic\n".into(),
                    });
                }
                printed("ok line\n")
            },
            &mut out,
            &mut err,
        )
        .unwrap();
        assert_eq!(status, 1, "{code}");
        // Its heading was said when it began; its lines are not said, they are in the failure.
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "first look (run 1):\n  ok line\nfirst look (run 2):\n  ok line\nfirst look parts (run 1):\n",
            "{code}"
        );
        assert_eq!(
            String::from_utf8(err).unwrap(),
            format!(
                "the line it got to\nthe panic\n\
                 xtask: first_look_parts failed: its cargo test ended with status {code}\n"
            ),
            "{code}"
        );
        // Nothing after it is run.
        assert_eq!(
            asked,
            ["first_look", "first_look", "first_look_parts"],
            "{code}"
        );
    }
}

#[test]
fn whichever_of_the_four_measures_fails_the_status_is_1_and_the_ones_after_it_are_not_run() {
    let names = ["first_look", "first_look_parts", "unchanged_look", "reread"];
    for (index, failing) in names.into_iter().enumerate() {
        let (mut out, mut err, mut asked) = (Vec::new(), Vec::new(), Vec::new());
        let status = report(
            1,
            |name| {
                asked.push(name.to_string());
                if name == failing {
                    return Ok(Captured {
                        code: 101,
                        stdout: String::new(),
                        stderr: "it failed\n".into(),
                    });
                }
                printed("ok line\n")
            },
            &mut out,
            &mut err,
        )
        .unwrap();
        assert_eq!(status, 1, "{failing}");
        assert_eq!(asked, names[..=index], "{failing}");
        // Each one before it said its line, and the one that failed only its heading.
        assert_eq!(
            String::from_utf8(out)
                .unwrap()
                .matches("  ok line\n")
                .count(),
            index,
            "{failing}"
        );
        assert_eq!(
            String::from_utf8(err).unwrap(),
            format!("it failed\nxtask: {failing} failed: its cargo test ended with status 101\n"),
            "{failing}"
        );
    }
}

#[test]
fn a_measure_that_cannot_be_started_ends_the_command_with_that_and_after_its_heading() {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let failed = report(
        1,
        |_| {
            Err(process::Failure::NotFound {
                program: "cargo".into(),
            })
        },
        &mut out,
        &mut err,
    )
    .unwrap_err();
    assert_eq!(
        failed.to_string(),
        "`cargo` was not found: is it installed, and on the PATH?"
    );
    assert!(matches!(failed, Failure::Process(_)));
    assert_eq!(String::from_utf8(out).unwrap(), "first look:\n");
    assert!(err.is_empty());
}

#[test]
fn the_command_runs_in_rust_and_is_no_script() {
    assert_eq!(COMMANDS[0].words, ["bench", "records-memory"]);
    assert!(matches!(COMMANDS[0].run, Run::Native(_)));
    assert!(COMMANDS[0].usage.starts_with("[--runs N]"));
}

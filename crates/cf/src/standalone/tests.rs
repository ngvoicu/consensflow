//! Which words this module answers and which it leaves to the CLI's Node
//! sources: the switch, a window's token, and the verbs not ported. What it
//! says for the ones it answers is held to Node's recording
//! (`tests/cli_goldens.rs`).

use std::io;

use super::*;

fn env(vars: &[(&str, &str)]) -> Env {
    Env::from_vars(vars.iter().copied())
}

fn words(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

/// What `run` made of the words: its code, and what it said on each output.
fn ran(vars: &[(&str, &str)], args: &[&str]) -> (Option<u8>, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run(&env(vars), &words(args), &mut out, &mut err).unwrap();
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// An environment that says nothing: the verbs are answered by default.
const NONE: [(&str, &str); 0] = [];

#[test]
fn it_answers_by_default_and_the_old_switch_in_the_environment_changes_nothing() {
    // `CONSENSFLOW_DAEMON` was the switch of the releases before the flip; the
    // product reads no environment variable for which implementation answers.
    for value in ["native", "node", "NATIVE", "1", ""] {
        let vars = [("CONSENSFLOW_DAEMON", value)];
        let (code, said, wrong) = ran(&vars, &["help"]);
        assert_eq!((code, wrong.as_str()), (Some(0), ""), "{value:?}");
        assert!(said.starts_with("consensflow "), "{value:?}: {said}");
    }
}

#[test]
fn the_verbs_that_wait_for_another_landing_go_on_to_node() {
    for verb in ["setup", "doctor", "ui"] {
        assert_eq!(
            ran(&NONE, &[verb, "--json"]),
            (None, String::new(), String::new()),
            "{verb}"
        );
    }
}

#[test]
fn the_usage_is_the_one_node_printed_with_this_builds_version_and_a_blank_line_at_its_end() {
    for args in [&[][..], &["help"], &["--help"], &["help", "agent"]] {
        let (code, said, wrong) = ran(&NONE, args);
        assert_eq!((code, wrong.as_str()), (Some(0), ""), "{args:?}");
        assert!(
            said.starts_with(&format!(
                "consensflow {}\n\nUsage: cf <command>",
                env!("CARGO_PKG_VERSION")
            )),
            "{said}"
        );
        assert!(said.ends_with("(cf help there says more).\n\n"), "{said:?}");
        assert!(!said.contains("{version}"));
    }
}

#[test]
fn the_version_alone_in_each_of_its_three_spellings() {
    for spelling in ["--version", "-v", "version"] {
        assert_eq!(
            ran(&NONE, &[spelling, "x"]),
            (
                Some(0),
                format!("{}\n", env!("CARGO_PKG_VERSION")),
                String::new()
            ),
            "{spelling}"
        );
    }
}

#[test]
fn a_command_it_has_not_is_said_as_json_writes_it_and_fails() {
    for (word, written) in [
        ("frobnicate", r#""frobnicate""#),
        ("", r#""""#),
        ("a\"b", r#""a\"b""#),
        ("a\nb", r#""a\nb""#),
        ("é😀", "\"é😀\""),
        ("-h", r#""-h""#),
    ] {
        assert_eq!(
            ran(&NONE, &[word]),
            (
                Some(1),
                String::new(),
                format!("cf: unknown command {written} — run `cf help`\n")
            ),
            "{word:?}"
        );
    }
}

/// An output nobody reads.
struct Gone;

impl io::Write for Gone {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_reader_that_went_away_is_the_callers_to_end_quietly_and_no_message_of_the_verb() {
    let home = tempfile::tempdir().unwrap();
    let vars = [("CONSENSFLOW_HOME", home.path().to_str().unwrap())];
    for args in [&["help"][..], &["catalog"], &["agent", "list"]] {
        let mut err = Vec::new();
        let ended = run(&env(&vars), &words(args), &mut Gone, &mut err).unwrap_err();
        assert_eq!(ended.kind(), io::ErrorKind::BrokenPipe, "{args:?}");
        assert!(err.is_empty(), "{args:?}");
    }
}

#[test]
fn with_no_folder_to_keep_the_agents_in_a_verb_that_needs_it_says_so() {
    for args in [
        &["agent", "list"][..],
        &["agent", "edit", "x"],
        &["agent", "remove", "x"],
        &["agent", "add", "x", "--harness", "claude", "--model", "m"],
    ] {
        assert_eq!(
            ran(&NONE, args),
            (
                Some(1),
                String::new(),
                "cf: ConsensFlow has no folder to keep its things in: set CONSENSFLOW_HOME, or HOME\n"
                    .to_owned()
            ),
            "{args:?}"
        );
    }
    // The catalog is no file of the home's.
    assert_eq!(ran(&NONE, &["catalog", "--harness", "pi"]).0, Some(0));
}

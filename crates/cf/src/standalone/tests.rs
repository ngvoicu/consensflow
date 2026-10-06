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

const NATIVE: [(&str, &str); 1] = [("CONSENSFLOW_DAEMON", "native")];

#[test]
fn it_answers_nothing_while_the_switch_is_off_whatever_the_verb() {
    for args in [
        &[][..],
        &["help"],
        &["--version"],
        &["catalog"],
        &["agent", "list"],
        &["agent", "add", "x"],
        &["frobnicate"],
    ] {
        assert_eq!(
            ran(&[], args),
            (None, String::new(), String::new()),
            "{args:?}"
        );
    }
}

#[test]
fn the_switch_is_native_and_no_other_word() {
    for value in ["node", "NATIVE", "Native", "1", "true", "", " native"] {
        let vars = [("CONSENSFLOW_DAEMON", value)];
        assert_eq!(ran(&vars, &["help"]).0, None, "{value:?}");
    }
    assert_eq!(ran(&NATIVE, &["help"]).0, Some(0));
}

#[test]
fn a_window_token_makes_cf_the_board_which_this_module_does_not_answer() {
    let vars = [
        ("CONSENSFLOW_DAEMON", "native"),
        ("CONSENSFLOW_TOKEN", "participant"),
    ];
    assert_eq!(
        ran(&vars, &["catalog"]),
        (None, String::new(), String::new())
    );
    // An empty token is none, as in Node.
    let empty = [("CONSENSFLOW_DAEMON", "native"), ("CONSENSFLOW_TOKEN", "")];
    assert_eq!(ran(&empty, &["help"]).0, Some(0));
}

#[test]
fn the_verbs_that_wait_for_another_landing_go_on_to_node_with_the_switch_on() {
    for verb in ["setup", "doctor", "ui"] {
        assert_eq!(
            ran(&NATIVE, &[verb, "--json"]),
            (None, String::new(), String::new()),
            "{verb}"
        );
    }
}

#[test]
fn the_usage_is_the_one_node_printed_with_this_builds_version_and_a_blank_line_at_its_end() {
    for args in [&[][..], &["help"], &["--help"], &["help", "agent"]] {
        let (code, said, wrong) = ran(&NATIVE, args);
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
            ran(&NATIVE, &[spelling, "x"]),
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
            ran(&NATIVE, &[word]),
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
    let vars = [
        ("CONSENSFLOW_DAEMON", "native"),
        ("CONSENSFLOW_HOME", home.path().to_str().unwrap()),
    ];
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
            ran(&NATIVE, args),
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
    assert_eq!(ran(&NATIVE, &["catalog", "--harness", "pi"]).0, Some(0));
}

//! Which words this module answers, whatever the environment says: the verbs,
//! and as unknown commands every word that is none, `ui` (the daemon's) among
//! them. What it says for the verbs is held to Node's recording
//! (`tests/cli_goldens`); here are the orders and refusals that recording
//! cannot hold, because Node asked the system for what the environment does
//! not name.

use std::path::Path;
use std::{fs, io};

use super::*;

fn env(vars: &[(&str, &str)]) -> Env {
    Env::from_vars(vars.iter().copied())
}

/// What a user has who keeps ConsensFlow's home, Claude Code's folder and the
/// whole of what they look in under `dir`, with no harness on the PATH.
fn homed(dir: &Path) -> Vec<(&'static str, String)> {
    let at = |name: &str| dir.join(name).to_string_lossy().into_owned();
    vec![
        ("CONSENSFLOW_HOME", at("consensflow")),
        ("HOME", at("home")),
        ("CLAUDE_CONFIG_DIR", at("claude")),
        ("PATH", at("bin")),
    ]
}

/// `ran` for a user of `homed`.
fn ran_in(dir: &Path, args: &[&str]) -> (u8, String, String) {
    let vars = homed(dir);
    let vars: Vec<(&str, &str)> = vars
        .iter()
        .map(|(name, text)| (*name, text.as_str()))
        .collect();
    ran(&vars, args)
}

fn words(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

/// What `run` made of the words: its code, and what it said on each output.
fn ran(vars: &[(&str, &str)], args: &[&str]) -> (u8, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run(&env(vars), &words(args), &mut out, &mut err).unwrap();
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// An environment that says nothing.
const NONE: [(&str, &str); 0] = [];

#[test]
fn a_stale_old_switch_in_the_environment_changes_nothing() {
    // `CONSENSFLOW_DAEMON` was the switch of the releases before the flip, and a
    // shell profile may still set it; there is no other implementation to switch
    // to, and nothing reads it.
    for value in ["native", "node", "NATIVE", "1", ""] {
        let vars = [("CONSENSFLOW_DAEMON", value)];
        let (code, said, wrong) = ran(&vars, &["help"]);
        assert_eq!((code, wrong.as_str()), (0, ""), "{value:?}");
        assert!(said.starts_with("consensflow "), "{value:?}: {said}");
    }
}

#[test]
fn ui_is_the_daemons_and_reaching_this_module_it_is_an_unknown_command() {
    // `main` runs the daemon before the words get here, and starts nothing
    // for a word that does: this module has nothing to hand it to.
    for args in [&["ui"][..], &["ui", "--json"], &["ui", "--no-open"]] {
        assert_eq!(
            ran(&NONE, args),
            (
                1,
                String::new(),
                "cf: unknown command \"ui\" — run `cf help`\n".to_owned()
            ),
            "{args:?}"
        );
    }
}

#[test]
fn the_usage_is_the_one_node_printed_with_this_builds_version_and_a_blank_line_at_its_end() {
    for args in [&[][..], &["help"], &["--help"], &["help", "agent"]] {
        let (code, said, wrong) = ran(&NONE, args);
        assert_eq!((code, wrong.as_str()), (0, ""), "{args:?}");
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
            (0, format!("{}\n", env!("CARGO_PKG_VERSION")), String::new()),
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
                1,
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
    // `setup` and `doctor` say it before they make or say anything: the
    // launcher would say its own words for a home it cannot find, and the
    // roster would refuse again after.
    for args in [
        &["agent", "list"][..],
        &["agent", "edit", "x"],
        &["agent", "remove", "x"],
        &["agent", "add", "x", "--harness", "claude", "--model", "m"],
        &["setup"],
        &["doctor"],
        &["doctor", "--anything"],
    ] {
        assert_eq!(
            ran(&NONE, args),
            (
                1,
                String::new(),
                "cf: ConsensFlow has no folder to keep its things in: set CONSENSFLOW_HOME, or HOME\n"
                    .to_owned()
            ),
            "{args:?}"
        );
    }
    // The catalog is no file of the home's.
    assert_eq!(ran(&NONE, &["catalog", "--harness", "pi"]).0, 0);
}

#[test]
fn setup_reads_its_words_before_it_makes_anything() {
    let dir = tempfile::tempdir().unwrap();
    for (word, said) in [
        (
            "x",
            "Unexpected argument 'x'. This command does not take positional arguments",
        ),
        ("--json", "Unknown option '--json'"),
    ] {
        assert_eq!(
            ran_in(dir.path(), &["setup", word]),
            (1, String::new(), format!("cf: {said}\n")),
            "{word}"
        );
        // Not a launcher, nor the folder for one.
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0, "{word}");
    }
}

#[test]
fn setup_makes_the_command_in_the_home_and_says_what_it_found() {
    let dir = tempfile::tempdir().unwrap();
    let (code, said, wrong) = ran_in(dir.path(), &["setup"]);
    assert_eq!((code, wrong.as_str()), (0, ""));
    let (harnesses, agents) = said.split_once('\n').unwrap();
    assert_eq!(harnesses, "harnesses: none found on PATH");
    assert!(
        agents.starts_with("agents: ")
            && agents.ends_with(" saved — manage them with cf ui or cf agent\n"),
        "{agents:?}"
    );
    let extension = if cfg!(windows) { ".cmd" } else { "" };
    for name in ["cf", "consensflow"] {
        let command = dir
            .path()
            .join("consensflow/bin")
            .join(format!("{name}{extension}"));
        let text = fs::read_to_string(&command).unwrap();
        assert!(text.contains("Installed by ConsensFlow"), "{name}: {text}");
    }
}

#[test]
fn doctor_says_what_stops_it_after_the_lines_it_has_said() {
    let dir = tempfile::tempdir().unwrap();
    // The command is there and cannot be read: a folder where it goes.
    let first = if cfg!(windows) {
        "consensflow.cmd"
    } else {
        "consensflow"
    };
    fs::create_dir_all(dir.path().join("consensflow/bin").join(first)).unwrap();
    let (code, said, wrong) = ran_in(dir.path(), &["doctor"]);
    assert_eq!(code, 1);
    assert_eq!(
        wrong,
        "cf: EISDIR: illegal operation on a directory, read\n"
    );
    let home = dir.path().join("consensflow");
    assert!(
        said.starts_with(&format!(
            "consensflow {}\nhome:         {}\nharnesses:    none on PATH\nagents:       ",
            env!("CARGO_PKG_VERSION"),
            home.display()
        )),
        "{said}"
    );
    assert!(
        said.ends_with("roles:        bundled chief, worker, reviewer and advisor; prepared when a window launches\n"),
        "{said}"
    );
}

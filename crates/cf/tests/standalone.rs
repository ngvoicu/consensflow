//! Which `cf` answers the standalone verbs, as a process: the ones Rust answers
//! once the switch (`CONSENSFLOW_DAEMON=native`) is on, with no runtime named;
//! the ones it still hands to Node's sources while it is off; and a window's
//! token, which makes `cf` the board whatever the switch says. What the verbs
//! say is held to Node's recording in `cli_goldens`.
//!
//! And what `setup` and `doctor` do with the command that `setup` makes, which
//! only a binary in a place of its own can show: a `cf` is told apart from
//! another by where it is, so a copy of the binary is another install of
//! ConsensFlow.

// The tests' own folders and copies: a failure to make one is the test's.
#![allow(clippy::expect_used)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{cf, cf_at, own_cf};

const NODE_NEEDED: &str = "cf: CONSENSFLOW_NODE is not set:";

fn said(ran: &std::process::Output) -> (Option<i32>, String, String) {
    (
        ran.status.code(),
        String::from_utf8_lossy(&ran.stdout).into_owned(),
        String::from_utf8_lossy(&ran.stderr).into_owned(),
    )
}

#[test]
fn the_switch_is_what_turns_the_verbs_on_and_no_runtime_is_named_for_them() {
    for args in [&["help"][..], &["--version"], &["catalog"], &["bogus"]] {
        // Off: Node's sources answer, and none is named.
        let (code, out, err) = said(&cf(args, &[], ""));
        assert_eq!((code, out.as_str()), (Some(1), ""), "{args:?}");
        assert!(err.starts_with(NODE_NEEDED), "{args:?}: {err}");
        // On: Rust answers, and still none is named.
        let (_, _, err) = said(&cf(args, &[("CONSENSFLOW_DAEMON", "native")], ""));
        assert!(!err.contains("CONSENSFLOW_NODE"), "{args:?}: {err}");
    }
}

#[test]
fn a_window_token_is_the_board_whatever_the_switch_says() {
    let (code, out, err) = said(&cf(
        &["help"],
        &[
            ("CONSENSFLOW_DAEMON", "native"),
            ("CONSENSFLOW_TOKEN", "participant"),
        ],
        "",
    ));
    assert_eq!((code, err.as_str()), (Some(0), ""));
    assert!(out.starts_with("cf inside a ConsensFlow window"), "{out}");
}

#[test]
fn a_json_word_is_the_verbs_own_and_is_not_taken_out_before_the_verb_reads_it() {
    // The board takes `--json` out wherever it stands; the CLI's verbs are
    // handed the words as they came, so here it is the command.
    let (code, _, err) = said(&cf(
        &["--json", "catalog"],
        &[("CONSENSFLOW_DAEMON", "native")],
        "",
    ));
    assert_eq!(code, Some(1));
    assert_eq!(err, "cf: unknown command \"--json\" — run `cf help`\n");
}

/// A user whose ConsensFlow home, home and Claude Code's folder are in a
/// folder of their own, with no harness on the PATH: what `setup` makes and
/// `doctor` reads is all in it.
struct User {
    /// Kept for as long as the user is, and removed with them.
    _folder: tempfile::TempDir,
    root: PathBuf,
}

impl User {
    fn new() -> Self {
        let folder = tempfile::tempdir().expect("a folder");
        let root = fs::canonicalize(folder.path()).expect("its place");
        Self {
            _folder: folder,
            root,
        }
    }

    fn at(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    /// The names of the launcher in its folder: `.cmd` where Windows has them.
    fn command(&self, name: &str) -> PathBuf {
        let extension = if cfg!(windows) { ".cmd" } else { "" };
        self.at("consensflow/bin")
            .join(format!("{name}{extension}"))
    }

    /// `cf args` as the binary at `program` runs it for this user, the switch
    /// on or off.
    fn run_at(
        &self,
        program: &Path,
        switch: Option<&str>,
        args: &[&str],
    ) -> (Option<i32>, String, String) {
        let at = |name: &str| self.at(name).to_string_lossy().into_owned();
        let mut vars = vec![
            ("CONSENSFLOW_HOME", at("consensflow")),
            ("HOME", at("home")),
            ("CLAUDE_CONFIG_DIR", at("claude")),
            ("PATH", at("bin")),
        ];
        vars.extend(switch.map(|switch| ("CONSENSFLOW_DAEMON", switch.to_owned())));
        let vars: Vec<(&str, &str)> = vars
            .iter()
            .map(|(name, text)| (*name, text.as_str()))
            .collect();
        said(&cf_at(program, args, &vars, ""))
    }

    /// `cf args` as this build runs it for this user, with the switch on.
    fn run(&self, args: &[&str]) -> (Option<i32>, String, String) {
        self.run_at(&own_cf(), Some("native"), args)
    }

    /// A copy of the binary in a folder of its own under this user's: another `cf`.
    fn copy(&self, folder: &str) -> PathBuf {
        let place = self.at(folder);
        fs::create_dir_all(&place).expect("a folder");
        let copy = place.join(own_cf().file_name().expect("a name"));
        fs::copy(own_cf(), &copy).expect("a copy");
        copy
    }
}

/// What `doctor` says of the command: the line, or none.
fn command_line(out: &str) -> Option<&str> {
    out.lines().find(|line| line.starts_with("command:"))
}

const CLAIMS: &str =
    " — another ConsensFlow. `cf` runs that one; `cf setup` from this one claims the command.";

#[test]
fn setup_and_doctor_are_the_switchs_too_and_name_no_runtime() {
    for verb in ["setup", "doctor"] {
        let user = User::new();
        // Off: Node's sources answer, and none is named.
        let (code, out, err) = user.run_at(&own_cf(), None, &[verb]);
        assert_eq!((code, out.as_str()), (Some(1), ""), "{verb}");
        assert!(err.starts_with(NODE_NEEDED), "{verb}: {err}");
        assert!(!user.at("consensflow").exists(), "{verb}: made while off");
        // On: Rust answers, and still none is named.
        let (code, out, err) = user.run(&[verb]);
        assert_eq!((code, err.as_str()), (Some(0), ""), "{verb}");
        assert!(!out.is_empty(), "{verb}");
    }
}

#[test]
fn the_command_setup_makes_is_the_one_doctor_says_runs_this_cf() {
    let user = User::new();
    let own = own_cf();
    // Nothing is said of a command there is none of.
    let (_, out, _) = user.run(&["doctor"]);
    assert_eq!(command_line(&out), None, "{out}");
    assert_eq!(user.run(&["setup"]).0, Some(0));
    for name in ["consensflow", "cf"] {
        let text = fs::read_to_string(user.command(name)).expect("the command");
        assert!(
            text.contains(&format!("\"{}\"", own.display())),
            "{name}: {text}"
        );
    }
    let (code, out, _) = user.run(&["doctor"]);
    assert_eq!(code, Some(0));
    assert_eq!(
        command_line(&out),
        Some(format!("command:      {}", own.display()).as_str()),
        "{out}"
    );
}

#[test]
fn a_command_that_runs_another_cf_is_another_consensflow_until_setup_from_this_one_claims_it() {
    let user = User::new();
    let own = own_cf();
    let other = user.copy("other");
    assert_eq!(user.run_at(&other, Some("native"), &["setup"]).0, Some(0));
    let theirs = format!("command:      {}", other.display());
    let (_, out, _) = user.run(&["doctor"]);
    assert_eq!(
        command_line(&out),
        Some(format!("{theirs}{CLAIMS}").as_str())
    );
    // From the copy it runs, it is plain.
    let (_, out, _) = user.run_at(&other, Some("native"), &["doctor"]);
    assert_eq!(command_line(&out), Some(theirs.as_str()));
    // This one claims it.
    assert_eq!(user.run(&["setup"]).0, Some(0));
    let (_, out, _) = user.run(&["doctor"]);
    assert_eq!(
        command_line(&out),
        Some(format!("command:      {}", own.display()).as_str())
    );
    let (_, out, _) = user.run_at(&other, Some("native"), &["doctor"]);
    assert_eq!(
        command_line(&out),
        Some(format!("command:      {}{CLAIMS}", own.display()).as_str())
    );
}

#[test]
fn a_command_that_runs_a_cf_that_is_gone_is_missing() {
    let user = User::new();
    let gone = user.copy("gone");
    assert_eq!(user.run_at(&gone, Some("native"), &["setup"]).0, Some(0));
    fs::remove_file(&gone).expect("removed");
    let (code, out, _) = user.run(&["doctor"]);
    assert_eq!(code, Some(0));
    assert_eq!(
        command_line(&out),
        Some(
            format!(
                "command:      {} — MISSING. Reinstall from the app to point the command at its cf.",
                gone.display()
            )
            .as_str()
        )
    );
}

/// The command an older build wrote: its runtime and the `cf.mjs` it runs.
fn older_command(home: &Path, runtime: &Path, entry: &Path) -> String {
    if cfg!(windows) {
        format!(
            "@echo off\r\nREM Installed by ConsensFlow. Runs the app's own runtime and its own copy of\r\nREM the CLI, so the terminal and the window never drift apart.\r\nsetlocal\r\nset \"CONSENSFLOW_HOME={}\"\r\n\"{}\" \"{}\" %*\r\n",
            home.display(),
            runtime.display(),
            entry.display()
        )
    } else {
        format!(
            "#!/bin/sh\n# Installed by ConsensFlow. Runs the app's own runtime and its own copy of\n# the CLI, so the terminal and the window never drift apart.\nexport CONSENSFLOW_HOME=\"{}\"\nexec \"{}\" \"{}\" \"$@\"\n",
            home.display(),
            runtime.display(),
            entry.display()
        )
    }
}

#[test]
fn an_older_command_is_this_copys_by_the_cf_mjs_it_runs_and_not_by_the_runtime_that_runs_it() {
    let user = User::new();
    let own = own_cf();
    // A runtime that is there, which is not this one's: any program will do.
    let runtime = own.clone();
    let beside = own.with_file_name("cf.mjs");
    let elsewhere = user.at("elsewhere").join("cf.mjs");
    fs::create_dir_all(user.at("consensflow/bin")).expect("a folder");
    for (entry, said_of_it) in [(&beside, ""), (&elsewhere, CLAIMS)] {
        let text = older_command(&user.at("consensflow"), &runtime, entry);
        fs::write(user.command("consensflow"), text).expect("the command");
        let (code, out, _) = user.run(&["doctor"]);
        assert_eq!(code, Some(0));
        let line = format!("runtime:      {}{said_of_it}", runtime.display());
        assert!(out.lines().any(|each| each == line), "{out}");
    }
}

/// An app opened from Downloads runs from a copy macOS makes for the run,
/// which is gone when the app ends. `setup` is the user's own act, and names
/// the `cf` it runs as, wherever it is; `doctor` says nothing of the place.
/// (What the app's repair at its start does with such a `cf` is left to
/// `cf-launcher`.)
#[cfg(unix)]
#[test]
fn a_cf_run_from_where_macos_translocates_an_app_still_makes_the_command_it_is_asked_to() {
    let user = User::new();
    let transient =
        user.copy("AppTranslocation/0A1B2C/d/ConsensFlow.app/Contents/Resources/cli/bin");
    assert_eq!(
        user.run_at(&transient, Some("native"), &["setup"]).0,
        Some(0)
    );
    let text = fs::read_to_string(user.command("consensflow")).expect("the command");
    assert!(
        text.contains(&format!("exec \"{}\"", transient.display())),
        "{text}"
    );
    let (code, out, _) = user.run_at(&transient, Some("native"), &["doctor"]);
    assert_eq!(code, Some(0));
    assert_eq!(
        command_line(&out),
        Some(format!("command:      {}", transient.display()).as_str())
    );
}

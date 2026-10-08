//! Which `cf` answers the standalone verbs, as a process: this binary answers
//! all of them, `setup` and `doctor` too, with no runtime named and no
//! environment variable asked, and in a home that still has the `use-node`
//! file the flip release sent a home to Node by; and a window's token makes
//! `cf` the board. What the verbs say is held to Node's recording in
//! `cli_goldens`.
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

use common::{cf, cf_at, own_cf, plain};

fn said(ran: &std::process::Output) -> (Option<i32>, String, String) {
    (
        ran.status.code(),
        String::from_utf8_lossy(&ran.stdout).into_owned(),
        String::from_utf8_lossy(&ran.stderr).into_owned(),
    )
}

#[test]
fn every_verb_is_answered_here_whatever_the_old_switch_says_and_no_runtime_is_named_for_it() {
    for args in [&["help"][..], &["--version"], &["catalog"], &["bogus"]] {
        let (_, answer, expected_err) = said(&cf(args, &[], ""));
        for stray in ["native", "node", ""] {
            // `CONSENSFLOW_DAEMON` was the switch before the flip: nothing reads it.
            let ran = said(&cf(args, &[("CONSENSFLOW_DAEMON", stray)], ""));
            assert_eq!(
                ran,
                (ran.0, answer.clone(), expected_err.clone()),
                "{args:?}"
            );
        }
        assert!(
            !expected_err.contains("Node") && !expected_err.contains("CONSENSFLOW_NODE"),
            "{args:?}: {expected_err}"
        );
    }
    let (code, out, err) = said(&cf(&["bogus"], &[], ""));
    assert_eq!(
        (code, out.as_str(), err.as_str()),
        (
            Some(1),
            "",
            "cf: unknown command \"bogus\" — run `cf help`\n"
        )
    );
}

#[test]
fn a_window_token_is_the_board() {
    let (code, out, err) = said(&cf(&["help"], &[("CONSENSFLOW_TOKEN", "participant")], ""));
    assert_eq!((code, err.as_str()), (Some(0), ""));
    assert!(out.starts_with("cf inside a ConsensFlow window"), "{out}");
}

#[test]
fn a_json_word_is_the_verbs_own_and_is_not_taken_out_before_the_verb_reads_it() {
    // The board takes `--json` out wherever it stands; the CLI's verbs are
    // handed the words as they came, so here it is the command.
    let (code, _, err) = said(&cf(&["--json", "catalog"], &[], ""));
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
        // As `cf` reports the places under it, so a copy is named as it says.
        let root = plain(fs::canonicalize(folder.path()).expect("its place"));
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

    /// `cf args` as the binary at `program` runs it for this user, with `extra`
    /// in its environment besides what makes the user's.
    fn run_with(
        &self,
        program: &Path,
        extra: &[(&str, &str)],
        args: &[&str],
    ) -> (Option<i32>, String, String) {
        let at = |name: &str| self.at(name).to_string_lossy().into_owned();
        let mut vars = vec![
            ("CONSENSFLOW_HOME", at("consensflow")),
            ("HOME", at("home")),
            ("CLAUDE_CONFIG_DIR", at("claude")),
            ("PATH", at("bin")),
        ];
        vars.extend(extra.iter().map(|(name, text)| (*name, (*text).to_owned())));
        let vars: Vec<(&str, &str)> = vars
            .iter()
            .map(|(name, text)| (*name, text.as_str()))
            .collect();
        said(&cf_at(program, args, &vars, ""))
    }

    /// `cf args` as the binary at `program` runs it for this user.
    fn run_at(&self, program: &Path, args: &[&str]) -> (Option<i32>, String, String) {
        self.run_with(program, &[], args)
    }

    /// `cf args` as this build runs it for this user.
    fn run(&self, args: &[&str]) -> (Option<i32>, String, String) {
        self.run_at(&own_cf(), args)
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

/// The `use-node` file in `user`'s home: the flip release sent a home that had
/// it to Node's sources, and a user who took that way back may still have it.
fn leave_the_flip_releases_way_back(user: &User) {
    fs::create_dir_all(user.at("consensflow")).expect("the home");
    fs::write(user.at("consensflow").join("use-node"), "").expect("the file");
}

#[test]
fn a_use_node_file_left_in_the_home_changes_no_answer() {
    // Nothing is bundled to send the commands to, and nothing reads the file.
    for args in [
        &["help"][..],
        &["--version"],
        &["catalog", "--harness", "pi"],
        &["agent", "list"],
        &["bogus"],
    ] {
        let plain = User::new();
        let left = User::new();
        leave_the_flip_releases_way_back(&left);
        assert_eq!(left.run(args), plain.run(args), "{args:?}");
    }
}

#[test]
fn setup_and_doctor_in_a_home_with_a_use_node_file_make_and_read_the_command_here() {
    let user = User::new();
    leave_the_flip_releases_way_back(&user);
    assert_eq!(user.run(&["setup"]).0, Some(0));
    let text = fs::read_to_string(user.command("cf")).expect("the command");
    assert!(
        text.contains(&format!("\"{}\"", own_cf().display())),
        "{text}"
    );
    let (code, out, err) = user.run(&["doctor"]);
    assert_eq!((code, err.as_str()), (Some(0), ""));
    assert_eq!(
        command_line(&out),
        Some(format!("command:      {}", own_cf().display()).as_str()),
        "{out}"
    );
    // The file is the user's: nothing took it away.
    assert!(user.at("consensflow").join("use-node").is_file());
}

#[test]
fn setup_and_doctor_are_answered_whatever_the_old_switch_says_and_name_no_runtime() {
    // `CONSENSFLOW_DAEMON` was the switch before the flip: nothing reads it.
    for verb in ["setup", "doctor"] {
        for stray in [None, Some("native"), Some("node"), Some("")] {
            let user = User::new();
            let extra: Vec<_> = stray
                .map(|stray| ("CONSENSFLOW_DAEMON", stray))
                .into_iter()
                .collect();
            let (code, out, err) = user.run_with(&own_cf(), &extra, &[verb]);
            assert_eq!((code, err.as_str()), (Some(0), ""), "{verb}, {stray:?}");
            assert!(!out.is_empty(), "{verb}, {stray:?}");
            // `setup` makes the command in the home, and `doctor` makes nothing.
            assert_eq!(
                user.at("consensflow").exists(),
                verb == "setup",
                "{verb}, {stray:?}"
            );
        }
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
    assert_eq!(user.run_at(&other, &["setup"]).0, Some(0));
    let theirs = format!("command:      {}", other.display());
    let (_, out, _) = user.run(&["doctor"]);
    assert_eq!(
        command_line(&out),
        Some(format!("{theirs}{CLAIMS}").as_str())
    );
    // From the copy it runs, it is plain.
    let (_, out, _) = user.run_at(&other, &["doctor"]);
    assert_eq!(command_line(&out), Some(theirs.as_str()));
    // This one claims it.
    assert_eq!(user.run(&["setup"]).0, Some(0));
    let (_, out, _) = user.run(&["doctor"]);
    assert_eq!(
        command_line(&out),
        Some(format!("command:      {}", own.display()).as_str())
    );
    let (_, out, _) = user.run_at(&other, &["doctor"]);
    assert_eq!(
        command_line(&out),
        Some(format!("command:      {}{CLAIMS}", own.display()).as_str())
    );
}

#[test]
fn a_command_that_runs_a_cf_that_is_gone_is_missing() {
    let user = User::new();
    let gone = user.copy("gone");
    assert_eq!(user.run_at(&gone, &["setup"]).0, Some(0));
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
    assert_eq!(user.run_at(&transient, &["setup"]).0, Some(0));
    let text = fs::read_to_string(user.command("consensflow")).expect("the command");
    assert!(
        text.contains(&format!("exec \"{}\"", transient.display())),
        "{text}"
    );
    let (code, out, _) = user.run_at(&transient, &["doctor"]);
    assert_eq!(code, Some(0));
    assert_eq!(
        command_line(&out),
        Some(format!("command:      {}", transient.display()).as_str())
    );
}

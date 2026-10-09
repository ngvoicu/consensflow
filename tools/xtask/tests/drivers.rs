//! The thin drivers that run cargo as processes: `app test`, `app clippy`,
//! `clippy-windows` (and the archiver it has cc-rs run) and `departures`, started
//! the way `cargo xtask` starts them, from the folders a developer is in, against
//! a stand-in for `cargo`: the fake child, copied under that name into a folder
//! that is all the PATH it has. What each runs, in which folder, with which words
//! and which environment, and the status it answers, are held here (`bench
//! records-memory` is `tests/bench.rs`'s); how each builds its command line, word
//! by word, is the unit tests' (in each module).
#![allow(clippy::disallowed_methods, clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::fs;

use common::{
    arg_line, arg_lines, assert_ran_in, checkout, err, out, programs, shown, var_line, with_cargo,
    xtask,
};

/// What the app's resources are left out with, in the variable Tauri reads.
const NO_RESOURCES: &str = r#"{"bundle":{"resources":null}}"#;

#[test]
fn app_test_runs_cargo_test_in_the_apps_folder_with_its_resources_left_out_and_every_word_after_a_double_dash(
) {
    let ran = with_cargo("TAURI_CONFIG", &["app", "test", "portable::", "a b", ""]);
    assert!(ran.status.success(), "{}", err(&ran));
    let report = out(&ran);
    assert_ran_in(&report, &checkout().join("app").join("src-tauri"));
    assert_eq!(
        arg_lines(&report),
        ["test", "--offline", "--", "portable::", "a b", ""].map(arg_line)
    );
    assert_eq!(
        var_line(&report, "TAURI_CONFIG"),
        Some(shown(NO_RESOURCES).as_str())
    );
    // Nothing of its own: what the child said is all that was said.
    assert_eq!(err(&ran), "");
}

#[test]
fn app_clippy_runs_cargo_clippy_in_the_apps_folder_over_every_target_with_the_words_after_the_denied_warnings(
) {
    let ran = with_cargo("TAURI_CONFIG", &["app", "clippy", "-W", "clippy::all"]);
    assert!(ran.status.success(), "{}", err(&ran));
    let report = out(&ran);
    assert_ran_in(&report, &checkout().join("app").join("src-tauri"));
    assert_eq!(
        arg_lines(&report),
        [
            "clippy",
            "--offline",
            "--all-targets",
            "--",
            "-D",
            "warnings",
            "-W",
            "clippy::all"
        ]
        .map(arg_line)
    );
    assert_eq!(
        var_line(&report, "TAURI_CONFIG"),
        Some(shown(NO_RESOURCES).as_str())
    );
    assert_eq!(err(&ran), "");
}

#[test]
fn departures_runs_the_engines_dispatcher_tests_from_the_root_with_the_variable_that_records_set() {
    // From `app/`, and from nowhere near the checkout: the root is where xtask was built.
    let nowhere = tempfile::tempdir().unwrap();
    let tools = programs(&["cargo"]);
    for cwd in [checkout().join("app"), nowhere.path().to_path_buf()] {
        let ran = xtask(
            &cwd,
            tools.path(),
            &[("FAKE_CHILD_REPORT", "CF_RERECORD_DEPARTED")],
            &["departures"],
        );
        assert!(ran.status.success(), "{}", err(&ran));
        let report = out(&ran);
        assert_ran_in(&report, &checkout());
        assert_eq!(
            arg_lines(&report),
            ["test", "-p", "cf-engine", "--test", "dispatcher"].map(arg_line)
        );
        assert_eq!(
            var_line(&report, "CF_RERECORD_DEPARTED"),
            Some(shown("1").as_str())
        );
        assert_eq!(err(&ran), "");
    }
}

#[test]
fn clippy_windows_lints_the_workspace_but_the_app_for_the_target_from_the_root() {
    let tools = programs(&["cargo"]);
    let ran = xtask(
        &checkout().join("app"),
        tools.path(),
        &[(
            "FAKE_CHILD_REPORT",
            "CC_x86_64_pc_windows_msvc,AR_x86_64_pc_windows_msvc,TAURI_CONFIG,PATH",
        )],
        &["clippy-windows"],
    );
    assert!(ran.status.success(), "{}", err(&ran));
    let report = out(&ran);
    assert_ran_in(&report, &checkout());
    assert_eq!(
        arg_lines(&report),
        [
            "clippy",
            "--offline",
            "--target",
            "x86_64-pc-windows-msvc",
            "--all-targets",
            "--workspace",
            "--exclude",
            "app",
            "--",
            "-D",
            "warnings"
        ]
        .map(arg_line)
    );
    // The C is not compiled; the archive is made by xtask, run as cc-rs runs an
    // archiver: the program it is, then the word that tells it so.
    assert_eq!(
        var_line(&report, "CC_x86_64_pc_windows_msvc"),
        Some(shown("true").as_str())
    );
    let archiver = var_line(&report, "AR_x86_64_pc_windows_msvc").unwrap();
    assert!(
        archiver.starts_with('"') && archiver.ends_with(" --as-archiver\""),
        "{archiver}"
    );
    #[cfg(unix)]
    assert_eq!(
        archiver,
        shown(&format!(
            "{} --as-archiver",
            fs::canonicalize(env!("CARGO_BIN_EXE_xtask"))
                .unwrap()
                .display()
        ))
    );
    // Nothing of the app's, and the PATH as it is: no stand-in is put on it.
    assert_eq!(var_line(&report, "TAURI_CONFIG"), Some("unset"));
    assert_eq!(
        var_line(&report, "PATH"),
        Some(shown(tools.path().to_str().unwrap()).as_str())
    );
    assert_eq!(err(&ran), "");
}

#[test]
fn clippy_windows_names_each_crate_it_is_given_as_a_package_to_lint() {
    let ran = with_cargo("", &["clippy-windows", "cf-daemon", "cf-process"]);
    assert!(ran.status.success(), "{}", err(&ran));
    assert_eq!(
        arg_lines(&out(&ran)),
        [
            "clippy",
            "--offline",
            "--target",
            "x86_64-pc-windows-msvc",
            "--all-targets",
            "-p",
            "cf-daemon",
            "-p",
            "cf-process",
            "--",
            "-D",
            "warnings"
        ]
        .map(arg_line)
    );
}

/// A folder holding a shell script as `cargo`, the only program there is.
#[cfg(unix)]
fn script_as_cargo(body: &str) -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("cargo");
    fs::write(&file, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
    folder
}

#[test]
#[cfg(unix)]
fn clippy_windows_of_the_app_leaves_its_resources_out_and_finds_a_resource_compiler_that_does_nothing(
) {
    // What the app's build script does for its icon and its manifest: runs `llvm-rc`.
    let tools = script_as_cargo(
        r#"llvm-rc /fo out.res in.rc
echo "llvm-rc ended with $?"
echo "config: $TAURI_CONFIG"
echo "args: $*"
exit 7"#,
    );
    let ran = xtask(
        &checkout(),
        tools.path(),
        &[],
        &["clippy-windows", "cf-daemon", "app"],
    );
    assert_eq!(ran.status.code(), Some(7), "{}", err(&ran));
    assert_eq!(
        out(&ran),
        format!(
            "llvm-rc ended with 0\nconfig: {NO_RESOURCES}\nargs: clippy --offline --target \
             x86_64-pc-windows-msvc --all-targets -p cf-daemon -p app -- -D warnings\n"
        )
    );
    assert_eq!(err(&ran), "");

    // Where the app is not linted there is none, and its resources are as they were.
    let ran = xtask(
        &checkout(),
        tools.path(),
        &[("TAURI_CONFIG", "")],
        &["clippy-windows", "cf-daemon"],
    );
    assert_eq!(ran.status.code(), Some(7));
    assert!(
        out(&ran).starts_with("llvm-rc ended with 127\nconfig: \n"),
        "{}",
        out(&ran)
    );
}

#[test]
#[cfg(unix)]
fn the_archiver_cc_rs_is_told_is_xtask_which_makes_each_archive_it_is_asked_for() {
    let archives = tempfile::tempdir().unwrap();
    let (lib, ar) = (
        archives.path().join("sqlite3.lib"),
        archives.path().join("libsqlite3.a"),
    );
    // What cc-rs does with the variable: splits it at the spaces into the program and
    // its first word, and runs that with the archive it wants made, in `lib`'s way or
    // `ar`'s, from a folder of its own (a build script's), not the checkout's.
    let tools = script_as_cargo(
        r#"cd "$FOLDER" || exit 10
$AR_x86_64_pc_windows_msvc -out:"$LIB" -nologo a.o || exit 11
$AR_x86_64_pc_windows_msvc cq "$AR" a.o || exit 12"#,
    );
    let ran = xtask(
        &checkout(),
        tools.path(),
        &[
            ("FOLDER", archives.path().to_str().unwrap()),
            ("LIB", lib.to_str().unwrap()),
            ("AR", ar.to_str().unwrap()),
        ],
        &["clippy-windows"],
    );
    assert_eq!(ran.status.code(), Some(0), "{}", err(&ran));
    assert_eq!(fs::read(&lib).unwrap(), b"");
    assert_eq!(fs::read(&ar).unwrap(), b"");
}

#[test]
fn the_archiver_makes_the_empty_archive_it_is_asked_for_leaves_one_that_is_there_and_says_nothing()
{
    let nowhere = tempfile::tempdir().unwrap();
    let archives = tempfile::tempdir().unwrap();
    let (lib, ar) = (
        archives.path().join("sqlite3.lib"),
        archives.path().join("libsqlite3.a"),
    );
    let lib_line = format!("-out:{}", lib.display());
    for (line, archive) in [
        (vec!["--as-archiver", &lib_line, "-nologo", "a.o"], &lib),
        (
            vec!["--as-archiver", "cq", ar.to_str().unwrap(), "a.o"],
            &ar,
        ),
    ] {
        let ran = xtask(nowhere.path(), nowhere.path(), &[], &line);
        assert_eq!(ran.status.code(), Some(0), "{line:?}: {}", err(&ran));
        assert_eq!(
            (out(&ran), err(&ran)),
            (String::new(), String::new()),
            "{line:?}"
        );
        assert_eq!(fs::read(archive).unwrap(), b"", "{line:?}");

        // A second batch of objects is appended to what is there: it stays.
        fs::write(archive, b"!<arch>\n").unwrap();
        let ran = xtask(nowhere.path(), nowhere.path(), &[], &line);
        assert_eq!(ran.status.code(), Some(0), "{line:?}: {}", err(&ran));
        assert_eq!(fs::read(archive).unwrap(), b"!<arch>\n", "{line:?}");
    }
}

#[test]
fn an_archive_that_cannot_be_made_is_one_error_with_status_1() {
    let nowhere = tempfile::tempdir().unwrap();
    let missing = nowhere.path().join("no-such-folder").join("lib.a");
    let ran = xtask(
        nowhere.path(),
        nowhere.path(),
        &[],
        &["--as-archiver", "cq", missing.to_str().unwrap()],
    );
    assert_eq!(ran.status.code(), Some(1));
    assert_eq!(out(&ran), "");
    let said = err(&ran);
    assert!(
        said.starts_with("xtask: could not make the archive "),
        "{said}"
    );
    assert!(said.ends_with('\n') && said.lines().count() == 1, "{said}");
}

/// The command lines of the thin drivers that run cargo and answer its status.
const CARGO_COMMANDS: [&[&str]; 4] = [
    &["app", "test"],
    &["app", "clippy"],
    &["clippy-windows"],
    &["departures"],
];

#[test]
fn the_exit_status_of_cargo_is_the_commands_own_and_it_says_nothing_of_its_own() {
    let tools = programs(&["cargo"]);
    for words in CARGO_COMMANDS {
        for status in [0, 1, 2, 3, 42, 101, 255] {
            let said = status.to_string();
            let ran = xtask(
                &checkout(),
                tools.path(),
                &[("FAKE_CHILD_EXIT", &said), ("FAKE_CHILD_QUIET", "1")],
                words,
            );
            assert_eq!(ran.status.code(), Some(status), "{words:?}");
            assert_eq!(
                (out(&ran), err(&ran)),
                (String::new(), String::new()),
                "{words:?}"
            );
        }
    }
}

#[test]
fn a_cargo_that_is_not_there_is_one_error_with_status_1() {
    let no_programs = tempfile::tempdir().unwrap();
    for words in CARGO_COMMANDS {
        let ran = xtask(&checkout(), no_programs.path(), &[], words);
        assert_eq!(ran.status.code(), Some(1), "{words:?}");
        assert_eq!(out(&ran), "", "{words:?}");
        assert_eq!(
            err(&ran),
            "xtask: `cargo` was not found: is it installed, and on the PATH?\n",
            "{words:?}"
        );
    }
}

#[test]
fn departures_takes_no_arguments_and_is_refused_with_status_2_before_anything_is_started() {
    let no_programs = tempfile::tempdir().unwrap();
    let ran = xtask(
        &checkout().join("app"),
        no_programs.path(),
        &[],
        &["departures", "--fast"],
    );
    assert_eq!(ran.status.code(), Some(2));
    assert_eq!(out(&ran), "");
    assert_eq!(
        err(&ran),
        "xtask: departures takes no arguments\nsee `cargo xtask --help`\n"
    );
}

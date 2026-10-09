//! `cargo xtask portable pack` and `portable inspect` as processes: what they
//! write where, the status they end with, and what they say of a command line
//! they do not take and of a file they cannot read. Their defaults and every way
//! to give them an argument, on a checkout of their own, are the unit tests'
//! (`src/portable/tests.rs`).
#![allow(clippy::disallowed_methods, clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use common::{checkout, err, out, xtask};

/// The app's exe in the release folder `release` makes: seven bytes.
const APP: &str = "the app";

/// A release folder as `tauri build` leaves it on Windows, in `dir`.
fn release(dir: &Path) {
    for (path, body) in [
        ("ConsensFlow.exe", APP),
        ("cli/bin/cf.exe", "the native cf"),
        ("conpty.dll", "the console host"),
        ("OpenConsole.exe", "its process"),
        ("OpenConsole-LICENSE.txt", "its license"),
    ] {
        let file = dir.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, body).unwrap();
    }
}

/// `cargo xtask <args>` as a developer in `app/` would run it, with no program
/// of any kind to find: these commands start none.
fn run(args: &[&str]) -> Output {
    let nowhere = tempfile::tempdir().unwrap();
    xtask(&checkout().join("app"), nowhere.path(), &[], args)
}

fn path_arg(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// The exe `pack` makes of a fake release folder in `dir`, by the command line
/// alone: where it is.
fn packed(dir: &Path, version: &str) -> PathBuf {
    let release_dir = dir.join("release");
    release(&release_dir);
    let out_dir = dir.join("out");
    let ran = run(&[
        "portable",
        "pack",
        "--release",
        path_arg(&release_dir),
        "--out",
        path_arg(&out_dir),
        "--version",
        version,
    ]);
    assert!(ran.status.success(), "{}", err(&ran));
    out_dir.join(format!("ConsensFlow_{version}_x64-portable.exe"))
}

#[test]
fn pack_writes_the_exe_and_says_where_and_inspect_reads_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let release_dir = dir.path().join("release");
    release(&release_dir);
    let out_dir = dir.path().join("out");

    let ran = run(&[
        "portable",
        "pack",
        "--release",
        path_arg(&release_dir),
        "--out",
        path_arg(&out_dir),
        "--version",
        "3.0.0-alpha.99",
    ]);

    assert_eq!(ran.status.code(), Some(0), "{}", err(&ran));
    let exe = out_dir.join("ConsensFlow_3.0.0-alpha.99_x64-portable.exe");
    assert_eq!(out(&ran), format!("portable: {}\n", exe.display()));
    assert_eq!(err(&ran), "");

    // The app first, the footer's tag last, and the payload between.
    let bytes = fs::read(&exe).unwrap();
    assert!(bytes.starts_with(APP.as_bytes()));
    assert!(bytes.ends_with(b"CFPAYLD1"));
    let length = bytes.len() - APP.len() - 16;
    let found = cf_portable::inspect(&exe).unwrap();
    assert_eq!(found.length, length as u64);

    // From any folder: this one is nowhere near the checkout.
    let nowhere = tempfile::tempdir().unwrap();
    let ran = xtask(
        nowhere.path(),
        nowhere.path(),
        &[],
        &[
            "portable",
            "inspect",
            path_arg(&exe),
            "--version",
            "3.0.0-alpha.99",
        ],
    );
    assert_eq!(ran.status.code(), Some(0), "{}", err(&ran));
    assert_eq!(
        out(&ran),
        format!(
            "payload: {length} bytes\ncrc: {:08x}\nruntime: 3.0.0-alpha.99-{:08x}\n",
            found.crc, found.crc
        )
    );
    assert_eq!(err(&ran), "");
}

#[test]
fn inspect_without_the_version_says_the_payload_and_the_crc_only() {
    let dir = tempfile::tempdir().unwrap();
    let exe = packed(dir.path(), "3.0.0-alpha.99");

    let ran = run(&["portable", "inspect", path_arg(&exe)]);

    assert_eq!(ran.status.code(), Some(0), "{}", err(&ran));
    let found = cf_portable::inspect(&exe).unwrap();
    assert_eq!(
        out(&ran),
        format!("payload: {} bytes\ncrc: {:08x}\n", found.length, found.crc)
    );
}

/// The status is 1, the words are the library's behind `xtask: portable: `, and
/// the file is named.
#[test]
fn a_file_without_the_footer_ends_with_status_1_naming_it() {
    let dir = tempfile::tempdir().unwrap();
    let installed = dir.path().join("ConsensFlow.exe");
    fs::write(&installed, "MZ an installed app, longer than a footer").unwrap();

    let ran = run(&["portable", "inspect", path_arg(&installed)]);

    assert_eq!(ran.status.code(), Some(1));
    assert_eq!(out(&ran), "");
    assert_eq!(
        err(&ran),
        format!(
            "xtask: portable: {} does not end with the portable footer\n",
            installed.display()
        )
    );
}

#[test]
fn a_file_that_is_not_there_ends_with_status_1_naming_it() {
    let dir = tempfile::tempdir().unwrap();
    let absent = dir.path().join("absent.exe");

    let ran = run(&["portable", "inspect", path_arg(&absent)]);

    assert_eq!(ran.status.code(), Some(1));
    assert_eq!(out(&ran), "");
    assert!(
        err(&ran).starts_with(&format!(
            "xtask: portable: could not open {}: ",
            absent.display()
        )),
        "{}",
        err(&ran)
    );
}

/// A relative path is from the checkout's root, not from the folder xtask was
/// started in (`app/`, here, where there is no `Cargo.toml`): the file named in
/// the answer is the root's.
#[test]
fn a_relative_path_is_from_the_checkouts_root_whatever_folder_xtask_is_run_in() {
    let ran = run(&["portable", "inspect", "Cargo.toml"]);

    assert_eq!(ran.status.code(), Some(1));
    assert_eq!(
        err(&ran),
        format!(
            "xtask: portable: {} does not end with the portable footer\n",
            checkout().join("Cargo.toml").display()
        )
    );
}

#[test]
fn a_release_folder_missing_a_piece_ends_with_status_1_naming_it_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let release_dir = dir.path().join("release");
    release(&release_dir);
    fs::remove_file(release_dir.join("OpenConsole.exe")).unwrap();
    let out_dir = dir.path().join("out");

    let ran = run(&[
        "portable",
        "pack",
        "--release",
        path_arg(&release_dir),
        "--out",
        path_arg(&out_dir),
        "--version",
        "3.0.0-alpha.99",
    ]);

    assert_eq!(ran.status.code(), Some(1));
    assert_eq!(out(&ran), "");
    assert_eq!(
        err(&ran),
        format!(
            "xtask: portable: OpenConsole.exe is missing from {}; \
             build first with npm --prefix app run build\n",
            release_dir.display()
        )
    );
    assert!(!out_dir.exists());
}

/// A command line they do not take ends with the status 2 and the pointer to
/// the help, before anything is read or written.
#[test]
fn what_they_do_not_take_ends_with_status_2_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let out_dir = dir.path().join("out");
    let out_arg = path_arg(&out_dir);
    for (args, said) in [
        (
            vec!["portable"],
            "portable takes a command: pack, inspect".to_string(),
        ),
        (
            vec!["portable", "unpack"],
            "unknown command: portable unpack".to_string(),
        ),
        (
            vec!["portable", "pack", "--ou", out_arg],
            "portable pack takes [--release DIR] [--out DIR] [--version X], not --ou".to_string(),
        ),
        (
            vec!["portable", "pack", "--out", out_arg, "here"],
            "portable pack takes [--release DIR] [--out DIR] [--version X], not here".to_string(),
        ),
        (
            vec!["portable", "pack", "--out", out_arg, "--out", out_arg],
            "portable pack: --out is given twice".to_string(),
        ),
        (
            vec!["portable", "pack", "--out"],
            "portable pack: --out needs a value".to_string(),
        ),
        (
            vec!["portable", "inspect"],
            "portable inspect takes EXE [--version X]".to_string(),
        ),
        (
            vec!["portable", "inspect", "a.exe", "b.exe"],
            "portable inspect takes EXE [--version X], not a.exe b.exe".to_string(),
        ),
    ] {
        let ran = run(&args);
        assert_eq!(ran.status.code(), Some(2), "{args:?}");
        assert_eq!(out(&ran), "", "{args:?}");
        assert_eq!(
            err(&ran),
            format!("xtask: {said}\nsee `cargo xtask --help`\n"),
            "{args:?}"
        );
        assert!(!out_dir.exists(), "{args:?}");
    }
}

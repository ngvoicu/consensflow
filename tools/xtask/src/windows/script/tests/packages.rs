//! The packages a run without `--build` stands on: held to the lockfiles the run shipped,
//! and made so (`npm ci` of the folder that differs, said) where they are not, never run on
//! silently. On 2026-10-17 a plant run on zeewin used `app/node_modules` from before
//! `mermaid` was added, and 12 UI plants failed "Could not resolve mermaid".

use std::fs;
use std::path::Path;

use cf_base::env::Env;

use super::*;
use crate::process::{self, Invocation};

/// The marks the run script puts around the check, so that a test can run it alone.
const BEGIN: &str = "# packages: begin\n";
const END: &str = "# packages: end\n";

/// The check as the run script holds it, between its marks.
fn check_in(text: &str) -> String {
    let (_, after) = text
        .split_once(BEGIN)
        .unwrap_or_else(|| panic!("the run script holds no check of its packages: {text}"));
    let (check, _) = after
        .split_once(END)
        .unwrap_or_else(|| panic!("the check of the packages has no end: {text}"));
    check.to_owned()
}

/// A PowerShell of this machine: the Mac's (`pwsh`) or Windows' own; none where the
/// machine has neither, and the tests that run it say so.
fn shell() -> Option<&'static str> {
    ["pwsh", "powershell"].into_iter().find(|name| {
        process::capture(
            &Invocation::new(name, ".").args([
                "-NoProfile",
                "-Command",
                "$PSVersionTable.PSVersion.Major",
            ]),
            &Env::from_process(),
        )
        .is_ok_and(|ran| ran.code == 0)
    })
}

/// A lockfile's `packages`, from `(key, fields)` each: with the root entry first, as npm
/// writes the shipped one, or without it, as it writes the hidden one of `node_modules`.
fn packages(entries: &[(&str, &str)], root: bool) -> String {
    let mut all = Vec::new();
    if root {
        all.push(r#""": {"name": "root"}"#.to_owned());
    }
    all.extend(
        entries
            .iter()
            .map(|(key, fields)| format!(r#""node_modules/{key}": {{{fields}}}"#)),
    );
    format!(
        r#"{{"lockfileVersion": 3, "packages": {{{}}}}}"#,
        all.join(", ")
    )
}

fn lock(entries: &[(&str, &str)]) -> String {
    packages(entries, true)
}

fn hidden(entries: &[(&str, &str)]) -> String {
    packages(entries, false)
}

/// A folder holding `root` and `app`, each with the lockfile it was shipped and the hidden
/// one of its `node_modules` (none where it is `None`).
fn copy(root: (&str, Option<&str>), app: (&str, Option<&str>)) -> tempfile::TempDir {
    let folder = tempfile::tempdir().unwrap();
    for (at, (shipped, installed)) in [(".", root), ("app", app)] {
        let at = folder.path().join(at);
        fs::create_dir_all(at.join("node_modules")).unwrap();
        if !shipped.is_empty() {
            fs::write(at.join("package-lock.json"), shipped).unwrap();
        }
        if let Some(installed) = installed {
            fs::write(
                at.join("node_modules").join(".package-lock.json"),
                installed,
            )
            .unwrap();
        }
    }
    folder
}

/// What the check makes of `copy`: the steps it adds to a run's, and what it said.
fn checked(shell: &str, copy: &Path) -> (Vec<String>, String) {
    let check = check_in(&script(&[], "echo hi", &[]));
    let file = copy.join("check.ps1");
    fs::write(
        &file,
        format!(
            "$ErrorActionPreference = 'Continue'\nSet-Location -LiteralPath '{}'\n$steps = @()\n{check}\nWrite-Output ('STEPS:' + ($steps -join '|'))\n",
            copy.display()
        ),
    )
    .unwrap();
    let ran = process::capture(
        &Invocation::new(shell, copy)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(file.display().to_string()),
        &Env::from_process(),
    )
    .unwrap();
    assert_eq!(ran.code, 0, "{}", said(&ran));
    let out = ran.stdout.replace("\r\n", "\n");
    let steps = out
        .lines()
        .find_map(|line| line.strip_prefix("STEPS:"))
        .unwrap_or_else(|| panic!("the check did not end: {out}"));
    let steps = steps
        .split('|')
        .filter(|step| !step.is_empty())
        .map(str::to_owned)
        .collect();
    (steps, out)
}

const ROOT_STEP: &str = "npm ci --no-audit --no-fund";
const APP_STEP: &str = "npm ci --prefix app --no-audit --no-fund";

#[test]
fn a_run_without_the_build_holds_its_packages_to_the_lockfiles_before_its_steps_and_one_with_it_does_not(
) {
    let without = script(&[], "echo hi", &[]);
    let check = check_in(&without);
    // Both folders `npm ci` makes, by the words the build runs: the same two lines.
    assert!(
        check.contains(ROOT_STEP) && check.contains(APP_STEP),
        "{check}"
    );
    // Before the steps run (those the check adds are among them) and before the command's.
    let steps = without
        .find("foreach ($step in $steps)")
        .expect("the steps are run from the list");
    assert!(without.find(BEGIN).unwrap() < steps, "{without}");
    assert!(without.contains("$steps = @()\n"), "{without}");
    // A run that installs them itself has nothing to compare.
    let with = script(&BUILD, "echo hi", &[]);
    assert!(!with.contains(BEGIN), "{with}");
    assert!(
        with.contains("$steps = @('npm ci --no-audit --no-fund', 'npm run build:bridge')"),
        "{with}"
    );
}

#[test]
fn packages_that_are_the_lockfiles_are_left_alone_the_ones_that_are_not_for_this_system_included() {
    let Some(shell) = shell() else {
        eprintln!("skipped: no PowerShell on this machine");
        return;
    };
    // `esbuild`'s binary for the other system is in the lockfile and npm leaves it out; so is one
    // for another processor, and one marked optional that failed to install.
    let shipped = lock(&[
        ("vite", r#""version": "5.0.0", "integrity": "sha512-a""#),
        (
            "@esbuild/other-os",
            r#""version": "1.0.0", "optional": true, "os": ["not-this-one"]"#,
        ),
        (
            "@esbuild/other-cpu",
            r#""version": "1.0.0", "optional": true, "cpu": ["not-this-one"]"#,
        ),
        (
            "@x/not-here",
            r#""version": "1.0.0", "dev": true, "os": ["!win32", "!darwin", "!linux"]"#,
        ),
        ("fsevents", r#""version": "2.3.3", "optional": true"#),
    ]);
    let installed = hidden(&[("vite", r#""version": "5.0.0", "integrity": "sha512-a""#)]);
    let folder = copy((&shipped, Some(&installed)), (&shipped, Some(&installed)));
    let (steps, said) = checked(shell, folder.path());
    assert_eq!(steps, Vec::<String>::new(), "{said}");
    assert!(!said.contains("== packages"), "nothing to say: {said}");
}

#[test]
fn a_folder_whose_packages_are_not_the_lockfiles_is_made_so_by_its_own_npm_ci_and_says_why() {
    let Some(shell) = shell() else {
        eprintln!("skipped: no PowerShell on this machine");
        return;
    };
    let shipped = lock(&[
        ("vite", r#""version": "5.1.0", "integrity": "sha512-b""#),
        ("mermaid", r#""version": "11.4.0", "integrity": "sha512-m""#),
        ("kept", r#""version": "1.0.0""#),
    ]);
    let same = lock(&[("kept", r#""version": "1.0.0""#)]);
    for (what, installed, said_words) in [
        // The 2026-10-17 case: a package added to the lockfile since the build.
        (
            "a package the lockfile added",
            hidden(&[
                ("vite", r#""version": "5.1.0", "integrity": "sha512-b""#),
                ("kept", r#""version": "1.0.0""#),
            ]),
            ["mermaid", "missing"],
        ),
        (
            "a package at another version",
            hidden(&[
                ("vite", r#""version": "5.0.0", "integrity": "sha512-b""#),
                ("mermaid", r#""version": "11.4.0", "integrity": "sha512-m""#),
                ("kept", r#""version": "1.0.0""#),
            ]),
            ["vite", "5.0.0"],
        ),
        (
            "a package the lockfile dropped",
            hidden(&[
                ("vite", r#""version": "5.1.0", "integrity": "sha512-b""#),
                ("mermaid", r#""version": "11.4.0", "integrity": "sha512-m""#),
                ("kept", r#""version": "1.0.0""#),
                ("gone", r#""version": "9.9.9""#),
            ]),
            ["gone", "extra"],
        ),
    ] {
        // Only the folder that differs is made so: the other one is the lockfile's.
        let folder = copy(
            (&same, Some(&hidden(&[("kept", r#""version": "1.0.0""#)]))),
            (&shipped, Some(&installed)),
        );
        let (steps, out) = checked(shell, folder.path());
        assert_eq!(steps, [APP_STEP], "{what}: {out}");
        let line = out
            .lines()
            .find(|line| line.starts_with("== packages"))
            .unwrap_or_else(|| panic!("{what}: said nothing: {out}"));
        assert!(line.contains("app/node_modules"), "{what}: {line}");
        for word in said_words {
            assert!(line.contains(word), "{what}: {word} is not in {line}");
        }
    }
}

#[test]
fn a_folder_with_no_installed_packages_at_all_is_made_so_and_one_with_no_lockfile_is_left_alone() {
    let Some(shell) = shell() else {
        eprintln!("skipped: no PowerShell on this machine");
        return;
    };
    let shipped = lock(&[("vite", r#""version": "5.1.0""#)]);
    let installed = hidden(&[("vite", r#""version": "5.1.0""#)]);
    // Never built in: both, in the order the build runs them.
    let folder = copy((&shipped, None), (&shipped, None));
    let (steps, out) = checked(shell, folder.path());
    assert_eq!(steps, [ROOT_STEP, APP_STEP], "{out}");
    assert_eq!(
        out.lines()
            .filter(|line| line.starts_with("== packages"))
            .count(),
        2,
        "{out}"
    );
    // A tree that has no lockfile in a folder has nothing to hold its packages to there.
    let folder = copy(("", None), (&shipped, Some(&installed)));
    let (steps, out) = checked(shell, folder.path());
    assert_eq!(steps, Vec::<String>::new(), "{out}");
}

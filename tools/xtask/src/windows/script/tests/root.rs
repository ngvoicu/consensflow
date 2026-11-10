//! What the sweep deletes from is the copy's own, and nothing beside it (T-1368, after the second
//! review, T-1377).
//!
//! The sweep removes from the build copy every file the tree it was sent does not list, and prunes
//! `evals/reports` and `.runs` by age. A folder that is a link leads somewhere else: if the copy's
//! root, or the profile above it, is one (a junction a person made to put the build on another disk,
//! a link a program left), the files the sweep takes are the folder's it leads to, not the copy's, and
//! "the tree does not list it" is no reason to remove what was never sent there. So the sweep looks at
//! the profile and at the copy before it removes or writes anything, and where either is a link it
//! says so, removes nothing and fails the run. And it removes nothing beside the copy: the tarballs
//! runs left in the profile were once taken by the shape of their names and their age, which any
//! program's file can have; the run's own tarball is removed by its exact name by the run that made it,
//! and what a run that was killed leaves is for a person to remove.

use std::fs;
use std::path::Path;

use super::powershell::{self, shell};
use super::runs::{aged, sweep_as, sweep_raw, write};
use super::*;

/// A link at `at` to the folder `to`, made by the machine's PowerShell: a junction on Windows (a
/// symbolic link takes a privilege there), a symbolic link on the Mac.
pub(super) fn link(shell: &str, folder: &Path, at: &Path, to: &Path) {
    powershell::run(
        shell,
        folder,
        &format!(
            "$ErrorActionPreference = 'Stop'\nif ($PSVersionTable.PSEdition -eq 'Core' -and -not $IsWindows) {{ New-Item -ItemType SymbolicLink -Path '{at}' -Target '{to}' | Out-Null }} else {{ New-Item -ItemType Junction -Path '{at}' -Target '{to}' | Out-Null }}\n",
            at = at.display(),
            to = to.display()
        ),
    );
}

/// What the sweep would take from a copy, and what is kept in it: the tree's `listed.txt`, a `stray.txt`
/// it does not list, a folder of someone's, and the old files of the two pruned folders.
fn fill(copy: &Path) {
    for name in ["listed.txt", "stray.txt", "someones/personal.txt"] {
        write(copy, name);
    }
    aged(copy, ".runs/old/task.log", 20);
    aged(copy, "evals/reports/old.json", 40);
}

/// Every file `fill` made is still there.
fn whole(copy: &Path) -> bool {
    [
        "listed.txt",
        "stray.txt",
        "someones/personal.txt",
        ".runs/old/task.log",
        "evals/reports/old.json",
    ]
    .iter()
    .all(|name| copy.join(name).is_file())
}

#[test]
fn a_copy_that_is_a_link_is_not_swept_and_the_run_fails_saying_so() {
    let Some(shell) = shell() else {
        eprintln!("skipped: no PowerShell on this machine");
        return;
    };
    let profile = tempfile::tempdir().unwrap();
    // The folder the link leads to: not the copy's, whatever the link says.
    let elsewhere = tempfile::tempdir().unwrap();
    fill(elsewhere.path());
    let copy = profile.path().join("consensflow-build");
    link(shell, profile.path(), &copy, elsewhere.path());
    let out = sweep_raw(shell, &copy, &["listed.txt"], profile.path(), "", "");
    assert!(
        whole(elsewhere.path()),
        "what the link leads to is whole: {out}"
    );
    assert!(
        !out.contains("== removed"),
        "nothing is said removed: {out}"
    );
    assert!(
        out.contains(&format!("{} is a link", copy.display())),
        "the link is named: {out}"
    );
    assert!(out.contains("FAILED=1\n"), "the run is failed by it: {out}");
}

#[test]
fn a_profile_that_is_a_link_is_not_swept_either() {
    let Some(shell) = shell() else {
        eprintln!("skipped: no PowerShell on this machine");
        return;
    };
    let holder = tempfile::tempdir().unwrap();
    // The profile the user has is a link to a folder with a real copy in it.
    let real = tempfile::tempdir().unwrap();
    let inside = real.path().join("consensflow-build");
    fs::create_dir(&inside).unwrap();
    fill(&inside);
    let profile = holder.path().join("profile");
    link(shell, holder.path(), &profile, real.path());
    let copy = profile.join("consensflow-build");
    let out = sweep_raw(shell, &copy, &["listed.txt"], &profile, "", "");
    assert!(whole(&inside), "the copy behind the link is whole: {out}");
    assert!(
        !out.contains("== removed"),
        "nothing is said removed: {out}"
    );
    assert!(
        out.contains(&format!("{} is a link", profile.display())),
        "the profile is named: {out}"
    );
    assert!(out.contains("FAILED=1\n"), "the run is failed by it: {out}");
}

#[test]
fn a_copy_under_a_real_profile_is_swept_as_it_always_was() {
    let Some(shell) = shell() else {
        eprintln!("skipped: no PowerShell on this machine");
        return;
    };
    let profile = tempfile::tempdir().unwrap();
    let copy = profile.path().join("consensflow-build");
    fs::create_dir(&copy).unwrap();
    fill(&copy);
    let out = sweep_as(shell, &copy, &["listed.txt"], profile.path(), "", "");
    assert!(out.contains("== removed stray.txt"), "{out}");
    assert!(out.contains("== removed someones"), "{out}");
    assert!(out.contains("== removed .runs/old/task.log"), "{out}");
    assert!(out.contains("== removed evals/reports/old.json"), "{out}");
    assert!(copy.join("listed.txt").is_file());
    assert!(!copy.join("stray.txt").exists());
}

#[test]
fn nothing_beside_the_copy_is_removed_whatever_its_name_and_age() {
    let Some(shell) = shell() else {
        eprintln!("skipped: no PowerShell on this machine");
        return;
    };
    let profile = tempfile::tempdir().unwrap();
    let copy = profile.path().join("consensflow-build");
    fs::create_dir(&copy).unwrap();
    write(&copy, "listed.txt");
    // Files a run once took by the shape of their names and their age, all of them days old: the copy's
    // own upload names, the name the runs had before each had its own, the upload of a folder that is
    // gone, and another program's file of the same shape with no folder of its name beside it.
    let beside = [
        "consensflow-build.0000abcd-12345678.tgz",
        "consensflow-build.tgz",
        "gone.0000abcd-12345678.tgz",
        "x.0123abcd-89abcdef.tgz",
    ];
    for name in beside {
        aged(profile.path(), name, 3);
    }
    let out = sweep_as(shell, &copy, &["listed.txt"], profile.path(), "", "");
    assert!(copy.join("listed.txt").is_file(), "{out}");
    for name in beside {
        assert!(profile.path().join(name).is_file(), "{name} went: {out}");
    }
}

#[test]
fn the_sweep_names_no_file_beside_the_copy_to_remove() {
    let text = sweep_script();
    // The script holds no look at the profile's files and no removal by `Remove-Item` at all: what it
    // removes is the copy's, by .NET's `Delete`, in the passes that begin at the copy's root.
    for gone in ["Remove-Item", ".tgz", "$beside", "$root.Parent"] {
        assert!(!text.contains(gone), "the sweep still holds {gone}: {text}");
    }
    // The run's own tarball and markers are removed by exact names, by the scripts that made them.
    assert!(bootstrap()
        .contains("Remove-Item -LiteralPath $tarball -Force -ErrorAction SilentlyContinue"));
}

#[test]
fn the_check_of_the_profile_and_the_copy_comes_before_every_pass_that_removes() {
    let text = sweep_script();
    let guard = text
        .find("is a link")
        .expect("the sweep says a link is refused");
    for pass in [
        "Clear-Stale $root",
        "Clear-Old $folder",
        "Save-Tree }",
        "$sha.TransformFinalBlock",
    ] {
        let at = text.find(pass).unwrap_or_else(|| panic!("{pass}: {text}"));
        assert!(guard < at, "the check follows {pass}: {text}");
    }
    // Both folders are looked at, the profile first, and a folder that cannot be looked at ends the run.
    assert!(
        text.contains("foreach ($folder in @($env:USERPROFILE, $dir)) {"),
        "{text}"
    );
    assert!(text.contains("[IO.FileAttributes]::ReparsePoint"), "{text}");
}

//! What one run's sweep leaves of another tree's run (T-1342). The build copy of a machine is one
//! folder that every tree's runs share, one at a time, and the sweep takes from it what the tree
//! of the run that holds the lock does not list. A task keeps its logs in `.runs/<task>` of its
//! tree (untracked, so they go over with the tree's files, and a detached job on the machine
//! writes there too), and the sweep of T-1328's run took nine of T-1329's, `.runs/t1329/*.log`,
//! while T-1329 was still working (zeewin2, 2026-11-08). The copy cannot tell which tree wrote a
//! file that is no tree's, so `.runs` is kept whole, as `evals/reports` is, and cleared by age.

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use cf_base::env::Env;

use super::powershell::{self, shell};
use super::*;
use crate::context::Context;
use crate::windows::tree;

/// A file of the copy `name` (its path from `root`, with `/`), made with its folders.
fn write(root: &Path, name: &str) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, name).unwrap();
}

/// The file `name` of `root`, last written `days` ago.
fn aged(root: &Path, name: &str, days: u64) {
    write(root, name);
    let file = fs::OpenOptions::new()
        .write(true)
        .open(root.join(name))
        .unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(days * 24 * 60 * 60 + 600))
        .unwrap();
}

/// The sweep of a run whose tree lists `listed`, run on the copy `dir` as the bootstrap runs it
/// (the files are there, as the unpack leaves them): what it said.
fn sweep(shell: &str, dir: &Path, listed: &[&str]) -> String {
    let profile = tempfile::tempdir().unwrap();
    fs::write(dir.join(SYNCED), listed.join("\n") + "\n").unwrap();
    let context = Context {
        root: dir.to_path_buf(),
        env: Env::from_process(),
    };
    let names: Vec<String> = listed.iter().map(|name| (*name).to_owned()).collect();
    let hash = tree::content(&context, &names).unwrap();
    fs::write(dir.join(TREE), format!("none {hash}\n")).unwrap();
    fs::write(dir.join(SWEEP), sweep_script()).unwrap();
    let out = powershell::run(
        shell,
        profile.path(),
        &format!(
            "$ProgressPreference = 'SilentlyContinue'\n$ErrorActionPreference = 'Stop'\n$env:USERPROFILE = '{}'\n$dir = '{}'\n. ([scriptblock]::Create([IO.File]::ReadAllText((Join-Path $dir '{SWEEP}'))))\nWrite-Output \"FAILED=$($failed.Count)\"\n",
            profile.path().display(),
            dir.display()
        ),
    );
    assert!(out.contains("FAILED=0"), "the sweep failed: {out}");
    out
}

#[test]
fn the_runs_folder_is_kept_whole_and_pruned_by_age_and_the_copy_is_swept_of_everything_else() {
    let kept = KEPT.iter().position(|name| *name == ".runs");
    assert!(kept.is_some(), "the copy keeps `.runs`: {KEPT:?}");
    assert_eq!(RUNS_KEPT_DAYS, 14);
    let text = sweep_script();
    assert!(
        text.contains("$runs = Join-Path $dir '.runs'"),
        "the sweep prunes `.runs` by age: {text}"
    );
}

#[test]
fn a_run_of_another_tree_leaves_the_logs_the_first_left_in_its_runs_folder_and_takes_the_rest() {
    let Some(shell) = shell() else {
        eprintln!("skipped: no PowerShell on this machine");
        return;
    };
    // A name that is not a dot-file's: PowerShell on the Mac takes a folder so named for hidden.
    let copy = tempfile::Builder::new()
        .prefix("cf-copy-")
        .tempdir()
        .unwrap();
    let dir = copy.path();
    // The first run: tree A, whose `.runs/a/mirror.log` is untracked and so goes over with it.
    for name in ["a-only.txt", "shared.txt", ".runs/a/mirror.log"] {
        write(dir, name);
    }
    let first = sweep(
        shell,
        dir,
        &["a-only.txt", "shared.txt", ".runs/a/mirror.log"],
    );
    assert!(!first.contains("== removed"), "nothing yet: {first}");
    // What A's command writes on the machine, and a job of A's still writing: tree A's logs.
    write(dir, ".runs/a/job.log");
    aged(dir, ".runs/a/young.log", 13);
    aged(dir, ".runs/a/old.log", 15);
    // What no run wants: a stray file, and a folder only named like the logs'.
    write(dir, "stray.txt");
    write(dir, ".runs-other/x.txt");

    // The second run: tree B, whose tree has none of these.
    for name in ["b-only.txt", "shared.txt"] {
        write(dir, name);
    }
    let second = sweep(shell, dir, &["b-only.txt", "shared.txt"]);
    for kept in [
        ".runs/a/mirror.log",
        ".runs/a/job.log",
        ".runs/a/young.log",
        "shared.txt",
        "b-only.txt",
    ] {
        assert!(dir.join(kept).exists(), "{kept} stays: {second}");
    }
    assert!(
        !second.contains("== removed .runs/a/job.log")
            && !second.contains("== removed .runs/a/mirror.log"),
        "the logs of the first run are not said removed: {second}"
    );
    for gone in ["a-only.txt", "stray.txt", ".runs-other", ".runs/a/old.log"] {
        assert!(!dir.join(gone).exists(), "{gone} goes: {second}");
        assert!(
            second.contains(&format!("== removed {gone}")),
            "{gone} is said removed: {second}"
        );
    }
}

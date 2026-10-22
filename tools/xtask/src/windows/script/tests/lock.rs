//! The lock, tried by the machine's own PowerShell: the script of two runs as it is sent,
//! on one folder and one profile, started at the same moment (each waits on a file for the
//! other to be ready, and both go on when it is made), as two workers' runs are when they
//! start together. Of the two exactly one runs and the other is refused with 75, and the
//! one that was refused leaves nothing of its own there. A lock that was looked at and
//! then written let both through (the case is the proof it does not, and the plant of
//! the lock that is only looked at is shown on the machine to fail it).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use cf_base::env::Env;

use super::*;
use crate::context::Context;
use crate::process::{self, Invocation};
use crate::windows::tree;

/// How many pairs of runs are started together.
const ROUNDS: usize = 8;
/// The folder of the copy the runs share.
const SHARED: &str = "race-copy";

/// This process's environment less what a hook that runs the tests would leave in it
/// for git to read.
fn environment() -> Env {
    Env::from_vars(
        Env::from_process()
            .iter()
            .filter(|(name, _)| !name.to_string_lossy().starts_with("GIT_"))
            .map(|(name, value)| (name.to_owned(), value.to_owned())),
    )
}

/// The tarball a run sends of a tree of one file, as `tree::pack` makes it.
fn tarball(scratch: &Path, env: &Env) -> PathBuf {
    let root = scratch.join("tree");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("keep.txt"), "kept").unwrap();
    let context = Context {
        root,
        env: env.clone(),
    };
    for words in [&["init", "-q"][..], &["add", "-A"]] {
        let ran =
            process::capture(&Invocation::new("git", &context.root).args(words), env).unwrap();
        assert_eq!(ran.code, 0, "git {words:?}: {}", ran.stderr);
    }
    let files = tree::files(&context).unwrap();
    let identity = tree::identity(&context, &files).unwrap();
    tree::pack(&context, &files, &identity, scratch).unwrap()
}

/// What one run came to: the status its last line gave, and what it said.
struct Came {
    status: Option<i32>,
    said: String,
}

/// Two runs of the script as it is sent, started together on one profile.
fn together(scratch: &Path, env: &Env) -> (PathBuf, [Came; 2]) {
    let profile = scratch.join("profile");
    fs::create_dir_all(&profile).unwrap();
    let packed = tarball(scratch, env);
    let go = scratch.join("go");
    let mut runs = Vec::new();
    let mut ready = Vec::new();
    for (at, id) in ["0000a001-00000001", "0000a002-00000002"]
        .iter()
        .enumerate()
    {
        fs::copy(&packed, profile.join(upload_name(SHARED, id))).unwrap();
        let (here, file) = (
            scratch.join(format!("ready-{at}")),
            scratch.join(format!("run-{at}.ps1")),
        );
        let sent = remote_script(&Script {
            id,
            dir: SHARED,
            inside: ".",
            steps: &[],
            command: "ping -n 3 127.0.0.1",
            env: &[],
        });
        // Each says it is ready, and waits on the file that the case makes when both are.
        let text = format!(
            "New-Item -ItemType File '{}' | Out-Null\nwhile (-not (Test-Path '{}')) {{ }}\n{sent}",
            here.display(),
            go.display()
        );
        fs::write(&file, text).unwrap();
        let start = Invocation::new("powershell", scratch)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&file)
            .var("USERPROFILE", &profile);
        runs.push(process::spawn(&start, env, Stdio::null(), false).unwrap());
        ready.push(here);
    }
    let until = Instant::now() + Duration::from_secs(120);
    while !ready.iter().all(|file| file.exists()) {
        assert!(Instant::now() < until, "the runs were never ready");
        std::thread::sleep(Duration::from_millis(20));
    }
    fs::write(&go, "").unwrap();
    let mut came = runs.into_iter().map(|run| {
        let done = run.wait_with_output().unwrap();
        let said = String::from_utf8_lossy(&done.stdout).into_owned();
        let status = said.lines().rev().find_map(exit_said);
        Came { status, said }
    });
    (profile, [came.next().unwrap(), came.next().unwrap()])
}

#[test]
fn of_two_runs_started_together_on_one_folder_one_runs_and_the_other_is_refused_every_time() {
    let env = environment();
    for round in 0..ROUNDS {
        let scratch = tempfile::tempdir().unwrap();
        let (profile, runs) = together(scratch.path(), &env);
        let statuses = runs.each_ref().map(|came| came.status);
        let describe = || {
            format!(
                "round {round}: {statuses:?}\n--- first\n{}\n--- second\n{}",
                runs[0].said, runs[1].said
            )
        };
        let (won, refused) = match statuses {
            [Some(0), Some(75)] => (&runs[0], &runs[1]),
            [Some(75), Some(0)] => (&runs[1], &runs[0]),
            _ => panic!("one run, and the other refused with 75: {}", describe()),
        };
        assert!(won.said.contains("== tree none "), "{}", describe());
        assert!(
            refused
                .said
                .contains("A run is still going on this machine"),
            "{}",
            describe()
        );
        assert!(!refused.said.contains("== tree "), "{}", describe());
        // What either sent is gone, the refused one's too; what the run unpacked is there.
        let left: Vec<String> = fs::read_dir(&profile)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tgz"))
            .collect();
        assert!(left.is_empty(), "{left:?}\n{}", describe());
        let copy = profile.join(SHARED);
        assert_eq!(fs::read_to_string(copy.join("keep.txt")).unwrap(), "kept");
        // The lock stays where it is, closed; the record of the run that held it is gone.
        assert!(copy.join(LOCK).exists(), "{}", describe());
        assert!(!copy.join(RUNNING).exists(), "{}", describe());
        assert!(!copy.join(OWNER).exists(), "{}", describe());
    }
}

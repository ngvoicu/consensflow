//! The bootstrap as it is sent, tried by the machine's own PowerShell on a folder and a
//! profile made for each case (`USERPROFILE` is the case's): the lock, the records beside
//! it, the stop that ends a run, and the files unpacked held to the tree that was sent.
//!
//! The first case starts two runs at the same moment (each waits on a file for the other
//! to be ready, and both go on when it is made), as two workers' runs are when they start
//! together. Of the two exactly one runs and the other is refused with 75, and the one that
//! was refused leaves nothing of its own there. A lock that was looked at and then written
//! let both through (the case is the proof it does not, and the plant of the lock that is
//! only looked at is shown on the machine to fail it).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant, SystemTime};

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

/// A checkout of these files, in a repository of its own (no commit: none is needed).
fn checkout(scratch: &Path, env: &Env, files: &[(&str, &[u8])]) -> Context {
    let root = scratch.join("tree");
    fs::create_dir_all(&root).unwrap();
    for (name, bytes) in files {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    let context = Context {
        root,
        env: env.clone(),
    };
    for words in [&["init", "-q"][..], &["add", "-A"]] {
        let ran =
            process::capture(&Invocation::new("git", &context.root).args(words), env).unwrap();
        assert_eq!(ran.code, 0, "git {words:?}: {}", ran.stderr);
    }
    context
}

/// The tarball a run sends of `context`'s tree, with `identity` as the tree's and a run
/// script that runs `command`, as `tree::pack` makes it.
fn tarball(scratch: &Path, context: &Context, identity: &str, command: &str, id: &str) -> PathBuf {
    let files = tree::files(context).unwrap();
    let run = run_script(&Script {
        id,
        dir: SHARED,
        inside: ".",
        steps: &[],
        command,
        env: &[],
    });
    let into = scratch.join(id);
    fs::create_dir_all(&into).unwrap();
    tree::pack(context, &files, identity, &run, &into).unwrap()
}

/// What one run came to: the status its last line gave, and what it said.
struct Came {
    status: Option<i32>,
    said: String,
}

impl Came {
    fn of(run: Child) -> Self {
        let done = run.wait_with_output().unwrap();
        let said = String::from_utf8_lossy(&done.stdout).into_owned();
        let status = said.lines().rev().find_map(exit_said);
        Self { status, said }
    }
}

/// A run of the bootstrap named `id` on `profile`, with `packed` as its tarball, started
/// at once or, with a `gate`, when the file `go` is made (it makes `ready` first).
fn start(
    scratch: &Path,
    env: &Env,
    profile: &Path,
    id: &str,
    packed: &Path,
    gate: Option<(&Path, &Path)>,
) -> Child {
    fs::copy(packed, profile.join(upload_name(SHARED, id))).unwrap();
    let sent = remote_script(&Script {
        id,
        dir: SHARED,
        inside: ".",
        steps: &[],
        command: "",
        env: &[],
    });
    let text = match gate {
        Some((ready, go)) => format!(
            "New-Item -ItemType File '{}' | Out-Null\nwhile (-not (Test-Path '{}')) {{ }}\n{sent}",
            ready.display(),
            go.display()
        ),
        None => sent,
    };
    let file = scratch.join(format!("run-{id}.ps1"));
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
        .var("USERPROFILE", profile);
    process::spawn(&start, env, Stdio::null(), false).unwrap()
}

/// A PowerShell that does nothing for a minute: a live process of the kind a run is.
fn sleeper(env: &Env) -> Child {
    let start = Invocation::new("powershell", ".").args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "Start-Sleep 60",
    ]);
    process::spawn(&start, env, Stdio::null(), false).unwrap()
}

/// Ends a process a case started, and waits for it.
fn end(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Waits until `what` holds, or ends the case saying it never did.
fn until(what: &str, holds: impl Fn() -> bool) {
    let until = Instant::now() + Duration::from_secs(120);
    while !holds() {
        assert!(Instant::now() < until, "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The `.tgz` files in `profile`.
fn tarballs_in(profile: &Path) -> Vec<String> {
    fs::read_dir(profile)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tgz"))
        .collect()
}

/// Two runs of the script as it is sent, started together on one profile.
fn together(scratch: &Path, env: &Env) -> (PathBuf, [Came; 2]) {
    let profile = scratch.join("profile");
    fs::create_dir_all(&profile).unwrap();
    let context = checkout(scratch, env, &[("keep.txt", b"kept")]);
    let identity = tree::identity(&context, &tree::files(&context).unwrap()).unwrap();
    let go = scratch.join("go");
    let ids = ["0000a001-00000001", "0000a002-00000002"];
    let readies: Vec<PathBuf> = (0..2)
        .map(|at| scratch.join(format!("ready-{at}")))
        .collect();
    let runs: Vec<Child> = ids
        .iter()
        .zip(&readies)
        .map(|(id, ready)| {
            let packed = tarball(scratch, &context, &identity, "ping -n 3 127.0.0.1", id);
            start(scratch, env, &profile, id, &packed, Some((ready, &go)))
        })
        .collect();
    until("the runs were never ready", || {
        readies.iter().all(|file| file.exists())
    });
    fs::write(&go, "").unwrap();
    let mut came = runs.into_iter().map(Came::of);
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
        let left = tarballs_in(&profile);
        assert!(left.is_empty(), "{left:?}\n{}", describe());
        let copy = profile.join(SHARED);
        assert_eq!(fs::read_to_string(copy.join("keep.txt")).unwrap(), "kept");
        // The lock stays where it is, closed; the records of the run that held it are gone.
        assert!(copy.join(LOCK).exists(), "{}", describe());
        assert!(!copy.join(RUNNING).exists(), "{}", describe());
        assert!(!copy.join(OWNER).exists(), "{}", describe());
    }
}

/// A profile with one run's tarball in it, and the scratch the case works in.
struct Case {
    scratch: tempfile::TempDir,
    env: Env,
    profile: PathBuf,
}

impl Case {
    fn new() -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let profile = scratch.path().join("profile");
        fs::create_dir_all(profile.join(SHARED)).unwrap();
        Self {
            scratch,
            env: environment(),
            profile,
        }
    }

    /// One run of `command` on a tree of `files` sent with `identity` (the tree's own
    /// when none is given).
    fn run(
        &self,
        id: &str,
        files: &[(&str, &[u8])],
        identity: Option<&str>,
        command: &str,
    ) -> Child {
        let context = checkout(self.scratch.path(), &self.env, files);
        let made = tree::identity(&context, &tree::files(&context).unwrap()).unwrap();
        let packed = tarball(
            self.scratch.path(),
            &context,
            identity.unwrap_or(&made),
            command,
            id,
        );
        start(
            self.scratch.path(),
            &self.env,
            &self.profile,
            id,
            &packed,
            None,
        )
    }

    fn copy(&self) -> PathBuf {
        self.profile.join(SHARED)
    }
}

#[test]
fn files_that_do_not_hash_to_the_identity_end_the_run_before_it_says_a_tree_or_runs_anything() {
    let case = Case::new();
    let ran = Came::of(case.run(
        "0000b002-00000002",
        &[("keep.txt", b"kept")],
        Some("none 0000000000000000"),
        "ping -n 3 127.0.0.1",
    ));
    assert_eq!(ran.status, Some(1), "{}", ran.said);
    assert!(
        ran.said
            .contains("The files unpacked are not the tree that was sent: they hash to "),
        "{}",
        ran.said
    );
    assert!(
        ran.said.contains("and its identity says 0000000000000000."),
        "{}",
        ran.said
    );
    assert!(!ran.said.contains("== tree "), "{}", ran.said);
    assert!(!ran.said.contains("Pinging"), "{}", ran.said);
    assert!(tarballs_in(&case.profile).is_empty());
}

#[test]
fn a_lock_that_cannot_be_opened_for_a_cause_but_its_being_open_is_refused_with_75_and_the_cause() {
    // A folder where the lock's file would be: the open is refused as no file, not as an
    // open one. The run ends with its exit line all the same, and takes its tarball away.
    let case = Case::new();
    fs::create_dir_all(case.copy().join(LOCK)).unwrap();
    let ran = Came::of(case.run(
        "0000b003-00000003",
        &[("keep.txt", b"kept")],
        None,
        "cmd /c exit 0",
    ));
    assert_eq!(ran.status, Some(75), "{}", ran.said);
    assert!(
        ran.said.contains("A run is still going on this machine"),
        "{}",
        ran.said
    );
    assert!(
        ran.said.contains("(the lock could not be opened: "),
        "{}",
        ran.said
    );
    assert!(tarballs_in(&case.profile).is_empty());
}

#[test]
fn a_record_of_the_older_form_that_names_no_process_of_a_run_does_not_stop_a_run() {
    let case = Case::new();
    let record = case.copy().join(RUNNING);
    let wanderer = sleeper(&case.env);
    let id = wanderer.id();
    for (what, text) in [
        ("an empty file", String::new()),
        ("a word", "not-a-process".to_owned()),
        ("a process that is gone", "2147483000".to_owned()),
        // A process that is alive and is no PowerShell: this test's own.
        ("another program", std::process::id().to_string()),
    ] {
        fs::write(&record, text).unwrap();
        let ran = Came::of(case.run(
            "0000b004-00000004",
            &[("keep.txt", b"kept")],
            None,
            "cmd /c exit 0",
        ));
        assert_eq!(ran.status, Some(0), "{what}: {}", ran.said);
        assert!(!case.copy().join(RUNNING).exists(), "{what}");
    }
    // A PowerShell that was started after the file was written is a process id that was
    // given again: the file is older than the process, so it is no run's.
    fs::write(&record, id.to_string()).unwrap();
    fs::File::options()
        .write(true)
        .open(&record)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000))
        .unwrap();
    let ran = Came::of(case.run(
        "0000b005-00000005",
        &[("keep.txt", b"kept")],
        None,
        "cmd /c exit 0",
    ));
    assert_eq!(
        ran.status,
        Some(0),
        "a process id given again: {}",
        ran.said
    );
    end(wanderer);
}

#[test]
fn a_powershell_that_holds_the_older_file_and_was_started_before_it_was_written_refuses_a_run() {
    let case = Case::new();
    let holder = sleeper(&case.env);
    // Let it start, then write the file: the process is older than the file.
    std::thread::sleep(Duration::from_secs(2));
    let record = case.copy().join(RUNNING);
    let holder_id = holder.id();
    fs::write(&record, holder_id.to_string()).unwrap();
    let ran = Came::of(case.run(
        "0000b006-00000006",
        &[("keep.txt", b"kept")],
        None,
        "cmd /c exit 0",
    ));
    end(holder);
    assert_eq!(ran.status, Some(75), "{}", ran.said);
    assert!(
        ran.said.contains(&format!(
            "A run is still going on this machine (process {holder_id})"
        )),
        "{}",
        ran.said
    );
    assert!(tarballs_in(&case.profile).is_empty());
}

#[test]
fn a_stop_ends_the_run_the_owner_file_names_by_its_process_whatever_the_older_file_says() {
    // A script of the older form wrote its own process over the older file while this run
    // held the lock (the milliseconds between the open and the write): the stop must end
    // the run, not the process in that file, and must leave that file to the script it is.
    let case = Case::new();
    let id = "0000b007-00000007";
    let run = case.run(id, &[("keep.txt", b"kept")], None, "ping -n 60 127.0.0.1");
    until("the run never named itself", || {
        case.copy().join(OWNER).exists()
    });
    let other = sleeper(&case.env);
    let other_id = other.id();
    fs::write(case.copy().join(RUNNING), other_id.to_string()).unwrap();
    let stop = process::capture(
        &Invocation::new("powershell", ".")
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
            .arg(encode_script(&stop_script(SHARED, id)).unwrap())
            .var("USERPROFILE", &case.profile),
        &case.env,
    )
    .unwrap();
    assert_eq!(stop.code, 0, "{}{}", stop.stdout, stop.stderr);
    // The run is gone: what the stop ends is the run, so it is the run's process that is waited
    // for (a minute at most), not the end of its output, which stays open as long as any
    // process that holds a copy of it lives, the run's own or another's.
    let mut run = run;
    let until = Instant::now() + Duration::from_secs(60);
    while run.try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < until,
            "the run's process outlived its stop by a minute"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(!case.copy().join(OWNER).exists());
    // The process in the older file is alive and its file is as it was written.
    let left = fs::read_to_string(case.copy().join(RUNNING)).unwrap();
    assert_eq!(left.trim(), other_id.to_string());
    assert!(process::capture(
        &Invocation::new("powershell", ".").args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!("if (Get-Process -Id {other_id} -ErrorAction SilentlyContinue) {{ exit 0 }} else {{ exit 3 }}"),
        ]),
        &case.env,
    )
    .unwrap()
    .code
        == 0);
    end(other);
    assert!(tarballs_in(&case.profile).is_empty());
}

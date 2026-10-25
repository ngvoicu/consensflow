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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
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

/// What one run came to: the status its last line gave, what it said, how its process
/// ended, and what its error stream held (as text).
struct Came {
    status: Option<i32>,
    said: String,
    code: Option<i32>,
    errors: String,
}

impl Came {
    fn of(run: Child) -> Self {
        let done = run.wait_with_output().unwrap();
        let said = String::from_utf8_lossy(&done.stdout).into_owned();
        let status = said.lines().rev().find_map(exit_said);
        let errors = said_on_stderr(&String::from_utf8_lossy(&done.stderr));
        Self {
            status,
            said,
            code: done.status.code(),
            errors,
        }
    }

    /// Everything the run left, for a failed case to say.
    fn describe(&self) -> String {
        format!(
            "status {:?}, process code {:?}\n--- it said\n{}\n--- its error stream\n{}",
            self.status, self.code, self.said, self.errors
        )
    }
}

/// The runs a case has started and not yet seen through: ended when the case ends, however it
/// ends, so that one that fails leaves no PowerShell spinning on its gate (up to
/// [`GATE_SECONDS`]) to slow down every case that runs after it.
struct Started(Vec<Child>);

impl Drop for Started {
    fn drop(&mut self) {
        for run in &mut self.0 {
            let _ = run.kill();
            let _ = run.wait();
        }
    }
}

/// How long the runs of a case have to make their `ready` files, in seconds.
const READY_SECONDS: u64 = 120;

/// Waits until every run has made its `ready` file. A run whose process ended without making
/// it never will: the case fails at once, saying how that run ended and what it left (CI run
/// 40132724600, 2026-10-24: "the runs were never ready" after two minutes, and no word of
/// what had become of them). A run still alive and not ready after [`READY_SECONDS`] is said
/// to be that: slow, which is another thing.
fn all_ready(runs: &mut Started, readies: &[PathBuf]) {
    let started = Instant::now();
    loop {
        let mut waiting = Vec::new();
        for (at, (run, ready)) in runs.0.iter_mut().zip(readies).enumerate() {
            if ready.exists() {
                continue;
            }
            if let Some(status) = run.try_wait().unwrap() {
                let read = |stream: Option<&mut dyn std::io::Read>| {
                    let mut bytes = Vec::new();
                    if let Some(stream) = stream {
                        let _ = stream.read_to_end(&mut bytes);
                    }
                    String::from_utf8_lossy(&bytes).into_owned()
                };
                let said = read(run.stdout.as_mut().map(|s| s as &mut dyn std::io::Read));
                let errors = said_on_stderr(&read(
                    run.stderr.as_mut().map(|s| s as &mut dyn std::io::Read),
                ));
                panic!(
                    "run {at} ended ({status}) {:.1} s after it was started and before it was ready\n--- it said\n{said}\n--- its error stream\n{errors}",
                    started.elapsed().as_secs_f32()
                );
            }
            waiting.push(at);
        }
        if waiting.is_empty() {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(READY_SECONDS),
            "runs {waiting:?} were still starting after {READY_SECONDS} s, alive and not ready"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// How long a gate waits for its file, in seconds: longer than the two minutes a case gives
/// the runs to be ready.
const GATE_SECONDS: u32 = 180;

/// The PowerShell a gated run begins with: it makes `ready`, then waits for `go` in a tight
/// loop (the runs it holds back must go together) and ends the run with 97 once `seconds` have
/// gone without it. A case that ends before it makes `go` would otherwise leave the run
/// spinning on the machine for good.
fn gate_script(ready: &Path, go: &Path, seconds: u32) -> String {
    format!(
        "New-Item -ItemType File '{}' | Out-Null\n$watch = [Diagnostics.Stopwatch]::StartNew()\nwhile (-not (Test-Path '{}')) {{ if ($watch.Elapsed.TotalSeconds -gt {seconds}) {{ exit 97 }} }}\n",
        ready.display(),
        go.display()
    )
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
    let before = match gate {
        Some((ready, go)) => gate_script(ready, go, GATE_SECONDS),
        None => String::new(),
    };
    start_after(scratch, env, profile, id, packed, &before)
}

/// A run of the bootstrap named `id` on `profile`, with `packed` as its tarball, after the
/// PowerShell `before`.
fn start_after(
    scratch: &Path,
    env: &Env,
    profile: &Path,
    id: &str,
    packed: &Path,
    before: &str,
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
    let file = scratch.join(format!("run-{id}.ps1"));
    fs::write(&file, format!("{before}{sent}")).unwrap();
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

/// `env` with `name` set to `value`, whatever the case of the name it had there (Windows
/// keeps `Path`, and reads every name in any case).
fn with_var(env: &Env, name: &str, value: impl Into<std::ffi::OsString>) -> Env {
    let value = value.into();
    Env::from_vars(
        env.iter()
            .filter(|(given, _)| !given.eq_ignore_ascii_case(name))
            .map(|(given, was)| (given.to_owned(), was.to_owned()))
            .chain([(name.into(), value)]),
    )
}

/// Git for Windows' folder of the programs its bash runs (`usr\bin`), which holds a GNU tar:
/// the folder CI's Git Bash puts first on the PATH.
fn gits_usr_bin(env: &Env) -> PathBuf {
    let ran = process::capture(&Invocation::new("git", ".").arg("--exec-path"), env).unwrap();
    assert_eq!(ran.code, 0, "git --exec-path: {}", ran.stderr);
    // `<Git>/mingw64/libexec/git-core`.
    let exec = PathBuf::from(ran.stdout.trim());
    let usr_bin = exec.ancestors().nth(3).unwrap().join("usr").join("bin");
    assert!(
        usr_bin.join("tar.exe").is_file(),
        "no tar.exe in {}: the case would prove nothing",
        usr_bin.display()
    );
    usr_bin
}

/// How long a program a case holds lives if the case never ends it, in seconds: longer than
/// any case takes on a machine under load (a case that took more than the minute the sleepers
/// had found them gone, zeewin, 2026-10-25), and not for ever.
const HELD_SECONDS: u32 = 300;

/// A program a case started and waits on, ended when the case ends however it ends: a case
/// that fails must not leave it on the machine for the [`HELD_SECONDS`].
struct Held {
    child: Child,
    since: Instant,
}

impl Held {
    fn new(child: Child) -> Self {
        Self {
            child,
            since: Instant::now(),
        }
    }

    fn id(&self) -> u32 {
        self.child.id()
    }

    /// Whether it still runs, or how long after it was started it was found to have ended.
    fn runs(&mut self) -> Result<(), String> {
        match self.child.try_wait().unwrap() {
            None => Ok(()),
            Some(status) => Err(format!(
                "it ended ({status}) {:?} after it was started",
                self.since.elapsed()
            )),
        }
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A PowerShell that does nothing for [`HELD_SECONDS`]: a live process of the kind a run is.
fn sleeper(env: &Env) -> Held {
    let start = Invocation::new("powershell", ".").args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        &format!("Start-Sleep {HELD_SECONDS}"),
    ]);
    Held::new(process::spawn(&start, env, Stdio::null(), false).unwrap())
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

/// The programs of a case that are marked by the address they ping (`mark`): their process ids.
/// The processes are asked of the system by their command line, so that nothing another case
/// of this binary starts is taken for one. A question the system does not answer is asked
/// again, and then fails the case: no answer is not "none".
fn marked(env: &Env, mark: &str) -> Vec<u32> {
    let script = format!(
        "$ProgressPreference = 'SilentlyContinue'\n$ErrorActionPreference = 'Stop'\nGet-CimInstance Win32_Process -Filter \"Name='PING.EXE'\" | Where-Object {{ $_.CommandLine -like '*{mark}*' }} | ForEach-Object {{ $_.ProcessId }}\n"
    );
    for attempt in 0..4 {
        let ran = process::capture(
            &Invocation::new("powershell", ".")
                .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
                .arg(encode_script(&script).unwrap()),
            env,
        )
        .unwrap();
        if ran.code == 0 {
            return ran
                .stdout
                .lines()
                .filter_map(|line| line.trim().parse().ok())
                .collect();
        }
        assert!(
            attempt < 3,
            "the system would not say: {}",
            said_on_stderr(&ran.stderr)
        );
        std::thread::sleep(Duration::from_millis(500));
    }
    unreachable!("the last attempt panics")
}

/// Ends the programs marked by `mark` when it is dropped, whatever the case ended with:
/// a case that fails must not leave them running on the machine it ran on.
struct Marks<'a> {
    env: &'a Env,
    mark: &'static str,
}

impl Drop for Marks<'_> {
    fn drop(&mut self) {
        // Asked and ended in one script: a program ended by a process id found a moment
        // before (by the next program to start, once the id is given again) may be another's
        // by then, and the gap between two programs is long on a machine under load.
        let script = format!(
            "$ProgressPreference = 'SilentlyContinue'\nGet-CimInstance Win32_Process -Filter \"Name='PING.EXE'\" | Where-Object {{ $_.CommandLine -like '*{}*' }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }}\n",
            self.mark
        );
        let _ = process::capture(
            &Invocation::new("powershell", ".")
                .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
                .arg(encode_script(&script).unwrap()),
            self.env,
        );
    }
}

/// The stop of the run `id`, as ssh has the machine run it: its own PowerShell, with the
/// profile of the case.
fn stop(case: &Case, id: &str) {
    let stop = process::capture(
        &Invocation::new("powershell", ".")
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
            .arg(encode_script(&stop_script(SHARED, id)).unwrap())
            .var("USERPROFILE", &case.profile),
        &case.env,
    )
    .unwrap();
    assert_eq!(stop.code, 0, "{}", said(&stop));
    // It says nothing: what `taskkill` says of the programs the system is ending as it
    // reads them ("could not be terminated", "not supported") is for no one, and the
    // person who stopped the run would have it on their terminal.
    assert!(
        stop.stdout.is_empty() && stop.stderr.is_empty(),
        "{}",
        said(&stop)
    );
}

/// Waits for the run's own process to be gone (a minute at most): what a stop ends is the run, so
/// it is the process that is waited for, not the end of its output, which stays open as long as
/// any process that holds a copy of it lives.
fn gone(mut run: Child) {
    let until = Instant::now() + Duration::from_secs(60);
    while run.try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < until,
            "the run's process outlived its stop by a minute"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
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
    let mut runs = Started(
        ids.iter()
            .zip(&readies)
            .map(|(id, ready)| {
                let packed = tarball(scratch, &context, &identity, "ping -n 3 127.0.0.1", id);
                start(scratch, env, &profile, id, &packed, Some((ready, &go)))
            })
            .collect(),
    );
    all_ready(&mut runs, &readies);
    fs::write(&go, "").unwrap();
    let mut came = std::mem::take(&mut runs.0).into_iter().map(Came::of);
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
                "round {round}: {statuses:?}\n=== first\n{}\n=== second\n{}",
                runs[0].describe(),
                runs[1].describe()
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

#[test]
fn a_gate_nobody_opens_ends_its_run_with_97_and_one_that_is_opened_lets_it_go_on() {
    let case = Case::new();
    for (opened, status) in [(true, 0), (false, 97)] {
        let ready = case.scratch.path().join(format!("gate-ready-{opened}"));
        let go = case.scratch.path().join(format!("gate-go-{opened}"));
        if opened {
            fs::write(&go, "").unwrap();
        }
        let file = case.scratch.path().join(format!("gate-{opened}.ps1"));
        fs::write(&file, format!("{}exit 0\n", gate_script(&ready, &go, 2))).unwrap();
        let start = Invocation::new("powershell", case.scratch.path())
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&file);
        let mut run = process::spawn(&start, &case.env, Stdio::null(), false).unwrap();
        let until = Instant::now() + Duration::from_secs(60);
        let ended = loop {
            if let Some(ended) = run.try_wait().unwrap() {
                break ended;
            }
            if Instant::now() >= until {
                end(run);
                panic!(
                    "the gate (opened: {opened}) was still going a minute after its two seconds"
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        assert_eq!(ended.code(), Some(status), "opened: {opened}");
        assert!(ready.exists(), "opened: {opened}");
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
    drop(wanderer);
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
    drop(holder);
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
    let _marks = Marks {
        env: &case.env,
        mark: "127.0.0.76",
    };
    let id = "0000b007-00000007";
    let run = case.run(id, &[("keep.txt", b"kept")], None, "ping -n 300 127.0.0.76");
    // The command is running: its program is there to be ended with the run.
    until("the run never started its command", || {
        !marked(&case.env, "127.0.0.76").is_empty()
    });
    let mut other = sleeper(&case.env);
    let other_id = other.id();
    fs::write(case.copy().join(RUNNING), other_id.to_string()).unwrap();
    stop(&case, id);
    gone(run);
    // Nothing the run started is running: its `ping -n 300` was five minutes from its end.
    assert_eq!(marked(&case.env, "127.0.0.76"), Vec::<u32>::new());
    assert!(!case.copy().join(OWNER).exists());
    // The process in the older file is alive and its file is as it was written.
    let left = fs::read_to_string(case.copy().join(RUNNING)).unwrap();
    assert_eq!(left.trim(), other_id.to_string());
    if let Err(why) = other.runs() {
        panic!("the process in the older file was ended by the stop, or ended: {why}");
    }
    drop(other);
    assert!(tarballs_in(&case.profile).is_empty());
}

/// How many programs the batch file of [`spawner`] starts that end by themselves, at the
/// most, and how many seconds each lives.
const SPAWNED: usize = 200;
const SPAWNED_SECONDS: usize = 3;

/// A batch file that keeps starting programs, as a test runner does: first `long` that live
/// a minute (a long tree, for the walk of one), then [`SPAWNED`] that end by themselves after
/// [`SPAWNED_SECONDS`], ten a second (so that some thirty are alive at once, as a stop that
/// landed on a run was seen to find, and never a machine full: it was three thousand pings of
/// twelve seconds, started as fast as the machine could, and under load a thousand and more
/// were alive at once, the whole suite took four times as long, and other cases' PowerShells
/// were starved of their start, zeewin and CI, 2026-10-24/25), and last one program that
/// the run waits for, five minutes at the most, so that the run is there for the stop to land
/// on however slow the machine is. `mark` is the address they ping; the wait between two
/// starts is a ping of an address nothing answers, for a tenth of a second.
fn spawner(mark: &str, long: usize) -> Vec<u8> {
    let long = if long == 0 {
        String::new()
    } else {
        format!("for /L %%i in (1,1,{long}) do start \"\" /b ping -n 60 {mark} >nul\r\n")
    };
    format!(
        "@echo off\r\n{long}for /L %%i in (1,1,{SPAWNED}) do (start \"\" /b ping -n {} {mark} >nul & ping -n 1 -w 100 192.0.2.1 >nul)\r\nping -n 300 {mark} >nul\r\n",
        SPAWNED_SECONDS + 1
    )
    .into_bytes()
}

#[test]
fn a_stop_that_lands_while_the_run_is_starting_programs_leaves_none_of_them_running() {
    // Seen on zeewin, 2026-10-23: the stop was `taskkill /T /F`, which reads the tree and then
    // ends what it read, leaves first and the program that starts the others last (about 7 ms
    // each), and whatever is started while it goes on it never read. A stop that landed on a
    // run starting programs left 21 to 50 of them running, every time (and a single `ping -n 60` 7
    // times in 142 when it landed as the run started its command, which then held the
    // run's output open for its whole minute). The run is in a job now, that the system ends
    // with the run's process, started programs and all.
    let mark = "127.0.0.71";
    for round in 0..2 {
        let case = Case::new();
        let _marks = Marks {
            env: &case.env,
            mark,
        };
        let id = format!("0000b1{round:02x}-0000000{round}");
        // Ten of its programs live a minute, so that a run that is not in its job leaves some
        // alive past the grace below (the short ones are gone in 3 s).
        let spawner = spawner(mark, 10);
        let run = case.run(&id, &[("spawn.cmd", &spawner)], None, "spawn.cmd");
        until("the run never started its programs", || {
            marked(&case.env, mark).len() >= 6
        });
        stop(&case, &id);
        gone(run);
        // Still going, for a moment after the run is gone: the system is ending them, and a
        // process it ended is listed until its last handle is closed. Not for the seconds of a
        // ping that nothing ended (the ones the stop did not read lived 11 s).
        let until = Instant::now() + Duration::from_secs(2);
        let mut left = marked(&case.env, mark);
        while !left.is_empty() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(100));
            left = marked(&case.env, mark);
        }
        assert!(
            left.is_empty(),
            "round {round}: {} of the programs the run started are still running after its stop",
            left.len()
        );
        assert!(tarballs_in(&case.profile).is_empty());
    }
}

/// How many programs of other cases are alive at once at the most, and for how many seconds
/// they are started at the most (a case that ends without finishing them stops them).
const BYSTANDERS_ALIVE: usize = 16;
const BYSTANDERS_SECONDS: u64 = 90;

/// The programs of other cases, as any machine has them: while it is at work, one is started
/// as soon as one of at most [`BYSTANDERS_ALIVE`] has ended, each ending by itself, with 0,
/// after about two seconds (`ping -n 3` of the loopback address). Each is given the process id
/// a program that ended a moment ago had, as the system gives them out again. It counts how
/// many it started, and which of them ended with anything else: only something that ended it
/// from outside does.
struct Bystanders {
    over: Arc<AtomicBool>,
    thread: JoinHandle<Seen>,
}

/// What the programs of other cases came to: how many were started, how many of them were
/// given a process id that an earlier one of them had had, and which ended otherwise than by
/// themselves.
struct Seen {
    started: usize,
    reused: usize,
    ended_otherwise: Vec<String>,
}

impl Bystanders {
    fn start(env: &Env) -> Self {
        let over = Arc::new(AtomicBool::new(false));
        let thread = std::thread::spawn({
            let over = Arc::clone(&over);
            let env = env.clone();
            move || {
                let ping = Invocation::new("ping", ".").args(["-n", "3", "127.0.0.9"]);
                let mut alive: Vec<(usize, Child)> = Vec::new();
                let mut started = 0;
                let mut ids = std::collections::HashSet::new();
                let mut reused = 0;
                let mut ended_otherwise = Vec::new();
                let mut reap = |alive: &mut Vec<(usize, Child)>, all: bool| {
                    let mut kept = Vec::new();
                    for (at, mut child) in alive.drain(..) {
                        let status = if all {
                            Some(child.wait().unwrap())
                        } else {
                            child.try_wait().unwrap()
                        };
                        match status {
                            Some(status) if status.code() != Some(0) => {
                                ended_otherwise.push(format!("#{at}: {status}"));
                            }
                            Some(_) => {}
                            None => kept.push((at, child)),
                        }
                    }
                    *alive = kept;
                };
                let until = Instant::now() + Duration::from_secs(BYSTANDERS_SECONDS);
                while !over.load(Ordering::Relaxed) && Instant::now() < until {
                    reap(&mut alive, false);
                    if alive.len() < BYSTANDERS_ALIVE {
                        let child = process::spawn(&ping, &env, Stdio::null(), false).unwrap();
                        if !ids.insert(child.id()) {
                            reused += 1;
                        }
                        alive.push((started, child));
                        started += 1;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                reap(&mut alive, true);
                Seen {
                    started,
                    reused,
                    ended_otherwise,
                }
            }
        });
        Self { over, thread }
    }

    /// Stops starting them, waits for the last to end, and says how many there were and how
    /// each that did not end by itself ended.
    fn finish(self) -> Seen {
        self.over.store(true, Ordering::Relaxed);
        self.thread.join().unwrap()
    }
}

#[test]
fn a_stop_of_a_run_that_is_starting_programs_ends_no_program_that_is_not_the_runs() {
    // CI, 2026-10-24 (run 40132724600): two PowerShells that other cases of this binary had
    // started a moment before were ended by the stop of the case above, one a run of the
    // lock's case (so that it was never ready) and one the sweep's (it said `#< CLIXML` and
    // ended with 1, which is what `taskkill /F` ends a program with). The stop read the tree
    // of a run that was starting programs and ended each program of it by its process id, a
    // walk of seconds; every program of the list that had ended since had its id given to
    // the next program to start, and the walk ended that one. The run is in a job: ending
    // its process ends everything it started, and the tree is not read at all.
    //
    // The run has a long tree (LONG programs that live a minute, and the walk is some 7 ms
    // each), and the short programs it goes on starting end during the walk: their ids go to
    // the programs of other cases, which the walk then reaches. The stop's duration and how
    // many programs were alive are said when the case fails.
    const LONG: usize = 30;
    let mark = "127.0.0.73";
    for round in 0..2 {
        let case = Case::new();
        let _marks = Marks {
            env: &case.env,
            mark,
        };
        let id = format!("0000b4{round:02x}-0000000{round}");
        let spawner = spawner(mark, LONG);
        let run = case.run(&id, &[("spawn.cmd", &spawner)], None, "spawn.cmd");
        until("the run never started its long programs", || {
            marked(&case.env, mark).len() >= LONG
        });
        // It says it is in its job, in the owner file, where a stop reads it.
        let owner = fs::read_to_string(case.copy().join(OWNER)).unwrap();
        let lines: Vec<&str> = owner.lines().map(str::trim).collect();
        assert_eq!(lines.len(), 3, "{owner:?}");
        assert_eq!((lines[0], lines[2]), (id.as_str(), IN_JOB), "{owner:?}");
        std::thread::sleep(Duration::from_secs(2));
        let alive = marked(&case.env, mark).len();
        let bystanders = Bystanders::start(&case.env);
        let stopping = Instant::now();
        stop(&case, &id);
        gone(run);
        let took = stopping.elapsed();
        let Seen {
            started,
            reused,
            ended_otherwise,
        } = bystanders.finish();
        eprintln!(
            "round {round}: the stop took {took:?} with {alive} of the run's programs alive; {started} programs of other cases were started meanwhile, {reused} of them with a process id an earlier one had had; {} ended otherwise than by themselves",
            ended_otherwise.len()
        );
        // Some were started while the stop went on: how many depends on how fast the machine
        // starts a program (8 in the 1.7 s of a stop under load, zeewin, 2026-10-25), and the
        // case proves nothing if none was.
        assert!(
            started >= 1,
            "round {round}: no program of other cases was started while the stop went on ({took:?}, {alive} of the run's alive when it began)"
        );
        assert!(
            ended_otherwise.is_empty(),
            "round {round}: {} of the {started} programs of other cases, started while the stop of a run went on ({took:?}, {alive} of the run's alive when it began), ended otherwise than by themselves: {ended_otherwise:?}",
            ended_otherwise.len()
        );
        assert!(tarballs_in(&case.profile).is_empty());
    }
}

#[test]
fn a_stop_ends_the_process_the_owner_file_names_only_if_it_is_a_powershell_older_than_the_file() {
    // A run that ended leaves its record only if the stop landed as it ended, and its process
    // id is the next program's by then: a stop that ended what the record names would end
    // that program.
    let case = Case::new();
    let id = "0000b402-00000002";
    let owner = case.copy().join(OWNER);
    // A program that is no PowerShell.
    let mut ping = Held::new(
        process::spawn(
            &Invocation::new("ping", ".").args(["-n", &HELD_SECONDS.to_string(), "127.0.0.74"]),
            &case.env,
            Stdio::null(),
            false,
        )
        .unwrap(),
    );
    fs::write(&owner, format!("{id}\n{}\n", ping.id())).unwrap();
    stop(&case, id);
    if let Err(why) = ping.runs() {
        panic!("a ping the owner file named was ended: {why}");
    }
    assert!(!owner.exists(), "the record stayed");
    drop(ping);
    // A PowerShell that was started after the file was written.
    let mut later = sleeper(&case.env);
    fs::write(&owner, format!("{id}\n{}\n", later.id())).unwrap();
    fs::File::options()
        .write(true)
        .open(&owner)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000))
        .unwrap();
    stop(&case, id);
    if let Err(why) = later.runs() {
        panic!("a PowerShell started after the owner file was written was ended: {why}");
    }
    assert!(!owner.exists(), "the record stayed");
    drop(later);
}

#[test]
fn what_a_command_leaves_running_when_the_run_ends_by_itself_is_left_running() {
    // As before the job: a program a command starts and leaves (a daemon a test left, a
    // build's watcher) is not ended by the run's end, only by a stop. The job's end-on-close
    // is taken away where the run ends by itself.
    let mark = "127.0.0.72";
    let case = Case::new();
    let _marks = Marks {
        env: &case.env,
        mark,
    };
    let leave = format!("@echo off\r\nstart \"\" /b ping -n 30 {mark} >nul\r\n").into_bytes();
    let mut run = case.run(
        "0000b201-00000001",
        &[("leave.cmd", &leave)],
        None,
        "leave.cmd",
    );
    // The run's process is gone (its output is not: the program left holds a copy of it).
    let until = Instant::now() + Duration::from_secs(60);
    while run.try_wait().unwrap().is_none() {
        assert!(Instant::now() < until, "the run never ended");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        run.try_wait().unwrap().and_then(|status| status.code()),
        Some(0)
    );
    assert_eq!(
        marked(&case.env, mark).len(),
        1,
        "the program it left is running"
    );
    assert!(tarballs_in(&case.profile).is_empty());
}

#[test]
fn a_stop_that_cannot_take_the_records_away_says_nothing_of_it() {
    // The run that ends by itself takes its records away too, and one that does so between the
    // stop's reading of the owner file and its removal of it, or holds it open, left PowerShell's
    // own "Cannot find path" / "being used by another process" on the stop's error stream (CI,
    // and zeewin under load, 2026-10-25): the stop says nothing, whatever becomes of its records.
    // The owner file is held open for reading by this test, so that it cannot be removed.
    use std::os::windows::fs::OpenOptionsExt;

    let case = Case::new();
    let id = "0000b302-00000002";
    let mut ended = process::spawn(
        &Invocation::new("cmd", ".").args(["/c", "exit", "0"]),
        &case.env,
        Stdio::null(),
        false,
    )
    .unwrap();
    ended.wait().unwrap();
    let pid = ended.id();
    let owner = case.copy().join(OWNER);
    fs::write(&owner, format!("{id}\n{pid}\n")).unwrap();
    fs::write(case.copy().join(RUNNING), pid.to_string()).unwrap();
    // FILE_SHARE_READ: others may read it, none may write it or remove it.
    let holding = fs::File::options()
        .read(true)
        .share_mode(1)
        .open(&owner)
        .unwrap();
    stop(&case, id);
    assert!(owner.exists(), "the file was removed while it was held");
    drop(holding);
}

#[test]
fn a_stop_of_a_run_that_has_already_ended_says_nothing_and_takes_its_records_away() {
    // A Ctrl-C that lands as the run ends, or the records a killed run left: the owner file
    // names a process that is not there. The stop has nothing to end, says nothing (what
    // `taskkill` says of a process it cannot find is for no one) and clears the records.
    let case = Case::new();
    let id = "0000b301-00000001";
    let mut ended = process::spawn(
        &Invocation::new("cmd", ".").args(["/c", "exit", "0"]),
        &case.env,
        Stdio::null(),
        false,
    )
    .unwrap();
    ended.wait().unwrap();
    let pid = ended.id();
    fs::write(case.copy().join(OWNER), format!("{id}\n{pid}\n")).unwrap();
    fs::write(case.copy().join(RUNNING), pid.to_string()).unwrap();
    stop(&case, id);
    assert!(!case.copy().join(OWNER).exists());
    assert!(!case.copy().join(RUNNING).exists());
}

#[test]
fn a_run_started_with_gits_programs_first_on_the_path_unpacks_the_tree_with_windows_own_tar() {
    // As CI's Windows steps start it, from Git Bash: Git's usr\bin, and its GNU tar, before
    // System32 (CI run 39896583469: every such run ended 2 before it said a tree).
    let mut case = Case::new();
    let first = gits_usr_bin(&case.env);
    let path = case.env.search_path().unwrap_or_default().to_owned();
    let mut gits_first = std::env::join_paths([first]).unwrap();
    gits_first.push(";");
    gits_first.push(path);
    case.env = with_var(&case.env, "PATH", gits_first);
    let ran = Came::of(case.run(
        "0000b009-00000009",
        &[("keep.txt", b"kept")],
        None,
        "cmd /c exit 0",
    ));
    assert_eq!(ran.status, Some(0), "{}", ran.said);
    assert!(ran.said.contains("== tree "), "{}", ran.said);
    assert_eq!(
        fs::read_to_string(case.copy().join("keep.txt")).unwrap(),
        "kept"
    );
    assert!(tarballs_in(&case.profile).is_empty());
}

#[test]
fn a_run_on_a_machine_without_windows_own_tar_ends_saying_so_before_it_unpacks_anything() {
    // The system's folder named elsewhere, where there is no tar, by the script itself (a
    // PowerShell started with another SystemRoot does not start): the run ends with its exit
    // line, says why, and takes its tarball away.
    let case = Case::new();
    let elsewhere = case.scratch.path().join("no-system");
    fs::create_dir_all(elsewhere.join("System32")).unwrap();
    let id = "0000b00a-0000000a";
    let context = checkout(case.scratch.path(), &case.env, &[("keep.txt", b"kept")]);
    let made = tree::identity(&context, &tree::files(&context).unwrap()).unwrap();
    let packed = tarball(case.scratch.path(), &context, &made, "cmd /c exit 0", id);
    let ran = Came::of(start_after(
        case.scratch.path(),
        &case.env,
        &case.profile,
        id,
        &packed,
        &format!("$env:SystemRoot = '{}'\n", elsewhere.display()),
    ));
    assert_eq!(ran.status, Some(1), "{}", ran.said);
    assert!(
        ran.said.contains(&format!(
            "Windows' own tar is not there ({}\\System32\\tar.exe), so the tree cannot be unpacked.",
            elsewhere.display()
        )),
        "{}",
        ran.said
    );
    assert!(!ran.said.contains("== tree "), "{}", ran.said);
    assert!(!case.copy().join("keep.txt").exists());
    assert!(tarballs_in(&case.profile).is_empty());
}

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
    let before = match gate {
        Some((ready, go)) => format!(
            "New-Item -ItemType File '{}' | Out-Null\nwhile (-not (Test-Path '{}')) {{ }}\n",
            ready.display(),
            go.display()
        ),
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
        assert!(attempt < 3, "the system would not say: {}", ran.stderr);
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
        for id in marked(self.env, self.mark) {
            let _ = process::capture(
                &Invocation::new("taskkill", ".").args(["/F", "/PID", &id.to_string()]),
                self.env,
            );
        }
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
    assert_eq!(stop.code, 0, "{}{}", stop.stdout, stop.stderr);
    // It says nothing: what `taskkill` says of the programs the system is ending as it
    // reads them ("could not be terminated", "not supported") is for no one, and the
    // person who stopped the run would have it on their terminal.
    assert_eq!((stop.stdout.as_str(), stop.stderr.as_str()), ("", ""));
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
    let _marks = Marks {
        env: &case.env,
        mark: "127.0.0.76",
    };
    let id = "0000b007-00000007";
    let run = case.run(id, &[("keep.txt", b"kept")], None, "ping -n 60 127.0.0.76");
    // The command is running: its program is there to be ended with the run.
    until("the run never started its command", || {
        !marked(&case.env, "127.0.0.76").is_empty()
    });
    let other = sleeper(&case.env);
    let other_id = other.id();
    fs::write(case.copy().join(RUNNING), other_id.to_string()).unwrap();
    stop(&case, id);
    gone(run);
    // Nothing the run started is running: its `ping -n 60` was a minute from its end.
    assert_eq!(marked(&case.env, "127.0.0.76"), Vec::<u32>::new());
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

/// A batch file that keeps starting programs, as a test runner does (a thousand pings of a
/// few seconds each, three thousand starts at the most, so that a stop that fails leaves a
/// few seconds' worth and not a machine full): `mark` is the address they ping.
fn spawner(mark: &str) -> Vec<u8> {
    format!("@echo off\r\nfor /L %%i in (1,1,3000) do start \"\" /b ping -n 12 {mark} >nul\r\n")
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
        let spawner = spawner(mark);
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

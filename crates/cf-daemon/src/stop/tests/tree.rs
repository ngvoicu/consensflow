//! The stop ends the whole tree of what it ends, not the program alone. An
//! update runs through Homebrew or npm, which start programs of their own: a
//! daemon that ended only the program it started said `exit 0` with the
//! installer's children still changing the installation.
//!
//! Each program here is a parent that starts a grandchild and waits for it
//! (a shell and a sleep; on Windows PowerShell and ping), and writes both
//! pids to a file. After the stop neither may be left. A grandchild that
//! holds the program's streams keeps the wait for it going, and one that has
//! let them go does not: so that a stop that ended the parent alone is seen
//! either way, one program of each is run.

use std::path::Path;
use std::time::Instant;

use cf_process::{alive, terminate, Ending};
use tokio::task::JoinHandle;

use super::*;

/// How long the system is given to be rid of a process the stop ended.
const GONE_WITHIN: Duration = Duration::from_secs(20);

/// How long a tree is given to start and write its pids: how soon it starts
/// is not what is tested, and PowerShell started cold beside the crate's other
/// tests on a slow runner took longer than the ending is given (CI's Windows,
/// run 39055885804).
const STARTS_WITHIN: Duration = Duration::from_secs(120);

/// How long the daemon is given, once stopped, to answer the run and the
/// capture: ended with it, they fail at once.
const ENDED_WITHIN: Duration = Duration::from_secs(25);

/// How long the programs of a tree run unless they are ended: longer than all
/// the test waits while they could run (both starts, the ending, and each of
/// the four processes gone), so that a tree the stop left is still there when
/// it is looked for. Were it shorter, a tree that started at once beside one
/// slow to start would end of itself under a stop that ended its parent alone,
/// and pass. `Left` ends them as the test ends.
const LIVES: Duration = Duration::from_secs(600);

// With a minute to spare.
const _: () = assert!(
    LIVES.as_secs()
        > 2 * STARTS_WITHIN.as_secs() + ENDED_WITHIN.as_secs() + 4 * GONE_WITHIN.as_secs() + 60
);

/// A parent that starts a grandchild, waits for it, and writes the pids of
/// both to `file`. Both run for [`LIVES`], and the grandchild holds the
/// parent's streams unless it `lets_them_go` (on Windows PowerShell's never
/// does).
fn tree(file: &Path, #[cfg_attr(windows, allow(unused_variables))] lets_them_go: bool) -> Program {
    let lives = LIVES.as_secs();
    #[cfg(unix)]
    return Program {
        executable: "/bin/sh".into(),
        args: vec![
            "-c".to_owned(),
            format!(
                "sleep {lives} {} & printf '%s %s\\n' \"$$\" \"$!\" > '{}'; wait",
                if lets_them_go {
                    "</dev/null >/dev/null 2>&1"
                } else {
                    ""
                },
                file.display()
            ),
        ],
        cwd: None,
        env: Env::from_vars([("PATH", "/usr/bin:/bin")]),
    };
    #[cfg(windows)]
    return Program {
        executable: r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe".into(),
        args: vec![
            "-NoProfile".to_owned(),
            "-Command".to_owned(),
            format!(
                "$p = Start-Process -PassThru -WindowStyle Hidden -FilePath 'C:\\Windows\\System32\\PING.EXE' -ArgumentList '-n','{lives}','127.0.0.1'; \
                 Set-Content -Path '{}' -Value (@($PID, $p.Id) -join ' '); Start-Sleep {lives}",
                file.display().to_string().replace('\'', "''")
            ),
        ],
        cwd: None,
        // PowerShell wants the system's own variables.
        env: Env::from_process(),
    };
}

/// The pids a tree wrote to `file`, once it has written both: it is running.
/// One whose `program` ended first fails at once, as such, and not a while
/// later as one slow to start.
async fn pids_written_to<T>(file: &Path, program: &JoinHandle<T>) -> Vec<u32> {
    let until = Instant::now() + STARTS_WITHIN;
    while Instant::now() < until {
        let ended = program.is_finished();
        let written = std::fs::read_to_string(file).unwrap_or_default();
        let pids: Vec<u32> = written
            .split_whitespace()
            .filter_map(|word| word.parse().ok())
            .collect();
        // A line is written whole, its end last.
        if pids.len() == 2 && written.ends_with('\n') {
            return pids;
        }
        assert!(
            !ended,
            "the program ended before it wrote its pids to {}",
            file.display()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the program never wrote its pids to {}", file.display());
}

/// Whether the system is rid of `pid` soon: an orphan is its init's to reap.
fn gone(pid: u32) -> bool {
    let until = Instant::now() + GONE_WITHIN;
    while alive(pid) {
        if Instant::now() >= until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    true
}

/// What a test that fails with a tree running ends as it goes, a panic
/// included: the daemon that ran it is the test, and ten minutes are long to
/// leave a sleep. A tree whose pids were never read is ended as the test
/// unwinds, with the run or the capture dropped: on Unix with its group, on
/// Windows the program, its grandchild with this process's job.
struct Left(Vec<u32>);

impl Drop for Left {
    fn drop(&mut self) {
        for &pid in &self.0 {
            if alive(pid) {
                terminate(pid, Ending::Forced);
            }
        }
    }
}

/// The pings a PowerShell started (`PING.EXE`, its children) that are still running.
#[cfg(windows)]
#[allow(clippy::disallowed_methods)]
fn pings_of(parent: u32) -> Vec<u32> {
    let listing = std::process::Command::new(
        r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
    )
    .args(["-NoProfile", "-NonInteractive", "-Command"])
    .arg(format!(
        "Get-CimInstance Win32_Process -Filter \"ParentProcessId={parent} AND Name='PING.EXE'\" | ForEach-Object {{ $_.ProcessId }}"
    ))
    .output()
    .expect("PowerShell lists the processes");
    String::from_utf8_lossy(&listing.stdout)
        .split_whitespace()
        .filter_map(|word| word.parse().ok())
        .collect()
}

/// What a test that fails with a tree whose pids were never written ends as it goes: the
/// program, and the pings it started, which nothing else knows of.
#[cfg(windows)]
struct Unwritten(u32);

#[cfg(windows)]
impl Drop for Unwritten {
    fn drop(&mut self) {
        for pid in pings_of(self.0).into_iter().chain([self.0]) {
            if alive(pid) {
                terminate(pid, Ending::Forced);
            }
        }
    }
}

/// How long a tree whose write failed is given to end by itself: far less than the ten minutes
/// it would sleep, and more than a cold PowerShell takes to start.
#[cfg(windows)]
const ENDS_BY_ITSELF_WITHIN: Duration = Duration::from_secs(60);

/// A tree's write of its pids fails after its ping started (a PowerShell whose `Set-Content`
/// failed): the test fails without the pids, and the pings, which nothing knows of by then,
/// outlived it by up to ten minutes (T-138, the stop test's P2 of 2026-10-17). The tree ends
/// the ping itself, and ends.
#[cfg(windows)]
#[test]
#[allow(clippy::disallowed_methods)]
fn a_tree_whose_write_of_its_pids_failed_ends_the_ping_it_started_and_itself() {
    use std::process::Stdio;

    let dir = tempfile::tempdir().unwrap();
    // Its folder is not there: the write fails once the ping has started.
    let file = dir.path().join("no-such-folder").join("pids");
    let program = tree(&file, false);
    let mut child = std::process::Command::new(&program.executable)
        .args(&program.args)
        .env_clear()
        .envs(program.env.iter())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the tree starts");
    let parent = child.id();
    let _left = Unwritten(parent);
    let until = Instant::now() + ENDS_BY_ITSELF_WITHIN;
    let ended = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= until {
            break None;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let status = ended.unwrap_or_else(|| {
        panic!("the program went on sleeping {ENDS_BY_ITSELF_WITHIN:?} after its write failed")
    });
    assert!(!status.success(), "it failed, and said so: {status}");
    let until = Instant::now() + GONE_WITHIN;
    let mut pings = pings_of(parent);
    while !pings.is_empty() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(200));
        pings = pings_of(parent);
    }
    assert!(
        pings.is_empty(),
        "the ping the program started is still running: {pings:?}"
    );
}

#[tokio::test]
async fn the_whole_trees_of_the_programs_still_running_are_ended_on_the_way_out() {
    LocalSet::new()
        .run_until(async {
            let rig = rig(idle_pass()).await;
            let dir = tempfile::tempdir().unwrap();
            // As the engine's probes of a CLI are (`run`) and the harness
            // admin's update is (`capture`).
            let limits = Limits {
                timeout: Duration::ZERO,
                max_buffer: 1024 * 1024,
            };
            let (ran, captured) = (dir.path().join("run"), dir.path().join("capture"));
            let (running, capturing) = (Rc::clone(&rig.processes), Rc::clone(&rig.processes));
            // The run's grandchild holds its streams, the capture's does not.
            let (run_tree, capture_tree) = (tree(&ran, false), tree(&captured, true));
            let run = tokio::task::spawn_local(async move { running.run(run_tree, limits).await });
            let capture =
                tokio::task::spawn_local(
                    async move { capturing.capture(capture_tree, limits).await },
                );
            let mut left = Left(Vec::new());
            left.0.extend(pids_written_to(&ran, &run).await);
            left.0.extend(pids_written_to(&captured, &capture).await);
            assert!(
                !run.is_finished() && !capture.is_finished(),
                "both are running"
            );

            rig.stopping.run("SIGTERM").await;
            // Left running, they would answer in ten minutes: ended with the
            // daemon, they answer now, as failures.
            let (run, capture) =
                tokio::time::timeout(ENDED_WITHIN, async { (run.await, capture.await) })
                    .await
                    .expect("both were ended with the daemon");
            assert!(run.expect("the run was polled").is_err(), "run");
            assert!(capture.expect("the capture was polled").is_err(), "capture");
            assert_eq!(*rig.exited.borrow(), [0]);
            for (what, pid) in [
                "run's parent",
                "run's grandchild",
                "capture's parent",
                "capture's grandchild",
            ]
            .into_iter()
            .zip(left.0.clone())
            {
                assert!(gone(pid), "{what} is left running after the daemon's stop");
            }
        })
        .await;
}

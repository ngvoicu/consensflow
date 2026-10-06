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

use super::*;

/// How long the system is given to be rid of a process the stop ended.
const GONE_WITHIN: Duration = Duration::from_secs(20);

/// A parent that starts a grandchild, waits for it, and writes the pids of
/// both to `file`. The grandchild runs for a minute, and holds the parent's
/// streams unless it `lets_them_go` (on Windows PowerShell's never does).
fn tree(file: &Path, #[cfg_attr(windows, allow(unused_variables))] lets_them_go: bool) -> Program {
    #[cfg(unix)]
    return Program {
        executable: "/bin/sh".into(),
        args: vec![
            "-c".to_owned(),
            format!(
                "sleep 60 {} & printf '%s %s\\n' \"$$\" \"$!\" > '{}'; wait",
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
                "$p = Start-Process -PassThru -WindowStyle Hidden -FilePath 'C:\\Windows\\System32\\PING.EXE' -ArgumentList '-n','60','127.0.0.1'; \
                 Set-Content -Path '{}' -Value (@($PID, $p.Id) -join ' '); Start-Sleep 60",
                file.display().to_string().replace('\'', "''")
            ),
        ],
        cwd: None,
        // PowerShell wants the system's own variables.
        env: Env::from_process(),
    };
}

/// The pids a tree wrote to `file`, once it has written both: it is running.
async fn pids_written_to(file: &Path) -> Vec<u32> {
    let until = Instant::now() + GONE_WITHIN;
    while Instant::now() < until {
        let written = std::fs::read_to_string(file).unwrap_or_default();
        let pids: Vec<u32> = written
            .split_whitespace()
            .filter_map(|word| word.parse().ok())
            .collect();
        // A line is written whole, its end last.
        if pids.len() == 2 && written.ends_with('\n') {
            return pids;
        }
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

/// What a test that fails with a tree running ends as it goes: the daemon
/// that ran it is the test, and a minute is long to leave a sleep.
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
            left.0.extend(pids_written_to(&ran).await);
            left.0.extend(pids_written_to(&captured).await);
            assert!(
                !run.is_finished() && !capture.is_finished(),
                "both are running"
            );

            rig.stopping.run("SIGTERM").await;
            // Left running, they would answer in a minute: ended with the
            // daemon, they answer now, as failures.
            let (run, capture) = tokio::time::timeout(Duration::from_secs(25), async {
                (run.await, capture.await)
            })
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

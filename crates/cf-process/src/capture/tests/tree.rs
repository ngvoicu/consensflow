//! What ending a capture ends with it: the tree of the program, not the
//! program alone. An installer (Homebrew, npm) starts programs of its own, and
//! what a stop, a time running out or a dropped wait left of them went on
//! changing the installation after the daemon had said `exit 0`.
//!
//! Each test runs a parent that starts a grandchild, and writes both their
//! pids to a file (a shell and a sleep; on Windows PowerShell and ping), ends
//! the capture one of the ways it can end, and asks the system what is left.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::*;
use crate::testing::{gone, pids_written_to_soon, Survivors};
#[cfg(unix)]
use crate::testing::{gone_within, pids_in};

/// How long the capture of a tree that is ended has to end: far less than the
/// minute its programs run, so that one that is not ended fails the test where
/// it waits, and is not mistaken for one that was.
const ENDS_WITHIN: Duration = Duration::from_secs(25);

/// A parent that starts a grandchild, waits for it, and writes the pids of
/// both to `file`. The grandchild runs for a minute.
fn tree(file: &Path) -> Run {
    #[cfg(unix)]
    return shell(&format!(
        "sleep 60 & printf '%s %s\\n' \"$$\" \"$!\" > '{}'; wait",
        file.display()
    ));
    #[cfg(windows)]
    return Run {
        program: PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"),
        args: [
            "-NoProfile".to_owned(),
            "-Command".to_owned(),
            format!(
                "$p = Start-Process -PassThru -WindowStyle Hidden -FilePath 'C:\\Windows\\System32\\PING.EXE' -ArgumentList '-n','60','127.0.0.1'; \
                 Set-Content -Path '{}' -Value (@($PID, $p.Id) -join ' '); Start-Sleep 60",
                file.display().to_string().replace('\'', "''")
            ),
        ]
        .map(OsString::from)
        .to_vec(),
        verbatim: false,
    };
}

/// What `tree` runs in: PowerShell wants the system's own variables.
fn tree_env() -> Env {
    #[cfg(unix)]
    return system_env();
    #[cfg(windows)]
    return Env::from_process();
}

/// What `capture` of `run` came to, the pids its program wrote to `file`, and
/// whether the capture was still going `wait` after they were written, when
/// its ender forced it, as a process on its way out does.
fn forced_after(
    run: &Run,
    file: &Path,
    wait: Duration,
) -> (Result<Captured, CaptureFailed>, Vec<u32>, bool) {
    let kept: Rc<RefCell<Option<Ender>>> = Rc::default();
    let done = Rc::new(Cell::new(false));
    let env = tree_env();
    let (handed, finished) = (Rc::clone(&kept), Rc::clone(&done));
    let mut left = Survivors::default();
    let (captured, (pids, going)) = runtime()
        .block_on(async {
            let capturing = async {
                let captured = capture(run, None, &env, ROOMY, move |ender| {
                    *handed.borrow_mut() = Some(ender);
                })
                .await;
                finished.set(true);
                captured
            };
            let ending = async {
                let pids = pids_written_to_soon(file, 2).await;
                left.0.clone_from(&pids);
                tokio::time::sleep(wait).await;
                let going = !done.get();
                kept.borrow().as_ref().expect("handed over").force();
                (pids, going)
            };
            tokio::time::timeout(ENDS_WITHIN, async { tokio::join!(capturing, ending) }).await
        })
        .expect("the capture ended with the force");
    (captured, pids, going)
}

#[test]
fn an_ender_ends_the_whole_tree_of_a_capture_still_running() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pids");
    let (captured, pids, going) = forced_after(&tree(&file), &file, Duration::from_millis(300));
    let _left = Survivors(pids.clone());
    assert!(going, "it was running when it was forced");
    assert!(captured.is_err(), "ended, not answered");
    assert!(gone(pids[0]), "the parent");
    assert!(gone(pids[1]), "the grandchild went with it");
}

#[cfg(unix)]
#[test]
fn a_parent_that_ended_before_its_grandchild_leaves_the_grandchild_the_stop_s_to_end() {
    // The parent ends at once, with 0, leaving a grandchild that holds the
    // capture's streams: the capture goes on waiting for it, the parent
    // not yet waited for, and so the program is still the capture's to end.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pids");
    let run = shell(&format!(
        "sleep 60 & printf '%s %s\\n' \"$$\" \"$!\" > '{}'",
        file.display()
    ));
    let (captured, pids, going) = forced_after(&run, &file, Duration::from_millis(500));
    let _left = Survivors(pids.clone());
    assert!(going, "the capture waits for what holds its streams");
    // What its parent said is its answer: the parent had ended with 0.
    assert!(captured.is_ok(), "{captured:?}");
    assert!(gone(pids[1]), "the grandchild was ended with the stop");
}

#[cfg(unix)]
#[test]
fn a_capture_dropped_ends_the_tree_it_started() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pids");
    let (run, env) = (tree(&file), tree_env());
    let pids = runtime().block_on(async {
        let capturing = capture(&run, None, &env, ROOMY, |_| {});
        // The wait for the capture is given up as soon as the tree is there.
        tokio::select! {
            _ = capturing => panic!("a program that sleeps for a minute does not end"),
            pids = pids_written_to_soon(&file, 2) => pids,
        }
    });
    let _left = Survivors(pids.clone());
    // The parent is a zombie until tokio reaps what it was told to kill, and
    // nothing here runs it; the grandchild, which an orphan's init reaps, is
    // the proof.
    assert!(
        gone(pids[1]),
        "the grandchild went with the dropped capture"
    );
}

#[cfg(unix)]
#[test]
fn a_capture_out_of_time_ends_the_whole_tree_it_asked_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pids");
    let limits = Limits {
        timeout: Duration::from_millis(1500),
        max_buffer: 1024,
    };
    let failed = run_now(&tree(&file), None, limits).unwrap_err();
    assert!(failed.killed);
    let pids = pids_in(&file, 2).expect("written long before its time ran out");
    let _left = Survivors(pids.clone());
    assert!(gone(pids[0]), "the parent");
    assert!(gone(pids[1]), "the grandchild, asked to end with it");
}

#[cfg(unix)]
#[test]
fn a_tree_that_ignores_being_asked_is_forced_whole_a_moment_later() {
    // The parent ignores SIGTERM, and what it starts inherits that.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pids");
    let run = shell(&format!(
        "trap '' TERM; sleep 60 & printf '%s %s\\n' \"$$\" \"$!\" > '{}'; wait",
        file.display()
    ));
    let limits = Limits {
        timeout: Duration::from_millis(1500),
        max_buffer: 1024,
    };
    let failed = run_now(&run, None, limits).unwrap_err();
    assert!(failed.killed);
    let pids = pids_in(&file, 2).expect("written long before its time ran out");
    let _left = Survivors(pids.clone());
    assert!(gone(pids[0]), "the parent");
    assert!(gone(pids[1]), "the grandchild, forced with it");
}

#[cfg(unix)]
#[test]
fn a_capture_that_ended_on_its_own_leaves_nothing_for_its_ender_to_end() {
    // The parent lets its child go from every stream and ends at once, so the
    // capture is answered. Its ender is held on, as the daemon's registry
    // holds it, and forced at the stop: the parent has been waited for, its
    // pid is no longer its, nor is the group's id, and what is left in that
    // group stands for whatever may hold the number now.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pids");
    let run = shell(&format!(
        "sleep 60 </dev/null >/dev/null 2>&1 & printf '%s %s\\n' \"$$\" \"$!\" > '{}'",
        file.display()
    ));
    let kept: Rc<RefCell<Option<Ender>>> = Rc::default();
    let handed = Rc::clone(&kept);
    let answered = runtime().block_on(capture(&run, None, &tree_env(), ROOMY, move |ender| {
        *handed.borrow_mut() = Some(ender);
    }));
    assert!(answered.is_ok(), "{answered:?}");
    let pids = pids_in(&file, 2).expect("written before it ended");
    let _left = Survivors(vec![pids[1]]);
    let kept = kept.borrow();
    let ender = kept.as_ref().expect("handed over");
    assert!(!ender.running());
    ender.force();
    assert!(
        !gone_within(pids[1], Duration::from_millis(500)),
        "nothing was sent to the group once its program was waited for"
    );
}

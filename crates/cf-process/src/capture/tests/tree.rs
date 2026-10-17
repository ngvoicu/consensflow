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
use crate::testing::{gone, gone_within, pids_in, pids_written_to_soon, Survivors};

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
fn a_child_that_ignores_being_asked_is_forced_though_its_parent_ended_at_the_ask() {
    // The parent ends at the ask; the child it started takes no notice of it, and
    // would go on after the capture answered (a command of the plants' runner whose
    // child goes on writing the sources once they are put back, 2026-10-16).
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pids");
    let run = shell(&format!(
        "trap 'exit 0' TERM; (trap '' TERM; exec sleep 60) & printf '%s %s\\n' \"$$\" \"$!\" > '{}'; wait",
        file.display()
    ));
    let limits = Limits {
        timeout: Duration::from_millis(1500),
        max_buffer: 1024,
    };
    // What the parent said is the answer: it ended with 0 at the ask.
    let answered = run_now(&run, None, limits);
    assert!(answered.is_ok(), "{answered:?}");
    let pids = pids_in(&file, 2).expect("written long before its time ran out");
    let _left = Survivors(pids.clone());
    assert!(gone(pids[0]), "the parent, ended at the ask");
    assert!(
        gone_within(pids[1], Duration::from_secs(1)),
        "the child that took no notice of the ask, forced once its time to end ran out"
    );
}

#[cfg(unix)]
#[test]
fn held_whole_a_program_that_ended_with_0_at_the_ask_has_failed_out_of_its_time() {
    // The program `capture` answers above, as Node does: held whole, a time that ran
    // out is a time that ran out (a run of the plants that hung, 2026-10-17).
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pids");
    let run = shell(&format!(
        "trap 'exit 0' TERM; (trap '' TERM; exec sleep 60) & printf '%s %s\\n' \"$$\" \"$!\" > '{}'; wait",
        file.display()
    ));
    let limits = Limits {
        timeout: Duration::from_millis(1500),
        max_buffer: 1024,
    };
    let failed = runtime()
        .block_on(capture_whole(&run, None, &tree_env(), limits, |_| {}))
        .unwrap_err();
    assert!(failed.killed, "{failed:?}");
    assert_eq!(failed.code, Some(0), "it did exit with 0, once asked");
    let pids = pids_in(&file, 2).expect("written long before its time ran out");
    let _left = Survivors(pids.clone());
    assert!(gone(pids[0]), "the parent");
    assert!(
        gone_within(pids[1], Duration::from_secs(1)),
        "the child, forced"
    );
}

/// A program that leaves a child running and ends at once: the child has none of
/// its streams, and writes `late` to the file `marked` once `after` has passed;
/// `pids` gets the program's pid and the child's, as `tree` writes them.
fn leaves_a_writer(pids: &Path, marked: &Path, after: u32) -> Run {
    #[cfg(unix)]
    return shell(&format!(
        "(sleep {after}; echo late > '{}') </dev/null >/dev/null 2>&1 & printf '%s %s\\n' \"$$\" \"$!\" > '{}'",
        marked.display(),
        pids.display()
    ));
    #[cfg(windows)]
    return Run {
        program: PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"),
        args: [
            "-NoProfile".to_owned(),
            "-Command".to_owned(),
            format!(
                "$p = Start-Process -PassThru -WindowStyle Hidden -FilePath 'C:\\Windows\\System32\\cmd.exe' -ArgumentList '/d','/c','ping -n {} 127.0.0.1 >NUL & echo late> \"{}\"'; \
                 Set-Content -Path '{}' -Value (@($PID, $p.Id) -join ' ')",
                after + 1,
                marked.display(),
                pids.display().to_string().replace('\'', "''")
            ),
        ]
        .map(OsString::from)
        .to_vec(),
        verbatim: false,
    };
}

#[test]
fn held_whole_what_a_program_left_running_is_ended_once_it_has_ended() {
    // The program ends with 0 at once, and its child would write after the capture
    // answered: as a run's child wrote the sources once they were put back.
    let dir = tempfile::tempdir().unwrap();
    let (file, marked) = (dir.path().join("pids"), dir.path().join("late"));
    let run = leaves_a_writer(&file, &marked, 2);
    let limits = Limits {
        timeout: Duration::from_secs(60),
        max_buffer: 1024,
    };
    let answered = runtime().block_on(capture_whole(&run, None, &tree_env(), limits, |_| {}));
    assert!(answered.is_ok(), "{answered:?}");
    let pids = pids_in(&file, 2).expect("written before it ended");
    let _left = Survivors(pids.clone());
    assert!(
        gone_within(pids[1], Duration::from_secs(1)),
        "the child it left running is ended with it"
    );
    std::thread::sleep(Duration::from_secs(4));
    assert!(!marked.exists(), "nothing it left running wrote after it");
}

#[cfg(unix)]
#[test]
fn held_whole_a_program_has_ended_when_it_exits_though_what_it_left_holds_its_streams() {
    // zeewin, round 7, decision 8: a run held whole was read until its streams closed,
    // as long as a grandchild given them lived, and only then ended. Its child holds
    // them, and would write a moment later.
    let dir = tempfile::tempdir().unwrap();
    let (file, marked) = (dir.path().join("pids"), dir.path().join("late"));
    let run = shell(&format!(
        "(sleep 5; echo late > '{}') & printf '%s %s\\n' \"$$\" \"$!\" > '{}'; echo said",
        marked.display(),
        file.display()
    ));
    let began = Instant::now();
    let answered = runtime().block_on(capture_whole(&run, None, &tree_env(), ROOMY, |_| {}));
    let elapsed = began.elapsed();
    let pids = pids_in(&file, 2).expect("written before it ended");
    let _left = Survivors(pids.clone());
    assert_eq!(
        answered.map(|captured| captured.stdout),
        Ok("said\n".to_owned()),
        "what it wrote is read to the end once what it left was ended"
    );
    assert!(
        elapsed < Duration::from_secs(3),
        "it ended as it exited: {elapsed:?}"
    );
    assert!(
        gone_within(pids[1], Duration::from_secs(1)),
        "the child that held its streams is ended with it"
    );
    std::thread::sleep(Duration::from_secs(4));
    assert!(!marked.exists(), "nothing it left running wrote after it");
}

#[cfg(unix)]
#[test]
fn held_whole_streams_still_held_after_its_end_by_what_is_out_of_reach_fail_the_capture() {
    // A grandchild that left the run's session once its parent had ended (out of
    // reach, the group module says) holds the streams: what was read of them is not
    // all, and the run is not over.
    let dir = tempfile::tempdir().unwrap();
    let escaped = dir.path().join("escaped");
    let run = shell(&format!(
        "perl -MPOSIX -e 'exit if fork; POSIX::setsid(); open my $f, \">\", $ARGV[0]; print $f \"$$\\n\"; close $f; sleep 6' '{escaped}' & \
         while [ ! -s '{escaped}' ]; do sleep 0.01; done; echo said",
        escaped = escaped.display()
    ));
    let began = Instant::now();
    let answered = runtime().block_on(capture_whole(&run, None, &tree_env(), ROOMY, |_| {}));
    let elapsed = began.elapsed();
    let _left = Survivors(pids_in(&escaped, 1).expect("it said where it went"));
    let failed = answered.expect_err("not over");
    assert_eq!(
        failed.message,
        format!("{LEFT_RUNNING}: its output was still held open 2 s after it ended")
    );
    assert_eq!((failed.code, failed.killed), (Some(0), false));
    assert_eq!(failed.stdout, "said\n", "what was read of it is kept");
    assert!(
        elapsed < Duration::from_secs(5),
        "held a moment, no more: {elapsed:?}"
    );
}

#[cfg(windows)]
#[test]
fn held_whole_a_grandchild_started_at_once_is_in_the_job_and_ends_with_it() {
    // Both reviewers' look at round 5: the program ran before it was put in its job, and
    // what it started at once was not in it. It starts a grandchild at once, with none of
    // its streams, and ends at once; the grandchild would write a moment later. Round 7,
    // decision 8 (zeewin): the grandchild inherits the program's streams as Windows hands
    // a child every handle it may inherit, and the run, read until they closed, ended
    // only with it, 3.24 s later.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("writer.cmd"),
        "@echo off\r\nping -n 8 127.0.0.1 >NUL\r\necho late> \"%~dp0late\"\r\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("parent.cmd"),
        "@echo off\r\nstart \"\" /b cmd /d /c \"%~dp0writer.cmd\" <NUL >NUL 2>NUL\r\necho said\r\nexit /b 0\r\n",
    )
    .unwrap();
    let run = Run {
        program: PathBuf::from(r"C:\Windows\System32\cmd.exe"),
        args: ["/d", "/c"]
            .into_iter()
            .map(OsString::from)
            .chain([dir.path().join("parent.cmd").into_os_string()])
            .collect(),
        verbatim: false,
    };
    let limits = Limits {
        timeout: Duration::from_secs(60),
        max_buffer: 1024,
    };
    let began = Instant::now();
    let answered = runtime().block_on(capture_whole(&run, None, &tree_env(), limits, |_| {}));
    let elapsed = began.elapsed();
    assert_eq!(
        answered.map(|captured| captured.stdout.trim().to_owned()),
        Ok("said".to_owned()),
        "what it wrote is read to the end once its job was ended"
    );
    assert!(
        elapsed < Duration::from_secs(3),
        "it ended at once: {elapsed:?}"
    );
    std::thread::sleep(Duration::from_secs(9));
    assert!(
        !dir.path().join("late").exists(),
        "the grandchild was in the job, and ended with it"
    );
}

/// A shell that starts `job` in the background in a group of its own (`setpgid`, as
/// nextest puts each test; a shell's `set -m` does so only where it is bash), with none
/// of its streams, writes its pid and the job's to `pids`, waits until the job is in its
/// group (it marks so first), and then runs `then`: a program that ended first would
/// take the job with its own group. `job` holds no double quote and no `$`.
#[cfg(unix)]
fn job_in_a_group_of_its_own(job: &str, pids: &Path, then: &str) -> Run {
    let ready = pids.with_extension("ready");
    shell(&format!(
        "perl -MPOSIX -e 'POSIX::setpgid(0, 0); exec @ARGV' /bin/sh -c \": > '{ready}'; {job}\" </dev/null >/dev/null 2>&1 & printf '%s %s\\n' \"$$\" \"$!\" > '{pids}'; while [ ! -e '{ready}' ]; do sleep 0.01; done; {then}",
        ready = ready.display(),
        pids = pids.display()
    ))
}

/// Whether the process `pid` leads a group of its own, as the system says, within the
/// moment it takes to make it so.
#[cfg(unix)]
fn leads_a_group(pid: u32) -> bool {
    let pid = i32::try_from(pid).unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until {
        let leads = crate::table::processes()
            .iter()
            .any(|process| process.identity.pid == pid && process.group == pid);
        if leads {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[cfg(unix)]
#[test]
fn held_whole_what_a_program_left_in_a_group_of_its_own_is_ended_once_it_has_ended() {
    // Both reviewers' look at round 5: nextest puts each test in a group of its own, and
    // what one left was out of the run's group's reach. The program leads a session,
    // which the job is in.
    let dir = tempfile::tempdir().unwrap();
    let (file, marked) = (dir.path().join("pids"), dir.path().join("late"));
    let job = format!("sleep 2; echo late > '{}'", marked.display());
    let run = job_in_a_group_of_its_own(&job, &file, "exit 0");
    let limits = Limits {
        timeout: Duration::from_secs(60),
        max_buffer: 1024,
    };
    let answered = runtime().block_on(capture_whole(&run, None, &tree_env(), limits, |_| {}));
    assert!(answered.is_ok(), "{answered:?}");
    let pids = pids_in(&file, 2).expect("written before it ended");
    let _left = Survivors(pids.clone());
    assert!(
        gone_within(pids[1], Duration::from_secs(1)),
        "the job in a group of its own is ended with the program"
    );
    std::thread::sleep(Duration::from_secs(3));
    assert!(!marked.exists(), "nothing it left running wrote after it");
}

#[cfg(unix)]
#[test]
fn held_whole_out_of_its_time_what_is_in_a_group_of_its_own_and_ignores_the_ask_is_forced() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pids");
    let run = job_in_a_group_of_its_own("trap '' TERM; exec sleep 60", &file, "wait");
    let limits = Limits {
        timeout: Duration::from_millis(1500),
        max_buffer: 1024,
    };
    let failed = runtime()
        .block_on(capture_whole(&run, None, &tree_env(), limits, |_| {}))
        .unwrap_err();
    assert!(failed.killed, "{failed:?}");
    let pids = pids_in(&file, 2).expect("written long before its time ran out");
    let _left = Survivors(pids.clone());
    assert!(gone(pids[0]), "the program");
    assert!(
        gone_within(pids[1], Duration::from_secs(1)),
        "the job, which led a group of its own and took no notice of the ask"
    );
}

#[cfg(unix)]
#[test]
fn held_whole_a_capture_dropped_asks_its_program_first_and_then_forces_what_is_left() {
    // As the plants' runner drops a run at a stop: the program is asked to end (nextest
    // ends its tests so), and what took no notice is forced, in a group of its own too.
    let dir = tempfile::tempdir().unwrap();
    let (file, asked, armed) = (
        dir.path().join("pids"),
        dir.path().join("asked"),
        dir.path().join("armed"),
    );
    // The program says once it listens for the ask, and is dropped then.
    let then = format!(
        "trap \"echo asked > '{}'; exit 0\" TERM; : > '{}'; wait",
        asked.display(),
        armed.display()
    );
    let run = job_in_a_group_of_its_own("trap '' TERM; exec sleep 60", &file, &then);
    let (env, limits) = (
        tree_env(),
        Limits {
            timeout: Duration::from_secs(60),
            max_buffer: 1024,
        },
    );
    let (pids, led) = runtime().block_on(async {
        let capturing = capture_whole(&run, None, &env, limits, |_| {});
        let written = async {
            let pids = pids_written_to_soon(&file, 2).await;
            while !armed.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let led = leads_a_group(pids[1]);
            (pids, led)
        };
        tokio::select! {
            _ = capturing => panic!("a program that waits for a minute does not end"),
            written = written => written,
        }
    });
    let _left = Survivors(pids.clone());
    assert!(led, "the job led a group of its own");
    assert!(asked.exists(), "the program was asked first");
    assert!(
        gone_within(pids[1], Duration::from_secs(1)),
        "the job that took no notice of the ask was forced"
    );
}

/// What starts a program in a session of its own (as Playwright starts a browser), which
/// leaves the run's, and has none of the streams of the one that started it.
#[cfg(unix)]
const OWN_SESSION: &str =
    "perl -MPOSIX -e 'POSIX::setsid(); exec @ARGV' sleep 60 </dev/null >/dev/null 2>&1";

/// What a capture held whole, out of its time, made of `run`, which writes its pid and
/// a child's to `file`: the pids.
#[cfg(unix)]
fn out_of_time_whole(run: &Run, file: &Path) -> Vec<u32> {
    let limits = Limits {
        timeout: Duration::from_millis(1500),
        max_buffer: 1024,
    };
    let failed = runtime()
        .block_on(capture_whole(run, None, &tree_env(), limits, |_| {}))
        .unwrap_err();
    assert!(failed.killed, "{failed:?}");
    pids_in(file, 2).expect("written long before its time ran out")
}

#[cfg(unix)]
#[test]
fn held_whole_out_of_its_time_a_child_in_a_session_of_its_own_is_noted_as_the_program_is_asked() {
    // The program ends at the ask, and its child, out of its session and its group, is
    // given to another: noted while the table linked it to the program, it is ended.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pids");
    let run = shell(&format!(
        "{OWN_SESSION} & printf '%s %s\\n' \"$$\" \"$!\" > '{}'; trap 'exit 0' TERM; wait",
        file.display()
    ));
    let pids = out_of_time_whole(&run, &file);
    let _left = Survivors(pids.clone());
    assert!(gone(pids[0]), "the program");
    assert!(
        gone_within(pids[1], Duration::from_secs(1)),
        "the child in a session of its own"
    );
}

#[cfg(unix)]
#[test]
fn held_whole_out_of_its_time_a_child_the_program_starts_as_it_is_asked_is_ended_too() {
    // The program starts a child in a session of its own as it is asked to end, and
    // takes no notice of the ask: the child is linked to it as it is forced.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pids");
    let run = shell(&format!(
        "trap \"{OWN_SESSION} & printf '%s %s\\\\n' \\\"\\$\\$\\\" \\\"\\$!\\\" > '{}'\" TERM; sleep 60 & wait; wait",
        file.display()
    ));
    let pids = out_of_time_whole(&run, &file);
    let _left = Survivors(pids.clone());
    assert!(gone(pids[0]), "the program");
    assert!(
        gone_within(pids[1], Duration::from_secs(1)),
        "the child it started as it was asked"
    );
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

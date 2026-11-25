use std::collections::HashMap;
#[cfg(unix)]
use std::io::{self, BufRead, BufReader, Read};
use std::path::Path;
use std::sync::Arc;
#[cfg(unix)]
use std::thread;
#[cfg(unix)]
use std::time::{Duration, Instant};

use portable_pty::PtySize;

#[cfg(unix)]
use super::{
    process_exists, serial_pty_test, write_paste_via, OpenedPane, PaneEnvironment, PaneKey,
    Sentinel,
};
use super::{PaneError, PaneTable, CANCELLED_KEPT};
use cf_proto::agents::parent_session_variables;

fn terminal_size(rows: u16, cols: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

#[cfg(unix)]
fn shell(script: &str) -> Vec<String> {
    vec!["/bin/sh".to_string(), "-c".to_string(), script.to_string()]
}

#[cfg(unix)]
fn open_shell(table: &PaneTable, script: &str) -> OpenedPane {
    table
        .open(
            Path::new("/tmp"),
            &shell(script),
            &HashMap::new(),
            terminal_size(40, 120),
        )
        .expect("open shell in a PTY")
}

#[cfg(unix)]
unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
    fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
}

#[cfg(unix)]
fn child_process_id(table: &PaneTable, key: &PaneKey) -> i32 {
    table
        .panes
        .lock()
        .unwrap()
        .get(key)
        .and_then(|pane| pane.child.process_id())
        .expect("the pane child reports a process id") as i32
}

#[cfg(unix)]
#[test]
fn a_child_reaped_outside_the_table_still_reopens_update_admission() {
    let _serial = serial_pty_test();
    let table = Arc::new(PaneTable::new());
    let pane = open_shell(&table, "exec /bin/cat");
    let pid = child_process_id(&table, &pane.key);

    // Someone else reaps the child first, so the table's own wait sees
    // ECHILD. That is proof the process is gone, not a doubt to hold on to.
    // SAFETY: kill takes two integers and touches no memory of ours.
    assert_eq!(unsafe { kill(pid, 9) }, 0, "signal the pane child");
    let mut status = 0;
    assert_eq!(
        // SAFETY: `status` is a live local, writable for the one int
        // waitpid stores in it.
        unsafe { waitpid(pid, &mut status, 0) },
        pid,
        "reap the pane child outside the table"
    );

    table
        .kill(&pane.key)
        .expect("closing a pane whose child is already reaped succeeds");
    assert!(
        table.update_blockers().unwrap().is_empty(),
        "a reaped child leaves no cleanup blocker behind"
    );
    assert!(
        table.begin_update().is_ok(),
        "update admission reopens once every child is gone"
    );
}

/// A window whose opener no longer owns it is refused at the insertion, with no program
/// started: the daemon's launch that reaches the table after the daemon has stopped.
#[test]
fn a_window_its_opener_no_longer_owns_is_not_opened_and_starts_no_program() {
    use super::{PaneEnvironment, PaneKey};

    let table = Arc::new(PaneTable::new());
    // A program that is never started here: the open is refused before any is.
    #[cfg(unix)]
    let program = std::path::PathBuf::from("/bin/sh");
    #[cfg(windows)]
    let program = std::path::PathBuf::from(r"C:\Windows\System32\cmd.exe");
    let key = PaneKey::new("late-pane", 1);
    let refused = table.open_streamed_at_while(
        &|| false,
        key.clone(),
        Path::new("."),
        &[program.to_string_lossy().into_owned()],
        PaneEnvironment::new(&HashMap::new(), &[]),
        PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        },
        1024,
    );
    if let Ok(streamed) = &refused {
        let _ = table.kill(&streamed.key);
    }
    assert!(
        matches!(&refused, Err(PaneError::Fenced(refused)) if *refused == key),
        "an open its opener no longer owns was let in"
    );
    assert!(
        table.pending().expect("what is left").is_clean(),
        "a refused open left something of a window behind"
    );
}

#[test]
fn what_is_left_of_the_windows_says_when_no_program_of_theirs_can_still_run() {
    let table = PaneTable::new();
    assert!(table.pending().unwrap().is_clean());
    table.teardown.begin();
    let ending = table.pending().unwrap();
    assert_eq!((ending.finishing, ending.is_clean()), (1, false));
    table.teardown.resolve(true);
    assert!(table.pending().unwrap().is_clean());
    table.teardown.begin();
    table.teardown.resolve(false);
    let unproven = table.pending().unwrap();
    assert_eq!((unproven.unconfirmed, unproven.is_clean()), (1, false));
}

#[test]
fn a_table_is_reaped_until_no_program_of_its_windows_can_still_run_and_not_for_ever() {
    use std::time::{Duration, Instant};

    use crate::headless::{reap_until_clean, Reaped};

    let patience = Duration::from_secs(5);
    let clean = Arc::new(PaneTable::new());
    assert_eq!(reap_until_clean(&clean, patience), Reaped::Clean);

    // A close that has taken its pane out and not read its program's end: waited for, and
    // clean once it has.
    let ending = Arc::new(PaneTable::new());
    ending.teardown.begin();
    let late = {
        let table = Arc::clone(&ending);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            table.teardown.resolve(true);
        })
    };
    let began = Instant::now();
    assert_eq!(reap_until_clean(&ending, patience), Reaped::Clean);
    assert!(
        began.elapsed() >= Duration::from_millis(150),
        "a close under way was not waited for"
    );
    late.join().expect("the close");

    // One that does not end in the time given: told so, in that time.
    let stuck = Arc::new(PaneTable::new());
    stuck.teardown.begin();
    let began = Instant::now();
    assert_eq!(
        reap_until_clean(&stuck, Duration::from_millis(150)),
        Reaped::TooLate
    );
    assert!(began.elapsed() < Duration::from_secs(3));

    // One whose program's end was never proven.
    let unproven = Arc::new(PaneTable::new());
    unproven.teardown.begin();
    unproven.teardown.resolve(false);
    assert_eq!(reap_until_clean(&unproven, patience), Reaped::Unconfirmed);
}

/// F2 (T-1540): a window whose start fails after it is in the table is ended, and the
/// answer says why it did not open.
#[cfg(unix)]
#[test]
fn a_window_that_fails_to_open_after_it_is_in_the_table_leaves_nothing_behind() {
    let _serial = serial_pty_test();
    let table = Arc::new(PaneTable::new());
    super::TERMINAL_FAILS.set(true);
    let opened = table.open_streamed_at(
        PaneKey::new("half-open", 1),
        Path::new("/tmp"),
        &shell("exec /bin/cat"),
        PaneEnvironment::new(&HashMap::new(), &[]),
        terminal_size(24, 80),
        1024,
    );
    super::TERMINAL_FAILS.set(false);
    assert!(
        matches!(&opened, Err(error) if error.to_string().contains("planted: no terminal")),
        "the answer does not say why the window did not open"
    );
    assert!(
        table.pending().expect("what is left").is_clean(),
        "a window that failed to open left something of itself behind"
    );
}

/// F2 (T-1540): where the end of a window that failed to open fails too, the answer says
/// both, the cause first.
#[cfg(unix)]
#[test]
fn a_window_that_fails_to_open_and_cannot_be_ended_says_both() {
    let _serial = serial_pty_test();
    let table = Arc::new(PaneTable::new());
    let pane = open_shell(&table, "exec /bin/cat");
    table.poison();
    let answer = table.abandon_open(
        &pane.key,
        pane.reader,
        PaneError::Pty("planted: no terminal".to_owned()),
    );
    let said = answer.to_string();
    assert!(
        said.starts_with("PTY error: planted: no terminal"),
        "the cause does not come first: {said}"
    );
    assert!(
        said.contains("poisoned"),
        "the failure to end the window is not said: {said}"
    );
}

/// F2 (T-1540): an exit watcher whose retirement fails or finds the program still running
/// does not leave the pane in the table: it is ended as a close ends it.
#[cfg(unix)]
#[test]
fn a_pane_whose_exit_watcher_could_not_retire_it_is_ended_all_the_same() {
    let _serial = serial_pty_test();
    for failure in [
        Err(PaneError::Io(io::Error::other("planted: no status"))),
        Ok(false),
    ] {
        let table = Arc::new(PaneTable::new());
        let pane = open_shell(&table, "exec /bin/cat");
        table.retire_after_exit_by(&pane.key, |_, _| failure);
        let until = Instant::now() + Duration::from_secs(20);
        while !table.pending().expect("what is left").is_clean() && Instant::now() < until {
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            table.pending().expect("what is left"),
            super::Pending {
                open: 0,
                finishing: 0,
                unconfirmed: 0
            },
            "a pane whose retirement failed stayed"
        );
    }
}

#[cfg(unix)]
#[test]
fn an_exit_watcher_whose_wait_failed_leaves_a_working_pane_alone() {
    let _serial = serial_pty_test();
    let table = Arc::new(PaneTable::new());
    let pane = open_shell(&table, "exec /bin/cat");
    let asked = std::cell::Cell::new(false);
    table.exit_watched(
        &pane.key,
        Err(io::Error::other("planted: the wait failed")),
        |_, _| {
            asked.set(true);
            Ok(false)
        },
    );
    assert!(!asked.get(), "a wait that failed still retired the pane");
    assert_eq!(
        table.pending().expect("what is left"),
        super::Pending {
            open: 1,
            finishing: 0,
            unconfirmed: 0
        },
        "a wait that failed ended a pane that is still working"
    );
    table.kill(&pane.key).expect("the pane ends");
}

#[test]
fn an_unproven_teardown_holds_admission_closed_under_its_own_name() {
    let table = Arc::new(PaneTable::new());
    table.teardown.begin();
    table.teardown.resolve(false);

    assert!(
        matches!(table.begin_update(), Err(PaneError::UpdateBlocked)),
        "a child that never proved it stopped keeps admission closed"
    );
    assert_eq!(
        table
            .update_blockers()
            .unwrap()
            .iter()
            .map(|key| key.id.clone())
            .collect::<Vec<_>>(),
        ["unconfirmed-pane-cleanup-0"],
        "the blocker names itself so the dialog can ask for a restart"
    );
}

#[cfg(unix)]
#[test]
fn update_installation_blocks_all_pane_launches_and_failure_restores_admission() {
    let _serial = serial_pty_test();
    let table = Arc::new(PaneTable::new());
    let pane = open_shell(&table, "exec /bin/cat");
    assert!(
        table.begin_update().is_err(),
        "any open pane prevents installation"
    );
    table.kill(&pane.key).unwrap();
    let permit = table.begin_update().unwrap();
    let launched = table.open_at(
        PaneKey::new("racing-worker", 7),
        Path::new("/tmp"),
        &shell("exec /bin/cat"),
        &HashMap::new(),
        terminal_size(40, 120),
    );
    assert!(matches!(launched, Err(PaneError::Updating)));
    assert!(table.list().unwrap().is_empty());
    assert!(
        table.begin_update().is_err(),
        "only one installer may hold admission"
    );
    drop(permit); // failed install/panic: admission must reopen automatically
    let pane = open_shell(&table, "exec /bin/cat");
    table.kill(&pane.key).unwrap();
    assert!(table.begin_update().is_ok());
}

/// T-1328: a program the system cannot start says why by the system's name for it, from the
/// fixed table, beside its words; the daemon reads it back as the kind of failure a note says,
/// and the words (which name the program) stay the person's.
#[cfg(unix)]
#[test]
fn a_program_the_system_cannot_start_names_why_by_its_code() {
    let _serial = serial_pty_test();
    let table = PaneTable::new();
    let missing = table.open_at(
        PaneKey::new("missing-program", 1),
        Path::new("/tmp"),
        &["/nonexistent/hunter22 program".to_owned()],
        &HashMap::new(),
        terminal_size(40, 120),
    );
    let Err(error) = missing else {
        panic!("a program that is not there opened");
    };
    assert_eq!(error.code(), Some("ENOENT"), "{error}");
    // Other failures of a table name none.
    assert_eq!(PaneError::EmptyArgv.code(), None);
    assert_eq!(PaneError::Pty("boom".to_owned()).code(), None);
}

#[cfg(unix)]
#[test]
fn update_installation_and_real_spawn_share_one_atomic_boundary() {
    let _serial = serial_pty_test();
    for _ in 0..20 {
        let table = Arc::new(PaneTable::new());
        let start = Arc::new(std::sync::Barrier::new(2));
        let child_table = table.clone();
        let child_start = start.clone();
        let launch = thread::spawn(move || {
            child_start.wait();
            child_table.open(
                Path::new("/tmp"),
                &shell("exec /bin/cat"),
                &HashMap::new(),
                terminal_size(40, 120),
            )
        });
        start.wait();
        let install = table.begin_update();
        let opened = launch.join().unwrap();
        assert_ne!(
            install.is_ok(),
            opened.is_ok(),
            "exactly one side is admitted"
        );
        if let Ok(pane) = opened {
            table.kill(&pane.key).unwrap();
        }
    }
}

#[cfg(unix)]
fn read_to_end(mut reader: Box<dyn Read + Send>) -> Vec<u8> {
    let (sender, receiver) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let mut output = Vec::new();
        let result = reader.read_to_end(&mut output).map(|_| output);
        let _ = sender.send(result);
    });
    receiver
        .recv_timeout(Duration::from_secs(3))
        .expect("PTY reader did not reach EOF")
        .expect("read PTY output")
}

#[cfg(unix)]
#[test]
fn open_reads_hello_then_eof() {
    let _pty_guard = serial_pty_test();
    let table = PaneTable::new();
    let OpenedPane { key, reader } = open_shell(&table, "printf hello");

    assert_eq!(read_to_end(reader), b"hello");
    // The output can end a moment before the exit is seen: a slow runner
    // read EOF while the shell was still being reaped (release run
    // 37319546451). The listing that first sees it ended says so at once.
    let deadline = Instant::now() + Duration::from_secs(3);
    let listed = loop {
        let listed = table.list().expect("list panes");
        if listed.iter().all(|pane| !pane.alive) || Instant::now() > deadline {
            break listed;
        }
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(
        listed,
        vec![super::PaneInfo {
            id: key.id,
            generation: key.generation,
            alive: false,
            idle_ms: 0,
        }]
    );
}

/// A window the daemon killed before its open reached the table never opens: the
/// daemon gave up on that open at its deadline and its kill found nothing (kluris,
/// 2026-10-26: the window started three seconds after the deadline and worked on
/// unseen). The next generation of the window opens as ever.
#[cfg(unix)]
#[test]
fn a_window_killed_before_its_open_never_opens_and_its_next_generation_does() {
    let _pty_guard = serial_pty_test();
    let table = PaneTable::new();
    let argv = shell("sleep 1000");
    let env = HashMap::new();
    let given_up = PaneKey::new("worker", 1);
    table
        .kill(&given_up)
        .expect("a kill that finds nothing is no failure");

    let late = table.open_at(
        given_up.clone(),
        Path::new("/tmp"),
        &argv,
        &env,
        terminal_size(24, 80),
    );
    match late {
        Err(PaneError::Cancelled(key)) => assert_eq!(key, given_up),
        Err(other) => panic!("refused for another reason: {other}"),
        Ok(_) => panic!("the window given up on opened"),
    }
    assert!(table.list().expect("list").is_empty());

    let next = PaneKey::new("worker", 2);
    let reader = table
        .open_at(
            next.clone(),
            Path::new("/tmp"),
            &argv,
            &env,
            terminal_size(24, 80),
        )
        .expect("the next generation opens");
    table.kill(&next).expect("kill the next generation");
    assert_eq!(read_to_end(reader), b"");
}

/// A kill of a window that a kill before it is still ending answers once that one
/// has ended its program: the daemon asks again where its first kill of a window it
/// gave up on went unanswered, and opens the next window on the answer.
#[cfg(unix)]
#[test]
fn a_kill_of_a_window_another_kill_is_ending_answers_once_its_program_is_gone() {
    let _pty_guard = serial_pty_test();
    let table = Arc::new(PaneTable::closing_within(Duration::from_secs(2)));
    let OpenedPane { key, mut reader } =
        open_shell(&table, &cf_base::fixture::deaf("TERM HUP", "printf ready"));
    let mut shown = Vec::new();
    let mut chunk = [0_u8; 64];
    while !String::from_utf8_lossy(&shown).contains("ready") {
        let read = reader.read(&mut chunk).expect("read the pane");
        assert!(read > 0, "the pane ended before it was ready");
        shown.extend_from_slice(&chunk[..read]);
    }
    let pid = child_process_id(&table, &key);
    let first = {
        let (table, key) = (Arc::clone(&table), key.clone());
        thread::spawn(move || table.kill(&key))
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while !table.list().expect("list").is_empty() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        table.list().expect("list").is_empty(),
        "the first kill took it"
    );
    assert!(process_exists(pid), "it waits out its grace");

    table.kill(&key).expect("the second kill");
    assert!(!process_exists(pid), "answered while its program still ran");
    first.join().unwrap().expect("the first kill");
    drop(reader);
}

/// The windows killed before they opened are the latest [`CANCELLED_KEPT`]: the kills
/// of windows long gone, which find nothing as well, push out the oldest, and a window
/// is kept from opening once.
#[test]
fn the_windows_killed_before_they_opened_are_the_latest_few() {
    let table = PaneTable::new();
    let key = |generation: u64| super::PaneKey::new("worker", generation);
    for generation in 0..=u64::try_from(CANCELLED_KEPT).unwrap() {
        table.kill(&key(generation)).expect("kill");
    }
    let panes = table.lock_panes().expect("lock");
    assert!(!table.uncancel(&key(0)), "the oldest made room");
    assert!(table.uncancel(&key(1)));
    assert!(!table.uncancel(&key(1)), "once");
    let last = u64::try_from(CANCELLED_KEPT).unwrap();
    assert!(table.uncancel(&key(last)));
    drop(panes);
}

#[cfg(unix)]
#[test]
fn pane_table_keys_same_id_by_generation() {
    let _pty_guard = serial_pty_test();
    let table = PaneTable::new();
    let argv = shell("sleep 1000");
    let env = HashMap::new();
    let first = PaneKey::new("worker", 1);
    let second = PaneKey::new("worker", 2);

    let first_reader = table
        .open_at(
            first.clone(),
            Path::new("/tmp"),
            &argv,
            &env,
            terminal_size(24, 80),
        )
        .expect("open first generation");
    let second_reader = table
        .open_at(
            second.clone(),
            Path::new("/tmp"),
            &argv,
            &env,
            terminal_size(24, 80),
        )
        .expect("open second generation");

    let listed = table.list().expect("list panes");
    assert_eq!(listed.len(), 2);
    assert!(listed
        .iter()
        .any(|pane| pane.id == "worker" && pane.generation == 1));
    assert!(listed
        .iter()
        .any(|pane| pane.id == "worker" && pane.generation == 2));

    table.kill(&first).expect("kill first generation");
    table.kill(&second).expect("kill second generation");
    assert_eq!(read_to_end(first_reader), b"");
    assert_eq!(read_to_end(second_reader), b"");
}

#[cfg(unix)]
#[test]
fn streamed_open_at_preserves_the_store_reserved_identity() {
    let _pty_guard = serial_pty_test();
    let table = Arc::new(PaneTable::new());
    let key = PaneKey::new("reserved-worker", 7);
    let streamed = table
        .open_streamed_at(
            key.clone(),
            Path::new("/tmp"),
            &shell("printf reserved"),
            PaneEnvironment::new(&HashMap::new(), &[]),
            terminal_size(24, 80),
            1024,
        )
        .expect("open the store-reserved pane identity");

    assert_eq!(streamed.key, key);
    let mut bytes = Vec::<u8>::new();
    while let Ok(chunk) = streamed.output.recv_timeout(Duration::from_secs(1)) {
        bytes.extend_from_slice(&chunk.bytes);
        table
            .ack(&key, chunk.seq)
            .expect("ack reserved pane output");
    }
    assert_eq!(bytes, b"reserved");
}

#[cfg(unix)]
#[test]
fn resize_reaches_the_child_terminal() {
    let _pty_guard = serial_pty_test();
    let table = PaneTable::new();
    let OpenedPane { key, reader } = table
        .open(
            Path::new("/tmp"),
            &shell("sleep 0.1; stty size"),
            &HashMap::new(),
            terminal_size(10, 20),
        )
        .expect("open shell in a PTY");

    table.resize(&key, 24, 80).expect("resize PTY");

    assert_eq!(
        String::from_utf8(read_to_end(reader))
            .expect("UTF-8 stty output")
            .trim(),
        "24 80"
    );
}

#[cfg(unix)]
#[test]
fn kill_closes_the_reader_and_reaps_the_process_group() {
    let _pty_guard = serial_pty_test();
    let mut outsider = Sentinel::start();
    let table = PaneTable::new();
    let OpenedPane { key, reader } =
        open_shell(&table, "sleep 1000 & printf '%s\\n' \"$!\"; sleep 1000");
    let process_group = table.process_group_id(&key).expect("read process group id");
    let mut reader = BufReader::new(reader);
    let mut child_pid = String::new();
    reader
        .read_line(&mut child_pid)
        .expect("read background child pid");
    let child_pid = child_pid.trim().parse::<i32>().expect("numeric child pid");
    assert!(process_exists(process_group));
    assert!(process_exists(child_pid));

    table.kill(&key).expect("kill process group");

    let mut tail = Vec::new();
    reader.read_to_end(&mut tail).expect("reader reaches EOF");
    let deadline = Instant::now() + Duration::from_secs(2);
    while (process_exists(process_group) || process_exists(child_pid)) && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !process_exists(process_group),
        "process group survived kill"
    );
    assert!(!process_exists(child_pid), "background child survived kill");
    assert!(outsider.runs(), "a process outside the window");
}

#[cfg(unix)]
#[test]
fn kill_after_natural_exit_is_cleanup_success() {
    let _pty_guard = serial_pty_test();
    let table = PaneTable::new();
    let OpenedPane { key, reader } = open_shell(&table, "printf done");

    assert_eq!(read_to_end(reader), b"done");
    table
        .kill(&key)
        .expect("a gone process group is already clean");
    assert!(table.list().expect("list after cleanup").is_empty());
}

/// A program that ended takes its pane out of the table as a kill would:
/// its child reaped, its master gone and installation admitted again.
#[cfg(unix)]
#[test]
fn a_pane_whose_program_ended_retires() {
    let _pty_guard = serial_pty_test();
    let table = Arc::new(PaneTable::new());
    let OpenedPane { key, reader } = open_shell(&table, "printf done");
    let pid = child_process_id(&table, &key);

    assert_eq!(read_to_end(reader), b"done");
    assert!(table.retire_exited(&key).expect("retire the pane"));
    assert!(table.list().expect("list").is_empty());
    assert!(!process_exists(pid), "the ended child was not reaped");
    assert!(
        table.begin_update().is_ok(),
        "an ended window holds up no update"
    );
    assert!(
        table.retire_exited(&key).expect("retire again"),
        "a pane that is gone already is out of the table"
    );
}

/// A pane whose program still runs is never ended on its own, whatever
/// its output did: it waits for a kill.
#[cfg(unix)]
#[test]
fn a_pane_whose_program_runs_on_is_not_retired() {
    let _pty_guard = serial_pty_test();
    let table = Arc::new(PaneTable::new());
    let OpenedPane {
        key,
        reader: _reader,
    } = open_shell(&table, "exec /bin/sleep 30");
    let pid = child_process_id(&table, &key);

    assert!(!table
        .retire_exited_within(&key, Duration::from_millis(100))
        .expect("look at the pane"));
    assert!(process_exists(pid), "a running program was ended");
    assert_eq!(table.list().expect("list").len(), 1);
    table.kill(&key).expect("kill the pane");
    assert!(table.list().expect("list").is_empty());
}

/// A pane's screen is read through the table, and the table's resize is
/// the screen's too: what no longer fits at the top is gone.
#[cfg(unix)]
#[test]
fn a_panes_screen_is_read_through_the_table_and_follows_its_resize() {
    let _pty_guard = serial_pty_test();
    let table = Arc::new(PaneTable::new());
    let streamed = table
        .open_streamed(
            Path::new("/tmp"),
            &shell("exec /bin/sleep 30"),
            PaneEnvironment::new(&HashMap::new(), &[]),
            terminal_size(5, 20),
            1024,
        )
        .expect("open a pane");
    streamed.screen.feed(b"a1\r\na2\r\na3\r\na4");
    assert_eq!(
        table.tail(&streamed.key, 9).expect("read the screen"),
        ["a1", "a2", "a3", "a4"]
    );
    table.resize(&streamed.key, 2, 20).expect("resize the pane");
    assert_eq!(
        table.tail(&streamed.key, 9).expect("read the screen again"),
        ["a3", "a4"]
    );
    assert!(matches!(
        table.tail(&PaneKey::new("nobody", 1), 3),
        Err(PaneError::NotFound(_))
    ));
    table.kill(&streamed.key).expect("kill the pane");
    assert!(
        matches!(table.tail(&streamed.key, 3), Err(PaneError::NotFound(_))),
        "a pane out of the table has no screen to read through it"
    );
    // Its own handle still reads the last it showed.
    assert_eq!(streamed.screen.tail(9), ["a3", "a4"]);
}

/// How a program ended is kept by whoever read its status first, and told
/// by the pane's own slot after the pane has left the table.
#[cfg(unix)]
#[test]
fn a_programs_exit_code_is_waited_for_and_survives_its_pane_leaving_the_table() {
    let _pty_guard = serial_pty_test();
    let table = Arc::new(PaneTable::new());
    let open = |script: &str| {
        table
            .open_streamed(
                Path::new("/tmp"),
                &shell(script),
                PaneEnvironment::new(&HashMap::new(), &[]),
                terminal_size(24, 80),
                1024,
            )
            .expect("open a pane")
    };

    let exits = open("printf done; exit 3");
    let wait = Duration::from_secs(5);
    let ended = table
        .wait_ended(&exits.key, &exits.ended, wait)
        .expect("the exit code of a program that ended");
    assert_eq!((ended.code, ended.signal), (3, None));
    assert!(table.retire_exited(&exits.key).expect("retire the pane"));
    assert_eq!(
        table.wait_ended(&exits.key, &exits.ended, Duration::ZERO),
        exits.ended.get(),
        "the same after the pane left the table"
    );

    let running = open("exec /bin/sleep 30");
    assert_eq!(
        table.wait_ended(&running.key, &running.ended, Duration::from_millis(100)),
        None,
        "a program that runs on has no exit code to wait for"
    );
    table.kill(&running.key).expect("kill the pane");
    let killed = table
        .wait_ended(&running.key, &running.ended, wait)
        .expect("how a program that was killed ended");
    assert_eq!(killed.code, 1);
    assert!(
        killed.signal.is_some_and(|signal| !signal.is_empty()),
        "a program ended by a signal says which"
    );
}

#[cfg(target_os = "macos")]
fn detached_child(
    table: &PaneTable,
    parent_exits: bool,
    clear_environment: bool,
) -> (PaneKey, i32) {
    let script = format!(
        "import subprocess,time; p=subprocess.Popen(['/bin/sleep','60'], start_new_session=True, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL{}); print(p.pid, flush=True); {}",
        if clear_environment { ", env={}" } else { "" },
        if parent_exits { "" } else { "time.sleep(60)" },
    );
    let OpenedPane { key, reader } = table
        .open(
            Path::new("/tmp"),
            &["/usr/bin/python3".into(), "-c".into(), script],
            &HashMap::new(),
            terminal_size(24, 80),
        )
        .expect("launch a parent with a detached child");
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let pid = line.trim().parse::<i32>().expect("native child pid");
    // The close ends the program first, and the child, in a session of its own, is
    // then a job with no parent: one the test started, which the guard is told of.
    crate::process_tree::guard::register(pid);
    assert_ne!(
        // SAFETY: getpgid takes an integer and touches no memory of ours.
        unsafe { libc::getpgid(pid) },
        table.process_group_id(&key).unwrap()
    );
    if parent_exits {
        let mut tail = Vec::new();
        reader.read_to_end(&mut tail).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while table.list().unwrap().iter().any(|p| p.alive) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
    }
    (key, pid)
}

#[cfg(target_os = "macos")]
fn stopped_with_cleanup(pid: i32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while process_exists(pid) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    let stopped = !process_exists(pid);
    // A failing regression must not leave the test's own sleeper behind.
    if !stopped {
        // SAFETY: kill takes two integers and touches no memory of ours;
        // the pid is the test's own sleeper.
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
    stopped
}

#[cfg(target_os = "macos")]
#[test]
fn close_stops_detached_children_but_preserves_other_panes() {
    let _pty_guard = serial_pty_test();
    let mut outsider = Sentinel::start();
    let table = PaneTable::new();
    let (foreign, foreign_pid) = detached_child(&table, false, false);
    let (own, own_pid) = detached_child(&table, false, false);
    table.kill(&own).unwrap();
    let own_stopped = stopped_with_cleanup(own_pid);
    let foreign_alive = process_exists(foreign_pid);
    table.kill(&foreign).unwrap();
    let foreign_stopped = stopped_with_cleanup(foreign_pid);
    assert!(own_stopped, "detached child survived pane close");
    assert!(foreign_alive, "closing a pane killed another pane's child");
    assert!(
        foreign_stopped,
        "second pane's child survived its own close"
    );
    assert!(outsider.runs(), "a process outside the window");
}

#[cfg(target_os = "macos")]
#[test]
fn close_stops_reparented_children_after_the_root_exits() {
    let _pty_guard = serial_pty_test();
    let mut outsider = Sentinel::start();
    let table = PaneTable::new();
    let (key, pid) = detached_child(&table, true, false);
    assert!(process_exists(pid), "fixture needs a live orphan");
    table.kill(&key).unwrap();
    assert!(
        stopped_with_cleanup(pid),
        "reparented child survived pane close"
    );
    assert!(outsider.runs(), "a process outside the window");
}

#[cfg(target_os = "macos")]
#[test]
fn close_stops_attached_descendants_even_with_a_cleared_environment() {
    let _pty_guard = serial_pty_test();
    let mut outsider = Sentinel::start();
    let table = PaneTable::new();
    let (key, pid) = detached_child(&table, false, true);
    table.kill(&key).unwrap();
    assert!(
        stopped_with_cleanup(pid),
        "owned child with empty environment survived"
    );
    assert!(outsider.runs(), "a process outside the window");
}

#[cfg(target_os = "macos")]
#[test]
fn drop_stops_detached_children() {
    let _pty_guard = serial_pty_test();
    let mut outsider = Sentinel::start();
    let table = PaneTable::new();
    let (_, pid) = detached_child(&table, false, false);
    drop(table);
    assert!(
        stopped_with_cleanup(pid),
        "detached child survived table drop"
    );
    assert!(outsider.runs(), "a process outside the window");
}

#[cfg(unix)]
#[test]
fn dropping_a_live_table_reaps_its_entire_process_group() {
    let _pty_guard = serial_pty_test();
    let mut outsider = Sentinel::start();
    let table = PaneTable::new();
    let OpenedPane { key, reader } =
        open_shell(&table, "sleep 1000 & printf '%s\\n' \"$!\"; sleep 1000");
    let process_group = table.process_group_id(&key).expect("read process group id");
    let mut reader = BufReader::new(reader);
    let mut child_pid = String::new();
    reader
        .read_line(&mut child_pid)
        .expect("read background child pid");
    let child_pid = child_pid.trim().parse::<i32>().expect("numeric child pid");
    assert!(process_exists(process_group));
    assert!(process_exists(child_pid));

    drop(table);

    let mut tail = Vec::new();
    reader.read_to_end(&mut tail).expect("reader reaches EOF");
    let deadline = Instant::now() + Duration::from_secs(2);
    while (process_exists(process_group) || process_exists(child_pid)) && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !process_exists(process_group),
        "process group survived Drop"
    );
    assert!(!process_exists(child_pid), "background child survived Drop");
    assert!(outsider.runs(), "a process outside the window");
}

#[cfg(unix)]
#[test]
fn environment_map_reaches_the_child() {
    let _pty_guard = serial_pty_test();
    let table = PaneTable::new();
    let mut env = HashMap::new();
    env.insert("PANE_TEST_VALUE".to_string(), "from-env-map".to_string());
    let OpenedPane { reader, .. } = table
        .open(
            Path::new("/tmp"),
            &shell("printf %s \"$PANE_TEST_VALUE\""),
            &env,
            terminal_size(24, 80),
        )
        .expect("open shell in a PTY");

    assert_eq!(read_to_end(reader), b"from-env-map");
}

#[cfg(unix)]
#[test]
fn child_environment_is_inherited_then_overlaid() {
    let _pty_guard = serial_pty_test();
    const SENTINEL: &str = "CONSENSFLOW_PTY_TEST_LAUNCHER_SENTINEL_7FC9A1";

    struct RemoveSentinel;
    impl Drop for RemoveSentinel {
        fn drop(&mut self) {
            std::env::remove_var(SENTINEL);
        }
    }

    std::env::set_var(SENTINEL, "launcher-only");
    let _remove_sentinel = RemoveSentinel;
    let table = PaneTable::new();
    let script = format!(
        "if [ \"${{{SENTINEL}+present}}\" = present ]; then printf inherited; else printf absent; fi"
    );
    let OpenedPane { reader, .. } = table
        .open(
            Path::new("/tmp"),
            &shell(&script),
            &HashMap::new(),
            terminal_size(24, 80),
        )
        .expect("open child without an overlay");
    assert_eq!(read_to_end(reader), b"inherited");

    let mut supplied = HashMap::new();
    supplied.insert(SENTINEL.to_string(), "role-value".to_string());
    let OpenedPane { reader, .. } = table
        .open(
            Path::new("/tmp"),
            &shell(&format!("printf %s \"${SENTINEL}\"")),
            &supplied,
            terminal_size(24, 80),
        )
        .expect("open child with the supplied sentinel");
    assert_eq!(read_to_end(reader), b"role-value");
}

/// Sets every variable of a parent session in the test's own environment, the host's,
/// as an app started from an agent's shell or a board's window has them
/// ([`parent_session_variables`]: a board's window's and every harness's), and takes
/// them out again.
pub(crate) struct InParentSessions;

impl InParentSessions {
    pub(crate) fn set() -> Self {
        for name in parent_session_variables() {
            std::env::set_var(name, "from-the-parent-session");
        }
        Self
    }
}

impl Drop for InParentSessions {
    fn drop(&mut self) {
        for name in parent_session_variables() {
            std::env::remove_var(name);
        }
    }
}

/// The names of a parent session's variables the pane's `env` printed, one a line.
#[cfg(unix)]
fn held(printed: &[u8]) -> Vec<&'static str> {
    let printed = String::from_utf8_lossy(printed);
    parent_session_variables()
        .filter(|name| {
            printed
                .lines()
                .any(|line| line.trim_end_matches('\r').starts_with(&format!("{name}=")))
        })
        .collect()
}

/// T-971, T-868: a pane's program holds none of a session the host was started in, a
/// board's window's or any harness's, but what its frame sets itself.
#[cfg(unix)]
#[test]
fn a_pane_inherits_nothing_of_a_session_the_host_was_started_in() {
    let _pty_guard = serial_pty_test();
    let _sessions = InParentSessions::set();
    let table = PaneTable::new();
    let OpenedPane { reader, .. } = table
        .open(
            Path::new("/tmp"),
            &shell("env"),
            &HashMap::new(),
            terminal_size(24, 80),
        )
        .expect("open env in a PTY");
    assert_eq!(held(&read_to_end(reader)), Vec::<&str>::new());

    let own = HashMap::from([("CLAUDECODE".to_string(), "the-window's".to_string())]);
    let OpenedPane { reader, .. } = table
        .open(
            Path::new("/tmp"),
            &shell("env"),
            &own,
            terminal_size(24, 80),
        )
        .expect("open env in a PTY");
    let printed = read_to_end(reader);
    assert_eq!(held(&printed), ["CLAUDECODE"]);
    assert!(String::from_utf8_lossy(&printed).contains("CLAUDECODE=the-window's"));
}

#[cfg(unix)]
#[test]
fn raw_mode_recorder_receives_exact_bytes() {
    let _pty_guard = serial_pty_test();
    let table = PaneTable::new();
    let OpenedPane { key, mut reader } =
        open_shell(&table, "stty raw -echo; printf ready; od -An -tx1 -N 5");
    let mut ready = [0_u8; 5];
    reader.read_exact(&mut ready).expect("raw recorder ready");
    assert_eq!(&ready, b"ready");

    table
        .write(&key, &[0x00, 0x09, 0x0d, 0x1b, 0xff])
        .expect("write raw bytes");

    let hex = String::from_utf8(read_to_end(reader)).expect("UTF-8 od output");
    assert_eq!(hex.split_whitespace().collect::<String>(), "00090d1bff");
}

/// A write into a pane whose program reads nothing blocks for good, and neither another
/// pane's write nor the kill of that pane waits for it. The test has the write in hand
/// before it goes on (the pane counts it before it waits for the writer, and a kill with a
/// write in hand ends the pane on a thread of its own), and then the other pane's write and
/// the kill each finish, and the blocked write is unblocked by the kill. There is no time
/// of the machine's in it but [`HANG`]: a write or a kill that has not finished in that long
/// is stuck, not slow, and the test ends the blocked program itself, so that it fails
/// instead of hanging, saying so.
#[cfg(unix)]
#[test]
fn blocked_write_does_not_stall_other_panes_or_kill() {
    /// Past anything a loaded machine takes for a write of a byte and a kill.
    const HANG: Duration = Duration::from_secs(30);
    let _pty_guard = serial_pty_test();
    let table = Arc::new(PaneTable::new());
    let OpenedPane {
        key: blocked_key,
        reader: _blocked_reader,
    } = open_shell(&table, "sleep 1000");
    let blocked_process_group = table
        .process_group_id(&blocked_key)
        .expect("blocked pane process group");
    let OpenedPane {
        key: responsive_key,
        mut reader,
    } = open_shell(&table, "stty raw -echo; printf ready; od -An -tx1 -N 1");
    let mut ready = [0_u8; 5];
    reader
        .read_exact(&mut ready)
        .expect("responsive pane ready");
    assert_eq!(&ready, b"ready");

    let (blocked_sender, blocked_receiver) = std::sync::mpsc::channel();
    let blocked_table = Arc::clone(&table);
    let writer_key = blocked_key.clone();
    let blocked_writer = thread::spawn(move || {
        let result = blocked_table.write(&writer_key, &vec![b'x'; 8 * 1024 * 1024]);
        let _ = blocked_sender.send(result);
    });
    // The write is in hand: the pane counts it before it waits for its writer, and the
    // program reads nothing, so it blocks there until the pane is ended.
    let deadline = Instant::now() + HANG;
    while table.writes_in_flight(&blocked_key) != Some(1) {
        assert!(Instant::now() < deadline, "the blocked write never began");
        thread::sleep(Duration::from_millis(1));
    }

    // What ends the blocked program if the test finds something stuck: the pane's
    // program leads its group, a test's own emergency cleanup of it.
    let mut emergency = false;
    let mut cleanup = || {
        emergency = true;
        // SAFETY: kill takes two integers and touches no memory of ours.
        let _ = unsafe { libc::kill(-blocked_process_group, libc::SIGKILL) };
    };

    let (responsive_sender, responsive_receiver) = std::sync::mpsc::channel();
    let responsive_table = Arc::clone(&table);
    let writer_key = responsive_key.clone();
    let responsive_writer = thread::spawn(move || {
        let _ = responsive_sender.send(responsive_table.write(&writer_key, b"R"));
    });
    let responsive_result = responsive_receiver.recv_timeout(HANG).unwrap_or_else(|_| {
        cleanup();
        responsive_receiver
            .recv_timeout(HANG)
            .expect("responsive pane write eventually finishes")
    });
    // The other pane's write finished while the blocked one was still in hand.
    let blocked_still_in_hand = matches!(
        blocked_receiver.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    );

    let (kill_sender, kill_receiver) = std::sync::mpsc::channel();
    let kill_table = Arc::clone(&table);
    let kill_key = blocked_key.clone();
    let killer = thread::spawn(move || {
        let _ = kill_sender.send(kill_table.kill(&kill_key));
    });
    let kill_result = kill_receiver.recv_timeout(HANG).unwrap_or_else(|_| {
        cleanup();
        kill_receiver
            .recv_timeout(HANG)
            .expect("blocked pane kill eventually finishes")
    });
    let _ = blocked_receiver
        .recv_timeout(HANG)
        .expect("blocked write unblocked after kill");
    blocked_writer.join().expect("blocked writer thread");
    responsive_writer.join().expect("responsive writer thread");
    killer.join().expect("killer thread");

    assert!(
        !emergency,
        "responsiveness or kill completed only after emergency process-group cleanup"
    );
    assert!(
        blocked_still_in_hand,
        "the blocked write had finished by the time another pane's write did"
    );
    responsive_result.expect("unrelated pane remains writable");
    kill_result.expect("kill is independent of the blocked writer");
    assert_eq!(
        String::from_utf8(read_to_end(reader))
            .expect("responsive recorder output")
            .split_whitespace()
            .collect::<String>(),
        "52"
    );
    let _ = table.kill(&responsive_key);
}

#[cfg(unix)]
#[test]
fn a_paste_through_the_table_writes_brackets_then_delayed_enter() {
    let _pty_guard = serial_pty_test();
    let table = PaneTable::new();
    let OpenedPane { key, mut reader } = open_shell(
        &table,
        "/bin/stty raw -echo; printf ready; /usr/bin/od -An -tx1 -N 17",
    );
    let mut ready = [0_u8; 5];
    reader.read_exact(&mut ready).expect("raw recorder ready");
    assert_eq!(&ready, b"ready");

    let started = Instant::now();
    write_paste_via(&table, &key, b"body", || {
        std::thread::sleep(Duration::from_millis(25));
    })
    .expect("write paste through PaneTable");

    assert!(started.elapsed() >= Duration::from_millis(25));
    assert_eq!(
        String::from_utf8(read_to_end(reader))
            .expect("UTF-8 od output")
            .split_whitespace()
            .collect::<String>(),
        "1b5b3230307e626f64791b5b3230317e0d"
    );
}

#[test]
fn open_refuses_a_bare_program_name() {
    let table = PaneTable::new();
    let result = table.open(
        Path::new("/tmp"),
        &[
            "sh".to_string(),
            "-c".to_string(),
            "printf nope".to_string(),
        ],
        &HashMap::new(),
        terminal_size(24, 80),
    );

    assert!(matches!(result, Err(PaneError::ProgramNotAbsolute(_))));
    assert!(table.list().expect("list panes").is_empty());
}

#[cfg(unix)]
fn fill_backlog(
    output: &std::sync::mpsc::Receiver<super::PaneOutput>,
    backlog_bytes: usize,
) -> (usize, u64) {
    let mut received_bytes = 0;
    let mut last_seq = 0;
    while received_bytes < backlog_bytes {
        let chunk = output
            .recv_timeout(Duration::from_secs(1))
            .expect("receive output up to backlog limit");
        assert!(chunk.seq > last_seq);
        received_bytes += chunk.bytes.len();
        last_seq = chunk.seq;
    }
    (received_bytes, last_seq)
}

#[cfg(unix)]
fn assert_output_closes(output: std::sync::mpsc::Receiver<super::PaneOutput>) {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        match output.recv_timeout(Duration::from_millis(20)) {
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
            Err(error) => panic!("output did not close after kill: {error}"),
        }
    }
}

#[cfg(unix)]
#[test]
fn reader_activity_resets_idle_then_silence_increases_it() {
    let _pty_guard = serial_pty_test();
    let table = Arc::new(PaneTable::new());
    let streamed = table
        .open_streamed(
            Path::new("/tmp"),
            // A second before its output: the child starts as the pane
            // opens, and a busy runner once took over 100 ms to return
            // from opening it, so a quarter second came before the look.
            &shell("/bin/sleep 1; printf x; /bin/sleep 1000"),
            PaneEnvironment::new(&HashMap::new(), &[]),
            terminal_size(24, 80),
            1024,
        )
        .expect("open delayed-output pane");

    thread::sleep(Duration::from_millis(150));
    let idle_before_output = table.list().expect("list before output")[0].idle_ms;
    let output = streamed
        .output
        .recv_timeout(Duration::from_secs(5))
        .expect("receive delayed output");
    assert_eq!(output.bytes, b"x");
    table.ack(&streamed.key, output.seq).expect("ack output");
    let idle_after_output = table.list().expect("list after output")[0].idle_ms;
    assert!(
        idle_after_output < idle_before_output,
        "nonempty output did not reset idle time: before={idle_before_output} after={idle_after_output}"
    );

    thread::sleep(Duration::from_millis(120));
    let idle_after_silence = table.list().expect("list after silence")[0].idle_ms;
    assert!(idle_after_silence >= idle_after_output + 80);
    table.kill(&streamed.key).expect("kill delayed-output pane");
}

#[cfg(unix)]
#[test]
fn unacked_yes_output_is_bounded_then_resumes_after_ack() {
    let _pty_guard = serial_pty_test();
    const BACKLOG_BYTES: usize = 128;

    let table = Arc::new(PaneTable::new());
    let streamed = table
        .open_streamed(
            Path::new("/tmp"),
            &["/usr/bin/yes".to_string()],
            PaneEnvironment::new(&HashMap::new(), &[]),
            terminal_size(24, 80),
            BACKLOG_BYTES,
        )
        .expect("open ack-gated yes");
    let key = streamed.key;
    let output = streamed.output;

    let (received_bytes, last_seq) = fill_backlog(&output, BACKLOG_BYTES);
    assert_eq!(received_bytes, BACKLOG_BYTES);
    assert!(matches!(
        output.recv_timeout(Duration::from_millis(40)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));

    table.ack(&key, last_seq).expect("ack bounded output");
    let resumed = output
        .recv_timeout(Duration::from_secs(1))
        .expect("reader resumes after ack");
    assert_eq!(resumed.seq, last_seq + 1);
    assert!(!resumed.bytes.is_empty());

    table.kill(&key).expect("kill streamed pane");
    assert_output_closes(output);
}

#[cfg(unix)]
#[test]
fn ack_rejects_future_sequences_and_cumulative_progress_stays_bounded() {
    const BACKLOG_BYTES: usize = 12;

    let _pty_guard = serial_pty_test();
    let table = Arc::new(PaneTable::new());
    let streamed = table
        .open_streamed(
            Path::new("/tmp"),
            &shell(
                "printf 1111; /bin/sleep 0.1; printf 2222; /bin/sleep 0.1; \
                 printf 3333; /bin/sleep 0.1; printf 4444; /bin/sleep 0.1; \
                 printf 5555; /bin/sleep 0.1; printf 6666; /bin/sleep 1000",
            ),
            PaneEnvironment::new(&HashMap::new(), &[]),
            terminal_size(24, 80),
            BACKLOG_BYTES,
        )
        .expect("open chunked output pane");
    let key = streamed.key;
    let output = streamed.output;

    let first = output
        .recv_timeout(Duration::from_secs(1))
        .expect("first output chunk");
    let second = output
        .recv_timeout(Duration::from_secs(1))
        .expect("second output chunk");
    let third = output
        .recv_timeout(Duration::from_secs(1))
        .expect("third output chunk");
    assert_eq!(
        [first.bytes, second.bytes, third.bytes].concat(),
        b"111122223333"
    );
    assert!(matches!(
        output.recv_timeout(Duration::from_millis(150)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));

    assert!(table.ack(&key, third.seq + 1).is_err());
    assert!(matches!(
        output.recv_timeout(Duration::from_millis(150)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));
    assert!(matches!(
        table.ack(&PaneKey::new(&key.id, key.generation + 1), second.seq),
        Err(PaneError::NotFound(_))
    ));

    table.ack(&key, second.seq).expect("cumulative ack");
    let mut resumed_bytes = Vec::new();
    while resumed_bytes.len() < 8 {
        resumed_bytes.extend(
            output
                .recv_timeout(Duration::from_secs(1))
                .expect("output released by cumulative ack")
                .bytes,
        );
    }
    assert_eq!(resumed_bytes, b"44445555");
    table.ack(&key, second.seq).expect("duplicate ack");
    table.ack(&key, first.seq).expect("older ack");
    assert!(matches!(
        output.recv_timeout(Duration::from_millis(150)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));

    table.ack(&key, third.seq).expect("partial cumulative ack");
    let sixth = output
        .recv_timeout(Duration::from_secs(1))
        .expect("sixth output chunk");
    assert_eq!(sixth.bytes, b"6666");
    table.kill(&key).expect("kill chunked output pane");
    assert_output_closes(output);
}

#[cfg(unix)]
#[test]
fn input_remains_responsive_while_output_waits_for_ack() {
    let _pty_guard = serial_pty_test();
    const BACKLOG_BYTES: usize = 64;
    const INPUT_BYTES: usize = 1 + 511 * 1024;
    // How long the flow may go without a step: a chunk of output, an ack, the write's end.
    // The flow itself is not timed: it is a thousand round trips of 64 bytes of output
    // and an ack each, then half a megabyte of input, and takes 4 s alone and over ten at
    // a load of 100 (T-581: 3 of 7 runs out of ten seconds at that load, the output
    // flowing all the while). A flow that is stuck is stuck for ever.
    const STALL: Duration = Duration::from_secs(30);

    let table = Arc::new(PaneTable::new());
    let streamed = table
        .open_streamed(
            Path::new("/tmp"),
            &shell(
                "/bin/stty raw -echo; printf READY; \
                 /usr/bin/yes flood | /usr/bin/head -c 65536; \
                 first=$(/usr/bin/od -An -tx1 -N 1); \
                 /bin/dd of=/dev/null bs=1024 count=511 2>/dev/null; \
                 printf 'INPUT:%s' \"$first\"; /bin/sleep 1000",
            ),
            PaneEnvironment::new(&HashMap::new(), &[]),
            terminal_size(24, 80),
            BACKLOG_BYTES,
        )
        .expect("open raw flooding input consumer");
    let key = streamed.key;
    let output = streamed.output;
    let mut initial_output = Vec::new();
    let mut last_seq = 0;
    while initial_output.len() < BACKLOG_BYTES {
        let chunk = output
            .recv_timeout(STALL)
            .expect("fill raw child output window");
        last_seq = chunk.seq;
        initial_output.extend(chunk.bytes);
    }
    assert!(initial_output.starts_with(b"READY"));
    assert!(matches!(
        output.recv_timeout(Duration::from_millis(100)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));

    let OpenedPane {
        key: responsive_key,
        reader: mut responsive_reader,
    } = open_shell(
        &table,
        "/bin/stty raw -echo; printf ready; /usr/bin/od -An -tx1 -N 1",
    );
    let mut responsive_ready = [0_u8; 5];
    responsive_reader
        .read_exact(&mut responsive_ready)
        .expect("responsive raw pane ready");
    assert_eq!(&responsive_ready, b"ready");

    let (write_sender, write_receiver) = std::sync::mpsc::channel();
    let writer_table = Arc::clone(&table);
    let writer_key = key.clone();
    let writer = thread::spawn(move || {
        let mut bytes = vec![b'Z'; INPUT_BYTES];
        bytes[0] = b'Z';
        let _ = write_sender.send(writer_table.write(&writer_key, &bytes));
    });
    thread::sleep(Duration::from_millis(50));
    assert!(matches!(
        write_receiver.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));

    // The write to the other pane returns while this one's is blocked: it would, if it
    // were held up by the flow, wait for acks that only come once it has returned.
    let (answer_sender, answer_receiver) = std::sync::mpsc::channel();
    let (answering_table, answering_key) = (Arc::clone(&table), responsive_key.clone());
    let answering = thread::spawn(move || {
        let _ = answer_sender.send(answering_table.write(&answering_key, b"R"));
    });
    answer_receiver
        .recv_timeout(STALL)
        .expect("write to unrelated pane returns while the flood cycle is blocked")
        .expect("write to unrelated pane while the flood cycle is blocked");
    answering.join().expect("responsive writer thread");
    assert!(
        matches!(
            write_receiver.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ),
        "the blocked write was still blocked when the other returned"
    );
    assert_eq!(
        String::from_utf8(read_to_end(responsive_reader))
            .expect("responsive recorder output")
            .split_whitespace()
            .collect::<String>(),
        "52"
    );

    let consumer_table = Arc::clone(&table);
    let consumer_key = key.clone();
    let consumer = thread::spawn(move || {
        let mut terminal_output = initial_output;
        consumer_table
            .ack(&consumer_key, last_seq)
            .expect("release initial output window");
        loop {
            let compact = String::from_utf8_lossy(&terminal_output)
                .split_whitespace()
                .collect::<String>();
            if compact.contains("INPUT:5a") {
                return terminal_output;
            }
            let chunk = output
                .recv_timeout(STALL)
                .expect("raw child did not consume input: no output after an ack");
            consumer_table
                .ack(&consumer_key, chunk.seq)
                .expect("ack flood output");
            terminal_output.extend(chunk.bytes);
        }
    });

    // The child prints INPUT once it has consumed all the input, and the write is over
    // by then: the output is followed to there, and the write's end is waited for after.
    let terminal_output = consumer.join().expect("output consumer thread");
    write_receiver
        .recv_timeout(STALL)
        .expect("blocked input write resumes after output acks")
        .expect("write complete input body");
    writer.join().expect("input writer thread");
    assert!(String::from_utf8_lossy(&terminal_output)
        .split_whitespace()
        .collect::<String>()
        .contains("INPUT:5a"));

    table.kill(&key).expect("kill streamed pane");
    table.kill(&responsive_key).expect("remove responsive pane");
}

#[cfg(windows)]
mod windows {
    use std::collections::HashMap;
    use std::sync::mpsc::RecvTimeoutError;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    use portable_pty::PtySize;

    use super::super::conpty_test::{line_echo, open, powershell, read_until, system};
    use super::super::{serial_pty_test, PaneEnvironment, PaneTable};
    use super::parent_session_variables;

    fn number_after(text: &str, marker: &str) -> u32 {
        let at = text.find(marker).expect("marker printed") + marker.len();
        text[at..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .expect("a number after the marker")
    }

    fn running(pid: u32) -> bool {
        // SAFETY: a query-only handle, closed before returning.
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return false;
            }
            let mut code = 0u32;
            let read = GetExitCodeProcess(process, &mut code);
            CloseHandle(process);
            read != 0 && code == STILL_ACTIVE as u32
        }
    }

    #[test]
    fn the_environment_and_a_resize_reach_the_child() {
        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let env = HashMap::from([("CF_PANE_VALUE".to_string(), "overlaid".to_string())]);
        let (key, output) = open(
            &table,
            &powershell(
                "Write-Output \"ENV=$env:CF_PANE_VALUE\"; [Console]::In.ReadLine() | Out-Null; \
                 Write-Output \"SIZE=$([Console]::WindowWidth)#\"",
            ),
            &env,
            1 << 20,
        );
        assert!(read_until(&table, &key, &output, "ENV=overlaid").contains("ENV=overlaid"));
        table.resize(&key, 30, 100).expect("resize the pane");
        table.write(&key, b"\r").expect("go on");
        let text = read_until(&table, &key, &output, "#");
        assert_eq!(number_after(&text, "SIZE="), 100, "{text:?}");
        table.kill(&key).expect("kill the pane");
    }

    /// T-971, T-868: as the Unix test of it says, on ConPTY: a pane's program holds none
    /// of a session the host was started in, but what its frame sets itself.
    #[test]
    fn a_pane_inherits_nothing_of_a_session_the_host_was_started_in() {
        let _pty_guard = serial_pty_test();
        let _sessions = super::InParentSessions::set();
        let names: Vec<String> = parent_session_variables()
            .map(|name| format!("'{name}'"))
            .collect();
        // One line a name it holds, short of the pane's width, and the end.
        let script = format!(
            "foreach ($n in @({})) {{ if (Test-Path \"env:$n\") {{ Write-Output \"HAS=$n\" }} }}; \
             Write-Output 'DONE#'",
            names.join(",")
        );
        let table = Arc::new(PaneTable::new());
        let (key, output) = open(&table, &powershell(&script), &HashMap::new(), 1 << 20);
        let text = read_until(&table, &key, &output, "DONE#");
        assert!(!text.contains("HAS="), "{text:?}");
        table.kill(&key).expect("kill the pane");

        let own = HashMap::from([("CLAUDECODE".to_string(), "1".to_string())]);
        let (key, output) = open(&table, &powershell(&script), &own, 1 << 20);
        let text = read_until(&table, &key, &output, "DONE#");
        assert!(text.contains("HAS=CLAUDECODE"), "{text:?}");
        assert_eq!(text.matches("HAS=").count(), 1, "{text:?}");
        table.kill(&key).expect("kill the pane");
    }

    #[test]
    fn kill_ends_everything_the_harness_started() {
        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let ping = system("PING.EXE");
        let (key, output) = open(
            &table,
            &powershell(&format!(
                "$p = Start-Process -PassThru -WindowStyle Hidden -FilePath '{ping}' -ArgumentList '-n','1000','127.0.0.1'; \
                 Write-Output \"PID=$($p.Id)#\"; Start-Sleep 1000"
            )),
            &HashMap::new(),
            1 << 20,
        );
        let text = read_until(&table, &key, &output, "#");
        let started = number_after(&text, "PID=");
        assert!(running(started), "the detached child is not running");

        table.kill(&key).expect("kill the pane");
        let deadline = Instant::now() + Duration::from_secs(5);
        while running(started) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            !running(started),
            "a child the harness started survived its window"
        );
    }

    /// ConPTY keeps the output open after the program ends, so without
    /// the exit watcher this pane's output never ended, and the end of a
    /// pane's output is what sends `pane.exit`.
    #[test]
    fn a_program_that_exits_on_its_own_ends_its_output_and_leaves_the_table() {
        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let (key, output) = open(
            &table,
            &powershell("Write-Output 'BYE#'"),
            &HashMap::new(),
            1 << 20,
        );
        read_until(&table, &key, &output, "BYE#");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match output.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                // The pane may have left the table by the time this
                // chunk is read, and an ack then has nothing to free.
                Ok(chunk) => {
                    let _ = table.ack(&key, chunk.seq);
                }
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    panic!("the output went on after the program exited")
                }
            }
        }
        assert!(
            table.list().expect("list the panes").is_empty(),
            "the ended pane stayed in the table"
        );
    }

    /// A pane names its program's process: PowerShell's own `$PID`.
    #[test]
    fn a_pane_names_its_programs_process() {
        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let streamed = table
            .open_streamed(
                &std::env::temp_dir(),
                &powershell("Write-Output \"PID=$PID#\"; Start-Sleep 1000"),
                PaneEnvironment::new(&HashMap::new(), &[]),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
                1 << 20,
            )
            .expect("open a ConPTY pane");
        let text = read_until(&table, &streamed.key, &streamed.output, "#");
        assert_eq!(streamed.pid, Some(number_after(&text, "PID=")), "{text:?}");
        table.kill(&streamed.key).expect("kill the pane");
    }

    /// The exit code of a program that ends on its own is kept for the
    /// exit event, which comes after the exit watcher has taken the pane
    /// out of the table, and what ConPTY drew of what it printed (with
    /// positions, not blanks) reads back as the text on the screen.
    #[test]
    fn a_programs_exit_code_and_screen_are_kept_when_it_exits_on_its_own() {
        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let streamed = table
            .open_streamed(
                &std::env::temp_dir(),
                &powershell("Write-Output 'NO API KEY FOUND#'; exit 3"),
                PaneEnvironment::new(&HashMap::new(), &[]),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
                1 << 20,
            )
            .expect("open a ConPTY pane");
        // What the host's output thread does with each chunk, and ConPTY's
        // question where the cursor is, until the output ends.
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match streamed.output.recv_timeout(left) {
                Ok(chunk) => {
                    let _ = table.ack(&streamed.key, chunk.seq);
                    if chunk.bytes.windows(4).any(|bytes| bytes == b"\x1b[6n") {
                        let _ = table.write(&streamed.key, b"\x1b[1;1R");
                    }
                    streamed.screen.feed(&chunk.bytes);
                }
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    panic!("the output went on after the program exited")
                }
            }
        }
        let ended = table
            .wait_ended(&streamed.key, &streamed.ended, Duration::from_secs(5))
            .expect("how the program ended");
        assert_eq!((ended.code, ended.signal), (3, None));
        let shown = streamed.screen.tail(12);
        assert!(
            shown.iter().any(|line| line.contains("NO API KEY FOUND#")),
            "{shown:?}"
        );
    }

    /// A program the engine closes still has how it ended, which the exit
    /// event is made from after the pane has left the table.
    #[test]
    fn a_killed_program_still_says_how_it_ended() {
        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let streamed = table
            .open_streamed(
                &std::env::temp_dir(),
                &powershell("Write-Output 'READY#'; Start-Sleep 1000"),
                PaneEnvironment::new(&HashMap::new(), &[]),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
                1 << 20,
            )
            .expect("open a ConPTY pane");
        read_until(&table, &streamed.key, &streamed.output, "READY#");
        table.kill(&streamed.key).expect("kill the pane");
        assert!(
            table
                .wait_ended(&streamed.key, &streamed.ended, Duration::from_secs(5))
                .is_some(),
            "a killed program has an exit status"
        );
    }

    #[test]
    fn input_reaches_the_child() {
        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let (key, output) = open(&table, &line_echo(), &HashMap::new(), 1 << 20);
        read_until(&table, &key, &output, "READY");
        table
            .write(&key, b"typed words\r")
            .expect("type into the pane");
        assert!(read_until(&table, &key, &output, "#").contains("GOT=[typed words]"));
        table.kill(&key).expect("kill the pane");
    }

    #[test]
    fn unacked_output_is_bounded_then_resumes_after_ack() {
        const BACKLOG_BYTES: usize = 128;
        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let (key, output) = open(
            &table,
            &[
                system("cmd.exe"),
                "/d".into(),
                "/c".into(),
                "for /L %i in (1,0,2) do @echo y".into(),
            ],
            &HashMap::new(),
            BACKLOG_BYTES,
        );
        let mut received = 0;
        let mut last_seq = 0;
        while received < BACKLOG_BYTES {
            let chunk = output
                .recv_timeout(Duration::from_secs(10))
                .expect("output up to the backlog");
            if chunk.bytes.windows(4).any(|bytes| bytes == b"\x1b[6n") {
                table
                    .write(&key, b"\x1b[1;1R")
                    .expect("answer the cursor query");
            }
            received += chunk.bytes.len();
            last_seq = chunk.seq;
        }
        assert_eq!(received, BACKLOG_BYTES);
        assert!(matches!(
            output.recv_timeout(Duration::from_millis(200)),
            Err(RecvTimeoutError::Timeout)
        ));
        table.ack(&key, last_seq).expect("ack the backlog");
        let resumed = output
            .recv_timeout(Duration::from_secs(5))
            .expect("output resumes after the ack");
        assert_eq!(resumed.seq, last_seq + 1);
        table.kill(&key).expect("kill the pane");
    }
}

/// N17 (T-2139 finding 1): a read is counted the moment the pty returns it, before the lock of
/// the pane's activity time: a thread kept from that lock has told the window's readers of its
/// bytes (the child's `Z` was read and the count still 258/258 while the activity mutex was
/// held, and the paste's Enter went).
#[cfg(unix)]
#[test]
fn a_read_is_counted_while_the_activity_lock_is_held() {
    let _pty_guard = serial_pty_test();
    let table = Arc::new(PaneTable::new());
    let streamed = table
        .open_streamed(
            Path::new("/tmp"),
            &shell("/bin/sleep 1.5; printf x; /bin/sleep 1000"),
            PaneEnvironment::new(&HashMap::new(), &[]),
            terminal_size(24, 80),
            1024,
        )
        .expect("open delayed-output pane");
    let activity = Arc::clone(
        &table
            .lock_panes()
            .unwrap()
            .get(&streamed.key)
            .expect("the pane")
            .last_activity,
    );
    let held = activity.lock().unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    while streamed.counts.both().0 == 0 {
        assert!(
            Instant::now() < until,
            "the byte the pty gave was not counted while the activity lock was held"
        );
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(streamed.counts.both(), (1, 0), "read, and not yet drawn");
    drop(held);
    table.kill(&streamed.key).expect("kill the pane");
}

/// T-2198 note 5: a gate asked with the pane's writer held looks at the screen it was given with
/// the writer, and asks the pane table for nothing: while a resize or the opening of another pane
/// holds the table through a native call, the writer of this pane would be held behind it.
/// Here another thread holds the table, as they do, from the moment the gate is asked until the
/// gate has looked (or three seconds).
#[cfg(unix)]
#[test]
fn a_gate_asked_with_the_writer_held_asks_the_table_for_nothing() {
    let _serial = serial_pty_test();
    let table = Arc::new(PaneTable::new());
    let OpenedPane {
        key: pane_key,
        mut reader,
    } = open_shell(&table, "exec /bin/cat");
    // The window's output is read, as the host reads it: a program that ends with its terminal
    // output unread waits for it to drain.
    let drain = thread::spawn(move || {
        let mut sink = [0; 256];
        while reader.read(&mut sink).is_ok_and(|read| read > 0) {}
    });
    let (asked_tx, asked) = std::sync::mpsc::channel();
    let (held_tx, held) = std::sync::mpsc::channel();
    let (looked_tx, looked) = std::sync::mpsc::channel();
    let holder = {
        let table = Arc::clone(&table);
        thread::spawn(move || {
            asked
                .recv_timeout(Duration::from_secs(10))
                .expect("the gate is asked");
            let busy = table.lock_panes().expect("the table");
            held_tx.send(()).unwrap();
            let waited = looked.recv_timeout(Duration::from_secs(3)).is_err();
            drop(busy);
            waited
        })
    };
    let (writing, key) = (Arc::clone(&table), pane_key.clone());
    let writer = thread::spawn(move || {
        writing
            .write_gated(&key, b"x", &mut |screening| {
                asked_tx.send(()).unwrap();
                held.recv_timeout(Duration::from_secs(10))
                    .expect("the table is held");
                let mut shown = false;
                screening(&mut |seen| shown = seen.is_some());
                looked_tx.send(()).unwrap();
                assert!(shown, "the screen was looked at");
                super::Gate::Go
            })
            .is_ok()
    });
    let written = writer.join().unwrap();
    let waited = holder.join().unwrap();
    table.kill(&pane_key).expect("kill the pane");
    drain.join().unwrap();
    assert!(written, "the write went");
    assert!(
        !waited,
        "the gate waited for the table while it held the writer"
    );
}

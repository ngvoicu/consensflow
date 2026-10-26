//! What the end of a window's output comes to, on Unix: the reader asks whether the
//! program has gone while its terminal says nothing, reads what the terminal still has
//! once it has, and a program that outlived its output is ended, and gone, before the
//! daemon is told, or named as one that could not be ended at the bound. A terminal opened
//! again after an end of its output, and Sol's sequence, are the headless helper's tests
//! (`tests/headless.rs`); a console host that died under a program that runs on is the
//! Windows test of the pane handlers (`pane_handlers/windows_tests.rs`), which drives it
//! as the host does.

#![cfg(unix)]

use std::cell::Cell;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::PtySize;

use super::{Gone, Got, Handed, Output};
use crate::ended::EndedSlot;
use crate::pty::close::terminate;
use crate::pty::{
    process_exists, serial_pty_test, terminal_of, PaneError, PaneKey, PaneTable, Sentinel,
    EXIT_GRACE,
};

/// The slot the pane's program's end is kept in.
fn slot_of(table: &PaneTable, key: &PaneKey) -> Arc<EndedSlot> {
    let panes = table.panes.lock().expect("the table");
    Arc::clone(&panes.get(key).expect("the pane").ended)
}

fn size() -> PtySize {
    PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// A pane of `script` in a shell, and its raw output.
fn open(table: &PaneTable, script: &str) -> (PaneKey, Box<dyn Read + Send>) {
    let opened = table
        .open(
            Path::new("/tmp"),
            &["/bin/sh".into(), "-c".into(), script.into()],
            &HashMap::new(),
            size(),
        )
        .expect("open a pane");
    (opened.key, opened.reader)
}

/// The pane's output as the host reads it, asking `gone` whether its program has gone.
fn output(table: &PaneTable, key: &PaneKey, reader: Box<dyn Read + Send>, gone: Gone) -> Output {
    let panes = table.panes.lock().expect("the table");
    let terminal = terminal_of(panes.get(key).expect("the pane")).expect("its terminal");
    Output::new(reader, terminal, gone)
}

/// Everything `output` gives until it ends, on a thread of its own: each piece as it
/// comes, and `None` at its end. Then, or once nothing takes the pieces, the terminal is
/// read on until nothing holds it, as the host reads it through a close: on macOS a
/// program's exit waits for what it wrote to be read, and a close waits for the exit. A
/// test can say its program has gone while it runs and writes on, where the host's reader
/// reads on; left unread, the close of such a program waited for ever (a macOS runner,
/// 2026-10-16).
fn read_all(mut output: Output) -> mpsc::Receiver<Option<Vec<u8>>> {
    let (send, read) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = [0; 1024];
        while let Got::Bytes(count) = output.read(&mut bytes) {
            if send.send(Some(bytes[..count].to_vec())).is_err() {
                break;
            }
        }
        let _ = send.send(None);
        while matches!(output.reader.read(&mut bytes), Ok(count) if count > 0) {}
    });
    read
}

/// What `pieces` gives until the output's end, within `wait`: none where it has not ended.
fn until_the_end(pieces: &mpsc::Receiver<Option<Vec<u8>>>, wait: Duration) -> Option<String> {
    let deadline = Instant::now() + wait;
    let mut seen = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match pieces.recv_timeout(left) {
            Ok(Some(bytes)) => seen.extend(bytes),
            Ok(None) => return Some(String::from_utf8_lossy(&seen).into_owned()),
            Err(_) => return None,
        }
    }
}

/// A program that says nothing and holds its terminal: the reader waited on its silence,
/// and an exit it did not see (its helper held the terminal: Sol's sequence) was never
/// told. It asks whether the program has gone while the terminal says nothing.
#[test]
fn a_reader_of_a_silent_terminal_ends_once_the_program_has_gone() {
    let _pty_guard = serial_pty_test();
    let table = PaneTable::closing_within(Duration::from_millis(500));
    let (key, reader) = open(&table, "exec /bin/sleep 1000");
    let gone = Arc::new(AtomicBool::new(false));
    let said = Arc::clone(&gone);
    let pieces = read_all(output(
        &table,
        &key,
        reader,
        Box::new(move |_| said.load(Ordering::SeqCst)),
    ));

    assert_eq!(
        until_the_end(&pieces, Duration::from_millis(500)),
        None,
        "a program that runs and says nothing has not ended its output"
    );
    gone.store(true, Ordering::SeqCst);
    assert!(
        until_the_end(&pieces, Duration::from_secs(3)).is_some(),
        "the output did not end once the program had gone"
    );
    table.kill(&key).expect("close the pane");
}

/// The program opens its terminal again, says its last words and exits between the end
/// the reader saw and its look at the program: the words are read before the output
/// ends (Astra's race, the second review of 2026-10-16), not dropped with what the reader
/// discards once the output has ended. On Linux the program alone leaves its words on the
/// terminal; macOS drops what a last descriptor of a terminal leaves unread as it closes,
/// so here a helper the program leaves holds the terminal, as Sol's does, saying nothing.
#[cfg(target_os = "macos")]
#[test]
fn what_a_program_says_after_an_end_of_its_output_and_before_it_is_seen_gone_is_read() {
    let _pty_guard = serial_pty_test();
    let table = Arc::new(PaneTable::closing_within(Duration::from_millis(500)));
    let asked = std::env::temp_dir().join(format!("cf-last-words-{}", std::process::id()));
    let _ = std::fs::remove_file(&asked);
    let script = format!(
        "exec /usr/bin/python3 -c '
import ctypes, os, signal, time
signal.signal(signal.SIGHUP, signal.SIG_IGN)
name = os.ttyname(0)
print(\"READY\", flush=True)
ctypes.CDLL(None).revoke(name.encode())
while not os.path.exists(\"{asked}\"):
    time.sleep(0.01)
again = os.open(name, os.O_RDWR | os.O_NOCTTY)
helper = os.fork()
if helper == 0:
    os.setsid()
    time.sleep(20)
    os._exit(0)
os.write(again, b\"HELPER %d LAST WORDS\\r\\n\" % helper)
os._exit(5)
'",
        asked = asked.display()
    );
    let (key, reader) = open(&table, &script);
    // Asked whether the program has gone, the host is slow to say: meanwhile the program
    // opens its terminal again, says its last words and exits.
    let (watched, file, slot) = (key.clone(), asked.clone(), slot_of(&table, &key));
    let looks = Arc::clone(&table);
    let gone = Box::new(move |_| {
        let _ = std::fs::write(&file, "go");
        looks.proven_within(&watched, &slot, Duration::from_secs(10))
    });
    let pieces = read_all(output(&table, &key, reader, gone));
    let seen = until_the_end(&pieces, Duration::from_secs(15));
    let _ = std::fs::remove_file(&asked);
    // The helper is a detached job: it left its session and lost its parent, and holds the
    // terminal. It is the test's own, so the guard of the test's signals is told so
    // (`register`) and the close of the pane may end it, as it ends what a window leaves
    // running. The test does not end it first: a helper the test had sent SIGKILL was still
    // dying when the close came for it, on a loaded machine (T-643: 4 of 588 runs at a load
    // of 130 to 190), and the guard, which knew nothing of it, stopped the close's SIGTERM
    // and the test with it. Here the close finds it running every time.
    let said = seen.clone().unwrap_or_default();
    let helper = said
        .split("HELPER ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|number| number.parse::<i32>().ok());
    if let Some(helper) = helper {
        crate::process_tree::guard::register(helper);
    }
    let seen = seen.expect("the output's end");
    assert!(
        seen.contains("LAST WORDS"),
        "its last words were dropped: {seen:?}"
    );
    table.kill(&key).expect("close the pane");
    // The helper is ended with the pane, or by the test: it is not left for its twenty seconds.
    if let Some(helper) = helper {
        let until = Instant::now() + Duration::from_secs(30);
        while process_exists(helper) && Instant::now() < until {
            thread::sleep(Duration::from_millis(10));
        }
        let left = process_exists(helper);
        if left {
            // SAFETY: signal 9 to the helper this test's program started, by its number.
            unsafe { libc::kill(helper, 9) };
        }
        assert!(
            !left,
            "the pane's close left the helper of its program running"
        );
    }
}

/// A program that outlived its output is ended, and gone, before the daemon is told the
/// window closed: told first, the daemon could open another window on the conversation
/// while the old program still ran (both reviews of 2026-10-16).
#[test]
fn a_program_that_outlived_its_output_is_gone_before_the_daemon_is_told_and_its_pane_leaves() {
    let _pty_guard = serial_pty_test();
    let mut outsider = Sentinel::start();
    let table = PaneTable::closing_within(Duration::from_millis(500));
    let (key, _reader) = open(&table, "exec /bin/sleep 1000");
    let program = table.process_group_id(&key).expect("the pane's program");
    assert!(process_exists(program));

    let told_while_it_ran = Cell::new(None);
    let slot = slot_of(&table, &key);
    let out = table.output_ended(&key, &slot, None, |handed| {
        told_while_it_ran.set(Some((process_exists(program), handed)));
    });
    assert!(
        out.expect("the end of its output"),
        "the pane leaves the table"
    );
    assert_eq!(
        told_while_it_ran.get(),
        Some((
            false,
            Handed {
                survivor: None,
                ended_by_host: true
            }
        )),
        "the daemon was told the window closed while its program ran"
    );
    assert!(table.list().expect("list the panes").is_empty());
    assert!(outsider.runs(), "a process outside the window");
}

/// A program that has gone is not waited for: the daemon is told at once.
#[test]
fn the_daemon_is_told_at_once_of_a_program_that_has_gone() {
    let _pty_guard = serial_pty_test();
    let table = PaneTable::closing_within(Duration::from_millis(500));
    let (key, _reader) = open(&table, "exit 0");
    let slot = slot_of(&table, &key);
    assert!(table.proven_within(&key, &slot, Duration::from_secs(5)));
    let started = Instant::now();
    let told = Cell::new(None);
    assert!(table
        .output_ended(&key, &slot, None, |handed| told.set(Some(handed)))
        .expect("the end of its output"));
    assert_eq!(
        told.get(),
        Some(Handed {
            survivor: None,
            ended_by_host: false
        })
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
}

/// A program that keeps writing (a helper it left, a byte every twenty milliseconds)
/// gives the reader something to read at every look: it asked whether the program had
/// gone only when nothing came, so it never asked, and no `pane.exit` came (both reviews
/// of round 3). It asks on a schedule of its own, and once the program has gone, what
/// still comes is read for a second at most.
#[test]
fn a_reader_of_a_terminal_that_never_goes_quiet_ends_once_the_program_has_gone() {
    let _pty_guard = serial_pty_test();
    let table = PaneTable::closing_within(Duration::from_millis(500));
    let (key, reader) = open(&table, "while :; do printf .; sleep 0.02; done");
    let gone = Arc::new(AtomicBool::new(false));
    let said = Arc::clone(&gone);
    let pieces = read_all(output(
        &table,
        &key,
        reader,
        Box::new(move |_| said.load(Ordering::SeqCst)),
    ));
    assert_eq!(
        until_the_end(&pieces, Duration::from_millis(500)),
        None,
        "a program that runs has not ended its output"
    );
    gone.store(true, Ordering::SeqCst);
    assert!(
        until_the_end(&pieces, Duration::from_secs(3)).is_some(),
        "the output did not end once the program had gone, though it went on coming"
    );
    table.kill(&key).expect("close the pane");
}

/// A window of the name `key`'s, the generation after it.
fn open_again(table: &PaneTable, key: &PaneKey) -> (PaneKey, Result<(), PaneError>) {
    let again = PaneKey::new(key.id.clone(), key.generation + 1);
    let opened = table.open_at(
        again.clone(),
        Path::new("/tmp"),
        &["/bin/sh".into(), "-c".into(), "exit 0".into()],
        &HashMap::new(),
        size(),
    );
    (again, opened.map(|_| ()))
}

/// A program the host cannot prove gone in the bound (here, one a close has taken out of
/// the table, which the test holds so that nothing ends it): the daemon is told the window
/// closed with the program named as one that could not be ended, and a window of the same
/// name opens all the same, the note warning that the program may still run. A registry of
/// such programs refused it until the proof came, and for ever where the proof never came
/// of a program that had gone (both reviews of round 4).
#[test]
fn a_program_not_proven_gone_in_the_bound_is_named_and_a_window_of_its_name_opens() {
    let _pty_guard = serial_pty_test();
    let mut outsider = Sentinel::start();
    let table = PaneTable::closing_within(Duration::from_millis(500));
    let (key, _reader) = open(&table, "exec /bin/sleep 1000");
    let program = table.process_group_id(&key).expect("the pane's program");
    let slot = slot_of(&table, &key);
    let mut held = table
        .panes
        .lock()
        .expect("the table")
        .remove(&key)
        .expect("the pane");

    let told = Cell::new(None);
    let pid = u32::try_from(program).ok();
    let out = table.output_ended_within(&key, &slot, pid, Duration::from_millis(1_500), |handed| {
        told.set(Some(handed));
    });
    let (again, opened) = open_again(&table, &key);
    let ran = process_exists(program);
    terminate(&mut held, Duration::from_millis(500)).expect("end the program");
    let _ = table.kill(&again);

    assert!(matches!(out, Ok(true)), "{out:?}");
    assert_eq!(
        told.get(),
        Some(Handed {
            survivor: pid,
            ended_by_host: false
        }),
        "the daemon was told the window closed as if its program had gone"
    );
    assert!(
        ran,
        "the program had gone before the window of its name opened"
    );
    assert!(
        opened.is_ok(),
        "a window of the name was refused: {:?}",
        opened.err()
    );
    assert!(outsider.runs(), "a process outside the window");
}

/// A program that ignores the request to end is given a close's grace, and a close waits
/// for its exit with no bound (on Windows a program that waits on I/O it cannot cancel is
/// not gone when it is terminated): the bound waited on the close it made, and neither
/// `pane.exit` nor the word that the program could not be ended came in it (both reviews
/// of round 4). The close goes on on a thread of its own, with the pane out of the table;
/// the daemon is told at the bound, the program named while it runs, and a window of the
/// same name opens. The grace is a minute here, and the program is the test's to end.
#[test]
fn a_program_the_close_has_not_ended_by_the_bound_is_named_then_and_its_close_goes_on() {
    let _pty_guard = serial_pty_test();
    let table = Arc::new(PaneTable::closing_within(Duration::from_secs(60)));
    let (key, _reader) = open(&table, "trap '' TERM HUP; exec /bin/sleep 1000");
    let program = table.process_group_id(&key).expect("the pane's program");
    let slot = slot_of(&table, &key);
    let pid = u32::try_from(program).ok();
    let (send, told) = mpsc::channel();
    let (ends, watched, kept) = (Arc::clone(&table), key.clone(), Arc::clone(&slot));
    // Past the wait for the exit, so the program is ended by the end of its output.
    let bound = EXIT_GRACE + Duration::from_secs(2);
    let ending = thread::spawn(move || {
        ends.output_ended_within(&watched, &kept, pid, bound, |handed| {
            let _ = send.send((handed, process_exists(program)));
        })
    });

    let handed = told.recv_timeout(bound + Duration::from_secs(5));
    let listed = table.list().expect("list the panes");
    let (again, opened) = open_again(&table, &key);
    // SAFETY: signal 9 to the pane's program, by the number its pane gave.
    unsafe { libc::kill(program, 9) };
    let _ = table.kill(&again);
    let out = ending.join().expect("the end of the output");

    assert_eq!(
        handed.ok(),
        Some((
            Handed {
                survivor: pid,
                ended_by_host: true
            },
            true
        )),
        "the daemon was not told in the bound of the program that ran"
    );
    assert!(matches!(out, Ok(true)), "{out:?}");
    assert!(
        listed.is_empty(),
        "the pane stayed in the table: {listed:?}"
    );
    assert!(
        opened.is_ok(),
        "a window of the name was refused: {:?}",
        opened.err()
    );
    assert!(
        table.proven_within(&key, &slot, Duration::from_secs(5)),
        "the close did not read the end of the program once it had gone"
    );
}

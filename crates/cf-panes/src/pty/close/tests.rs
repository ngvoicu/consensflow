//! A window closes the way a terminal does on Unix (`super`): its programs are
//! asked to end, with TERM and HUP, and given a grace, and only what is left
//! after it is killed. A job that traps the request gets to release what it
//! holds; one that ignores it is killed once the grace is over; every window of a
//! table that goes is asked before any is waited for. On Windows nothing is asked
//! and nothing waits: the pane's job object ends the tree at once, and a window
//! whose program is behind a `.cmd` shim closes without ending the process that
//! closes it.

#[cfg(unix)]
mod unix {
    use std::collections::HashMap;
    use std::fs;
    use std::io::Read;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    use portable_pty::PtySize;

    use crate::headless::reap_all;
    use crate::pty::{process_exists, serial_pty_test, OpenedPane, PaneTable, Sentinel};

    /// What a test's table gives the programs of a window it closes: long enough
    /// to tell from none, and short enough to wait for.
    const GRACE: Duration = Duration::from_secs(1);

    /// A pane running `script` under `/bin/sh`.
    fn open(table: &PaneTable, script: &str) -> OpenedPane {
        table
            .open(
                Path::new("/tmp"),
                &["/bin/sh".into(), "-c".into(), script.into()],
                &HashMap::new(),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            )
            .expect("open a shell in a pane")
    }

    /// A file of the test's own, not there yet.
    fn scratch(name: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "cf-close-{}-{}-{name}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_file(&path);
        path
    }

    /// Reads the pane's output until `marker` has shown: the program has set up
    /// what the test asks of it.
    fn until_shown(reader: &mut Box<dyn Read + Send>, marker: &str) {
        let mut seen = Vec::new();
        let mut chunk = [0u8; 256];
        while !String::from_utf8_lossy(&seen).contains(marker) {
            let read = reader.read(&mut chunk).expect("read the pane's output");
            assert!(read > 0, "the pane ended before it showed {marker}");
            seen.extend_from_slice(&chunk[..read]);
        }
    }

    /// Waits until a file is there, up to `wait`.
    fn until_there(file: &Path, wait: Duration) -> bool {
        let deadline = Instant::now() + wait;
        while !file.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        file.exists()
    }

    /// The number a job writes in `file` once it runs, waited for up to ten seconds.
    #[cfg(target_os = "macos")]
    fn pid_in(file: &Path) -> i32 {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(pid) = fs::read_to_string(file)
                .ok()
                .and_then(|text| text.trim().parse().ok())
            {
                return pid;
            }
            assert!(Instant::now() < deadline, "the job runs");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Waits until nothing of the process group is left, up to two seconds.
    fn gone(group: i32) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while process_exists(-group) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        !process_exists(-group)
    }

    /// A program that sets a trap with `handler` for the requests to end (TERM and
    /// HUP), starts a job that says in `started` that it runs, and waits. The
    /// test waits for the job before it closes the window: a job forked and not
    /// yet executed still has its program's traps, which take the requests in
    /// its place, and it outlives them and holds the close for the whole grace.
    fn trapping(handler: &str, started: &Path) -> String {
        format!(
            "trap '{handler}' TERM HUP; sh -c 'echo up > {}; exec sleep 30' & printf ready; wait",
            started.display()
        )
    }

    #[test]
    fn a_program_that_traps_the_request_to_end_runs_its_handler_and_the_close_does_not_wait_out_the_grace(
    ) {
        let _serial = serial_pty_test();
        let mut outsider = Sentinel::start();
        let table = PaneTable::closing_within(GRACE * 8);
        let (released, started) = (scratch("released"), scratch("started"));
        let OpenedPane { key, mut reader } = open(
            &table,
            &trapping(
                &format!("echo released > {}; exit 0", released.display()),
                &started,
            ),
        );
        until_shown(&mut reader, "ready");
        assert!(
            until_there(&started, Duration::from_secs(10)),
            "the job runs"
        );

        let closing = Instant::now();
        table.kill(&key).expect("close the pane");

        assert!(
            closing.elapsed() < GRACE * 4,
            "the programs had ended: nothing was left to wait for ({:?})",
            closing.elapsed()
        );
        assert_eq!(
            fs::read_to_string(&released)
                .expect("the handler ran")
                .trim(),
            "released"
        );
        assert!(outsider.runs(), "a process outside the window");
        let _ = fs::remove_file(&released);
        let _ = fs::remove_file(&started);
    }

    #[test]
    fn a_program_that_ignores_the_request_to_end_is_killed_once_the_grace_is_over() {
        let _serial = serial_pty_test();
        let mut outsider = Sentinel::start();
        let table = PaneTable::closing_within(GRACE);
        let OpenedPane { key, mut reader } = open(&table, &deaf());
        until_shown(&mut reader, "ready");
        let group = table.process_group_id(&key).expect("the pane's group");

        let closing = Instant::now();
        table.kill(&key).expect("close the pane");

        assert!(
            closing.elapsed() >= GRACE,
            "the program was given its grace: {:?}",
            closing.elapsed()
        );
        assert!(
            closing.elapsed() < GRACE * 4,
            "and no more: {:?}",
            closing.elapsed()
        );
        assert!(gone(group), "what ignored the request is killed");
        assert!(outsider.runs(), "a process outside the window");
    }

    #[test]
    fn closing_a_pane_that_already_ended_asks_nothing_and_waits_for_nothing() {
        let _serial = serial_pty_test();
        let mut outsider = Sentinel::start();
        let table = PaneTable::closing_within(GRACE * 8);
        let OpenedPane { key, mut reader } = open(&table, "printf done");
        until_shown(&mut reader, "done");
        // Its program is gone, which its exit status shows once it is read.
        std::thread::sleep(Duration::from_millis(200));

        let closing = Instant::now();
        table.kill(&key).expect("close the pane");

        assert!(closing.elapsed() < GRACE * 4, "{:?}", closing.elapsed());
        assert!(outsider.runs(), "a process outside the window");
    }

    /// A program that ignores the request to end and runs on, as long as the test that opened
    /// its window is there.
    fn deaf() -> String {
        cf_base::fixture::deaf("TERM HUP", "printf ready")
    }

    #[test]
    fn a_table_that_goes_asks_every_window_to_end_before_it_waits_on_any() {
        let _serial = serial_pty_test();
        let mut outsider = Sentinel::start();
        let table = PaneTable::closing_within(GRACE);
        let mut readers = Vec::new();
        for _ in 0..2 {
            let OpenedPane { mut reader, .. } = open(&table, &deaf());
            until_shown(&mut reader, "ready");
            readers.push(reader);
        }

        let closing = Instant::now();
        drop(table);

        assert!(
            closing.elapsed() >= GRACE && closing.elapsed() < GRACE * 2 - GRACE / 5,
            "one grace for both, not one after the other: {:?}",
            closing.elapsed()
        );
        assert!(outsider.runs(), "a process outside the window");
    }

    #[test]
    fn every_window_of_a_table_a_shutdown_closes_gets_its_grace_at_once() {
        let _serial = serial_pty_test();
        let mut outsider = Sentinel::start();
        let table = PaneTable::closing_within(GRACE);
        let mut readers = Vec::new();
        for _ in 0..2 {
            let OpenedPane { mut reader, .. } = open(&table, &deaf());
            until_shown(&mut reader, "ready");
            readers.push(reader);
        }

        let closing = Instant::now();
        reap_all(&table);

        assert!(
            closing.elapsed() >= GRACE && closing.elapsed() < GRACE * 2 - GRACE / 5,
            "one grace for both, not one after the other: {:?}",
            closing.elapsed()
        );
        assert!(table.list().expect("list the panes").is_empty());
        assert!(outsider.runs(), "a process outside the window");
    }

    /// A job the agent left in a session of its own is in no process group of the
    /// pane's: only the pane's tree finds it, on macOS.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_job_in_a_session_of_its_own_is_asked_to_end_with_the_rest_of_the_tree() {
        let _serial = serial_pty_test();
        let mut outsider = Sentinel::start();
        let table = PaneTable::closing_within(GRACE * 8);
        let (started, released) = (scratch("started"), scratch("released"));
        let job = format!(
            "import os,signal,sys,time\n\
         def release(*_):\n\
         \x20   open({released:?}, 'w').write('RELEASED')\n\
         \x20   sys.exit(0)\n\
         signal.signal(signal.SIGTERM, release)\n\
         signal.signal(signal.SIGHUP, release)\n\
         open({started:?}, 'w').write(str(os.getpid()))\n\
         time.sleep(60)\n"
        );
        let parent = format!(
            "import subprocess,sys,time\n\
         subprocess.Popen([sys.executable, '-c', {job:?}], start_new_session=True, \
         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)\n\
         print('ready', flush=True)\n\
         time.sleep(60)\n"
        );
        let OpenedPane { key, mut reader } = table
            .open(
                Path::new("/tmp"),
                &["/usr/bin/python3".into(), "-c".into(), parent],
                &HashMap::new(),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            )
            .expect("open the job's parent");
        until_shown(&mut reader, "ready");
        // The group's TERM ends the program first, and the job, in a session of its own, is
        // then one the test started with no parent: the guard of the real signals is told.
        crate::process_tree::guard::register(pid_in(&started));

        let closing = Instant::now();
        table.kill(&key).expect("close the pane");

        assert_eq!(
            fs::read_to_string(&released)
                .expect("the job was asked to end and released")
                .trim(),
            "RELEASED"
        );
        assert!(closing.elapsed() < GRACE * 4, "{:?}", closing.elapsed());
        assert!(outsider.runs(), "a process outside the window");
        let _ = fs::remove_file(&started);
        let _ = fs::remove_file(&released);
    }

    /// How long a kill takes with the real grace ([`PaneTable::new`]), which is how
    /// long the pane host takes to answer `pane.kill` (`pane_handlers`): what a member's
    /// give-up waits for, no longer than the engine's `KILL_ANSWER_WAIT` (T-77). A window
    /// whose program ends when asked, one that leaves a job that ends when asked, and one
    /// that leaves a job that ignores the request, which is ended after the grace. Ignored:
    /// it measures, and the last waits out the grace. Run with
    /// `cargo test -p cf-panes --lib kill_answer -- --ignored --nocapture`.
    #[test]
    #[ignore = "measures how long a kill is answered in (T-77); the last case waits out the grace"]
    fn kill_answer_times_with_the_real_grace() {
        let _serial = serial_pty_test();
        let table = PaneTable::new();
        let cases = [
            ("ends when asked", "printf ready; exec sleep 300"),
            (
                "leaves a job that ends when asked",
                "sleep 300 & printf ready; wait",
            ),
            (
                "leaves a job that ignores the request",
                "trap '' TERM HUP; (while kill -0 $PPID 2>/dev/null; do /bin/sleep 0.2; done) & printf ready; wait",
            ),
        ];
        for (case, script) in cases {
            let mut took = Vec::new();
            for _ in 0..3 {
                let OpenedPane { key, mut reader } = open(&table, script);
                until_shown(&mut reader, "ready");
                let closing = Instant::now();
                table.kill(&key).expect("close the pane");
                took.push(closing.elapsed());
            }
            took.sort();
            println!("kill answered, a window that {case}: {took:?}");
            assert!(
                took.iter()
                    .all(|took| *took < crate::pty::CLOSE_GRACE + Duration::from_secs(5)),
                "{case}: {took:?}"
            );
        }
    }
}

/// The same on Windows, where a window is ended at once and what ends it is the
/// pane's job object, whatever its program is. Asking its programs first, with
/// Ctrl+Break to the pane's console, did not stay in the pane's console for a
/// program behind a `.cmd` shim (as npm installs every harness): the Break
/// reached the console of the host itself and ended the process that closed the
/// pane (`STATUS_CONTROL_C_EXIT`, 0xc000013a), and the shim asked "Terminate
/// batch job (Y/N)?" there.
#[cfg(windows)]
mod windows {
    use std::collections::HashMap;
    use std::fs;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use crate::pty::conpty_test::{open, read_until, system};
    use crate::pty::{serial_pty_test, PaneTable};

    #[test]
    fn a_window_whose_program_is_behind_a_cmd_shim_is_closed_at_once_and_its_closer_goes_on() {
        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        // A shim as npm writes one: a batch file that runs the program, which is
        // PowerShell here, saying it is up and then staying.
        let name = format!("cf-shim-{}.cmd", std::process::id());
        let shim = std::env::temp_dir().join(&name);
        fs::write(
            &shim,
            format!(
                "@echo off\r\n\"{}\" -NoProfile -Command \"Write-Output 'READY#'; Start-Sleep 1000\"\r\n",
                system(r"WindowsPowerShell\v1.0\powershell.exe")
            ),
        )
        .expect("write the shim");
        // `cmd.exe /d /s /c x.cmd`, as a harness behind a shim is opened; the
        // pane's folder is the temporary one, where the shim is.
        let (key, output) = open(
            &table,
            &[
                system("cmd.exe"),
                "/d".into(),
                "/s".into(),
                "/c".into(),
                name,
            ],
            &HashMap::new(),
            1 << 20,
        );
        read_until(&table, &key, &output, "READY#");

        let closing = Instant::now();
        table.kill(&key).expect("close the pane");
        let closed_in = closing.elapsed();
        let _ = fs::remove_file(&shim);

        // This process is still here to say so, and the pane is gone.
        assert!(
            closed_in < Duration::from_secs(3),
            "nothing was waited for: the job ended the tree ({closed_in:?})"
        );
        assert!(table.list().expect("list the panes").is_empty());
    }

    /// How long a kill takes, which is how long the pane host takes to answer
    /// `pane.kill`: what a member's give-up waits for, no longer than the engine's
    /// `KILL_ANSWER_WAIT` (T-77). A window behind a `.cmd` shim, as a harness is, and
    /// PowerShell opened itself. Ignored: it measures. Run with
    /// `cargo test -p cf-panes --lib kill_answer -- --ignored --nocapture`.
    #[test]
    #[ignore = "measures how long a kill is answered in (T-77)"]
    fn kill_answer_times_with_the_real_grace() {
        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let name = format!("cf-shim-timing-{}.cmd", std::process::id());
        let shim = std::env::temp_dir().join(&name);
        let powershell = system(r"WindowsPowerShell\v1.0\powershell.exe");
        fs::write(
            &shim,
            format!(
                "@echo off\r\n\"{powershell}\" -NoProfile -Command \"Write-Output 'READY#'; Start-Sleep 1000\"\r\n"
            ),
        )
        .expect("write the shim");
        let cases = [
            (
                "behind a .cmd shim",
                vec![
                    system("cmd.exe"),
                    "/d".into(),
                    "/s".into(),
                    "/c".into(),
                    name.clone(),
                ],
            ),
            (
                "of PowerShell",
                vec![
                    powershell.clone(),
                    "-NoProfile".into(),
                    "-Command".into(),
                    "Write-Output 'READY#'; Start-Sleep 1000".into(),
                ],
            ),
        ];
        for (case, argv) in cases {
            let mut took = Vec::new();
            for _ in 0..3 {
                let (key, output) = open(&table, &argv, &HashMap::new(), 1 << 20);
                read_until(&table, &key, &output, "READY#");
                let closing = Instant::now();
                table.kill(&key).expect("close the pane");
                took.push(closing.elapsed());
            }
            took.sort();
            println!("kill answered, a window {case}: {took:?}");
            assert!(
                took.iter().all(|took| *took < Duration::from_secs(10)),
                "{case}: {took:?}"
            );
        }
        let _ = fs::remove_file(&shim);
    }
}

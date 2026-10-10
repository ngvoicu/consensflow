//! A window closes the way a terminal does (`super`): its programs are asked to
//! end and given a grace, and only what is left after it is killed. A job that
//! traps the request gets to release what it holds; one that ignores it is
//! killed once the grace is over; every window of a table that goes is asked
//! before any is waited for. A Unix program is asked with TERM and HUP; a
//! Windows program, with Ctrl+Break to its console.

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
    use crate::pty::{process_exists, serial_pty_test, OpenedPane, PaneTable};

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
        let _ = fs::remove_file(&released);
        let _ = fs::remove_file(&started);
    }

    #[test]
    fn a_program_that_ignores_the_request_to_end_is_killed_once_the_grace_is_over() {
        let _serial = serial_pty_test();
        let table = PaneTable::closing_within(GRACE);
        let OpenedPane { key, mut reader } = open(
            &table,
            "trap '' TERM HUP; printf ready; while :; do sleep 1; done",
        );
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
    }

    #[test]
    fn closing_a_pane_that_already_ended_asks_nothing_and_waits_for_nothing() {
        let _serial = serial_pty_test();
        let table = PaneTable::closing_within(GRACE * 8);
        let OpenedPane { key, mut reader } = open(&table, "printf done");
        until_shown(&mut reader, "done");
        // Its program is gone, which its exit status shows once it is read.
        std::thread::sleep(Duration::from_millis(200));

        let closing = Instant::now();
        table.kill(&key).expect("close the pane");

        assert!(closing.elapsed() < GRACE * 4, "{:?}", closing.elapsed());
    }

    /// A program that ignores the request to end and runs on.
    const DEAF: &str = "trap '' TERM HUP; printf ready; while :; do sleep 1; done";

    #[test]
    fn a_table_that_goes_asks_every_window_to_end_before_it_waits_on_any() {
        let _serial = serial_pty_test();
        let table = PaneTable::closing_within(GRACE);
        let mut readers = Vec::new();
        for _ in 0..2 {
            let OpenedPane { mut reader, .. } = open(&table, DEAF);
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
    }

    #[test]
    fn every_window_of_a_table_a_shutdown_closes_gets_its_grace_at_once() {
        let _serial = serial_pty_test();
        let table = PaneTable::closing_within(GRACE);
        let mut readers = Vec::new();
        for _ in 0..2 {
            let OpenedPane { mut reader, .. } = open(&table, DEAF);
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
    }

    /// A job the agent left in a session of its own is in no process group of the
    /// pane's: only the pane's tree finds it, on macOS.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_job_in_a_session_of_its_own_is_asked_to_end_with_the_rest_of_the_tree() {
        let _serial = serial_pty_test();
        let table = PaneTable::closing_within(GRACE * 8);
        let (started, released) = (scratch("started"), scratch("released"));
        let job = format!(
            "import signal,sys,time\n\
         def release(*_):\n\
         \x20   open({released:?}, 'w').write('RELEASED')\n\
         \x20   sys.exit(0)\n\
         signal.signal(signal.SIGTERM, release)\n\
         signal.signal(signal.SIGHUP, release)\n\
         open({started:?}, 'w').write('up')\n\
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
        assert!(
            until_there(&started, Duration::from_secs(10)),
            "the job runs"
        );

        let closing = Instant::now();
        table.kill(&key).expect("close the pane");

        assert_eq!(
            fs::read_to_string(&released)
                .expect("the job was asked to end and released")
                .trim(),
            "RELEASED"
        );
        assert!(closing.elapsed() < GRACE * 4, "{:?}", closing.elapsed());
        let _ = fs::remove_file(&started);
        let _ = fs::remove_file(&released);
    }
}

/// The same on Windows, where the programs are PowerShell's and the request is
/// Ctrl+Break to the pane's console: a program that handles it writes what it
/// was asked to, and `Console.CancelKeyPress` is the handler a .NET program
/// (PowerShell, through `Add-Type`) has for it.
#[cfg(windows)]
mod windows {
    use std::collections::HashMap;
    use std::fs;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use crate::pty::conpty_test::{open, powershell, read_until};
    use crate::pty::{serial_pty_test, PaneTable};

    /// A .NET class that, once installed, writes `released` to its file and
    /// ends the process when Ctrl+Break (or Ctrl+C) reaches it.
    const HOOK: &str = "Add-Type -TypeDefinition 'using System; public static class Hook { \
        public static string File; \
        static void OnBreak(object sender, ConsoleCancelEventArgs e) { \
        e.Cancel = true; System.IO.File.WriteAllText(File, \"released\"); Environment.Exit(0); } \
        public static void Install() { Console.CancelKeyPress += OnBreak; } }'; ";

    #[test]
    fn a_program_that_handles_ctrl_break_runs_its_handler_when_its_window_closes() {
        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::closing_within(Duration::from_secs(8)));
        let released = std::env::temp_dir().join(format!("cf-break-{}", std::process::id()));
        let _ = fs::remove_file(&released);
        let (key, output) = open(
            &table,
            &powershell(&format!(
                "{HOOK}[Hook]::File = '{}'; [Hook]::Install(); Write-Output 'READY#'; Start-Sleep 1000",
                released.display()
            )),
            &HashMap::new(),
            1 << 20,
        );
        read_until(&table, &key, &output, "READY#");

        let closing = Instant::now();
        table.kill(&key).expect("close the pane");

        assert!(
            closing.elapsed() < Duration::from_secs(6),
            "its handler ended it: nothing was left to wait out the grace for ({:?})",
            closing.elapsed()
        );
        assert_eq!(
            fs::read_to_string(&released).expect("the handler ran"),
            "released"
        );
        let _ = fs::remove_file(&released);
    }
}

//! Where a pane's turn began and what its program started after it
//! (`super`). The tree is looked at on macOS (the process tree of
//! `process_tree`) and on Windows (the job); elsewhere a pane has none, and the
//! tests that need one are macOS's and Windows'.

#[cfg(unix)]
mod unix {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::mpsc::Receiver;
    use std::sync::Arc;
    use std::time::Duration;

    use portable_pty::PtySize;

    use crate::pty::{
        serial_pty_test, PaneEnvironment, PaneError, PaneKey, PaneOutput, PaneTable, StreamedPane,
    };

    /// A pane running `script` under `/bin/sh`, streamed, so that what it leaves
    /// of its exit is read as the daemon reads it.
    fn open(table: &Arc<PaneTable>, script: &str) -> StreamedPane {
        let (overlay, nothing) = (HashMap::new(), Vec::new());
        table
            .open_streamed(
                Path::new("/tmp"),
                &["/bin/sh".into(), "-c".into(), script.into()],
                PaneEnvironment::new(&overlay, &nothing),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
                1024 * 1024,
            )
            .expect("open a shell in a pane")
    }

    /// Reads the pane's output until `marker` has shown.
    fn until_shown(output: &Receiver<PaneOutput>, marker: &str) {
        let mut seen = Vec::new();
        while !String::from_utf8_lossy(&seen).contains(marker) {
            let chunk = output
                .recv_timeout(Duration::from_secs(10))
                .unwrap_or_else(|_| panic!("the pane showed no {marker}: {seen:?}"));
            seen.extend_from_slice(&chunk.bytes);
        }
    }

    /// A program that has a server running before its turn, is told to go, and
    /// then starts a job and says so.
    const SCRIPT: &str = "sleep 33 & printf ready; read go; sleep 31 & printf started; wait";

    /// What the pane's tree has running that its turn started, once `command`
    /// is among it: the program says it started a job as it forks it, and until
    /// the job is executed it shows the command line of the program that forked it.
    #[cfg(target_os = "macos")]
    fn until_listed(
        table: &PaneTable,
        key: &PaneKey,
        command: &str,
    ) -> cf_proto::panes::Background {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let left = table.background(key).expect("what it left running");
            if left
                .processes
                .iter()
                .any(|process| process.command == command)
            {
                return left;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "{command} was never listed: {left:?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn what_the_tree_had_before_the_turn_is_not_the_turns_and_what_it_started_after_is() {
        let _serial = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let pane = open(&table, SCRIPT);
        until_shown(&pane.output, "ready");

        table.mark(&pane.key).expect("mark the turn");
        table.write(&pane.key, b"go\n").expect("tell it to go");
        until_shown(&pane.output, "started");

        let left = until_listed(&table, &pane.key, "sleep 31");
        assert_eq!(left.total, 1, "{left:?}");
        table.kill(&pane.key).expect("close the pane");
    }

    #[test]
    fn a_pane_never_marked_has_no_background_work() {
        let _serial = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let pane = open(&table, SCRIPT);
        until_shown(&pane.output, "ready");
        table.write(&pane.key, b"go\n").expect("tell it to go");
        until_shown(&pane.output, "started");

        let left = table.background(&pane.key).expect("what it left running");
        assert_eq!((left.total, left.processes), (0, Vec::new()));
        table.kill(&pane.key).expect("close the pane");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_new_mark_makes_what_runs_by_then_what_came_before() {
        let _serial = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let pane = open(&table, SCRIPT);
        until_shown(&pane.output, "ready");
        table.mark(&pane.key).expect("mark the first turn");
        table.write(&pane.key, b"go\n").expect("tell it to go");
        until_shown(&pane.output, "started");
        assert_eq!(table.background(&pane.key).expect("look").total, 1);

        table.mark(&pane.key).expect("mark the next turn");

        assert_eq!(table.background(&pane.key).expect("look").total, 0);
        table.kill(&pane.key).expect("close the pane");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_close_keeps_what_the_window_left_running_for_the_exit_it_sends() {
        let _serial = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let pane = open(&table, SCRIPT);
        until_shown(&pane.output, "ready");
        table.mark(&pane.key).expect("mark the turn");
        table.write(&pane.key, b"go\n").expect("tell it to go");
        until_shown(&pane.output, "started");
        until_listed(&table, &pane.key, "sleep 31");

        table.kill(&pane.key).expect("close the pane");

        // The close has ended what it listed: the exit says what it was.
        let kept = pane.ended.background().expect("the close kept its list");
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(kept[0].command, "sleep 31");
        assert_eq!(table.left_running_at_exit(&pane.key, &pane.ended), kept);
    }

    /// A program that ends on its own with a job still running: reparented, the
    /// job is found by the marker its environment carries (a system binary such as
    /// `sleep` shows no environment, so the job is a Python's), and the exit lists
    /// it before the retirement that follows ends it. The job ignores the hangup
    /// the terminal sends the program's group as its leader goes.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_program_that_exits_leaving_a_job_running_lists_it_in_its_exit() {
        let _serial = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let pane = open(
            &table,
            "trap '' HUP; printf ready; read go; \
         /usr/bin/python3 -c 'import time; time.sleep(31)' & printf started",
        );
        until_shown(&pane.output, "ready");
        table.mark(&pane.key).expect("mark the turn");
        table.write(&pane.key, b"go\n").expect("tell it to go");
        until_shown(&pane.output, "started");
        table
            .wait_ended(&pane.key, &pane.ended, Duration::from_secs(5))
            .expect("the program ended");

        let left = table.left_running_at_exit(&pane.key, &pane.ended);

        assert_eq!(left.len(), 1, "{left:?}");
        assert!(left[0].command.contains("time.sleep(31)"), "{left:?}");
        assert!(
            table.retire_exited(&pane.key).expect("retire the pane"),
            "the retirement ends what the program left"
        );
    }

    #[test]
    fn a_pane_that_is_not_open_has_nothing_to_mark_or_to_look_at() {
        let table = PaneTable::new();
        let missing = PaneKey::new("nobody", 1);
        assert!(matches!(
            table.mark(&missing),
            Err(PaneError::NotFound(key)) if key == missing
        ));
        assert!(matches!(
            table.background(&missing),
            Err(PaneError::NotFound(key)) if key == missing
        ));
    }
}

/// The same on Windows, where the tree is the pane's job: what PowerShell
/// starts after the mark is the turn's, what ran before it is not.
#[cfg(windows)]
mod windows {
    use std::collections::HashMap;
    use std::sync::Arc;

    use crate::pty::conpty_test::{open, powershell, read_until, system};
    use crate::pty::{serial_pty_test, PaneTable};

    #[test]
    fn what_the_job_had_before_the_turn_is_not_the_turns_and_what_it_started_after_is() {
        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let ping = system("PING.EXE");
        let (key, output) = open(
            &table,
            &powershell(&format!(
                "Write-Output 'READY#'; [Console]::In.ReadLine() | Out-Null; \
                 Start-Process -WindowStyle Hidden -FilePath '{ping}' -ArgumentList '-n','1000','127.0.0.1'; \
                 Write-Output 'STARTED#'; Start-Sleep 1000"
            )),
            &HashMap::new(),
            1 << 20,
        );
        read_until(&table, &key, &output, "READY#");
        assert_eq!(table.background(&key).expect("never marked").total, 0);

        table.mark(&key).expect("mark the turn");
        table.write(&key, b"\r").expect("go on");
        read_until(&table, &key, &output, "STARTED#");

        let left = table.background(&key).expect("what it left running");
        assert!(
            left.processes
                .iter()
                .any(|process| process.command.to_lowercase().contains("ping")),
            "{left:?}"
        );
        assert!(
            left.processes
                .iter()
                .all(|process| !process.command.to_lowercase().contains("powershell")),
            "the program itself came before the turn: {left:?}"
        );
        table.kill(&key).expect("close the pane");
    }
}

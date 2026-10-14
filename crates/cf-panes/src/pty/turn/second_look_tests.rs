//! The second look ([`crate::background`]): what a close and an ending name of what a
//! turn left running is what the host finds twice, [`SETTLE`](crate::background::SETTLE)
//! apart. The harness runs short commands of its own beside the agent's (Claude Code's
//! status line is a shell with a job, ended in a moment, in a group of its own, as its
//! tools' commands are), and the host cannot tell one from the agent's by anything but
//! that it ends: a look that finds a run names it as work the turn left, and an ending
//! of that look would end it, and tell the chief it did.

#[cfg(target_os = "macos")]
mod macos {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use cf_proto::panes::{Ending, Servers};

    use crate::background::SETTLE;
    use crate::headless::reap_all;
    use crate::pty::turn::support::{ended_pids, gone, listed, numbers, started};
    use crate::pty::{process_exists, serial_pty_test, PaneTable, Sentinel};

    /// What a test's table gives the programs it asks to end: long enough to tell from
    /// none, and short enough to wait for.
    const GRACE: Duration = Duration::from_millis(1500);

    /// The window of a harness once its turn is told to go: before the turn, a program
    /// of its own; after it, the harness's status line, a shell in a group of its own that
    /// runs a job and is done in a second and a bit (more than the first look takes to
    /// come, less than the moment the host waits for the second), and a command of the
    /// agent's that lasts, a shell in a group of its own with a job. The program says
    /// what it has started, and their numbers, once each has.
    const STATUS_LINE: &str = r#"
import json, os, subprocess, time

def command(script):
    # A command: a shell in a group of its own, which says its job's number.
    shell = subprocess.Popen(["/bin/sh", "-c", script], stdout=subprocess.PIPE, text=True,
                             preexec_fn=os.setpgrp)
    return shell, int(shell.stdout.readline())

before = subprocess.Popen(["sleep", "33"])
print("ready", flush=True)
input()
line, line_job = command("sleep 1.2 & echo $!; wait")
tool, tool_job = command("sleep 32 & echo $!; wait")
print("started " + json.dumps({
    "program": os.getpid(), "before": before.pid, "line": line.pid, "line_job": line_job,
    "tool": tool.pid, "tool_job": tool_job,
}), flush=True)
time.sleep(60)
"#;

    /// A window that starts nothing after its turn begins.
    const NOTHING: &str = r#"
import json, os, time

print("ready", flush=True)
input()
print("started " + json.dumps({"program": os.getpid()}), flush=True)
time.sleep(60)
"#;

    #[test]
    fn a_run_of_the_harnesss_status_line_is_named_by_one_look_and_none_of_an_ending() {
        let _serial = serial_pty_test();
        let mut outsider = Sentinel::start();
        let table = Arc::new(PaneTable::closing_within(GRACE));
        let (pane, pid) = started(&table, STATUS_LINE, Servers::InGroup, None, &HashMap::new());
        // The run is in a group of its own, and its shell began after the mark: a look
        // while it lives takes it, and its job, for the turn's work, beside the command.
        assert_eq!(
            listed(&table, &pane.key),
            numbers(&pid, &["line", "line_job", "tool", "tool_job"])
        );

        let ending = table.end_background(&pane.key).expect("end it");

        // By the second look the run has ended: the ending ends and names the command.
        let lasting = numbers(&pid, &["tool", "tool_job"]);
        assert_eq!(ended_pids(&ending), lasting, "{ending:?}");
        assert_eq!(ending.left.total, 0, "{ending:?}");
        for job in ["tool", "tool_job"] {
            assert!(gone(pid[job]), "{job} was ended");
        }
        assert!(
            process_exists(pid["program"]) && process_exists(pid["before"]),
            "the window is as it was"
        );
        assert!(outsider.runs(), "a process outside the window");
        table.kill(&pane.key).expect("close the pane");
        assert!(outsider.runs(), "nor a close's");
    }

    #[test]
    fn a_close_keeps_what_the_window_left_running_for_the_exit_and_not_a_run_of_the_harnesss_status_line(
    ) {
        let _serial = serial_pty_test();
        let mut outsider = Sentinel::start();
        let table = Arc::new(PaneTable::closing_within(GRACE));
        let (pane, pid) = started(&table, STATUS_LINE, Servers::InGroup, None, &HashMap::new());
        assert_eq!(
            listed(&table, &pane.key),
            numbers(&pid, &["line", "line_job", "tool", "tool_job"]),
            "a look while the run lives names it"
        );

        let closing = Instant::now();
        table.kill(&pane.key).expect("close the pane");

        // The close looked again before it said what the window left: by then the run
        // had ended.
        assert!(
            closing.elapsed() >= SETTLE,
            "the close looked again a moment later: {:?}",
            closing.elapsed()
        );
        let mut kept: Vec<u32> = pane
            .ended
            .background()
            .expect("the close kept its list")
            .iter()
            .map(|process| process.pid)
            .collect();
        kept.sort_unstable();
        assert_eq!(kept, numbers(&pid, &["tool", "tool_job"]));
        assert!(outsider.runs(), "a process outside the window");
    }

    #[test]
    fn a_close_after_the_exit_has_said_what_was_left_does_not_look_again() {
        let _serial = serial_pty_test();
        let mut outsider = Sentinel::start();
        let table = Arc::new(PaneTable::closing_within(GRACE));
        let (pane, _) = started(&table, STATUS_LINE, Servers::InGroup, None, &HashMap::new());
        // The exit of the window's program said what it left, and the close that follows
        // keeps that: the first look stays, and a look that would find work is not made.
        pane.ended.keep_background(Vec::new());

        let closing = Instant::now();
        table.kill(&pane.key).expect("close the pane");

        assert!(
            closing.elapsed() < SETTLE,
            "the close did not wait for a second look: {:?}",
            closing.elapsed()
        );
        assert_eq!(pane.ended.background(), Some(Vec::new()));
        assert!(outsider.runs(), "a process outside the window");
    }

    #[test]
    fn a_quit_ends_a_window_without_looking_at_what_it_left_running() {
        let _serial = serial_pty_test();
        let mut outsider = Sentinel::start();
        let table = Arc::new(PaneTable::closing_within(GRACE));
        let (pane, _) = started(&table, STATUS_LINE, Servers::InGroup, None, &HashMap::new());

        let quitting = Instant::now();
        reap_all(&table);

        assert!(
            quitting.elapsed() < SETTLE,
            "no one is left to be told, and a quit waits for no second look: {:?}",
            quitting.elapsed()
        );
        assert_eq!(
            pane.ended.background(),
            None,
            "nothing kept for an exit no one reads"
        );
        assert!(table.list().expect("list the panes").is_empty());
        assert!(outsider.runs(), "a process outside the window");
    }

    #[test]
    fn a_window_that_left_nothing_is_looked_at_once_by_an_ending_and_by_a_close() {
        let _serial = serial_pty_test();
        let mut outsider = Sentinel::start();
        let table = Arc::new(PaneTable::closing_within(GRACE));
        let (pane, _) = started(&table, NOTHING, Servers::InGroup, None, &HashMap::new());
        assert_eq!(listed(&table, &pane.key), Vec::<u32>::new());

        let ending = Instant::now();
        assert_eq!(
            table.end_background(&pane.key).expect("end it"),
            Ending::default()
        );
        assert!(
            ending.elapsed() < SETTLE,
            "a look that finds nothing is not made again: {:?}",
            ending.elapsed()
        );

        let closing = Instant::now();
        table.kill(&pane.key).expect("close the pane");

        assert!(
            closing.elapsed() < SETTLE,
            "nor is a close's: {:?}",
            closing.elapsed()
        );
        assert_eq!(pane.ended.background(), Some(Vec::new()));
        assert!(outsider.runs(), "a process outside the window");
    }
}

/// The same on Windows, where the tree is the pane's job: a command of the agent's that
/// lasts, and a shell that runs a ping a second long and stays, as a harness's helper that
/// starts a short run of its own beside the agent's does.
#[cfg(windows)]
mod windows {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use cf_proto::panes::{Background, Servers};

    use crate::pty::conpty_test::{open, powershell, read_until, system};
    use crate::pty::{serial_pty_test, PaneKey, PaneTable};

    /// Whether the list holds a process whose command line holds all of `words`, in lower
    /// case whatever case it is written in.
    fn says(list: &Background, words: &[&str]) -> bool {
        list.processes.iter().any(|process| {
            let command = process.command.to_lowercase();
            words.iter().all(|word| command.contains(word))
        })
    }

    /// Waits until a look at the pane's job lists a process whose command line holds all of
    /// `words`.
    fn until_listed(table: &PaneTable, key: &PaneKey, words: &[&str]) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let found = table.background(key).expect("what it left running");
            if says(&found, words) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{words:?} was never listed: {found:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn a_ping_that_ends_between_two_looks_is_none_of_what_an_ending_ends_or_names() {
        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let shell = system(r"WindowsPowerShell\v1.0\powershell.exe");
        let (key, output) = open(
            &table,
            &powershell(&format!(
                // Once told to go, a command of the agent's that lasts (a shell that runs
                // a ping and waits for it); once told to go again, a shell that runs a
                // ping a second long and stays.
                "Write-Output 'READY#'; [Console]::In.ReadLine() | Out-Null; \
                 Start-Process -WindowStyle Hidden -FilePath '{shell}' -ArgumentList '-NoProfile','-Command','ping -n 1006 127.0.0.1'; \
                 Write-Output 'STARTED#'; [Console]::In.ReadLine() | Out-Null; \
                 Start-Process -WindowStyle Hidden -FilePath '{shell}' -ArgumentList '-NoProfile','-Command','ping -n 2 127.0.0.1; Start-Sleep 1000'; \
                 Write-Output 'RUN#'; Start-Sleep 1000"
            )),
            &HashMap::new(),
            1 << 20,
        );
        read_until(&table, &key, &output, "READY#");
        table
            .mark(&key, Servers::Unknown, None)
            .expect("mark the turn");
        table.write(&key, b"\r").expect("go on");
        read_until(&table, &key, &output, "STARTED#");
        until_listed(&table, &key, &["ping.exe", "-n 1006"]);

        // The short one lives a second from the moment its shell starts it, however long
        // the shell took to start: a look that finds it is made while it lives, and the
        // look after the moment the host waits comes when it has ended.
        table.write(&key, b"\r").expect("go on");
        read_until(&table, &key, &output, "RUN#");
        until_listed(&table, &key, &["ping.exe", "-n 2 "]);

        let ending = table.end_background(&key).expect("end it");

        assert!(
            says(&ending.ended, &["ping.exe", "-n 1006"]),
            "what lasts is ended: {ending:?}"
        );
        assert!(
            !says(&ending.ended, &["ping.exe", "-n 2 "]) && !says(&ending.left, &["-n 2 "]),
            "the ping that had ended by the second look is none of it: {ending:?}"
        );
        assert_eq!(ending.left.total, 0, "{ending:?}");
        table.kill(&key).expect("close the pane");
    }
}

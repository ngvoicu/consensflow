//! Where a pane's turn began and what its program started after it that is the
//! turn's work (`super`). The tree is looked at on macOS (the process tree of
//! `process_tree`) and on Windows (the job); elsewhere a pane has none, and the
//! tests that need one are macOS's and Windows'.

#[cfg(unix)]
mod unix {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::mpsc::Receiver;
    use std::sync::Arc;
    use std::time::Duration;

    use cf_proto::panes::Servers;
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
    /// then runs a command of its agent's (a shell that waits for its job), and says
    /// so.
    const SCRIPT: &str =
        "sleep 33 & printf ready; read go; /bin/sh -c 'sleep 31 & wait' & printf started; wait";

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
    mod harnesses {
        use std::collections::HashMap;

        use cf_proto::panes::Servers;

        use crate::pty::process_exists;
        use crate::pty::turn::support::{kill, listed, numbers, started, Sentinel, DETACHED};

        use super::{serial_pty_test, Arc, PaneTable};

        /// The window of a harness that keeps its servers in the program's group, the
        /// way Claude Code does, once its turn is told to go: a server it had before
        /// the turn; then, after the mark, another that stays in the group and
        /// launches a browser, which leaves it (an MCP server and the browser a tool
        /// of its opens), and one a user wrapped in a shell, which stays in the group
        /// as well; and a command of the agent's, a shell in a group of its own with a
        /// job. A program that has started says so, and the numbers.
        const CLAUDE: &str = r#"
import json, os, subprocess, sys, time

BROWSER = "import os, time; os.setsid(); print(os.getpid(), flush=True); time.sleep(35)"
SERVER = (
    "import subprocess, sys, time; "
    "browser = subprocess.Popen([sys.executable, '-c', " + repr(BROWSER) + "], stdout=subprocess.PIPE, text=True); "
    "print(browser.stdout.readline().strip(), flush=True); time.sleep(36)"
)

before = subprocess.Popen(["sleep", "33"])
print("ready", flush=True)
input()
server = subprocess.Popen([sys.executable, "-c", SERVER], stdout=subprocess.PIPE, text=True)
browser = int(server.stdout.readline())
wrapper = subprocess.Popen(["/bin/sh", "-c", "sleep 34 & echo $!; wait"], stdout=subprocess.PIPE, text=True)
wrapped = int(wrapper.stdout.readline())
tool = subprocess.Popen(["/bin/sh", "-c", "sleep 32 & echo $!; wait"],
                        stdout=subprocess.PIPE, text=True, preexec_fn=os.setpgrp)
tool_job = int(tool.stdout.readline())
print("started " + json.dumps({
    "program": os.getpid(), "before": before.pid, "server": server.pid, "browser": browser,
    "wrapper": wrapper.pid, "wrapped": wrapped, "tool": tool.pid, "tool_job": tool_job,
}), flush=True)
time.sleep(60)
"#;

        /// The window of a harness that opens its servers with the window, each in a
        /// group of its own, the way Codex does (its supervisor is the program; its
        /// own process, which starts the agent's commands, and its servers are its
        /// children), once its turn is told to go: a server it started with the
        /// window; and, after the mark, a server it starts late (a plugin that needs
        /// none until a tool is called), and a command of the agent's, a shell in a
        /// group of its own with a job.
        const CODEX: &str = r#"
import json, os, subprocess, sys, time

HARNESS = """
import os, subprocess, sys, time
server = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(36)'], preexec_fn=os.setpgrp)
print(server.pid, flush=True)
input()
late = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(37)'], preexec_fn=os.setpgrp)
tool = subprocess.Popen(['/bin/sh', '-c', 'sleep 32 & echo $!; wait'],
                        stdout=subprocess.PIPE, text=True, preexec_fn=os.setpgrp)
job = tool.stdout.readline().strip()
print(late.pid, tool.pid, job, flush=True)
time.sleep(60)
"""
harness = subprocess.Popen([sys.executable, "-c", HARNESS],
                           stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
server = int(harness.stdout.readline())
print("ready", flush=True)
input()
harness.stdin.write("go\n")
harness.stdin.flush()
late, tool, job = (int(word) for word in harness.stdout.readline().split())
print("started " + json.dumps({
    "program": os.getpid(), "harness": harness.pid, "server": server,
    "late": late, "tool": tool, "tool_job": job,
}), flush=True)
time.sleep(60)
"#;

        #[test]
        fn a_job_detached_from_the_shell_that_started_it_is_no_work_of_the_turn_and_no_look_saw_it_under_the_program_so_the_close_leaves_it(
        ) {
            let _serial = serial_pty_test();
            let mut outsider = Sentinel::start();
            for servers in [Servers::InGroup, Servers::Unknown] {
                let table = Arc::new(PaneTable::new());
                let (pane, pid) = started(&table, DETACHED, servers, None, &HashMap::new());

                // The job's shell is gone, and the system names no parent of its that
                // the tree has: it is in the window's session, which is not listed as
                // the turn's, and no look saw it under the window's program: the close
                // ends what is under the program, and leaves it running.
                assert_eq!(listed(&table, &pane.key), Vec::<u32>::new(), "{servers:?}");
                table.kill(&pane.key).expect("close the pane");
                assert!(
                    process_exists(pid["job"]),
                    "{servers:?}: the close does not end what nothing saw under the program"
                );
                assert!(outsider.runs(), "{servers:?}: a process outside the window");
                kill(pid["job"]);
            }
        }

        #[test]
        fn a_server_the_harness_starts_late_is_no_work_of_the_turn_and_is_not_listed() {
            let _serial = serial_pty_test();
            let table = Arc::new(PaneTable::new());
            let (pane, pid) = started(&table, CLAUDE, Servers::Unknown, None, &HashMap::new());

            // The server and its browser began after the mark and no shell started
            // them: nothing says whose they are, and they are not named. The shell a
            // user wrapped a server in, and what it started, are taken for a command's
            // where the harness says nothing of how it keeps its servers.
            assert_eq!(
                listed(&table, &pane.key),
                numbers(&pid, &["wrapper", "wrapped", "tool", "tool_job"])
            );
            table.kill(&pane.key).expect("close the pane");
        }

        #[test]
        fn where_the_servers_are_in_the_group_a_server_wrapped_in_a_shell_is_still_the_harnesss() {
            let _serial = serial_pty_test();
            let mut outsider = Sentinel::start();
            let table = Arc::new(PaneTable::new());
            let (pane, pid) = started(&table, CLAUDE, Servers::InGroup, None, &HashMap::new());

            // The wrapper and what it started are in the program's group, as the
            // servers are: the harness's. A command is in a group of its own.
            assert_eq!(
                listed(&table, &pane.key),
                numbers(&pid, &["tool", "tool_job"])
            );

            // What the close ends and lists in its exit is the same.
            table.kill(&pane.key).expect("close the pane");
            let kept = pane.ended.background().expect("the close kept its list");
            assert_eq!(kept.len(), 2, "{kept:?}");
            assert!(outsider.runs(), "a process outside the window");
        }

        #[test]
        fn where_the_servers_open_with_the_window_what_the_harness_starts_late_is_not_the_turns_either(
        ) {
            let _serial = serial_pty_test();
            let table = Arc::new(PaneTable::new());
            let (pane, pid) = started(&table, CODEX, Servers::AtLaunch, None, &HashMap::new());

            // The server the window opened with is the harness's; one it starts when a
            // tool first needs it is no server of the launch, and no shell started it:
            // it is left alone and not named. The command is the agent's.
            assert_eq!(
                listed(&table, &pane.key),
                numbers(&pid, &["tool", "tool_job"])
            );
            table.kill(&pane.key).expect("close the pane");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn what_the_tree_had_before_the_turn_is_not_the_turns_and_what_a_shell_started_after_is() {
        let _serial = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let pane = open(&table, SCRIPT);
        until_shown(&pane.output, "ready");

        table
            .mark(&pane.key, Servers::Unknown, None)
            .expect("mark the turn");
        table.write(&pane.key, b"go\n").expect("tell it to go");
        until_shown(&pane.output, "started");

        let left = until_listed(&table, &pane.key, "sleep 31");
        assert_eq!(left.total, 2, "the shell and its job: {left:?}");
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
        table
            .mark(&pane.key, Servers::Unknown, None)
            .expect("mark the first turn");
        table.write(&pane.key, b"go\n").expect("tell it to go");
        until_shown(&pane.output, "started");
        until_listed(&table, &pane.key, "sleep 31");
        assert_eq!(table.background(&pane.key).expect("look").total, 2);

        table
            .mark(&pane.key, Servers::Unknown, None)
            .expect("mark the next turn");

        assert_eq!(table.background(&pane.key).expect("look").total, 0);
        table.kill(&pane.key).expect("close the pane");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_close_keeps_what_the_window_left_running_for_the_exit_it_sends() {
        let _serial = serial_pty_test();
        let mut outsider = crate::pty::turn::support::Sentinel::start();
        let table = Arc::new(PaneTable::new());
        let pane = open(&table, SCRIPT);
        until_shown(&pane.output, "ready");
        table
            .mark(&pane.key, Servers::Unknown, None)
            .expect("mark the turn");
        table.write(&pane.key, b"go\n").expect("tell it to go");
        until_shown(&pane.output, "started");
        until_listed(&table, &pane.key, "sleep 31");

        table.kill(&pane.key).expect("close the pane");

        // The close has ended what it listed: the exit says what it was.
        let kept = pane.ended.background().expect("the close kept its list");
        assert_eq!(kept.len(), 2, "{kept:?}");
        assert!(kept.iter().any(|process| process.command == "sleep 31"));
        assert_eq!(table.left_running_at_exit(&pane.key, &pane.ended), kept);
        assert!(outsider.runs(), "a process outside the window");
    }

    /// A program that ends on its own with a job still running: reparented, the job is
    /// the window's (it carries the marker its environment has, which a system binary
    /// such as `sleep` does not show, so the job is a Python's), but no work of the turn:
    /// no chain of processes that run leads from it to a shell the harness started,
    /// the program being gone. The exit lists none, and the retirement that follows
    /// ends it. The job ignores the hangup the terminal sends the program's group as
    /// its leader goes.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_program_that_exits_leaving_a_job_running_lists_none_of_it_in_its_exit_and_the_retirement_ends_it(
    ) {
        let _serial = serial_pty_test();
        let mut outsider = crate::pty::turn::support::Sentinel::start();
        let table = Arc::new(PaneTable::new());
        let pane = open(
            &table,
            // It says it started once the job is Python, and no longer the shell
            // it was forked as, which a look could catch first.
            "trap '' HUP; printf ready; read go; \
         /usr/bin/python3 -c 'import time; time.sleep(31)' & \
         until ps -o comm= -p $! | grep -q python; do sleep 0.05; done; printf started",
        );
        until_shown(&pane.output, "ready");
        table
            .mark(&pane.key, Servers::Unknown, None)
            .expect("mark the turn");
        table.write(&pane.key, b"go\n").expect("tell it to go");
        until_shown(&pane.output, "started");
        table
            .wait_ended(&pane.key, &pane.ended, Duration::from_secs(5))
            .expect("the program ended");

        let left = table.left_running_at_exit(&pane.key, &pane.ended);

        assert_eq!(left, Vec::new(), "no chain proves it the turn's");
        assert!(
            table.retire_exited(&pane.key).expect("retire the pane"),
            "the retirement ends what the program left"
        );
        assert!(outsider.runs(), "a process outside the window");
    }

    #[test]
    fn a_pane_that_is_not_open_has_nothing_to_mark_or_to_look_at() {
        let table = PaneTable::new();
        let missing = PaneKey::new("nobody", 1);
        assert!(matches!(
            table.mark(&missing, Servers::Unknown, None),
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

    use cf_proto::panes::Servers;

    use crate::pty::conpty_test::{open, powershell, read_until, system};
    use crate::pty::{serial_pty_test, PaneTable};

    #[test]
    fn what_the_job_had_before_the_turn_is_not_the_turns_and_what_a_shell_started_after_is() {
        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let ping = system("PING.EXE");
        let shell = system(r"WindowsPowerShell\v1.0\powershell.exe");
        let (key, output) = open(
            &table,
            &powershell(&format!(
                "Write-Output 'READY#'; [Console]::In.ReadLine() | Out-Null; \
                 Start-Process -WindowStyle Hidden -FilePath '{shell}' -ArgumentList '-NoProfile','-Command','ping -n 1000 127.0.0.1'; \
                 Start-Process -WindowStyle Hidden -FilePath '{ping}' -ArgumentList '-n','1001','127.0.0.1'; \
                 Write-Output 'STARTED#'; Start-Sleep 1000"
            )),
            &HashMap::new(),
            1 << 20,
        );
        read_until(&table, &key, &output, "READY#");
        assert_eq!(table.background(&key).expect("never marked").total, 0);

        table
            .mark(&key, Servers::Unknown, None)
            .expect("mark the turn");
        table.write(&key, b"\r").expect("go on");
        read_until(&table, &key, &output, "STARTED#");

        // The shell the program started after the mark is the turn's, and so is what
        // it ran. A ping the program itself started was begun by a process that
        // came before the turn, which is no shell's work, and is nobody's to tell.
        let found = |left: &cf_proto::panes::Background, word: &str| {
            left.processes
                .iter()
                .any(|process| process.command.to_lowercase().contains(word))
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let left = loop {
            let left = table.background(&key).expect("what it left running");
            if found(&left, "-n 1000") {
                break left;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the shell's ping was never listed: {left:?}"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        assert!(!found(&left, "-n 1001"), "{left:?}");
        assert!(
            !found(&left, "ready#"),
            "the program itself came before the turn: {left:?}"
        );
        table.kill(&key).expect("close the pane");
    }
}

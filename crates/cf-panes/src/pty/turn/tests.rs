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

    /// A pane running the Python program `code`, streamed.
    #[cfg(target_os = "macos")]
    fn open_python(table: &Arc<PaneTable>, code: &str) -> StreamedPane {
        let (overlay, nothing) = (HashMap::new(), Vec::new());
        table
            .open_streamed(
                Path::new("/tmp"),
                &["/usr/bin/python3".into(), "-c".into(), code.into()],
                PaneEnvironment::new(&overlay, &nothing),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
                1024 * 1024,
            )
            .expect("open a Python program in a pane")
    }

    /// The window of a harness that keeps its servers in the program's group, the
    /// way Claude Code does, once its turn is told to go: a server it had before
    /// the turn; then, after the mark, another that stays in the group and
    /// launches a browser, which leaves it (an MCP server and the browser a tool
    /// of its opens); and two jobs the agent runs, each of which leaves the group,
    /// one by a session of its own with a process of its own, one by a group of
    /// its own (a command a tool runs). A process that has left says so, and the
    /// program says it has started once each has.
    #[cfg(target_os = "macos")]
    const CLAUDE: &str = r#"
import subprocess, sys, time

def start(code):
    return subprocess.Popen([sys.executable, "-c", code], stdout=subprocess.PIPE, text=True)

BROWSER = "import os, time; os.setsid(); print(1, flush=True); time.sleep(35)"
SERVER = (
    "import subprocess, sys, time; "
    "browser = subprocess.Popen([sys.executable, '-c', " + repr(BROWSER) + "], stdout=subprocess.PIPE, text=True); "
    "browser.stdout.readline(); print(1, flush=True); time.sleep(36)"
)
TOOL = "import os, subprocess, time; os.setsid(); subprocess.Popen(['sleep', '32']); print(1, flush=True); time.sleep(34)"
TOOL_GROUP = "import os, time; os.setpgid(0, 0); print(1, flush=True); time.sleep(37)"

subprocess.Popen(["sleep", "33"])
print("ready", flush=True)
input()
started = [start(code) for code in (SERVER, TOOL, TOOL_GROUP)]
for process in started:
    process.stdout.readline()
print("started", flush=True)
time.sleep(60)
"#;

    /// The window of a harness that opens its servers with the window, each in a
    /// group of its own, the way Codex does (its supervisor is the program; its own
    /// process, which starts the agent's commands, and its servers are its
    /// children), once its turn is told to go: a server starts two processes (a
    /// plugin's REPL starting the workers of a tool call), and the harness's own
    /// process starts a command that leaves the group by a session of its own.
    #[cfg(target_os = "macos")]
    const CODEX: &str = r#"
import os, signal, subprocess, sys, time

up_r, up_w = os.pipe()
told_r, told_w = os.pipe()
done_r, done_w = os.pipe()

harness = os.fork()
if harness == 0:
    server = os.fork()
    if server == 0:
        os.setsid()
        def children(*_):
            for _ in range(2):
                subprocess.Popen(["sleep", "31"])
            os.write(done_w, b"s")
        signal.signal(signal.SIGUSR1, children)
        os.write(up_w, b"u")
        while True:
            time.sleep(0.1)
    def command(*_):
        subprocess.Popen([sys.executable, "-c", "import os, time; os.setsid(); time.sleep(32)"])
        os.write(done_w, b"c")
    signal.signal(signal.SIGUSR2, command)
    os.read(up_r, 1)
    os.write(told_w, ("%d %d\n" % (os.getpid(), server)).encode())
    while True:
        time.sleep(0.1)

told = os.read(told_r, 64).split()
harness_pid, server_pid = int(told[0]), int(told[1])
print("ready", flush=True)
input()
os.kill(server_pid, signal.SIGUSR1)
os.read(done_r, 1)
os.kill(harness_pid, signal.SIGUSR2)
os.read(done_r, 1)
print("started", flush=True)
time.sleep(60)
"#;

    /// The window of `code` after its turn began as the program was told, `servers`
    /// being what the daemon said of how its harness keeps them: what its tree
    /// has that the turn started.
    #[cfg(target_os = "macos")]
    fn started_in(
        code: &str,
        servers: Servers,
    ) -> (Arc<PaneTable>, StreamedPane, cf_proto::panes::Background) {
        let table = Arc::new(PaneTable::new());
        let pane = open_python(&table, code);
        until_shown(&pane.output, "ready");
        table.mark(&pane.key, servers).expect("mark the turn");
        table.write(&pane.key, b"go\n").expect("tell it to go");
        until_shown(&pane.output, "started");
        let left = table.background(&pane.key).expect("what it left running");
        (table, pane, left)
    }

    /// The commands of what a window left, for a message.
    #[cfg(target_os = "macos")]
    fn commands(left: &cf_proto::panes::Background) -> Vec<&str> {
        left.processes.iter().map(|p| p.command.as_str()).collect()
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn where_it_is_not_said_how_the_harness_keeps_its_servers_what_they_start_is_the_turns() {
        let _serial = serial_pty_test();
        let (table, pane, claude) = started_in(CLAUDE, Servers::Unknown);
        // The server, its browser, the two jobs and the first one's process.
        assert_eq!(
            claude.total,
            5,
            "told by time alone: {:?}",
            commands(&claude)
        );
        table.kill(&pane.key).expect("close the pane");

        let (table, pane, codex) = started_in(CODEX, Servers::Unknown);
        // The two processes of a server, and the command.
        assert_eq!(codex.total, 3, "told by time alone: {:?}", commands(&codex));
        table.kill(&pane.key).expect("close the pane");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn where_the_servers_are_in_the_group_what_they_start_is_not_the_turns_though_it_leaves_it() {
        let _serial = serial_pty_test();
        let (table, pane, left) = started_in(CLAUDE, Servers::InGroup);

        // The server is in the program's group, and so is the browser it started,
        // whatever group the browser went to. The jobs the program started left it:
        // they are the turn's, and so is what the first started.
        assert_eq!(left.total, 3, "{:?}", commands(&left));
        let commands_left = commands(&left);
        for turn in ["time.sleep(34)", "time.sleep(37)"] {
            assert!(
                commands_left.iter().any(|command| command.contains(turn)),
                "{turn}: {commands_left:?}"
            );
        }
        assert!(commands_left.contains(&"sleep 32"), "{commands_left:?}");
        for harness in ["time.sleep(35)", "time.sleep(36)"] {
            assert!(
                !commands_left
                    .iter()
                    .any(|command| command.contains(harness)),
                "{harness}: {commands_left:?}"
            );
        }

        // What the close ends and lists in its exit is the same.
        table.kill(&pane.key).expect("close the pane");
        let kept = pane.ended.background().expect("the close kept its list");
        assert_eq!(kept.len(), 3, "{kept:?}");
        assert_eq!(table.left_running_at_exit(&pane.key, &pane.ended), kept);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn where_the_servers_open_with_the_window_what_they_start_is_not_the_turns_and_a_command_is() {
        let _serial = serial_pty_test();
        let (table, pane, left) = started_in(CODEX, Servers::AtLaunch);

        // The server was there when the first turn began, outside the group, and
        // the two processes it started since are its own. The harness's process
        // was there too, in the group: the command it started is the agent's.
        assert_eq!(left.total, 1, "{:?}", commands(&left));
        assert!(
            left.processes[0].command.contains("time.sleep(32)"),
            "{left:?}"
        );

        table.kill(&pane.key).expect("close the pane");
        let kept = pane.ended.background().expect("the close kept its list");
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(table.left_running_at_exit(&pane.key, &pane.ended), kept);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn what_the_tree_had_before_the_turn_is_not_the_turns_and_what_it_started_after_is() {
        let _serial = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let pane = open(&table, SCRIPT);
        until_shown(&pane.output, "ready");

        table
            .mark(&pane.key, Servers::Unknown)
            .expect("mark the turn");
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
        table
            .mark(&pane.key, Servers::Unknown)
            .expect("mark the first turn");
        table.write(&pane.key, b"go\n").expect("tell it to go");
        until_shown(&pane.output, "started");
        assert_eq!(table.background(&pane.key).expect("look").total, 1);

        table
            .mark(&pane.key, Servers::Unknown)
            .expect("mark the next turn");

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
        table
            .mark(&pane.key, Servers::Unknown)
            .expect("mark the turn");
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
            // It says it started once the job is Python, and no longer the shell
            // it was forked as, which a look could catch first.
            "trap '' HUP; printf ready; read go; \
         /usr/bin/python3 -c 'import time; time.sleep(31)' & \
         until ps -o comm= -p $! | grep -q python; do sleep 0.05; done; printf started",
        );
        until_shown(&pane.output, "ready");
        table
            .mark(&pane.key, Servers::Unknown)
            .expect("mark the turn");
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
            table.mark(&missing, Servers::Unknown),
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

        table.mark(&key, Servers::Unknown).expect("mark the turn");
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

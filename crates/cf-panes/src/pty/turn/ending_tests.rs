//! Ending what a pane's turn left running, the window staying open
//! ([`PaneTable::end_background`]): the processes the turn started are asked to end
//! and given the table's grace, those that ignore it are killed, and what descends
//! from them goes with them; the program, the servers its harness keeps and what
//! the window had before the turn are never touched. Where the daemon did not say
//! how the harness keeps its servers only what an agent's shell started is ended,
//! and what is left is said. The tree is looked at on macOS and on Windows; elsewhere
//! a pane has none.

#[cfg(target_os = "macos")]
mod macos {
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc::Receiver;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use cf_proto::panes::{Ending, Servers};
    use portable_pty::PtySize;
    use serde_json::Value;

    use crate::pty::{
        process_exists, serial_pty_test, PaneEnvironment, PaneError, PaneKey, PaneOutput,
        PaneTable, StreamedPane,
    };

    /// What a test's table gives the programs it asks to end: long enough to tell
    /// from none, and short enough to wait for.
    const GRACE: Duration = Duration::from_millis(1500);

    /// The window of a harness that keeps its servers in the program's group, the
    /// way Claude Code does, once its turn is told to go: a server the window had
    /// before the turn; a server started after it that launches a browser, which
    /// leaves the group; and what the agent runs, each a job of its own: a command
    /// in a session of its own, with a child and a handler for the request to end
    /// (it writes `RELEASED`), and one in a group of its own that ignores the
    /// requests. The program says what it has started, and their numbers, once each
    /// has. A child it has ended is no zombie: its end is its number's.
    const TURN: &str = r#"
import json, os, signal, subprocess, sys, time

signal.signal(signal.SIGCHLD, signal.SIG_IGN)

def start(code):
    return subprocess.Popen([sys.executable, "-c", code], stdout=subprocess.PIPE, text=True)

BROWSER = "import os, time; os.setsid(); print(os.getpid(), flush=True); time.sleep(35)"
SERVER = (
    "import subprocess, sys, time; "
    "browser = subprocess.Popen([sys.executable, '-c', " + repr(BROWSER) + "], stdout=subprocess.PIPE, text=True); "
    "print(browser.stdout.readline().strip(), flush=True); time.sleep(36)"
)
TOOL = (
    "import os, signal, subprocess, time; os.setsid(); "
    "release = lambda *_: (open(os.environ['RELEASED'], 'w').write('x'), os._exit(0)); "
    "signal.signal(signal.SIGTERM, release); signal.signal(signal.SIGHUP, release); "
    "child = subprocess.Popen(['sleep', '32']); print(child.pid, flush=True); time.sleep(34)"
)
STUBBORN = (
    "import os, signal, time; os.setpgid(0, 0); "
    "signal.signal(signal.SIGTERM, signal.SIG_IGN); signal.signal(signal.SIGHUP, signal.SIG_IGN); "
    "print(os.getpid(), flush=True); time.sleep(37)"
)

before = subprocess.Popen(["sleep", "33"])
print("ready", flush=True)
input()
started = [start(code) for code in (SERVER, TOOL, STUBBORN)]
lines = [process.stdout.readline().strip() for process in started]
print("started " + json.dumps({
    "program": os.getpid(), "before": before.pid,
    "server": started[0].pid, "browser": int(lines[0]),
    "tool": started[1].pid, "tool_child": int(lines[1]), "stubborn": started[2].pid,
}), flush=True)
time.sleep(60)
"#;

    /// The window of a harness not known to keep its servers in a way, once its
    /// turn is told to go: a process the window had before the turn; then, as an
    /// agent's command runs, a shell with a job of its own (the shell waits for it);
    /// and a server the harness starts late, which no shell started.
    const SHELLS: &str = r#"
import json, os, signal, subprocess, sys, time

signal.signal(signal.SIGCHLD, signal.SIG_IGN)

before = subprocess.Popen(["sleep", "33"])
print("ready", flush=True)
input()
shell = subprocess.Popen(["/bin/sh", "-c", "sleep 31 & echo $!; wait"], stdout=subprocess.PIPE, text=True)
job = int(shell.stdout.readline().strip())
server = subprocess.Popen(
    [sys.executable, "-c", "import os, time; print(os.getpid(), flush=True); time.sleep(36)"],
    stdout=subprocess.PIPE, text=True)
server.stdout.readline()
print("started " + json.dumps({
    "program": os.getpid(), "before": before.pid,
    "shell": shell.pid, "job": job, "server": server.pid,
}), flush=True)
time.sleep(60)
"#;

    /// A file of the test's own, not there yet.
    fn scratch(name: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "cf-end-{}-{}-{name}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_file(&path);
        path
    }

    /// A pane running the Python program `code`, streamed, with `env` in its
    /// environment.
    fn open_python(
        table: &Arc<PaneTable>,
        code: &str,
        env: &HashMap<String, String>,
    ) -> StreamedPane {
        table
            .open_streamed(
                Path::new("/tmp"),
                &["/usr/bin/python3".into(), "-c".into(), code.into()],
                PaneEnvironment::new(env, &[]),
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

    /// Reads the pane's output until a line that begins `marker` is whole, and
    /// says the rest of it.
    fn until_line(output: &Receiver<PaneOutput>, marker: &str) -> String {
        let mut seen = String::new();
        loop {
            if let Some(at) = seen.find(marker) {
                if let Some(end) = seen[at..].find(['\r', '\n']) {
                    return seen[at + marker.len()..at + end].to_owned();
                }
            }
            let chunk = output
                .recv_timeout(Duration::from_secs(10))
                .unwrap_or_else(|_| panic!("the pane showed no {marker}: {seen:?}"));
            seen.push_str(&String::from_utf8_lossy(&chunk.bytes));
        }
    }

    /// The window of `code`, told its turn begins (the mark, then "go") and run to
    /// where it says what it started: the pane and the numbers it names.
    fn started(
        table: &Arc<PaneTable>,
        code: &str,
        servers: Servers,
        env: &HashMap<String, String>,
    ) -> (StreamedPane, HashMap<String, i32>) {
        let pane = open_python(table, code, env);
        until_line(&pane.output, "ready");
        table.mark(&pane.key, servers).expect("mark the turn");
        table.write(&pane.key, b"go\n").expect("tell it to go");
        let said = until_line(&pane.output, "started ");
        let numbers: Value = serde_json::from_str(&said).expect("the numbers it started");
        let pids = numbers
            .as_object()
            .expect("an object")
            .iter()
            .map(|(name, pid)| (name.clone(), pid.as_i64().expect("a number") as i32))
            .collect();
        (pane, pids)
    }

    /// What the pane's tree lists of the turn's, by number.
    fn listed(table: &PaneTable, key: &PaneKey) -> Vec<u32> {
        let left = table.background(key).expect("what it left running");
        left.processes.iter().map(|process| process.pid).collect()
    }

    /// Waits until the process is gone, up to two seconds.
    fn gone(pid: i32) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while process_exists(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        !process_exists(pid)
    }

    /// The numbers an ending names, as the commands of what it ended.
    fn ended_pids(ending: &Ending) -> Vec<u32> {
        ending
            .ended
            .processes
            .iter()
            .map(|process| process.pid)
            .collect()
    }

    fn env_with(released: &Path) -> HashMap<String, String> {
        HashMap::from([("RELEASED".to_owned(), released.display().to_string())])
    }

    #[test]
    fn what_the_turn_started_is_asked_to_end_and_killed_once_the_grace_is_over_and_the_harness_is_left_as_it_was(
    ) {
        let _serial = serial_pty_test();
        let table = Arc::new(PaneTable::closing_within(GRACE));
        let released = scratch("released");
        let (pane, pid) = started(&table, TURN, Servers::InGroup, &env_with(&released));
        let asked = listed(&table, &pane.key);
        // The jobs and the child of one: the server, its browser and what the window
        // had are the harness's.
        assert_eq!(asked.len(), 3, "{asked:?}");

        let closing = Instant::now();
        let ending = table.end_background(&pane.key, &asked).expect("end it");

        let mut ended = ended_pids(&ending);
        ended.sort_unstable();
        let mut turn = vec![
            pid["tool"] as u32,
            pid["tool_child"] as u32,
            pid["stubborn"] as u32,
        ];
        turn.sort_unstable();
        assert_eq!(ended, turn, "{ending:?}");
        assert_eq!(ending.ended.total, 3);
        assert_eq!(ending.left.total, 0, "{ending:?}");
        assert!(
            closing.elapsed() >= GRACE,
            "the job that ignores the request was given the grace: {:?}",
            closing.elapsed()
        );
        assert!(
            released.exists(),
            "the job that handles it was asked to end, not killed"
        );
        for job in ["tool", "tool_child", "stubborn"] {
            assert!(gone(pid[job]), "{job} was ended");
        }

        // The window is as it was: its program, what it had before the turn, and
        // what its harness keeps.
        for harness in ["program", "before", "server", "browser"] {
            assert!(process_exists(pid[harness]), "{harness} is left alone");
        }
        assert!(
            table.list().expect("list").iter().any(|info| info.alive),
            "the window is open"
        );
        assert_eq!(listed(&table, &pane.key), Vec::<u32>::new());
        table.kill(&pane.key).expect("close the pane");
    }

    #[test]
    fn an_ending_waits_for_no_longer_than_the_programs_take_to_end() {
        let _serial = serial_pty_test();
        let table = Arc::new(PaneTable::closing_within(Duration::from_secs(20)));
        let released = scratch("released");
        let (pane, pid) = started(&table, TURN, Servers::InGroup, &env_with(&released));
        // The job that ignores the requests is not asked for.
        let asked: Vec<u32> = listed(&table, &pane.key)
            .into_iter()
            .filter(|number| *number != pid["stubborn"] as u32)
            .collect();
        assert_eq!(asked.len(), 2, "{asked:?}");

        let closing = Instant::now();
        let ending = table.end_background(&pane.key, &asked).expect("end it");

        assert_eq!(ending.ended.total, 2, "{ending:?}");
        assert!(
            closing.elapsed() < Duration::from_secs(10),
            "it ended with what it asked to end: {:?}",
            closing.elapsed()
        );
        assert!(released.exists());
        assert!(
            process_exists(pid["stubborn"]),
            "what is not asked for is left"
        );
        table.kill(&pane.key).expect("close the pane");
    }

    #[test]
    fn what_a_close_lists_after_an_ending_is_none_of_what_it_ended() {
        let _serial = serial_pty_test();
        let table = Arc::new(PaneTable::closing_within(GRACE));
        let released = scratch("released");
        let (pane, _) = started(&table, TURN, Servers::InGroup, &env_with(&released));
        let asked = listed(&table, &pane.key);
        table.end_background(&pane.key, &asked).expect("end it");

        table.kill(&pane.key).expect("close the pane");

        let kept = pane.ended.background().expect("the close kept its list");
        assert!(kept.is_empty(), "{kept:?}");
    }

    #[test]
    fn where_it_is_not_said_how_the_harness_keeps_its_servers_only_what_a_shell_started_is_ended_and_the_rest_is_left_and_said_so(
    ) {
        let _serial = serial_pty_test();
        let table = Arc::new(PaneTable::closing_within(GRACE));
        let (pane, pid) = started(&table, SHELLS, Servers::Unknown, &HashMap::new());
        let asked = listed(&table, &pane.key);
        // The shell, its job, and the server: by time alone each is the turn's.
        assert_eq!(asked.len(), 3, "{asked:?}");

        let ending = table.end_background(&pane.key, &asked).expect("end it");

        let mut ended = ended_pids(&ending);
        ended.sort_unstable();
        let mut shell = vec![pid["shell"] as u32, pid["job"] as u32];
        shell.sort_unstable();
        assert_eq!(ended, shell, "{ending:?}");
        let left: Vec<u32> = ending
            .left
            .processes
            .iter()
            .map(|process| process.pid)
            .collect();
        assert_eq!(left, [pid["server"] as u32], "{ending:?}");
        assert_eq!(ending.left.total, 1);
        assert!(gone(pid["shell"]) && gone(pid["job"]));
        for alive in ["program", "before", "server"] {
            assert!(process_exists(pid[alive]), "{alive} is left alone");
        }
        table.kill(&pane.key).expect("close the pane");
    }

    #[test]
    fn only_the_processes_it_is_asked_to_end_are_ended_and_what_is_not_the_turns_is_nobodys_business(
    ) {
        let _serial = serial_pty_test();
        let table = Arc::new(PaneTable::closing_within(GRACE));
        let released = scratch("released");
        let (pane, pid) = started(&table, TURN, Servers::InGroup, &env_with(&released));

        // The window's program, what it had before the turn and a server are named,
        // and so is a number that is nothing's: none of them is the turn's.
        let ending = table
            .end_background(
                &pane.key,
                &[
                    pid["program"] as u32,
                    pid["before"] as u32,
                    pid["server"] as u32,
                    999_999,
                ],
            )
            .expect("end it");

        assert_eq!(ending.ended.total, 0, "{ending:?}");
        assert_eq!(ending.left.total, 0, "{ending:?}");
        for alive in ["program", "before", "server", "browser", "tool", "stubborn"] {
            assert!(process_exists(pid[alive]), "{alive} is left alone");
        }
        table.kill(&pane.key).expect("close the pane");
    }

    #[test]
    fn a_pane_never_marked_has_nothing_to_end_and_a_pane_that_is_not_open_has_no_ending() {
        let _serial = serial_pty_test();
        let table = Arc::new(PaneTable::closing_within(GRACE));
        let pane = open_python(
            &table,
            "import subprocess, time; subprocess.Popen(['sleep', '33']); print('ready', flush=True); time.sleep(60)",
            &HashMap::new(),
        );
        until_line(&pane.output, "ready");
        assert_eq!(
            table.end_background(&pane.key, &[1, 2, 3]).expect("end it"),
            Ending::default()
        );
        table.kill(&pane.key).expect("close the pane");

        let missing = PaneKey::new("nobody", 1);
        assert!(matches!(
            table.end_background(&missing, &[]),
            Err(PaneError::NotFound(key)) if key == missing
        ));
    }
}

/// The same on Windows, where the tree is the pane's job and the harness's servers
/// are never told by anything but what started a process: what a shell started
/// after the mark is the turn's and is ended, with what it started, and the rest
/// is left.
#[cfg(windows)]
mod windows {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use cf_proto::panes::Servers;

    use crate::pty::conpty_test::{open, powershell, read_until, system};
    use crate::pty::{serial_pty_test, PaneKey, PaneTable};

    /// What the pane's job lists that the turn started, whose command line holds
    /// `count`: the ping of that count.
    fn listed(table: &PaneTable, key: &PaneKey) -> Vec<(u32, String)> {
        table
            .background(key)
            .expect("what it left running")
            .processes
            .into_iter()
            .map(|process| (process.pid, process.command.to_lowercase()))
            .collect()
    }

    #[test]
    fn what_a_shell_started_after_the_mark_is_ended_with_what_it_started_and_the_window_its_program_and_a_server_it_started_late_are_left_as_they_were(
    ) {
        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let ping = system("PING.EXE");
        let shell = system(r"WindowsPowerShell\v1.0\powershell.exe");
        let (key, output) = open(
            &table,
            &powershell(&format!(
                // A server the window has before the turn; then, once told to go, an
                // agent's command (a shell that runs a ping and waits for it) and a
                // server the harness starts late, which no shell started.
                "Start-Process -WindowStyle Hidden -FilePath '{ping}' -ArgumentList '-n','1001','127.0.0.1'; \
                 Write-Output 'READY#'; [Console]::In.ReadLine() | Out-Null; \
                 Start-Process -WindowStyle Hidden -FilePath '{shell}' -ArgumentList '-NoProfile','-Command','ping -n 1002 127.0.0.1'; \
                 Start-Process -WindowStyle Hidden -FilePath '{ping}' -ArgumentList '-n','1003','127.0.0.1'; \
                 Write-Output 'STARTED#'; [Console]::In.ReadLine() | Out-Null; \
                 Write-Output 'ALIVE#'; Start-Sleep 1000"
            )),
            &HashMap::new(),
            1 << 20,
        );
        read_until(&table, &key, &output, "READY#");
        table.mark(&key, Servers::Unknown).expect("mark the turn");
        table.write(&key, b"\r").expect("go on");
        read_until(&table, &key, &output, "STARTED#");

        // Both pings run once the shell has started its own; the look finds them.
        let deadline = Instant::now() + Duration::from_secs(60);
        let before = loop {
            let found = listed(&table, &key);
            if found.iter().any(|(_, command)| command.contains("-n 1002"))
                && found.iter().any(|(_, command)| command.contains("-n 1003"))
            {
                break found;
            }
            assert!(
                Instant::now() < deadline,
                "the pings were never listed: {found:?}"
            );
            std::thread::sleep(Duration::from_millis(100));
        };
        let asked: Vec<u32> = before.iter().map(|(pid, _)| *pid).collect();

        let ending = table.end_background(&key, &asked).expect("end it");

        // Whether the list holds a process whose command line holds all of `words`.
        let says = |list: &cf_proto::panes::Background, words: &[&str]| {
            list.processes.iter().any(|process| {
                let command = process.command.to_lowercase();
                words.iter().all(|word| command.contains(word))
            })
        };
        assert!(says(&ending.ended, &["ping.exe", "-n 1002"]), "{ending:?}");
        assert!(
            says(&ending.ended, &["powershell.exe", "-n 1002"]),
            "the shell is ended with its ping: {ending:?}"
        );
        assert!(says(&ending.left, &["ping.exe", "-n 1003"]), "{ending:?}");
        assert!(!says(&ending.ended, &["-n 1003"]), "{ending:?}");
        assert!(
            !says(&ending.ended, &["-n 1001"]) && !says(&ending.left, &["-n 1001"]),
            "what the window had before the turn is nobody's business: {ending:?}"
        );

        // Of what the job lists of the turn, the late server is left, and none of
        // the rest; the program goes on to say it is there.
        let left = listed(&table, &key);
        assert!(
            left.iter().any(|(_, command)| command.contains("-n 1003")),
            "{left:?}"
        );
        assert!(
            left.iter().all(|(_, command)| !command.contains("-n 1002")),
            "{left:?}"
        );
        table.write(&key, b"\r").expect("go on");
        read_until(&table, &key, &output, "ALIVE#");
        table.kill(&key).expect("close the pane");
    }
}

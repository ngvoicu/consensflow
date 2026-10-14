//! Ending what a pane's turn left running, the window staying open
//! ([`PaneTable::end_background`]): the processes the turn's work is proved to be
//! are asked to end and given the table's grace, those that ignore it are killed, and
//! what they started while they were given it goes with them; the program, the
//! servers its harness keeps, what the window had before the turn and what the host
//! cannot tell the turn's are never touched, and not named. The tree is looked at on
//! macOS and on Windows; elsewhere a pane has none.

mod job;
#[cfg(target_os = "macos")]
mod programs;

#[cfg(target_os = "macos")]
mod macos {
    use std::collections::HashMap;
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use cf_proto::panes::{Ending, Servers};

    use super::programs::{HELPERS, LEFT, MANY, NEW, SPAWNER, TURN};
    use crate::background::SETTLE;
    use crate::pty::turn::support::{
        ended_pids, gone, harness_programs, kill, listed, numbers, open_python, scratch, started,
        until_line, DETACHED,
    };
    use crate::pty::{process_exists, serial_pty_test, PaneError, PaneKey, PaneTable, Sentinel};

    /// What a test's table gives the programs it asks to end: long enough to tell
    /// from none, and short enough to wait for.
    const GRACE: Duration = Duration::from_millis(1500);

    /// What `RELEASED` and, where it is wanted, `STUBBORN` say to the program.
    fn env_with(released: &Path, stubborn: bool) -> HashMap<String, String> {
        let mut env = HashMap::from([("RELEASED".to_owned(), released.display().to_string())]);
        if stubborn {
            env.insert("STUBBORN".to_owned(), "1".to_owned());
        }
        env
    }

    #[test]
    fn what_the_turn_started_is_asked_to_end_and_killed_once_the_grace_is_over_and_the_harness_is_left_as_it_was(
    ) {
        let _serial = serial_pty_test();
        let mut outsider = Sentinel::start();
        let table = Arc::new(PaneTable::closing_within(GRACE));
        let released = scratch("released");
        let (pane, pid) = started(
            &table,
            TURN,
            Servers::InGroup,
            None,
            &env_with(&released, true),
        );
        // The two commands and their jobs: the server, its browser and what the window
        // had are the harness's.
        let work = numbers(
            &pid,
            &["handled", "handled_job", "stubborn", "stubborn_job"],
        );
        assert_eq!(listed(&table, &pane.key), work);

        let closing = Instant::now();
        let ending = table.end_background(&pane.key).expect("end it");

        assert_eq!(ended_pids(&ending), work, "{ending:?}");
        assert_eq!(
            (ending.ended.total, ending.left.total),
            (4, 0),
            "{ending:?}"
        );
        assert!(
            closing.elapsed() >= GRACE,
            "the command that ignores the request was given the grace: {:?}",
            closing.elapsed()
        );
        assert!(
            released.exists(),
            "the command that handles it was asked to end, not killed"
        );
        for job in ["handled", "handled_job", "stubborn", "stubborn_job"] {
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
        assert!(
            outsider.runs(),
            "a process outside the window is no ending's to end"
        );
        table.kill(&pane.key).expect("close the pane");
        assert!(outsider.runs(), "nor a close's");
    }

    #[test]
    fn an_ending_waits_for_no_longer_than_the_programs_take_to_end() {
        let _serial = serial_pty_test();
        let mut outsider = Sentinel::start();
        let table = Arc::new(PaneTable::closing_within(Duration::from_secs(20)));
        let released = scratch("released");
        let (pane, pid) = started(
            &table,
            TURN,
            Servers::InGroup,
            None,
            &env_with(&released, false),
        );

        let closing = Instant::now();
        let ending = table.end_background(&pane.key).expect("end it");

        assert_eq!(ending.ended.total, 2, "{ending:?}");
        assert!(
            closing.elapsed() < Duration::from_secs(10),
            "it ended with what it asked to end: {:?}",
            closing.elapsed()
        );
        assert!(released.exists());
        for harness in ["program", "before", "server", "browser"] {
            assert!(process_exists(pid[harness]), "{harness} is left alone");
        }
        assert!(outsider.runs(), "a process outside the window");
        table.kill(&pane.key).expect("close the pane");
        assert!(outsider.runs(), "nor a close's");
    }

    #[test]
    fn what_a_close_lists_after_an_ending_is_none_of_what_it_ended() {
        let _serial = serial_pty_test();
        let mut outsider = Sentinel::start();
        let table = Arc::new(PaneTable::closing_within(GRACE));
        let released = scratch("released");
        let (pane, _) = started(
            &table,
            TURN,
            Servers::InGroup,
            None,
            &env_with(&released, false),
        );
        table.end_background(&pane.key).expect("end it");
        assert!(outsider.runs(), "a process outside the window");

        table.kill(&pane.key).expect("close the pane");

        let kept = pane.ended.background().expect("the close kept its list");
        assert!(kept.is_empty(), "{kept:?}");
        assert!(outsider.runs(), "nor a close's");
    }

    #[test]
    fn a_job_the_agent_detached_from_a_shell_that_ended_is_left_running_unnamed_and_the_close_ends_it(
    ) {
        let _serial = serial_pty_test();
        let mut outsider = Sentinel::start();
        for servers in [Servers::InGroup, Servers::Unknown] {
            let table = Arc::new(PaneTable::closing_within(GRACE));
            let (pane, pid) = started(&table, DETACHED, servers, None, &HashMap::new());

            // The shell has gone and the job has no parent: no chain of processes that
            // run leads from it to a shell the harness started, and its being in the
            // window's session, which it is, is no proof of whose work it is.
            assert_eq!(listed(&table, &pane.key), Vec::<u32>::new(), "{servers:?}");
            let ending = table.end_background(&pane.key).expect("end it");

            assert_eq!(ending, Ending::default(), "{servers:?}");
            assert!(process_exists(pid["job"]), "{servers:?}: it runs on");
            // It is the window's all the same: the window's close ends it.
            table.kill(&pane.key).expect("close the pane");
            assert!(gone(pid["job"]), "{servers:?}: the close ended it");
            assert!(outsider.runs(), "{servers:?}: a process outside the window");
        }
    }

    #[test]
    fn a_job_that_left_the_session_with_the_shell_that_started_it_gone_is_left_running_and_not_named(
    ) {
        let _serial = serial_pty_test();
        let mut outsider = Sentinel::start();
        let table = Arc::new(PaneTable::closing_within(GRACE));
        let (pane, pid) = started(&table, LEFT, Servers::Unknown, None, &HashMap::new());

        // `setsid` took it out of the window's session, what the system shows of a
        // system binary has no marker, and the processes that started it have gone:
        // nothing of it says it is the window's, let alone the turn's.
        assert_eq!(listed(&table, &pane.key), Vec::<u32>::new());
        let ending = table.end_background(&pane.key).expect("end it");

        assert_eq!(ending, Ending::default());
        assert!(
            process_exists(pid["job"]),
            "nothing ended what it did not find"
        );
        table.kill(&pane.key).expect("close the pane");
        assert!(
            process_exists(pid["job"]),
            "the close does not find it either"
        );
        assert!(outsider.runs(), "a process outside the window");
        kill(pid["job"]);
    }

    #[test]
    fn what_a_command_asked_to_end_started_while_it_was_given_the_grace_is_ended_with_it() {
        let _serial = serial_pty_test();
        let mut outsider = Sentinel::start();
        let table = Arc::new(PaneTable::closing_within(GRACE));
        let child = scratch("child");
        let env = HashMap::from([("CHILD".to_owned(), child.display().to_string())]);
        let (pane, pid) = started(&table, SPAWNER, Servers::InGroup, None, &env);
        assert_eq!(listed(&table, &pane.key), numbers(&pid, &["shell", "job"]));

        let ending = table.end_background(&pane.key).expect("end it");

        // The command's handler started a job of its own and went on: the job is what the
        // ending's own started, found by the identifier of its parent, and it is ended and
        // said.
        let started_late: i32 = fs::read_to_string(&child)
            .expect("the handler wrote the job's number")
            .trim()
            .parse()
            .expect("a number");
        assert!(
            ended_pids(&ending).contains(&(started_late as u32)),
            "{ending:?}"
        );
        assert_eq!(ending.ended.total, 3, "{ending:?}");
        assert_eq!(ending.left.total, 0, "{ending:?}");
        assert!(gone(started_late), "it was ended before the ending ended");
        for job in ["shell", "job"] {
            assert!(gone(pid[job]), "{job} was ended");
        }
        assert!(process_exists(pid["before"]));
        assert!(outsider.runs(), "a process outside the window");
        table.kill(&pane.key).expect("close the pane");
        assert!(outsider.runs(), "nor a close's");
    }

    #[test]
    fn new_work_the_window_starts_while_an_ending_gives_the_grace_is_no_part_of_the_ending() {
        let _serial = serial_pty_test();
        let mut outsider = Sentinel::start();
        let table = Arc::new(PaneTable::closing_within(GRACE));
        let (pane, pid) = started(&table, NEW, Servers::InGroup, None, &HashMap::new());
        let old = numbers(&pid, &["old", "old_job"]);
        assert_eq!(listed(&table, &pane.key), old);

        // The command ignores the request to end, so the ending waits the grace out; a
        // human types into the window meanwhile, once the ending has looked twice and asked
        // (the delivery of a message would be held), and the window's program starts new
        // work, a shell in a group of its own with a job, which the old turn's mark takes
        // for the turn's work as well.
        let (ending, new) = std::thread::scope(|scope| {
            let ending = scope.spawn(|| table.end_background(&pane.key).expect("end it"));
            std::thread::sleep(SETTLE + Duration::from_millis(700));
            table
                .write(&pane.key, b"more\n")
                .expect("type into the window");
            let said = until_line(&pane.output, "more ");
            let new: serde_json::Value = serde_json::from_str(&said).expect("the new work");
            (ending.join().expect("the ending ends"), new)
        });
        let (shell, job) = (
            new["new"].as_i64().expect("a number") as i32,
            new["new_job"].as_i64().expect("a number") as i32,
        );

        assert_eq!(ended_pids(&ending), old, "{ending:?}");
        assert_eq!(ending.left.total, 0, "{ending:?}");
        for process in ["old", "old_job"] {
            assert!(gone(pid[process]), "{process} was ended");
        }
        // The new work is untouched, though a look of the tree takes it for the old
        // turn's.
        assert!(process_exists(shell), "the new work's shell runs");
        assert!(process_exists(job), "the new work's job runs");
        let seen = listed(&table, &pane.key);
        assert!(
            seen.contains(&(shell as u32)) && seen.contains(&(job as u32)),
            "{seen:?}"
        );
        assert!(outsider.runs(), "a process outside the window");
        table.kill(&pane.key).expect("close the pane");
        assert!(outsider.runs(), "nor a close's");
    }

    #[test]
    fn more_commands_than_a_frame_names_are_all_ended_and_only_so_many_are_named() {
        let _serial = serial_pty_test();
        let mut outsider = Sentinel::start();
        let table = Arc::new(PaneTable::closing_within(GRACE));
        let (pane, pid) = started(&table, MANY, Servers::InGroup, None, &HashMap::new());
        assert_eq!(table.background(&pane.key).expect("look").total, 40);

        let ending = table.end_background(&pane.key).expect("end it");

        assert_eq!(ending.ended.total, 40, "all of it, not what is named");
        assert_eq!(
            ending.ended.processes.len(),
            cf_proto::panes::BACKGROUND_PROCESSES
        );
        assert_eq!(ending.left.total, 0, "{ending:?}");
        for n in 0..20 {
            assert!(gone(pid[&format!("job{n}")]), "job{n}");
            assert!(gone(pid[&format!("shell{n}")]), "shell{n}");
        }
        assert!(process_exists(pid["program"]));
        assert!(outsider.runs(), "a process outside the window");
        table.kill(&pane.key).expect("close the pane");
        assert!(outsider.runs(), "nor a close's");
    }

    #[test]
    fn a_program_of_the_harnesss_own_and_what_it_runs_are_never_ended_nor_is_a_server_it_starts_late(
    ) {
        let _serial = serial_pty_test();
        let mut outsider = Sentinel::start();
        let table = Arc::new(PaneTable::closing_within(GRACE));
        // The binary of the harness and a program named for it, beside it, in a folder
        // with a space in its name.
        let bins = harness_programs();
        let env = HashMap::from([("BINS".to_owned(), bins.display().to_string())]);
        let (pane, pid) = started(&table, HELPERS, Servers::Unknown, Some("codex"), &env);

        // The helper runs a shell, which no server of the window's explains: it is the
        // harness's by what it is named for and where it runs from, and so is what it
        // runs. The late server and its browser are no shell's work. The agent's
        // command is.
        let work = numbers(&pid, &["tool", "tool_job"]);
        assert_eq!(listed(&table, &pane.key), work);

        let ending = table.end_background(&pane.key).expect("end it");

        assert_eq!(ended_pids(&ending), work, "{ending:?}");
        assert_eq!(ending.left.total, 0, "{ending:?}");
        for alive in [
            "program",
            "before",
            "helper",
            "helper_shell",
            "helper_job",
            "server",
            "browser",
        ] {
            assert!(process_exists(pid[alive]), "{alive} is left alone");
        }
        assert!(outsider.runs(), "a process outside the window");
        table.kill(&pane.key).expect("close the pane");
        assert!(outsider.runs(), "nor a close's");
        let _ = fs::remove_dir_all(&bins);
    }

    #[test]
    fn a_pane_never_marked_has_nothing_to_end_and_a_pane_that_is_not_open_has_no_ending() {
        let _serial = serial_pty_test();
        let mut outsider = Sentinel::start();
        let table = Arc::new(PaneTable::closing_within(GRACE));
        let pane = open_python(
            &table,
            "import subprocess, time; subprocess.Popen(['/bin/sh', '-c', 'sleep 33']); print('ready', flush=True); time.sleep(60)",
            &HashMap::new(),
        );
        until_line(&pane.output, "ready");
        assert_eq!(
            table.end_background(&pane.key).expect("end it"),
            Ending::default()
        );
        table.kill(&pane.key).expect("close the pane");
        assert!(outsider.runs(), "a process outside the window");

        let missing = PaneKey::new("nobody", 1);
        assert!(matches!(
            table.end_background(&missing),
            Err(PaneError::NotFound(key)) if key == missing
        ));
    }
}

/// The same on Windows, where the tree is the pane's job: what a shell started
/// after the mark is the turn's and is ended, with what it started, and the rest (a
/// job with no parent left among it) is left, and not named.
#[cfg(windows)]
mod windows {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use cf_proto::panes::{Ending, Servers};

    use super::job::on_its_own;
    use crate::background::Member;
    use crate::pty::conpty_test::{open, powershell, read_until, system};
    use crate::pty::{serial_pty_test, PaneKey, PaneTable};

    /// What the pane's job lists that the turn's work is, whose command line holds
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
        table
            .mark(&key, Servers::Unknown, None)
            .expect("mark the turn");
        table.write(&key, b"\r").expect("go on");
        read_until(&table, &key, &output, "STARTED#");

        // Both pings run once the shell has started its own; the look finds the
        // shell's ping itself. The shell's own command line says `-n 1002` too, and
        // an ending that comes before the shell has started its ping ends the shell
        // and leaves no ping to end (zeewin, 2026-10-14).
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let found = listed(&table, &key);
            if found.iter().any(|(_, command)| {
                command.to_lowercase().contains("ping.exe") && command.contains("-n 1002")
            }) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the shell's ping was never listed: {found:?}"
            );
            std::thread::sleep(Duration::from_millis(100));
        }

        let ending = table.end_background(&key).expect("end it");

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
        // The late server is none of the turn's that can be proved: it is left alone
        // and is in neither list.
        assert!(!says(&ending.ended, &["-n 1003"]), "{ending:?}");
        assert!(!says(&ending.left, &["-n 1003"]), "{ending:?}");
        assert!(
            !says(&ending.ended, &["-n 1001"]) && !says(&ending.left, &["-n 1001"]),
            "what the window had before the turn is nobody's business: {ending:?}"
        );
        assert_eq!(ending.left.total, 0, "{ending:?}");

        // What is left alone is still running, and the program goes on to say it is.
        table.write(&key, b"\r").expect("go on");
        read_until(&table, &key, &output, "ALIVE#");
        table.kill(&key).expect("close the pane");
    }

    /// What the pane's job holds now: the window's own program, and what it started
    /// that runs.
    fn held(table: &PaneTable, key: &PaneKey) -> Vec<Member> {
        let panes = table.lock_panes().expect("the panes");
        let pane = panes.get(key).expect("the pane");
        pane.tree().members().expect("what the job holds")
    }

    #[test]
    fn a_job_whose_shell_has_ended_is_in_the_pane_job_still_with_no_parent_and_is_left_running_unnamed(
    ) {
        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let ping = system("PING.EXE");
        let shell = system(r"WindowsPowerShell\v1.0\powershell.exe");
        let (key, output) = open(
            &table,
            &powershell(&format!(
                // Once told to go, an agent's command that leaves a job as agents do:
                // a shell that starts a ping apart (`Start-Process`) and ends at once.
                "Write-Output 'READY#'; [Console]::In.ReadLine() | Out-Null; \
                 Start-Process -WindowStyle Hidden -FilePath '{shell}' -ArgumentList '-NoProfile','-Command','Start-Process -WindowStyle Hidden -FilePath ''{ping}'' -ArgumentList ''-n'',''1004'',''127.0.0.1'''; \
                 Write-Output 'STARTED#'; Start-Sleep 1000"
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

        // The job object holds what a process starts wherever it goes: the ping is in
        // the job once the shell that started it has gone, which is when no process of
        // the job started it. Not when no process has the shell's `-Command` among its
        // arguments: the window's own program has, as long as it runs, and the wait
        // never ended (the PTY tests' watchdog ended the run, 2026-10-14). The ping is
        // told by `-n 1004`, which no other command line holds as it is written here.
        // The wait is shorter than the watchdog, which would else end the run before
        // this said what the job held.
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let found = held(&table, &key);
            if on_its_own(&found, "-n 1004") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the ping was never in the job on its own: {found:?}"
            );
            std::thread::sleep(Duration::from_millis(100));
        }

        // With no process of the job that started it, no chain of processes that run
        // leads from the ping to a shell the harness started: nothing proves whose it
        // is, so it is not listed, not ended and not named.
        assert_eq!(listed(&table, &key), Vec::new());
        let ending = table.end_background(&key).expect("end it");
        assert_eq!(ending, Ending::default());
        assert!(on_its_own(&held(&table, &key), "-n 1004"), "it runs on");
        // The pane's job is the window's: its close ends it with the rest.
        table.kill(&key).expect("close the pane");
    }
}

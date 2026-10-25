//! What a pane's turn leaves running, as the host's look names it ([`PaneTable::background`])
//! and its close ends it: what the turn's work is proved to be is named, and the program, the
//! servers its harness keeps, what the window had before the turn and what the host cannot
//! tell the turn's are not. Nothing ends the turn's work but the window's close (turn-end:
//! a turn ends nothing it left running). The tree is looked at on macOS and on Windows;
//! elsewhere a pane has none.

mod job;
#[cfg(target_os = "macos")]
mod programs;

#[cfg(target_os = "macos")]
mod macos {
    use std::collections::HashMap;
    use std::fs;
    use std::sync::Arc;
    use std::time::Duration;

    use cf_proto::panes::Servers;

    use super::programs::{HELPERS, LEFT};
    use crate::pty::turn::support::{
        gone, harness_programs, kill, listed, numbers, started, DETACHED,
    };
    use crate::pty::{process_exists, serial_pty_test, PaneTable, Sentinel};

    /// What a test's table gives the programs its close asks to end: long enough to tell
    /// from none, and short enough to wait for.
    const GRACE: Duration = Duration::from_millis(1500);

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
        assert!(process_exists(pid["job"]), "it runs on");
        table.kill(&pane.key).expect("close the pane");
        assert!(
            process_exists(pid["job"]),
            "the close does not find it either"
        );
        assert!(outsider.runs(), "a process outside the window");
        kill(pid["job"]);
    }

    #[test]
    fn a_program_of_the_harnesss_own_and_what_it_runs_are_no_work_of_the_turn_nor_is_a_server_it_starts_late(
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
}

/// The same on Windows, where the tree is the pane's job: what a shell started
/// after the mark is the turn's, with what it started, and the rest (a job with no
/// parent left among it) is not named.
#[cfg(windows)]
mod windows {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use cf_proto::panes::Servers;

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
    fn what_a_shell_started_after_the_mark_is_the_turns_work_with_what_it_started_and_the_window_its_program_and_a_server_it_started_late_are_not(
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
        // a look that comes before the shell has started its ping finds no ping
        // (zeewin, 2026-10-14).
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

        let found = table.background(&key).expect("what it left running");

        // Whether the list holds a process whose command line holds all of `words`.
        let says = |words: &[&str]| {
            found.processes.iter().any(|process| {
                let command = process.command.to_lowercase();
                words.iter().all(|word| command.contains(word))
            })
        };
        assert!(
            says(&["powershell.exe", "-n 1002"]),
            "the shell with its ping: {found:?}"
        );
        // The late server is none of the turn's that can be proved, and what the window had
        // before the turn is nobody's business.
        assert!(!says(&["-n 1003"]), "{found:?}");
        assert!(!says(&["-n 1001"]), "{found:?}");

        // The program goes on to say it runs.
        table.write(&key, b"\r").expect("go on");
        read_until(&table, &key, &output, "ALIVE#");
        table.kill(&key).expect("close the pane");
    }

    /// Git Bash's `bash.exe`, where Git for Windows puts it.
    fn git_bash() -> Option<std::path::PathBuf> {
        [
            r"C:\Program Files\Git\bin\bash.exe",
            r"C:\Program Files (x86)\Git\bin\bash.exe",
        ]
        .iter()
        .map(std::path::PathBuf::from)
        .find(|bash| bash.is_file())
    }

    /// A command of Git Bash's, in a real pane whose program is in its job from its first
    /// instruction (T-323, P3-7 of T-194, the question of whether that made the repair of a
    /// Cygwin program's parent moot): `sh ./s.sh` runs `sleep 30`, and the sleep names the
    /// forked copy of the shell that started it, which has gone. It is in the job all the
    /// same, so the job's members without the repair have it with no parent; with the repair
    /// (what the host does) it is the script's, and the host lists it as the turn's work.
    #[test]
    fn a_git_bash_scripts_sleep_in_a_pane_whose_program_is_in_its_job_from_the_first_instruction_still_names_a_parent_that_has_gone(
    ) {
        let Some(bash) = git_bash() else {
            eprintln!("no Git Bash here: nothing to try the repair on");
            return;
        };
        let _pty_guard = serial_pty_test();
        let folder = std::env::temp_dir().join(format!("cf-panes-gitbash-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("a folder for the script");
        std::fs::write(folder.join("s.sh"), "#!/bin/sh\nsleep 30\n").expect("the script");
        let table = Arc::new(PaneTable::new());
        let (key, output) = open(
            &table,
            &powershell(&format!(
                // Once told to go, the agent's command: Git Bash's shell runs a script.
                "Write-Output 'READY#'; [Console]::In.ReadLine() | Out-Null; \
                 Write-Output 'STARTED#'; Set-Location '{}'; & '{}' -c 'sh ./s.sh'; \
                 Write-Output 'ALIVE#'; Start-Sleep 1000",
                folder.display(),
                bash.display()
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

        // The pane's job as Windows names the parents: the sleep is a member, with none. The
        // forked copy of the script's shell that started it is there for some milliseconds
        // after the sleep is, and a look in them finds a parent that is a process (T-375):
        // the look is every few milliseconds, and waits for the parent to be gone.
        let job = {
            let panes = table.lock_panes().expect("the panes");
            Arc::clone(&panes.get(&key).expect("the pane").job)
        };
        let is_sleep = |member: &Member| member.program.eq_ignore_ascii_case("sleep.exe");
        let deadline = Instant::now() + Duration::from_secs(60);
        let sleep = loop {
            let members = job.members_with(&|_| None).expect("what the job holds");
            if let Some(sleep) = members
                .iter()
                .find(|member| is_sleep(member) && member.parent.is_none())
            {
                break sleep.clone();
            }
            assert!(
                Instant::now() < deadline,
                "the script's sleep never began, or its starter never went: {members:?}"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(
            sleep.parent, None,
            "in the job from the first instruction, and its parent a process that has gone"
        );

        // As the host reads the job: the sleep is the script's.
        let members = held(&table, &key);
        let sleep = members
            .iter()
            .find(|member| is_sleep(member))
            .expect("the sleep");
        let script = members
            .iter()
            .find(|member| Some(member.key) == sleep.parent)
            .unwrap_or_else(|| panic!("the sleep has no parent among {members:?}"));
        assert!(
            script.program.eq_ignore_ascii_case("sh.exe") && script.command.contains("s.sh"),
            "{script:?}"
        );
        let listed = listed(&table, &key);
        assert!(
            listed
                .iter()
                .any(|(pid, command)| *pid == sleep.key.pid && command.contains("sleep.exe")),
            "the host does not name the sleep as the turn's work: {listed:?}"
        );

        // The window and its program go on.
        table.write(&key, b"\r").expect("go on");
        read_until(&table, &key, &output, "ALIVE#");
        table.kill(&key).expect("close the pane");
        let _ = std::fs::remove_dir_all(&folder);
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
        // is, so it is not listed and not named.
        assert_eq!(listed(&table, &key), Vec::new());
        assert!(on_its_own(&held(&table, &key), "-n 1004"), "it runs on");
        // The pane's job is the window's: its close ends it with the rest.
        table.kill(&key).expect("close the pane");
    }
}

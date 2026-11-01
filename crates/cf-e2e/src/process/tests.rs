use super::*;

#[test]
fn a_program_that_cannot_be_started_is_an_error_that_names_it() {
    let failed = Run::new("cf-e2e-no-such-program").run().unwrap_err();
    assert!(matches!(
        &failed,
        Error::Program { action: "start", program, .. } if program == "cf-e2e-no-such-program"
    ));
    assert!(
        failed
            .to_string()
            .starts_with("could not start `cf-e2e-no-such-program`: "),
        "{failed}"
    );
    let failed = Run::new("cf-e2e-no-such-program").spawn().unwrap_err();
    assert!(matches!(
        &failed,
        Error::Program { action: "start", program, .. } if program == "cf-e2e-no-such-program"
    ));
}

/// The rig starts daemons, hosts and stand-ins that run for a whole case, while other cases'
/// threads open sockets and pipes. On macOS std makes each close-on-exec in a second step, and a
/// program started between the two holds it to its end: a pipe another case closed would not be
/// at its end, a listener would go on answering. Here a descriptor is left in that moment while
/// a program starts, both ways the rig starts one, and the program says whether it has it.
#[cfg(target_os = "macos")]
#[test]
fn a_program_a_case_starts_holds_nothing_of_the_rigs_but_its_streams() {
    use std::os::fd::AsRawFd;

    use nix::fcntl::{fcntl, FcntlArg, FdFlag};

    let stray = std::fs::File::open("/dev/null").unwrap();
    fcntl(stray.as_raw_fd(), FcntlArg::F_SETFD(FdFlag::empty())).unwrap();
    let asking = || {
        Run::new("/bin/sh").args([
            "-c".to_owned(),
            format!(
                "if [ -e /dev/fd/{} ]; then echo held; else echo clear; fi",
                stray.as_raw_fd()
            ),
        ])
    };
    let ran = asking().run().unwrap();
    assert_eq!(ran.stdout, "clear\n");
    let mut spawned = asking().closed_input().spawn().unwrap();
    let mut said = String::new();
    std::io::BufRead::read_line(&mut spawned.take_output().unwrap(), &mut said).unwrap();
    spawned.wait(Duration::from_secs(60)).unwrap();
    assert_eq!(said, "clear\n");
}

#[test]
fn windows_is_given_what_it_needs_of_the_tests_where_the_case_names_none_and_others_nothing() {
    let names = |vars: &[(OsString, OsString)], finding_programs| -> Vec<&'static str> {
        required_on_windows(vars, finding_programs)
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    };
    if cfg!(windows) {
        // Every Windows process has a system folder.
        assert!(names(&[], false).contains(&"SYSTEMROOT"));
        // What a case names is its own, in whichever case it writes the name.
        let named = [(OsString::from("SystemRoot"), OsString::from("C:\\x"))];
        assert!(!names(&named, false).contains(&"SYSTEMROOT"));
        // The command interpreter and the list of extensions only where asked.
        assert!(!names(&[], false).contains(&"COMSPEC"));
        assert!(names(&[], true).contains(&"COMSPEC"));
        assert!(names(&[], true).contains(&"PATHEXT"));
    } else {
        assert_eq!(names(&[], false), Vec::<&str>::new());
        assert_eq!(names(&[], true), Vec::<&str>::new());
    }
}

#[test]
fn a_run_says_how_it_ended_and_what_it_printed_and_said() {
    let ran = |code, timed_out| Ran {
        code,
        stdout: "out".into(),
        stderr: "err".into(),
        timed_out,
        held: false,
    };
    assert_eq!(
        ran(Some(3), false).to_string(),
        "exit code 3\nstdout:\nout\nstderr:\nerr"
    );
    let held = Ran {
        held: true,
        ..ran(Some(0), false)
    };
    assert!(held.to_string().starts_with(
        "exit code 0; a process it left held its output open, which was not read to its end\n"
    ));
    assert!(ran(None, false)
        .to_string()
        .starts_with("ended by a signal\n"));
    assert!(ran(None, true)
        .to_string()
        .starts_with("ended for running past its limit\n"));
    assert_eq!(ran(Some(0), false).output(), "outerr");
}

#[test]
fn what_a_program_printed_as_json_is_read_and_what_is_not_is_an_error() {
    let said = |stdout: &str| Ran {
        code: Some(0),
        stdout: stdout.into(),
        stderr: String::new(),
        timed_out: false,
        held: false,
    };
    assert_eq!(said(r#"{"a":[1]}"#).json().unwrap()["a"][0], 1);
    let failed = said("not json").json().unwrap_err();
    assert!(matches!(failed, Error::Json { .. }), "{failed}");
    assert!(failed.to_string().ends_with("\nnot json"), "{failed}");
}

#[test]
fn the_tests_own_variable_is_read_here_and_one_that_is_not_there_is_none() {
    // Cargo sets the manifest's folder for the tests it runs.
    assert_eq!(
        own_var("CARGO_MANIFEST_DIR").as_deref(),
        Some(env!("CARGO_MANIFEST_DIR"))
    );
    assert_eq!(own_var("CF_E2E_NO_SUCH_VARIABLE"), None);
}

/// The variable that makes the test binary the programs the test below starts, and
/// which of them.
const SETTLE: &str = "CF_E2E_SETTLE_TEST";

/// The name the test binary is asked to run, to be those programs.
const SETTLE_PROGRAMS: &str = "process::tests::the_programs_the_settle_test_starts";

/// Not a test of its own: the test binary again, started by the test below as two programs
/// where there is no shell to do it with (on Windows). The first starts the second and ends
/// at once, the second says its piece on the error output they share 300 ms later and ends.
/// Run as a test it does nothing.
#[test]
// The first program is not to wait for the second: it ends first, and that is the case.
#[allow(clippy::zombie_processes)]
fn the_programs_the_settle_test_starts() {
    match own_var(SETTLE).as_deref() {
        Some("first") => {
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", SETTLE_PROGRAMS, "--nocapture"])
                .env(SETTLE, "second")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .spawn()
                .unwrap();
        }
        Some("second") => {
            thread::sleep(Duration::from_millis(300));
            writeln!(std::io::stderr(), "cf: it is refused").unwrap();
        }
        _ => {}
    }
}

#[test]
fn a_spawned_program_that_has_ended_has_said_all_it_said_on_its_error_output_wherever_it_runs() {
    // The daemon's refused second start ended with 1, its sentence on the pipe, and a case
    // that read its error output the moment it was gone found none (2026-10-23). Here the
    // first program ends before the sentence is all in: the second says it a moment later.
    // The case starts one program that writes a line after 300 ms: a verb of cf's time.
    let mut program = super::case_of(std::env::current_exe().unwrap(), SETTLE_PROGRAMS, LIMIT)
        .arg("--nocapture")
        .var(SETTLE, "first")
        .spawn()
        .unwrap();
    let status = program.wait(LIMIT).unwrap();
    assert_eq!(status.and_then(|status| status.code()), Some(0));
    assert!(
        program.errors().contains("cf: it is refused\n"),
        "{:?}",
        program.errors()
    );
}

/// The runs that need a shell to be told what to do.
#[cfg(unix)]
mod shell {
    use std::io::BufRead;
    use std::sync::mpsc;
    use std::time::Instant;

    use super::*;

    fn sh(script: &str) -> Run {
        Run::new("/bin/sh").args(["-c", script])
    }

    #[test]
    fn what_a_program_printed_said_and_exited_with_is_kept() {
        let ran = sh("printf out; printf err >&2; exit 3").run().unwrap();
        assert_eq!(
            ran,
            Ran {
                code: Some(3),
                stdout: "out".into(),
                stderr: "err".into(),
                timed_out: false,
                held: false,
            }
        );
    }

    #[test]
    fn a_program_that_leaves_a_process_holding_its_output_is_read_a_while_and_says_so() {
        // A wrapper says its version and ends, leaving a helper in the background that
        // holds its output: the run reads what it wrote, and goes on a few seconds
        // after its end, not when the helper's does.
        let started = Instant::now();
        let ran = sh("printf '1.2.3\\n'; sleep 20 & exit 0").run().unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(
            (ran.code, ran.stdout.as_str(), ran.held),
            (Some(0), "1.2.3\n", true)
        );
        // One that leaves nothing holding it is read to its end.
        assert!(!sh("printf done").run().unwrap().held);
    }

    #[test]
    fn a_program_is_given_the_variables_it_is_told_and_none_of_the_tests() {
        let probe = r#"printf '%s|%s' "${CF_E2E_A-unset}" "${CARGO_MANIFEST_DIR-unset}""#;
        let alone = sh(probe).var("CF_E2E_A", "1").run().unwrap();
        assert_eq!(alone.stdout, "1|unset");
        // Cargo sets the manifest's folder for the tests it runs.
        let with_the_tests = sh(probe)
            .var("CF_E2E_A", "1")
            .inheriting_env()
            .run()
            .unwrap();
        assert_eq!(
            with_the_tests.stdout,
            format!("1|{}", env!("CARGO_MANIFEST_DIR"))
        );
    }

    #[test]
    fn a_variable_told_twice_has_the_later_value_and_a_told_one_beats_the_tests() {
        let ran = sh(r#"printf '%s|%s' "$A" "$CARGO_MANIFEST_DIR""#)
            .vars([("A", "1"), ("A", "2")])
            .var("CARGO_MANIFEST_DIR", "told")
            .inheriting_env()
            .run()
            .unwrap();
        assert_eq!(ran.stdout, "2|told");
    }

    #[test]
    fn a_program_runs_in_the_folder_it_is_given() {
        let folder = tempfile::tempdir().unwrap();
        let ran = sh("pwd").cwd(folder.path()).run().unwrap();
        let there = std::fs::canonicalize(folder.path()).unwrap();
        assert_eq!(ran.stdout.trim_end(), there.to_string_lossy());
    }

    #[test]
    fn a_program_that_reads_its_input_finds_it_closed() {
        let ran = sh("cat; printf done").run().unwrap();
        assert_eq!((ran.code, ran.stdout.as_str()), (Some(0), "done"));
    }

    #[test]
    fn a_program_given_input_reads_all_of_it_and_then_its_end() {
        let ran = sh("cat; printf done")
            .input("first\nsecond\n")
            .run()
            .unwrap();
        assert_eq!(ran.stdout, "first\nsecond\ndone");
    }

    #[test]
    fn a_program_that_ends_without_reading_its_input_is_no_error() {
        // More than a pipe holds, to a program that reads none of it.
        let ran = sh("exit 4").input(vec![b'x'; 1 << 20]).run().unwrap();
        assert_eq!(ran.code, Some(4));
    }

    #[test]
    fn output_larger_than_a_pipe_holds_does_not_stop_the_program() {
        // Both pipes fill many times over, a line at a time and alternately.
        let script = "i=0; while [ $i -lt 20000 ]; do printf 'xxxxxxxxxxxxxxx\\n'; \
                      printf 'yyyyyyyyyyyyyyy\\n' >&2; i=$((i+1)); done";
        let ran = sh(script).run().unwrap();
        assert_eq!(ran.code, Some(0));
        assert_eq!((ran.stdout.len(), ran.stderr.len()), (320_000, 320_000));
    }

    #[test]
    fn a_program_that_runs_past_its_limit_is_ended_and_says_so() {
        let started = Instant::now();
        let ran = sh("printf begun; exec sleep 30")
            .limit(Duration::from_millis(200))
            .run()
            .unwrap();
        assert_eq!((ran.code, ran.timed_out), (None, true));
        assert_eq!(ran.stdout, "begun");
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn a_program_ended_by_a_signal_has_no_code() {
        let ran = sh("kill -9 $$").run().unwrap();
        assert_eq!((ran.code, ran.timed_out), (None, false));
    }

    #[test]
    fn a_spawned_program_is_written_to_and_read_from_while_it_runs() {
        let mut program =
            sh("read a; printf 'got %s\\n' \"$a\"; read b; printf 'got %s\\n' \"$b\"")
                .spawn()
                .unwrap();
        let mut lines = program.take_output().unwrap();
        assert!(program.take_output().is_none(), "the output is given once");
        let mut line = String::new();
        program.send("one\n");
        std::io::BufRead::read_line(&mut lines, &mut line).unwrap();
        assert_eq!(line, "got one\n");
        line.clear();
        program.send("two\n");
        program.end_input();
        std::io::BufRead::read_line(&mut lines, &mut line).unwrap();
        assert_eq!(line, "got two\n");
        let status = program.wait(Duration::from_secs(10)).unwrap();
        assert_eq!(status.and_then(|status| status.code()), Some(0));
    }

    #[test]
    fn a_spawned_programs_input_is_ended_after_everything_queued_is_written() {
        let mut program = sh("cat >&2").spawn().unwrap();
        program.send("queued ");
        program.send("twice");
        program.end_input();
        let status = program.wait(Duration::from_secs(10)).unwrap();
        assert_eq!(status.and_then(|status| status.code()), Some(0));
        // What it wrote is read on a thread of its own, which may not have run yet when the
        // program has ended: the wait has waited for it to the end of the stream (T-643, on
        // merge-flakes' settle).
        assert_eq!(program.errors(), "queued twice");
    }

    #[test]
    fn a_spawned_program_that_has_ended_has_said_all_it_said_on_its_error_output() {
        // The daemon's refused second start ended with 1, its sentence on the pipe, and
        // a case that read its error output the moment it was gone found none, the
        // thread that keeps it not having run yet (2026-10-23). Here the program ends
        // before the sentence is all in: a helper it left says it a moment later.
        let mut program = sh("(sleep 0.3; printf 'cf: it is refused\\n' >&2) & exit 0")
            .spawn()
            .unwrap();
        let status = program.wait(Duration::from_secs(10)).unwrap();
        assert_eq!(status.and_then(|status| status.code()), Some(0));
        assert_eq!(program.errors(), "cf: it is refused\n");
    }

    #[test]
    fn a_spawned_programs_status_is_looked_at_and_not_waited_for_though_a_process_it_left_holds_its_error_output(
    ) {
        let mut program = sh("printf 'said\\n' >&2; sleep 20 & exit 0")
            .spawn()
            .unwrap();
        let started = Instant::now();
        let status = loop {
            if let Some(status) = program.status() {
                break status;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "it never ended"
            );
            thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(status.code(), Some(0));
        assert!(program.has_exited());
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_spawned_program_that_left_a_process_holding_its_error_output_is_waited_for_a_while() {
        let started = Instant::now();
        let mut program = sh("printf 'said\\n' >&2; sleep 20 & exit 0")
            .spawn()
            .unwrap();
        let status = program.wait(Duration::from_secs(10)).unwrap();
        assert_eq!(status.and_then(|status| status.code()), Some(0));
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(program.errors(), "said\n");
    }

    #[test]
    fn a_spawned_program_is_given_the_input_it_was_told_first_and_its_error_output_is_kept() {
        let mut program = sh("read a; printf 'said %s' \"$a\" >&2; sleep 30")
            .input("first\n")
            .spawn()
            .unwrap();
        for _ in 0..500 {
            if !program.errors().is_empty() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(program.errors(), "said first");
        assert!(!program.has_exited(), "its input is left open");
        assert_eq!(program.wait(Duration::from_millis(50)).unwrap(), None);
    }

    #[test]
    fn a_spawned_program_with_a_closed_input_reads_its_end_at_once() {
        let mut program = sh("cat; printf done").closed_input().spawn().unwrap();
        let mut output = program.take_output().unwrap();
        let mut text = String::new();
        output.read_to_string(&mut text).unwrap();
        assert_eq!(text, "done");
    }

    #[test]
    fn a_spawned_program_is_signalled_and_waited_for_and_one_that_is_gone_is_no_error() {
        let mut program = sh("exec sleep 30").spawn().unwrap();
        assert!(is_alive(program.id()));
        program.signal(Signal::Terminate);
        let status = program.wait(Duration::from_secs(10)).unwrap();
        assert_eq!(
            status.and_then(|status| status.code()),
            None,
            "a signal ended it"
        );
        // Waited for, so reaped, so gone.
        assert!(!is_alive(program.id()));
        program.signal(Signal::Kill);
        program.kill();
    }

    #[test]
    fn a_stopped_program_answers_what_it_was_sent_meanwhile_once_it_is_continued() {
        let mut program = sh("exec cat").spawn().unwrap();
        let output = program.take_output().unwrap();
        let (lines, heard) = mpsc::channel();
        thread::spawn(move || {
            for line in output.lines().map_while(|line| line.ok()) {
                let _ = lines.send(line);
            }
        });
        program.send("before\n");
        assert_eq!(
            heard.recv_timeout(Duration::from_secs(10)).unwrap(),
            "before"
        );
        program.signal(Signal::Stop);
        program.send("meanwhile\n");
        assert!(
            heard.recv_timeout(Duration::from_millis(300)).is_err(),
            "a stopped program answered"
        );
        program.signal(Signal::Continue);
        assert_eq!(
            heard.recv_timeout(Duration::from_secs(10)).unwrap(),
            "meanwhile"
        );
    }

    #[test]
    fn a_stalled_program_is_stopped_at_once_and_goes_on_by_itself_after_its_span() {
        let mut program = sh("exec cat").spawn().unwrap();
        let output = program.take_output().unwrap();
        let (lines, heard) = mpsc::channel();
        thread::spawn(move || {
            for line in output.lines().map_while(|line| line.ok()) {
                let _ = lines.send(line);
            }
        });
        let span = Duration::from_millis(500);
        let stalled = Instant::now();
        stall(program.id(), span).unwrap();
        program.send("meanwhile\n");
        assert_eq!(
            heard.recv_timeout(Duration::from_secs(10)).unwrap(),
            "meanwhile"
        );
        assert!(
            stalled.elapsed() >= span,
            "answered after {:?}, in a stall of {span:?}",
            stalled.elapsed()
        );
        // And it runs as before once it has gone on.
        program.send("after\n");
        assert_eq!(
            heard.recv_timeout(Duration::from_secs(10)).unwrap(),
            "after"
        );
        // A number that is no process id is refused, and nothing is signalled for it.
        assert!(stall(u32::MAX, span).is_err());
    }

    #[test]
    fn a_spawned_program_is_ended_when_it_goes_out_of_scope() {
        let program = sh("exec sleep 30").spawn().unwrap();
        let id = program.id();
        assert!(is_alive(id));
        drop(program);
        assert!(!is_alive(id));
    }

    #[test]
    fn a_process_by_its_id_is_alive_until_it_is_gone_and_can_be_signalled() {
        let mut program = sh("exec sleep 30").spawn().unwrap();
        let id = program.id();
        assert!(is_alive(id));
        // Ended by a TERM: a run started in the background of a script (a gate run by
        // `nohup … &`) has its interrupts ignored, and so has every program under it.
        signal(id, Signal::Terminate).unwrap();
        assert!(
            program.wait(Duration::from_secs(10)).unwrap().is_some(),
            "the signal did not end it"
        );
        assert!(!is_alive(id));
        assert!(signal(id, Signal::Terminate).is_err(), "nothing to signal");
        // No such process id at all.
        assert!(!is_alive(u32::MAX));
    }

    #[test]
    fn a_process_began_when_the_system_says_and_a_process_that_is_gone_began_never() {
        let mut program = sh("exec sleep 30").spawn().unwrap();
        let id = program.id();
        let began = started(id).expect("the system says when it began");
        assert_eq!(
            started(id),
            Some(began.clone()),
            "the same one, the same word"
        );
        // Another process is another word, whatever its number is, and none is none.
        let mut other = sh("exec sleep 31").spawn().unwrap();
        assert_ne!(started(other.id()), None);
        assert_eq!(started(u32::MAX), None);
        program.kill();
        other.kill();
        assert_eq!(started(id), None, "it is gone: it began no more");
    }

    #[test]
    fn the_group_of_a_process_is_the_one_it_was_started_in_and_one_that_is_gone_has_none() {
        use std::os::unix::process::CommandExt;

        // Both sleep beside the other tests: what they were left of theirs they would hold.
        let sleep = || {
            let mut sleep = Command::new("sleep");
            sleep.arg("30");
            cf_process::hold_only_the_streams(&mut sleep);
            sleep
        };
        let mut leader = sleep().process_group(0).spawn().unwrap();
        let mut follower = sleep().spawn().unwrap();
        assert_eq!(group_of(leader.id()), Some(leader.id()), "it leads its own");
        assert_eq!(
            group_of(follower.id()),
            group_of(std::process::id()),
            "the other is in the group of the process that started it"
        );
        leader.kill().unwrap();
        leader.wait().unwrap();
        follower.kill().unwrap();
        follower.wait().unwrap();
        assert_eq!(group_of(leader.id()), None);
        assert_eq!(group_of(u32::MAX), None, "no such process id at all");
    }

    #[test]
    fn a_run_told_to_have_a_group_of_its_own_leads_one_and_one_not_told_is_in_the_tests() {
        let own = sh("sleep 30").own_group().closed_input().spawn().unwrap();
        let shared = sh("sleep 30").closed_input().spawn().unwrap();
        assert_eq!(group_of(own.id()), Some(own.id()));
        assert_eq!(group_of(shared.id()), group_of(std::process::id()));
    }
}

/// An app of the live app's identity, in a bundle of `identity` under `root`, whose program
/// leaves a file when it runs: the program, and the file.
#[cfg(unix)]
fn app_of(root: &std::path::Path, identity: &str) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let app = root.join(identity).join("ConsensFlow.app");
    let macos = app.join("Contents").join("MacOS");
    std::fs::create_dir_all(&macos).unwrap();
    std::fs::write(
        app.join("Contents").join("Info.plist"),
        format!(
            "<plist><dict><key>CFBundleIdentifier</key><string>{identity}</string></dict></plist>"
        ),
    )
    .unwrap();
    let ran = root.join(format!("{identity}.ran"));
    let program = macos.join("app");
    std::fs::write(&program, format!("#!/bin/sh\ntouch '{}'\n", ran.display())).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    (program, ran)
}

/// No case starts the program of an app of the live app's identity, either way it starts one,
/// and one of the smoke's is started as any program is (2026-10-28: a copy of the app of that
/// identity came to the front over the live app).
#[cfg(unix)]
#[test]
fn no_case_starts_an_app_of_the_live_apps_identity() {
    let root = tempfile::tempdir().unwrap();
    let (live, ran) = app_of(root.path(), "dev.ngvoicu.consensflow");
    let refused = Run::new(&live).run().unwrap_err().to_string();
    assert!(refused.contains("the live app's identity"), "{refused}");
    let refused = Run::new(&live)
        .spawn()
        .err()
        .map(|it| it.to_string())
        .unwrap_or_default();
    assert!(refused.contains("the live app's identity"), "{refused}");
    assert!(!ran.exists(), "the live app's program ran");
    let (smoke, ran) = app_of(root.path(), "dev.ngvoicu.consensflow.smoke");
    assert_eq!(Run::new(&smoke).run().unwrap().code, Some(0));
    assert!(ran.exists(), "the smoke's program did not run");
    // A program that reaches apps by its words, told to reach the live app: never started.
    let (osascript, ran) = reaching_stand_in(root.path(), "osascript");
    for refused in [
        Run::new(&osascript)
            .args([
                "-e",
                "tell application id \"dev.ngvoicu.consensflow\" to quit",
            ])
            .run()
            .err(),
        Run::new(&osascript)
            .args([
                "-e",
                "tell application id \"dev.ngvoicu.consensflow\" to quit",
            ])
            .spawn()
            .err(),
    ] {
        let refused = refused.map(|it| it.to_string()).unwrap_or_default();
        assert!(
            refused.contains("a test reaches no app but the smoke's"),
            "{refused}"
        );
    }
    assert!(!ran.exists(), "the stand-in for osascript ran");
}

/// A stand-in named `name` in `folder`, which leaves `ran` when it runs: what a program that
/// reaches apps by its words would do, done harmlessly, should a refusal fail to come.
#[cfg(unix)]
fn reaching_stand_in(
    folder: &std::path::Path,
    name: &str,
) -> (std::path::PathBuf, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let ran = folder.join(format!("{name}.ran"));
    let program = folder.join(name);
    std::fs::write(&program, format!("#!/bin/sh\ntouch '{}'\n", ran.display())).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    (program, ran)
}

/// The bundle the release ships is started only with leave for it ([`crate::shipped`]), and
/// leave for one bundle is none for another.
#[cfg(unix)]
#[test]
fn the_shipped_bundle_is_started_only_with_leave_for_it() {
    let root = tempfile::tempdir().unwrap();
    let (live, ran) = app_of(root.path(), "dev.ngvoicu.consensflow");
    let app = cf_base::bundle::bundle_of(&live).unwrap();
    let elsewhere = crate::shipped::Shipped::for_a_test(&root.path().join("Other.app"));
    let refused = Run::new(&live)
        .shipped(&elsewhere)
        .run()
        .unwrap_err()
        .to_string();
    assert!(refused.contains("the live app's identity"), "{refused}");
    assert!(!ran.exists());
    let leave = crate::shipped::Shipped::for_a_test(&app);
    assert_eq!(Run::new(&live).shipped(&leave).run().unwrap().code, Some(0));
    assert!(
        ran.exists(),
        "the shipped bundle was not started with leave for it"
    );
}

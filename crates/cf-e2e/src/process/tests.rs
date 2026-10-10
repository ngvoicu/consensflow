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
    };
    assert_eq!(
        ran(Some(3), false).to_string(),
        "exit code 3\nstdout:\nout\nstderr:\nerr"
    );
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

/// The runs that need a shell to be told what to do.
#[cfg(unix)]
mod shell {
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
            }
        );
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
        assert_eq!(program.errors(), "queued twice");
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
        signal(id, Signal::Interrupt).unwrap();
        program.wait(Duration::from_secs(10)).unwrap();
        assert!(!is_alive(id));
        assert!(signal(id, Signal::Terminate).is_err(), "nothing to signal");
        // No such process id at all.
        assert!(!is_alive(u32::MAX));
    }
}

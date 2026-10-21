use super::*;

fn words(line: &str) -> Vec<String> {
    line.split(' ')
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_job_is_a_number_of_seconds_and_a_file_and_may_be_deaf() {
    assert_eq!(
        Job::parse(&words("12 /jobs/job-1-1")),
        Some(Job {
            seconds: 12,
            file: PathBuf::from("/jobs/job-1-1"),
            deaf: false,
            group: Group::Own,
            browser: None,
        })
    );
    assert_eq!(
        Job::parse(&words("120 /jobs/job-1-2 --deaf")),
        Some(Job {
            seconds: 120,
            file: PathBuf::from("/jobs/job-1-2"),
            deaf: true,
            group: Group::Own,
            browser: None,
        })
    );
}

#[test]
fn a_job_that_launches_a_browser_is_told_its_file_in_any_order_with_the_other_words() {
    for line in [
        "600 /jobs/server-1-1 --in-group --browser /jobs/browser-1-1",
        "600 /jobs/server-1-1 --browser /jobs/browser-1-1 --in-group",
    ] {
        let job = Job::parse(&words(line)).unwrap();
        assert_eq!(
            job.browser,
            Some(PathBuf::from("/jobs/browser-1-1")),
            "{line}"
        );
        assert_eq!(job.group, Group::Window, "{line}");
    }
    assert_eq!(
        Job::parse(&words("600 /jobs/job-1-1")).unwrap().browser,
        None
    );
}

#[test]
fn a_job_is_in_a_group_of_its_own_unless_it_is_told_to_stay_in_its_windows() {
    for line in [
        "600 /jobs/server-1-1 --in-group",
        "600 /jobs/server-1-1 --in-group --deaf",
        "600 /jobs/server-1-1 --deaf --in-group",
    ] {
        let job = Job::parse(&words(line)).unwrap();
        assert_eq!(job.group, Group::Window, "{line}");
        assert_eq!(job.deaf, line.contains("--deaf"), "{line}");
    }
    assert_eq!(
        Job::parse(&words("600 /jobs/job-1-1")).unwrap().group,
        Group::Own
    );
}

#[test]
fn anything_else_is_no_job() {
    for line in [
        "",
        "12",
        "soon /jobs/a",
        "12 /jobs/a --loud",
        "12 /jobs/a --deaf more",
        "12 /jobs/a --deaf --deaf",
        "12 /jobs/a --in-group --in-group",
        "12 /jobs/a --in-group --deaf more",
        "12 /jobs/a --browser",
        "12 /jobs/a --browser /jobs/b --browser /jobs/c",
    ] {
        assert_eq!(Job::parse(&words(line)), None, "{line:?}");
    }
}

#[test]
fn a_harness_starts_a_server_with_its_window_after_its_first_message_or_not_at_all() {
    assert_eq!(Server::of(None), Server::Never);
    assert_eq!(Server::of(Some("")), Server::Never);
    // Any other word is the server the window has before any turn.
    assert_eq!(Server::of(Some("1")), Server::WithTheWindow);
    assert_eq!(Server::of(Some("yes")), Server::WithTheWindow);
    assert_eq!(Server::of(Some("late")), Server::AfterTheFirstMessage);
    assert_eq!(Server::of(Some("apart")), Server::ApartWithTheWindow);
}

#[test]
fn what_a_job_writes_is_its_file_and_a_kind_after_a_dot() {
    let job = Job::parse(&words("1 /jobs/job-1-1")).unwrap();
    assert_eq!(job.beside("pid"), PathBuf::from("/jobs/job-1-1.pid"));
    // A name with a dot of its own keeps it.
    let job = Job::parse(&words("1 /jobs/a.b")).unwrap();
    assert_eq!(job.beside("released"), PathBuf::from("/jobs/a.b.released"));
}

#[test]
fn the_jobs_of_a_window_are_in_a_folder_beside_its_home() {
    let jobs = Jobs::new(Path::new("/root/consensflow"), 4242);
    assert_eq!(jobs.folder, PathBuf::from("/root/jobs"));
    assert_eq!(jobs.window, 4242);
}

#[test]
fn a_command_of_the_agents_is_run_by_a_powershell_that_is_told_it_in_single_quotes() {
    let words: Vec<OsString> = ["--background-job", "600", r"C:\jobs\it's a job-1-1"]
        .into_iter()
        .map(Into::into)
        .collect();
    let program = Path::new(r"C:\Program Files\x\fake-agent.exe");
    assert_eq!(
        powershell_script(program, &words, false),
        r"& 'C:\Program Files\x\fake-agent.exe' '--background-job' '600' 'C:\jobs\it''s a job-1-1'"
    );
    // Detached, it is started and the shell ends; each word is quoted for the
    // command line that is made of them.
    assert_eq!(
        powershell_script(program, &words, true),
        r#"Start-Process -WindowStyle Hidden -FilePath 'C:\Program Files\x\fake-agent.exe' -ArgumentList '"--background-job"','"600"','"C:\jobs\it''s a job-1-1"'"#
    );
}

#[test]
fn a_command_of_the_agents_is_run_by_a_shell_that_waits_for_it_or_detaches_it() {
    // The shell is no `exec`: it stays the job's parent while the job runs.
    assert_eq!(shell_script(false), "\"$0\" \"$@\"; exit $?");
    // Detached, it ends a moment after the job is started, and the job has no marker
    // of the window's to be found by.
    assert_eq!(
        shell_script(true),
        "unset CF_PANE_PROCESS_OWNER; nohup \"$0\" \"$@\" >/dev/null 2>&1 & sleep 0.3"
    );
    // One that `exec`s it is the job from then on.
    assert_eq!(IN_PLACE, "exec \"$0\" \"$@\"");
}

#[cfg(unix)]
#[test]
fn a_shell_for_a_session_of_its_own_is_told_the_program_it_becomes() {
    // (A program that is named is run in place of this one, in a session of its own:
    // the rig's cases that detach a job are what run it.)
    assert_eq!(
        format!("{:?}", run_in_session(&[])),
        format!("{:?}", ExitCode::FAILURE)
    );
}

#[test]
fn a_server_launches_a_browser_where_the_variable_says_any_word() {
    let launches = |word| {
        Jobs::new(Path::new("/root/consensflow"), 1)
            .launching_a_browser(word)
            .browser
    };
    assert!(!launches(None));
    assert!(!launches(Some("")));
    assert!(launches(Some("1")));
}

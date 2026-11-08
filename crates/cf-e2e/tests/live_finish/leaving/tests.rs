//! What the step that leaves a command running is made of, which needs no harness and no
//! daemon: the verdict is held to what it must say of a worker's run as it was seen, and the
//! brief and the project are the ones the run is given.

use cf_e2e::live::Harness;

use super::*;

fn proc(pid: u64, command: &str) -> Proc {
    Proc {
        pid,
        command: command.to_owned(),
    }
}

const CLOSED: &str =
    "T-1's window closed with background work running, which its close ended: sleep 4711";

/// A worker whose command was listed, ended, and told to the chief.
fn good() -> Left {
    Left {
        state: "done".to_owned(),
        took: Duration::from_secs(20),
        listed: vec![proc(41, "sleep 4711")],
        result: Some("I started it.".to_owned()),
        finished: false,
        notes: vec![CLOSED.to_owned()],
        alive: Vec::new(),
        escaped: Vec::new(),
        after_close: None,
    }
}

#[test]
fn a_task_done_with_the_command_listed_gone_and_the_chief_told_of_the_close_passes() {
    assert_eq!(judge(&good(), Expect::Ended), Ok(()));
    assert_eq!(
        summary(&good(), &judge(&good(), Expect::Ended)),
        "T-1 done after 20.0 s; the command (41) was ended by the window's close and the chief told: T-1's window closed with background work running, which its close ended: sleep 4711"
    );
}

#[test]
fn a_command_the_harness_ended_with_its_tool_call_leaves_nothing_to_end_and_passes() {
    let mut left = good();
    left.listed.clear();
    left.notes.clear();
    assert_eq!(judge(&left, Expect::Ended), Ok(()));
    assert_eq!(
        summary(&left, &judge(&left, Expect::Ended)),
        "T-1 done after 20.0 s; the harness ended the command with its tool call: the host never listed it, and nothing of it runs on the machine"
    );
}

#[test]
fn a_command_the_host_never_listed_that_still_runs_on_the_machine_escaped_and_fails_and_says_what_the_close_did(
) {
    let mut left = good();
    left.listed.clear();
    left.notes.clear();
    left.escaped = vec!["4242 sleep 4711".to_owned()];
    let problems = judge(&left, Expect::Ended).unwrap_err();
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert!(
        problems[0].starts_with("the command escaped the ending")
            && problems[0].contains("no chain of processes that run led from it")
            && problems[0]
                .ends_with("4242 sleep 4711; the window had not closed when the run last looked"),
        "{}",
        problems[0]
    );
    // The window's close found it, and ended it unsaid: the chief was never told.
    left.after_close = Some(Vec::new());
    let problems = judge(&left, Expect::Ended).unwrap_err();
    assert!(
        problems[0].ends_with("4242 sleep 4711; the window's close ended it, unsaid"),
        "{}",
        problems[0]
    );
    // Or it found nothing of it, and it runs on.
    left.after_close = Some(vec!["4242 sleep 4711".to_owned()]);
    let problems = judge(&left, Expect::Ended).unwrap_err();
    assert!(
        problems[0].ends_with(
            "it still runs after the window closed, which found nothing of it: 4242 sleep 4711"
        ),
        "{}",
        problems[0]
    );
}

#[test]
fn the_command_of_a_harness_is_found_by_the_command_and_the_stand_ins_job_by_its_words() {
    // A harness's is the command itself, as this machine's shells know it: on Windows a
    // ping, whose seconds are not its last word (zeewin, round 7). The rig's stand-in starts
    // a job of its own, whose command line holds the seconds too.
    let harness = if cfg!(windows) {
        format!("ping -n {SECONDS} 127.0.0.1")
    } else {
        format!("sleep {SECONDS}")
    };
    assert_eq!(wanted(Who::Harness(Harness::Pi)), harness);
    assert_eq!(wanted(Who::StandIn), format!("--background-job {SECONDS}"));
}

/// A process `pid`, begun at `started`, started by `ppid`.
fn running(pid: u32, started: u64, ppid: u32, command: &str) -> Running {
    Running {
        identity: Identity { pid, started },
        ppid,
        command: command.to_owned(),
        exe: None,
    }
}

#[test]
fn what_the_run_ends_is_what_the_window_ran_of_the_command_and_the_stand_ins_own_jobs_and_nothing_else_that_says_it(
) {
    // A check that ended every process whose command line held its mark ended the worker
    // that ran it (2026-10-16): the run ends what it saw of the command in the worker's
    // window, and the jobs the rig's stand-in wrote it started, and nothing else.
    let window = Identity {
        pid: 100,
        started: 10,
    };
    let pids =
        |chosen: &[Running]| -> Vec<u32> { chosen.iter().map(|one| one.identity.pid).collect() };
    // A harness: the shell its tool ran the command in, the command, and what that started;
    // and outside the window a process whose command line says the same.
    let all = vec![
        running(100, 10, 1, "devin --config c"),
        running(101, 11, 100, "/bin/sh -c sleep 4711"),
        running(102, 12, 101, "sleep 4711"),
        running(103, 13, 102, "helper"),
        running(300, 5, 1, "claude -p Start the command `sleep 4711`"),
    ];
    let mut chosen = Chosen::default();
    chosen.choose(&all, Some(window), &[], "sleep 4711");
    assert_eq!(pids(&chosen.processes), [101, 102, 103]);
    // Each once, however often the run looks; and a window that has gone adds nothing.
    chosen.choose(&all, Some(window), &[], "sleep 4711");
    chosen.choose(&all, None, &[], "sleep 4711");
    assert_eq!(pids(&chosen.processes), [101, 102, 103]);
}

#[test]
fn a_stand_in_job_is_taken_by_its_own_file_at_the_first_look_and_its_number_never_again() {
    // A killed job leaves its number written, and a process of another suite's that took the
    // number and said the job's words was taken for it, and ended (Astra and Sol on round 6).
    // A job is the process of its number that holds its file, which is the rig's own.
    let wanted = "--background-job 4711";
    let job = |pid: u32, n: u32| (pid, format!("/rig/jobs/job-100-{n}"));
    let jobs = [job(200, 1), job(201, 2), job(202, 3)];
    let all = vec![
        running(100, 10, 1, "fake-agent"),
        // The job the stand-in detached, which has no window to descend from.
        running(
            200,
            14,
            1,
            "fake-agent --background-job 4711 /rig/jobs/job-100-1",
        ),
        // The number a job wrote is another program's now, and another rig's job's.
        running(201, 20, 1, "editor"),
        running(
            202,
            21,
            1,
            "fake-agent --background-job 4711 /other/jobs/job-7-1",
        ),
    ];
    let pids = |chosen: &Chosen| -> Vec<u32> {
        chosen
            .processes
            .iter()
            .map(|one| one.identity.pid)
            .collect()
    };
    let mut chosen = Chosen::default();
    chosen.choose(&all, None, &jobs, wanted);
    assert_eq!(pids(&chosen), [200]);
    // Later, 200's job has gone and another process has its number, the job's file in its
    // words: the job was taken once, as the process it was, and nothing is taken for it
    // again. The job of 201's file, which the first look did not find (it is the one now),
    // is taken now.
    let later = vec![
        running(
            200,
            99,
            1,
            "fake-agent --background-job 4711 /rig/jobs/job-100-1",
        ),
        running(
            201,
            98,
            1,
            "fake-agent --background-job 4711 /rig/jobs/job-100-2",
        ),
    ];
    chosen.choose(&later, None, &jobs, wanted);
    assert_eq!(
        chosen
            .processes
            .iter()
            .map(|one| one.identity)
            .collect::<Vec<_>>(),
        [
            Identity {
                pid: 200,
                started: 14
            },
            Identity {
                pid: 201,
                started: 98
            }
        ]
    );
}

#[test]
fn what_could_not_be_ended_or_looked_for_is_a_problem_of_the_run() {
    let mut chosen = Chosen::default();
    assert!(chosen.problems(&Ended::default()).is_empty());
    chosen.failed.push("ps failed".to_owned());
    chosen.failed.push("ps failed again".to_owned());
    let unended = Ended {
        failed: vec![(
            running(200, 14, 1, "sleep 4711"),
            "its end failed: refused".to_owned(),
        )],
        ..Ended::default()
    };
    assert_eq!(
        chosen.problems(&unended),
        [
            "the command: process 200 (sleep 4711) may still run: its end failed: refused",
            "the machine's processes could not be read at 2 look(s) while the task was watched, and what the window ran then may still run: ps failed",
        ]
    );
}

#[test]
fn what_a_task_left_is_looked_for_whatever_came_of_it() {
    // Leftovers were looked for only once the task was done: one that failed or ran out of
    // its time left its command unsaid (Astra on round 6).
    for state in ["done", "failed", "working"] {
        let mut left = good();
        left.state = state.to_owned();
        left.alive.clear();
        look_for_leftovers(
            &mut left,
            |listed| listed.iter().map(|process| process.pid).collect(),
            || vec!["4242 sleep 4711".to_owned()],
        );
        assert_eq!(left.escaped, ["4242 sleep 4711"], "{state}");
        // A task done waited for what the host listed to be gone; the others are looked at now.
        let alive: &[u64] = if state == "done" { &[] } else { &[41] };
        assert_eq!(left.alive, alive, "{state}");
    }
}

#[test]
fn what_a_task_that_was_not_done_left_running_is_a_problem_too() {
    // Leftovers were looked for only once the task was done: one that failed or ran out of
    // its time left its command unsaid (Astra on round 6).
    let mut left = good();
    left.state = "failed".to_owned();
    left.alive = vec![41];
    let problems = judge(&left, Expect::Ended).unwrap_err();
    assert!(
        problems.contains(
            &"the command is still running when the run last looked, T-1 being failed: 41 sleep 4711"
                .to_owned()
        ),
        "{problems:#?}"
    );
    let mut left = good();
    left.state = "working".to_owned();
    left.listed.clear();
    left.notes.clear();
    left.escaped = vec!["4242 sleep 4711".to_owned()];
    let problems = judge(&left, Expect::Ended).unwrap_err();
    assert!(
        problems.iter().any(|problem| problem.starts_with(
            "the command escaped the ending: the pane host never listed it, and it still ran when the run last looked, T-1 being working"
        )),
        "{problems:#?}"
    );
}

#[test]
fn a_command_whose_shell_the_host_listed_before_it_ended_and_whose_job_ran_on_apart_escaped_all_the_same(
) {
    // The host looks once a second, and the shell a detached job leaves lingers a third of one: a
    // look may or may not fall in it (CI run 42425306008, `checks (macos-latest)`). Where it did,
    // the host listed a process (the shell, which holds the job's words) and the shell ended, so
    // that nothing listed was still there; the job it detached ran on in a session of its own,
    // which the host never listed and the close did not find. That is the escape, and the verdict
    // says it as it does where no look fell in the shell.
    let mut left = good();
    left.listed = vec![proc(
        4241,
        "/bin/sh -c unset CF_PANE_PROCESS_OWNER; nohup \"$0\" \"$@\" & sleep 0.3 fake-agent --background-job 4711",
    )];
    left.notes.clear();
    left.escaped = vec![
        "4242 fake-agent --background-job 4711 (parent 1, group 4242, terminal ??)".to_owned(),
    ];
    left.after_close = Some(left.escaped.clone());
    let problems = judge(&left, Expect::Ended).unwrap_err();
    assert!(
        problems.iter().any(
            |problem| problem.starts_with("the command escaped the ending")
                && problem.contains("4242 fake-agent --background-job 4711")
                && problem
                    .contains("it still runs after the window closed, which found nothing of it")
        ),
        "{problems:#?}"
    );
    // The chief was told nothing of the work the window closed with, which is true as well.
    assert!(
        problems
            .contains(&"the chief was told nothing of the work the window closed with".to_owned()),
        "{problems:#?}"
    );
    // A process the host listed that still ran is said once, as it was: not again as an escape.
    let mut left = good();
    left.notes.clear();
    left.listed = vec![proc(4242, "sleep 4711")];
    left.alive = vec![4242];
    left.escaped = vec!["4242 sleep 4711 (parent 1, group 4242, terminal ??)".to_owned()];
    left.after_close = Some(left.escaped.clone());
    let problems = judge(&left, Expect::Ended).unwrap_err();
    assert!(
        problems
            .iter()
            .all(|problem| !problem.starts_with("the command escaped the ending")),
        "{problems:#?}"
    );
    // And a listed command the close ended, with nothing left on the machine, is no escape.
    assert_eq!(judge(&good(), Expect::Ended), Ok(()));
}

#[test]
fn a_command_that_ran_on_is_said_to_have_been_found_by_the_close_or_not_and_only_where_it_ran_on() {
    let mut left = good();
    assert_eq!(after_the_close(&left), None, "nothing ran on");
    left.escaped = vec!["4242 sleep 4711".to_owned()];
    assert_eq!(
        after_the_close(&left).as_deref(),
        Some("the window had not closed when the run last looked")
    );
    left.after_close = Some(Vec::new());
    assert_eq!(
        after_the_close(&left).as_deref(),
        Some("the window's close ended it, unsaid")
    );
    left.after_close = Some(left.escaped.clone());
    assert_eq!(
        after_the_close(&left).as_deref(),
        Some("it still runs after the window closed, which found nothing of it: 4242 sleep 4711")
    );
    // A command the host listed while its shell ran, which ran on, is said the same.
    left.listed = vec![proc(4242, "sleep 4711")];
    left.alive = vec![4242];
    let problems = judge(&left, Expect::Ended).unwrap_err();
    assert!(
        problems[0].starts_with("the command is still running after the task was done and the chief told: 4242 sleep 4711; it still runs after the window closed"),
        "{problems:#?}"
    );
}

#[test]
fn a_process_is_said_with_its_command_and_where_it_stands_by_the_process_that_started_it_its_group_and_its_terminal(
) {
    assert_eq!(
        standing("86330     1 86301 ??       sleep 4711").as_deref(),
        Some("86330 sleep 4711 (parent 1, group 86301, terminal ??)"),
        "a session of its own has no terminal"
    );
    assert_eq!(
        standing("51 4 50 ttys003  /bin/zsh   -c  sleep 4711").as_deref(),
        Some("51 /bin/zsh -c sleep 4711 (parent 4, group 50, terminal ttys003)"),
        "the window's terminal, the command on one line"
    );
    assert_eq!(standing("51 4 50 ??"), None, "a line that says no command");
    assert_eq!(standing(""), None);
}

#[test]
fn a_command_that_escaped_with_no_terminal_is_said_to_be_in_a_session_of_its_own() {
    let mut left = good();
    left.listed.clear();
    left.notes.clear();
    left.escaped = vec![standing("86330 1 86301 ?? sleep 4711").unwrap()];
    let said = judge(&left, Expect::Ended).unwrap_err().remove(0);
    assert!(
        said.contains(
            "86330 sleep 4711 (parent 1, group 86301, terminal ??); the window had not closed when the run last looked (terminal ??: it has none, so it is in a session of its own"
        ),
        "{said}"
    );
    // One that has a terminal is not said so, nor is one a Windows machine lists.
    left.escaped = vec![standing("86330 1 86301 ttys004 sleep 4711").unwrap()];
    assert!(!judge(&left, Expect::Ended).unwrap_err()[0].contains("session of its own"));
    left.escaped = vec!["4242 sleep 4711".to_owned()];
    assert!(!judge(&left, Expect::Ended).unwrap_err()[0].contains("session of its own"));
}

#[test]
fn a_command_still_running_once_the_chief_was_told_is_named_with_its_command_line() {
    let mut left = good();
    left.listed.push(proc(42, "/bin/zsh -c sleep 4711"));
    left.alive = vec![42];
    let problems = judge(&left, Expect::Ended).unwrap_err();
    assert_eq!(
        problems,
        ["the command is still running after the task was done and the chief told: 42 /bin/zsh -c sleep 4711"]
    );
}

#[test]
fn a_chief_told_nothing_of_the_close_is_a_run_that_fails() {
    let mut left = good();
    left.notes.clear();
    assert_eq!(
        judge(&left, Expect::Ended).unwrap_err(),
        ["the chief was told nothing of the work the window closed with"]
    );
}

#[test]
fn a_task_that_did_not_end_or_gave_no_result_says_so_first() {
    let left = Left {
        state: "working".to_owned(),
        took: Duration::from_secs(300),
        ..Left::default()
    };
    let problems = judge(&left, Expect::Ended).unwrap_err();
    assert_eq!(problems[0], "T-1 is working, not done after 300.0 s");
    assert_eq!(problems[1], "the chief was given no result");
    let mut empty = good();
    empty.result = Some("  \n".to_owned());
    assert_eq!(
        judge(&empty, Expect::Ended).unwrap_err(),
        ["the result the chief was given is empty"]
    );
}

#[test]
fn a_harness_is_asked_to_start_the_command_in_the_background_and_answer_and_the_stand_in_to_leave_a_job(
) {
    let asked = brief(Who::Harness(Harness::Devin));
    assert!(
        asked.starts_with("Start the command `")
            && asked.contains(&format!(" {SECONDS}"))
            && asked.contains("in the background")
            && asked.contains("Do not wait for it")
            && asked.contains("reply with one line"),
        "{asked}"
    );
    assert!(!asked.contains('\n'), "a brief is one line");
    // The stand-in leaves its job in a shell that waits for it, or detached from it.
    assert_eq!(
        brief(Who::StandIn),
        format!("BACKGROUND {SECONDS} left a job running")
    );
    assert_eq!(
        stand_in_brief(true),
        format!("BACKGROUND --detach {SECONDS} left a job running")
    );
    // The command is the platform's own.
    assert!(
        command() == format!("sleep {SECONDS}")
            || command() == format!("ping -n {SECONDS} 127.0.0.1"),
        "{}",
        command()
    );
}

#[test]
fn what_the_chief_was_told_is_said_a_line_to_each_note_or_that_it_was_told_none() {
    let mut left = good();
    assert_eq!(
        at_the_chief(&left),
        format!("what the chief was told of work the turn left:\n    {CLOSED}")
    );
    left.notes.clear();
    assert_eq!(
        at_the_chief(&left),
        "what the chief was told of work the turn left: nothing"
    );
    assert_eq!(listed_line(&[]), "none");
    assert_eq!(
        listed_line(&[proc(41, "sleep 4711"), proc(42, "sh -c sleep 4711")]),
        "41 sleep 4711; 42 sh -c sleep 4711"
    );
}

#[test]
fn the_project_of_a_worker_that_leaves_a_command_is_a_folder_of_its_own() {
    assert_eq!(plan(Who::StandIn).folder, "leaving-fake");
    assert_eq!(plan(Who::Harness(Harness::Pi)).folder, "leaving-pi");
}

#[cfg(unix)]
#[test]
fn a_process_that_runs_is_there_and_one_that_never_was_is_not() {
    let mut shell = cf_e2e::process::Run::new("/bin/sh")
        .args(["-c", "sleep 8; true"])
        .inheriting_env()
        .spawn()
        .unwrap();
    let pid = u64::from(shell.id());
    assert_eq!(still_there(&[proc(pid, "sh")]), [pid]);
    assert_eq!(
        still_there(&[proc(4_000_000_000, "none")]),
        Vec::<u64>::new()
    );
    shell.kill();
}

/// What Pi's and Devin's runs on a Mac came to (T-1068): the host listed none of the command, a
/// `sleep` in a session of its own (parent 1, no terminal) ran on after the window closed, and
/// the chief was told nothing.
fn in_a_session_of_its_own() -> Left {
    let mut left = good();
    left.listed.clear();
    left.notes.clear();
    left.escaped = vec![standing("99998 1 99997 ?? sleep 4711").unwrap()];
    left.after_close = Some(left.escaped.clone());
    left
}

#[test]
fn a_command_that_escaped_into_a_session_of_its_own_fails_the_case_as_it_was_and_holds_it_where_the_limit_is_expected(
) {
    let left = in_a_session_of_its_own();
    // As the case was, and still is for a harness whose close ends what it detaches.
    let problems = judge(&left, Expect::Ended).unwrap_err();
    assert!(
        problems[0].starts_with("the command escaped the ending"),
        "{problems:#?}"
    );
    // Expected, as Pi's and Devin's on a Mac: the task is done, the result is delivered, the
    // chief was told nothing false, and the run says what it saw.
    assert_eq!(judge(&left, Expect::OwnSession), Ok(()));
    assert_eq!(
        summary(&left, &judge(&left, Expect::OwnSession)),
        "T-1 done after 20.0 s; the agent detached the command into a session of its own, the known limit on a Mac: the host never listed it, the window's close left it running and the chief was told nothing; still running: 99998 sleep 4711 (parent 1, group 99997, terminal ??); the run ends what it can prove its own"
    );
}

/// The host's look fell on the shell before it had started the job (the shell lists with the job's
/// words in its own command line, and ends a moment after it starts the job): nothing it listed is
/// there when the run looks again, and the job it started ran on in a session of its own that the
/// host never listed and the close did not find (CI run 42425306008 held Pi's and Devin's limit
/// to the shape of a look that fell later). It is the same limit, and the chief told nothing false.
#[test]
fn a_shell_the_host_listed_alone_that_started_a_job_which_ran_on_in_a_session_of_its_own_is_the_limit_too(
) {
    let mut left = in_a_session_of_its_own();
    left.listed = vec![proc(
        99997,
        "/bin/sh -c nohup fake-agent --background-job 4711",
    )];
    assert_eq!(judge(&left, Expect::OwnSession), Ok(()));
    assert!(
        judge(&left, Expect::Ended).is_err(),
        "only where the limit is expected"
    );
    assert!(
        summary(&left, &judge(&left, Expect::OwnSession))
            .contains("the host listed it before its shell ended"),
        "{}",
        summary(&left, &judge(&left, Expect::OwnSession))
    );
    // A job the host listed that is said to be gone and runs on is no such job.
    let mut left = in_a_session_of_its_own();
    left.listed = vec![proc(99998, "sleep 4711")];
    assert!(judge(&left, Expect::OwnSession).is_err());
    // An ordinary run (its command listed, ended by the close, the chief told) is held to that
    // whatever else of the machine holds the command's words and is in a session of its own: the
    // run of another tree, which the run cannot tell from its own, passes nothing and fails nothing.
    let mut left = good();
    left.escaped = vec![standing("99998 1 99997 ?? sleep 4711").unwrap()];
    left.after_close = Some(left.escaped.clone());
    assert_eq!(judge(&left, Expect::OwnSession), Ok(()));
    assert_eq!(judge(&left, Expect::Ended), Ok(()));
}

/// The host's look fell while the job's shell still tied it to the window: it listed the job,
/// which then left with its shell into a session of its own that the close did not find (GitHub's
/// macOS runner, CI runs 41772376690 and 41781145403). Where the job is after the close decides:
/// it is the same limit, and the chief told nothing false.
#[test]
fn a_job_the_host_listed_before_its_shell_ended_is_the_limit_where_it_ran_on_in_a_session_of_its_own(
) {
    let mut left = in_a_session_of_its_own();
    left.listed = vec![proc(99998, "sleep 4711"), proc(99997, "/bin/sh -c nohup")];
    left.alive = vec![99998];
    assert_eq!(judge(&left, Expect::OwnSession), Ok(()));
    assert!(
        judge(&left, Expect::Ended).is_err(),
        "only where the limit is expected"
    );
    assert_eq!(
        summary(&left, &judge(&left, Expect::OwnSession)),
        "T-1 done after 20.0 s; the agent detached the command into a session of its own, the known limit on a Mac: the host listed it before its shell ended, the window's close left it running and the chief was told nothing; still running: 99998 sleep 4711 (parent 1, group 99997, terminal ??); the run ends what it can prove its own"
    );
    // A chief told of work the job outlived is told what the close did not do.
    left.notes = vec![CLOSED.to_owned()];
    assert!(judge(&left, Expect::OwnSession).is_err());
}

#[test]
fn the_limit_is_expected_only_of_the_shape_it_is_and_the_task_the_result_and_the_chief_are_held_as_ever(
) {
    let limit = in_a_session_of_its_own;
    // Each of these is a run that went another way, and fails even where the limit is expected.
    let mut with_a_terminal = limit();
    with_a_terminal.escaped = vec![standing("99998 1 99997 ttys004 sleep 4711").unwrap()];
    with_a_terminal.after_close = Some(with_a_terminal.escaped.clone());
    let mut with_a_parent = limit();
    with_a_parent.escaped = vec![standing("99998 4242 99997 ?? sleep 4711").unwrap()];
    with_a_parent.after_close = Some(with_a_parent.escaped.clone());
    let mut one_of_two_in_the_window = limit();
    one_of_two_in_the_window
        .escaped
        .push(standing("99999 1 99997 ttys004 sleep 4711").unwrap());
    one_of_two_in_the_window.after_close = Some(one_of_two_in_the_window.escaped.clone());
    // Listed while its shell still tied it to the window, and in the window's session after the
    // close: no limit.
    let mut listed_still_tied = limit();
    listed_still_tied.listed = vec![proc(99998, "sleep 4711")];
    listed_still_tied.alive = vec![99998];
    listed_still_tied.escaped = vec![standing("99998 1 99997 ttys004 sleep 4711").unwrap()];
    listed_still_tied.after_close = Some(listed_still_tied.escaped.clone());
    // Listed, and gone with no sign of where: what the close did is unknown, and nobody was told.
    let mut listed_gone = limit();
    listed_gone.listed = vec![proc(99998, "sleep 4711")];
    listed_gone.alive.clear();
    // Listed, and another process of it still runs that is not the one in a session of its own.
    let mut listed_and_another = limit();
    listed_and_another.listed = vec![proc(99998, "sleep 4711"), proc(99996, "sleep 4711")];
    listed_and_another.alive = vec![99998, 99996];
    let mut told = limit();
    told.notes = vec![CLOSED.to_owned()];
    let mut ended_unsaid = limit();
    ended_unsaid.after_close = Some(Vec::new());
    let mut window_open = limit();
    window_open.after_close = None;
    let mut not_done = limit();
    not_done.state = "working".to_owned();
    let mut no_result = limit();
    no_result.result = None;
    for (name, left) in [
        (
            "a terminal: the window's session, which its close ends",
            with_a_terminal,
        ),
        (
            "a parent: a shell that still runs started it",
            with_a_parent,
        ),
        (
            "one of two is in the window's session",
            one_of_two_in_the_window,
        ),
        (
            "the host listed it, and it is in the window's session after the close",
            listed_still_tied,
        ),
        ("the host listed it, and it is gone unsaid", listed_gone),
        (
            "the host listed another process of it that runs on outside the limit",
            listed_and_another,
        ),
        ("the chief was told of work, which this job was not", told),
        ("the close ended it, unsaid", ended_unsaid),
        ("the window had not closed", window_open),
        ("the task is not done", not_done),
        ("no result reached the chief", no_result),
    ] {
        assert!(
            judge(&left, Expect::OwnSession).is_err(),
            "{name}: the limit holds it no more than it did"
        );
    }
    // A chief told of work that the command outlived is told what the close did not do: the
    // problem says so, and says what to do if the host names the job now.
    let mut told = limit();
    told.notes = vec![CLOSED.to_owned()];
    let said = judge(&told, Expect::OwnSession).unwrap_err().join("\n");
    assert!(
        said.contains("the chief was told of work the window closed with")
            && said.contains("the limit is out of date"),
        "{said}"
    );
}

#[test]
fn a_worker_is_expected_to_come_to_the_limit_for_pi_and_devin_on_a_mac_and_to_the_ending_for_the_rest(
) {
    let mac = cfg!(target_os = "macos");
    for harness in [Harness::Pi, Harness::Devin] {
        let expected = if mac {
            Expect::OwnSession
        } else {
            Expect::Ended
        };
        assert_eq!(Expect::of(Who::Harness(harness)), expected, "{harness:?}");
    }
    // Claude Code keeps the shell it runs a background command in, Codex's and Muse's
    // harnesses end the command with the tool call: both pass the ending, and nothing else
    // is let off.
    for harness in [Harness::Claude, Harness::Codex, Harness::Muse] {
        assert_eq!(
            Expect::of(Who::Harness(harness)),
            Expect::Ended,
            "{harness:?}"
        );
    }
    assert_eq!(Expect::of(Who::StandIn), Expect::Ended);
}

#[test]
fn what_escaped_every_window_is_ended_when_it_is_provably_the_runs_own_and_said_when_it_is_not() {
    let folder = Path::new("/runs/leaving-pi");
    let window = Identity {
        pid: 100,
        started: 10,
    };
    let all = vec![
        running(100, 10, 1, "pi"),
        // The job the agent detached: an orphan, in the run's folder, begun after the window.
        running(300, 20, 1, "sleep 4711"),
        // Another's: the same command, and a folder of its own.
        running(301, 21, 1, "sleep 4711"),
        // A child of a shell that still runs, in no folder the run made: not an orphan.
        running(302, 22, 50, "sleep 4711"),
        // In the run's folder, and begun before the worker's window did: another run's.
        running(303, 5, 1, "sleep 4711"),
    ];
    let folder_of = |pid: u32| match pid {
        300 | 303 => Some(folder.to_path_buf()),
        301 => Some(PathBuf::from("/elsewhere")),
        _ => None,
    };
    let pids = |chosen: &Chosen| -> Vec<u32> {
        chosen
            .processes
            .iter()
            .map(|one| one.identity.pid)
            .collect()
    };
    let mut chosen = Chosen {
        strays_matter: true,
        ..Chosen::default()
    };
    chosen.choose(&all, Some(window), &[], "sleep 4711");
    assert_eq!(chosen.window_began, Some(10), "the window's start is kept");
    // The first look's, whatever a later one says of the number.
    chosen.choose(
        &all,
        Some(Identity {
            pid: 100,
            started: 15,
        }),
        &[],
        "sleep 4711",
    );
    assert_eq!(chosen.window_began, Some(10));
    assert!(
        chosen.processes.is_empty(),
        "nothing descends from the window"
    );
    let found = chosen.take_orphans(&all, folder, "sleep 4711", folder_of);
    assert_eq!(pids(&chosen), [300]);
    assert_eq!(found.len(), 1, "what the folder found is answered");
    // Once, however often it looks, and what the folder found is answered each time.
    let again = chosen.take_orphans(&all, folder, "sleep 4711", folder_of);
    assert_eq!(pids(&chosen), [300]);
    assert_eq!(again.len(), 1);
    // The other is not ended, and is a problem of the run: it may still run.
    assert_eq!(
        chosen.problems(&Ended::default()),
        ["the command escaped the window and cannot be proved the run's own (only an orphan that began after the worker's window and runs in the run's folder is), so it was not ended and may still run: 301 sleep 4711; 303 sleep 4711"]
    );
    // The stand-in's strays are other rigs' jobs, which tell their own by their files: they
    // are looked at and are no problem of the run.
    chosen.strays_matter = false;
    assert!(chosen.problems(&Ended::default()).is_empty());
    // A run that never saw its window's process proves nothing its own.
    let mut blind = Chosen {
        strays_matter: true,
        ..Chosen::default()
    };
    blind.take_orphans(&all, folder, "sleep 4711", folder_of);
    assert!(blind.processes.is_empty());
    assert_eq!(blind.problems(&Ended::default()).len(), 1);
    assert!(blind.problems(&Ended::default())[0]
        .ends_with("300 sleep 4711; 301 sleep 4711; 303 sleep 4711"));
}

#[cfg(target_os = "macos")]
#[test]
fn a_job_a_shell_detached_and_left_is_found_by_the_runs_folder_and_ended_by_which_process_it_is() {
    // A real orphan: a shell runs a short sleep in the background in a folder made for the test
    // and ends at once, so that the sleep's parent is init's. It is found by the folder and by
    // the window's start, and ended through the descent as any process the run chose.
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().canonicalize().unwrap();
    let began = ThisMachine
        .identity(std::process::id())
        .unwrap()
        .expect("this process is there")
        .started;
    let ran = cf_e2e::process::Run::new("/bin/sh")
        .args(["-c", "sleep 25 > /dev/null 2>&1 &"])
        .cwd(&folder)
        .inheriting_env()
        .run()
        .unwrap();
    assert_eq!(ran.code, Some(0));
    let mut chosen = Chosen {
        window_began: Some(began),
        ..Chosen::default()
    };
    let mut found = Vec::new();
    for _ in 0..100 {
        let all = ThisMachine.processes().unwrap();
        chosen.take_orphans(&all, &folder, "sleep 25", working_folder);
        found = chosen.processes.clone();
        if !found.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    // Ended whatever came of the assertions below: the sleep is short, and this ends it now.
    let ended = end_chosen(&found);
    assert_eq!(found.len(), 1, "the one orphan of the folder: {found:#?}");
    assert!(found[0].command.contains("sleep 25"), "{found:#?}");
    assert_eq!(ended.ended.len(), 1, "{ended:#?}");
    let pid = found[0].identity.pid;
    assert!(
        (0..100).any(|_| {
            std::thread::sleep(Duration::from_millis(100));
            !cf_e2e::process::is_alive(pid)
        }),
        "the orphan {pid} is left running"
    );
    assert!(chosen.problems(&ended).is_empty());
}

#[test]
fn the_run_looks_for_its_own_orphans_only_where_it_expects_the_limit_and_something_ran_on() {
    let ran_on = in_a_session_of_its_own();
    let mut nothing = in_a_session_of_its_own();
    nothing.escaped.clear();
    assert!(looks_for_its_own_orphans(Expect::OwnSession, &ran_on));
    assert!(!looks_for_its_own_orphans(Expect::OwnSession, &nothing));
    // A harness whose close ends what it detaches is not looked after: nothing of its own is
    // ended without the limit being expected, and a command that escaped fails the run.
    assert!(!looks_for_its_own_orphans(Expect::Ended, &ran_on));
}

#[test]
fn a_stray_orphan_fails_the_run_of_a_harness_and_not_the_rigs_stand_in() {
    for harness in Harness::ALL {
        assert!(strays_matter(Who::Harness(harness)), "{harness:?}");
    }
    assert!(!strays_matter(Who::StandIn));
}

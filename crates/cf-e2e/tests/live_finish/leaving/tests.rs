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

/// A folder of a run's own for a test's table of processes: made under a root of the test's, as
/// the rig makes the run's under its temp root ([`RunFolder::under`] reads both); kept while the
/// root is.
fn run_folder(name: &str) -> (tempfile::TempDir, RunFolder) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(name);
    std::fs::create_dir(&path).unwrap();
    let folder = RunFolder::under(root.path(), &path).expect("under the root");
    (root, folder)
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
    // ping, whose seconds are not its last word (zeewin, round 7), and the run's word in its
    // timeout. The rig's stand-in starts a job of its own, whose command line holds the seconds.
    let harness = if cfg!(windows) {
        format!("ping -n {} -w {} 127.0.0.1", seconds(), word())
    } else {
        format!("sleep {}", word())
    };
    assert_eq!(wanted(Who::Harness(Harness::Pi)), harness);
    assert_eq!(
        wanted(Who::StandIn),
        format!("--background-job {}", seconds())
    );
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
    let jobs = [job(200, 1), job(201, 2), job(202, 3), job(203, 4)];
    let window = Identity {
        pid: 100,
        started: 10,
    };
    let all = vec![
        running(100, 10, 1, "fake-agent"),
        // The number and the file of a job, in a process begun before the run's window: none of
        // the run's.
        running(
            203,
            5,
            1,
            "fake-agent --background-job 4711 /rig/jobs/job-100-4",
        ),
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
    chosen.choose(&all, Some(window), &jobs, wanted);
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
    chosen.choose(&later, Some(window), &jobs, wanted);
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

const ENDED: &str =
    "the command was ended by the window's close after the result, and the chief told";
const LIMIT: &str = "the command ran on in a session of its own, as the known limit on a Mac says it may, and the chief was told nothing";

/// A run that passed is said to have shown what its summary tells. A process of the machine that
/// holds the command's words and is none of the run's (another tree's run of the stand-in beside
/// this one; the loop of a worker who runs the suite twice) is found among `escaped` and changes
/// nothing of an ordinary run, in its name as in its verdict.
#[test]
fn a_pass_is_named_for_what_its_summary_tells_and_a_stray_of_the_machine_changes_neither() {
    let ordinary = good();
    assert_eq!(passed_as(&ordinary), ENDED);
    let mut with_a_stray = good();
    with_a_stray.escaped = vec![standing("99998 1 99997 ?? sleep 4711").unwrap()];
    with_a_stray.after_close = Some(with_a_stray.escaped.clone());
    assert_eq!(judge(&with_a_stray, Expect::Ended), Ok(()));
    assert!(
        summary(&with_a_stray, &judge(&with_a_stray, Expect::Ended))
            .contains("was ended by the window's close"),
        "{}",
        summary(&with_a_stray, &judge(&with_a_stray, Expect::Ended))
    );
    assert_eq!(passed_as(&with_a_stray), ENDED);
    // The limit, in either shape it takes: the host never listed the job, or listed the shell alone.
    let limit = in_a_session_of_its_own();
    assert_eq!(passed_as(&limit), LIMIT);
    let mut shell_alone = in_a_session_of_its_own();
    shell_alone.listed = vec![proc(
        99997,
        "/bin/sh -c nohup fake-agent --background-job 4711",
    )];
    assert_eq!(passed_as(&shell_alone), LIMIT);
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
            && asked.contains(&format!(" {}", word()))
            && asked.contains("in the background")
            && asked.contains("Do not wait for it")
            && asked.contains("reply with one line"),
        "{asked}"
    );
    assert!(!asked.contains('\n'), "a brief is one line");
    // The stand-in leaves its job in a shell that waits for it, or detached from it.
    assert_eq!(
        brief(Who::StandIn),
        format!("BACKGROUND {} left a job running", seconds())
    );
    assert_eq!(
        stand_in_brief(true),
        format!("BACKGROUND --detach {} left a job running", seconds())
    );
    // The command is the platform's own, and carries the run's word as a whole argument in
    // every form the worker is given.
    assert!(
        command() == format!("sleep {}", word())
            || command() == format!("ping -n {} -w {} 127.0.0.1", seconds(), word()),
        "{}",
        command()
    );
    assert!(has_arg(&command(), word()), "{}", command());
    assert!(
        detach().contains(&format!("'{}'", word())) || detach().contains(&command()),
        "{}",
        detach()
    );
}

#[test]
fn the_runs_word_is_its_seconds_and_twenty_digits_made_once() {
    let (seconds_part, digits) = word().split_once('.').expect("a point");
    assert_eq!(seconds_part, seconds().to_string());
    assert_eq!(digits.len(), 20, "{}", word());
    assert!(
        digits.bytes().all(|digit| digit.is_ascii_digit()),
        "{}",
        word()
    );
    assert_eq!(word(), word());
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
    let (root, own) = run_folder("leaving-pi");
    let folder = own.real();
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
    let mut chosen = Chosen::default();
    chosen.choose(&all, Some(window), &[], "sleep 4711");
    assert_eq!(chosen.window_began, Some(10), "the window's start is kept");
    assert_eq!(chosen.window, Some(window), "so is the window's process");
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
    // A folder outside the run's temp root (a live rig's project folder is in the person's home)
    // is no ground of the run's: nothing is taken by it.
    let elsewhere = tempfile::tempdir().unwrap();
    let outside = RunFolder::under(root.path(), elsewhere.path());
    assert_eq!(outside, None, "a folder outside the run's root is refused");
    let mut live = Chosen::default();
    live.choose(&all, Some(window), &[], "sleep 4711");
    assert!(live
        .take_orphans(&all, outside.as_ref(), "sleep 4711", folder_of)
        .is_empty());
    assert!(live.processes.is_empty());
    let found = chosen.take_orphans(&all, Some(&own), "sleep 4711", folder_of);
    assert_eq!(pids(&chosen), [300]);
    assert_eq!(found.len(), 1, "what the folder found is answered");
    // Once, however often it looks, and what the folder found is answered each time.
    let again = chosen.take_orphans(&all, Some(&own), "sleep 4711", folder_of);
    assert_eq!(pids(&chosen), [300]);
    assert_eq!(again.len(), 1);
    // The others are not ended, and are none of the run's either: they may be another run's,
    // which holds the same words, and a run that fails on them fails on a stranger's.
    assert_eq!(chosen.unproven, ["301 sleep 4711", "303 sleep 4711"]);
    assert!(chosen.problems(&Ended::default()).is_empty());
    // A run that never saw its window's process proves nothing its own.
    let mut blind = Chosen::default();
    blind.take_orphans(&all, Some(&own), "sleep 4711", folder_of);
    assert!(blind.processes.is_empty());
    assert_eq!(
        blind.unproven,
        ["300 sleep 4711", "301 sleep 4711", "303 sleep 4711"]
    );
    assert!(blind.problems(&Ended::default()).is_empty());
}

#[cfg(target_os = "macos")]
#[test]
fn a_job_a_shell_detached_and_left_is_found_by_the_runs_folder_and_ended_by_which_process_it_is() {
    // A real orphan: a shell runs a short sleep in the background in a folder made for the test
    // and ends at once, so that the sleep's parent is init's. It is found by the folder and by
    // the window's start, and ended through the descent as any process the run chose.
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().canonicalize().unwrap();
    let own = RunFolder::under(dir.path(), dir.path()).expect("the root itself");
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
        chosen.take_orphans(&all, Some(&own), "sleep 25", working_folder);
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

/// The processes of the machine that hold the command's words (`sleep 4711`, or the stand-in's
/// `--background-job 4711`) are of every tree's run that uses them, and a case that took them
/// all for its own failed a gate (T-592: another tree's `--background-job 4711`, pid 1832). A real
/// one, started here in a folder that is not the case's and by no window of it, is none of what
/// escaped, whichever way the run's own are told.
#[cfg(unix)]
#[test]
fn a_process_of_another_run_that_holds_the_commands_words_is_not_among_what_escaped() {
    let elsewhere = tempfile::tempdir().unwrap();
    let mut decoy = cf_e2e::process::Run::new("sleep")
        .arg("4711")
        .cwd(elsewhere.path())
        .inheriting_env()
        .spawn()
        .unwrap();
    let pid = decoy.id();
    let began = ThisMachine
        .identity(std::process::id())
        .unwrap()
        .expect("this process is there")
        .started;
    let folder = tempfile::tempdir().unwrap();
    let own = RunFolder::under(folder.path(), folder.path()).expect("the root itself");
    // The run's window began before the decoy did, and has a folder that is another.
    let chosen = Chosen {
        window_began: Some(began),
        ..Chosen::default()
    };
    let mut found = Vec::new();
    let mut seen = false;
    for _ in 0..50 {
        // Whether the decoy is among the processes at all, so that a look that never reads it is
        // not what passes.
        seen = ThisMachine
            .processes()
            .unwrap()
            .iter()
            .any(|process| process.identity.pid == pid);
        found = on_the_machine(|all| {
            chosen.of_the_run(
                all,
                (Some(&own), "sleep 4711", Some("/the-rigs-own/jobs/")),
                working_folder,
            )
        });
        if seen {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    decoy.kill();
    assert!(
        seen,
        "the decoy {pid} was never listed: the test proves nothing"
    );
    assert!(
        found.is_empty(),
        "the decoy {pid} of another folder is taken for what the case's window left: {found:#?}"
    );
}

#[test]
fn what_escaped_is_what_the_window_held_an_orphan_of_its_folder_since_it_began_and_what_holds_the_rigs_own_folder(
) {
    let (_root, own) = run_folder("leaving-claude");
    let folder = own.real();
    let window = Identity {
        pid: 100,
        started: 10,
    };
    let jobs = "/rig-7/jobs/";
    let all = vec![
        running(100, 10, 1, "claude"),
        // The window's own: the shell its tool ran, and the command.
        running(101, 11, 100, "/bin/sh -c sleep 4711"),
        running(102, 12, 101, "sleep 4711"),
        // The job the agent detached: an orphan, in the run's folder, begun after the window.
        running(300, 20, 1, "sleep 4711"),
        // Another tree's run of the same command: a folder of its own, and one that began
        // before the window.
        running(301, 21, 1, "sleep 4711"),
        running(302, 5, 1, "sleep 4711"),
        // A child of a shell of another tree.
        running(303, 22, 50, "sleep 4711"),
        // The stand-in's jobs: this rig's, and another tree's with the same number.
        running(
            400,
            23,
            1,
            "fake-agent --background-job 4711 /rig-7/jobs/job-100-1",
        ),
        running(
            401,
            24,
            1,
            "fake-agent --background-job 4711 /rig-8/jobs/job-9-1",
        ),
    ];
    let folder_of = |pid: u32| match pid {
        300 | 302 => Some(folder.to_path_buf()),
        301 => Some(PathBuf::from("/elsewhere")),
        _ => None,
    };
    let mut chosen = Chosen::default();
    chosen.choose(&all, Some(window), &[], "sleep 4711");
    let pids =
        |found: Vec<Running>| -> Vec<u32> { found.iter().map(|one| one.identity.pid).collect() };
    assert_eq!(
        pids(chosen.of_the_run(&all, (Some(&own), "sleep 4711", None), folder_of)),
        [101, 102, 300]
    );
    assert_eq!(
        pids(chosen.of_the_run(
            &all,
            (Some(&own), "--background-job 4711", Some(jobs)),
            folder_of
        )),
        [400]
    );
    // A run that never saw its window has none of it.
    assert!(Chosen::default()
        .of_the_run(&all, (Some(&own), "sleep 4711", None), folder_of)
        .is_empty());
}

/// Ends what it holds, each by which process it is, when the test ends however it does.
#[cfg(unix)]
struct Reaper(Vec<Running>);

#[cfg(unix)]
impl Drop for Reaper {
    fn drop(&mut self) {
        let _ = end_chosen(&self.0);
    }
}

/// A command of this run's own word that detached out of every look (its shell ended before the
/// host listed it, and the window has gone), in a folder that is none of the run's, escaped the
/// ending as surely as one the window held: the folder and the descent cannot see it, and Windows
/// has no folder to read at all. The word is in its command line wherever it went.
#[cfg(unix)]
#[test]
fn a_command_of_this_runs_word_that_detached_unseen_and_runs_in_another_folder_escaped_and_fails_the_verdict(
) {
    let elsewhere = tempfile::tempdir().unwrap();
    let ran = cf_e2e::process::Run::new("/bin/sh")
        .args(["-c", &format!("{} > /dev/null 2>&1 & echo $!", command())])
        .cwd(elsewhere.path())
        .inheriting_env()
        .run()
        .unwrap();
    let pid: u32 = ran.stdout.trim().parse().unwrap();
    let mut reaper = Reaper(Vec::new());
    let began = ThisMachine
        .identity(std::process::id())
        .unwrap()
        .expect("this process is there")
        .started;
    // The run's window began before it and has gone; no look saw the command under it.
    let chosen = Chosen {
        window_began: Some(began),
        ..run_chosen()
    };
    let wanted = wanted(Who::Harness(Harness::Claude));
    let mut found = Vec::new();
    for _ in 0..50 {
        if let Some(own) = ThisMachine
            .processes()
            .unwrap()
            .into_iter()
            .find(|one| one.identity.pid == pid)
        {
            reaper.0 = vec![own];
            found =
                on_the_machine(|all| chosen.of_the_run(all, (None, &wanted, None), working_folder));
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        !reaper.0.is_empty(),
        "the command {pid} never ran: the test proves nothing"
    );
    assert!(
        found
            .iter()
            .any(|line| line.starts_with(&format!("{pid} "))),
        "the command {pid} of the run's own word is not among what escaped: {found:#?}"
    );
    let mut left = good();
    left.listed.clear();
    left.notes.clear();
    left.escaped = found;
    let problems = judge(&left, Expect::Ended).unwrap_err();
    assert!(
        problems
            .iter()
            .any(|problem| problem.starts_with("the command escaped the ending")),
        "{problems:#?}"
    );
}

/// The platform's own command, started here and under no window: `sleep`, or on Windows
/// `ping -n … -w`, with a run's word as a whole argument. Found by the word alone; one of another
/// run's word, of the same seconds and started beside it, is not found. The word ends neither: the
/// test ends both by their own handles.
#[test]
fn the_command_of_this_runs_word_is_found_by_it_anywhere_and_another_runs_word_is_none_of_it() {
    use cf_e2e::process::Run;

    // Words of this test's own, made as the run's is: the run's own command of the other tests
    // runs beside it, and what finds the run's word ends it.
    let a_word = || {
        let (high, low) = uuid::Uuid::new_v4().as_u64_pair();
        format!("{}.{:020}", seconds(), high ^ low)
    };
    let spawn = |word: &str| {
        let (program, args): (&str, Vec<String>) = if cfg!(windows) {
            (
                "ping",
                ["-n", &seconds().to_string(), "-w", word, "127.0.0.1"]
                    .map(str::to_owned)
                    .to_vec(),
            )
        } else {
            ("sleep", vec![word.to_owned()])
        };
        Run::new(program)
            .args(args)
            .inheriting_env()
            .spawn()
            .unwrap()
    };
    let began = ThisMachine
        .identity(std::process::id())
        .unwrap()
        .expect("this process is there")
        .started;
    let (own_word, other_word) = (a_word(), a_word());
    let mut own = spawn(&own_word);
    let mut other = spawn(&other_word);
    let chosen = Chosen {
        word: Some(own_word.clone()),
        window_began: Some(began),
        ..Chosen::default()
    };
    let mut found = Vec::new();
    for _ in 0..100 {
        found = chosen.by_word(&ThisMachine.processes().unwrap());
        if found.iter().any(|one| one.identity.pid == own.id()) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        found.iter().any(|one| one.identity.pid == own.id()),
        "the command of the run's word is not found by it: {found:#?}"
    );
    assert!(
        found.iter().all(|one| one.identity.pid != other.id()),
        "another run's word is taken for this one's: {found:#?}"
    );
    other.kill();
    own.kill();
}

/// What holds the command's words in its own command line, and is not the command: the window,
/// whose brief names the command (T-548), another process of the window's program, anybody's `sleep`
/// of the same seconds, another run's word, and a program that quotes the run's word. The word
/// finds the command's own program alone, `sleep` (on Windows `ping`, which that table lists by the
/// path of its image): on a Mac 300, on Windows 302.
#[test]
fn the_window_and_its_program_hold_the_commands_words_in_their_own_command_line_and_are_none_of_what_the_word_finds(
) {
    let words = command();
    let all = vec![
        running(10, 1, 1, "pane-host"),
        running(
            100,
            10,
            10,
            &format!("claude -p Start the command {words} now"),
        ),
        // A process of the window's own program, which holds the words too.
        running(
            101,
            11,
            1,
            &format!("claude --resume Start the command {words} now"),
        ),
        running(300, 20, 1, &format!("sleep {}", word())),
        // Another run's word, and anybody's `sleep` of the same seconds.
        running(301, 21, 1, &format!("sleep {}.1", seconds())),
        running(304, 24, 1, &format!("sleep {}", seconds())),
        // Windows' table lists a ping by the path of its image: the words are not its line.
        running(
            302,
            22,
            1,
            &format!(
                "\"C:\\Windows\\system32\\PING.EXE\" -n {} -w {} 127.0.0.1",
                seconds(),
                word()
            ),
        ),
        // The same words in a program that is none of the command's, and not the window's.
        running(303, 23, 1, &format!("editor notes {}", word())),
        running(305, 25, 1, &format!("/usr/bin/grep -F {words}")),
    ];
    let chosen = Chosen {
        window_began: Some(10),
        window: Some(Identity {
            pid: 100,
            started: 10,
        }),
        ..run_chosen()
    };
    let pids: Vec<u32> = chosen
        .by_word(&all)
        .iter()
        .map(|one| one.identity.pid)
        .collect();
    assert_eq!(pids, [if cfg!(windows) { 302 } else { 300 }]);
}

/// The run's own command, as the member is told to run it, and an unrelated one of the same seconds
/// (anybody's `sleep`, or `ping -n` on Windows, begun after the run's word was made and after the
/// run's window), neither under a window of the run's: what the run counts as escaped is its own
/// command alone (T-1538's fifth finding), and the run ends neither (T-1557's first finding: the
/// word makes nothing the run's own to end). The test ends both by their own handles.
/// The table the run reads is cut to these two processes, so that nothing else on the machine is
/// reached.
#[test]
fn an_unrelated_command_of_the_same_seconds_begun_after_the_runs_word_is_none_of_the_run_and_runs_on(
) {
    use cf_e2e::process::Run;

    let spawn = |line: &str| {
        let mut words = line.split_whitespace();
        let program = words.next().unwrap().to_owned();
        Run::new(program)
            .args(words)
            .inheriting_env()
            .spawn()
            .unwrap()
    };
    // The run's window began before either.
    let began = ThisMachine
        .identity(std::process::id())
        .unwrap()
        .expect("this process is there")
        .started;
    let own_line = command();
    let unrelated_line = if cfg!(windows) {
        format!("ping -n {} 127.0.0.1", seconds())
    } else {
        format!("sleep {}", seconds())
    };
    let mut own = spawn(&own_line);
    let mut unrelated = spawn(&unrelated_line);
    let ours = |all: &[Running]| -> Vec<Running> {
        all.iter()
            .filter(|one| one.identity.pid == own.id() || one.identity.pid == unrelated.id())
            .cloned()
            .collect()
    };
    let mut table = Vec::new();
    for _ in 0..100 {
        table = ours(&ThisMachine.processes().unwrap());
        if table.len() == 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(table.len(), 2, "both commands run: {table:#?}");
    let mut chosen = Chosen {
        window_began: Some(began),
        ..run_chosen()
    };
    let wanted = wanted(Who::Harness(Harness::Claude));
    chosen.choose(&table, None, &[], &wanted);
    let escaped =
        on_the_machine(|all| chosen.of_the_run(&ours(all), (None, &wanted, None), working_folder));
    let ended = end_chosen(&chosen.processes);
    let own_runs_on = own.wait(Duration::from_millis(500)).unwrap().is_none();
    let runs_on = unrelated
        .wait(Duration::from_millis(500))
        .unwrap()
        .is_none();
    unrelated.kill();
    own.kill();
    assert!(
        escaped
            .iter()
            .all(|line| !line.starts_with(&format!("{} ", unrelated.id()))),
        "an unrelated `{unrelated_line}` is among what escaped: {escaped:#?}"
    );
    assert!(
        ended
            .ended
            .iter()
            .all(|one| one.identity.pid != unrelated.id()),
        "an unrelated `{unrelated_line}` was ended: {ended:#?}"
    );
    assert!(runs_on, "an unrelated `{unrelated_line}` was ended");
    assert!(
        escaped
            .iter()
            .any(|line| line.starts_with(&format!("{} ", own.id()))),
        "the run's own `{own_line}` is not among what escaped: {escaped:#?}"
    );
    assert!(
        own_runs_on,
        "the run's own `{own_line}`, found by its word alone, was ended: {ended:#?}"
    );
}

/// What holds the run's word but began before the run's first window is none of the run's: the
/// word is made for the run, and nothing of the run's runs before its window does.
#[test]
fn what_holds_the_runs_word_but_began_before_the_runs_window_is_none_of_the_run() {
    let line = command();
    let all = vec![running(300, 5, 1, &line), running(301, 30, 1, &line)];
    let chosen = Chosen {
        window_began: Some(20),
        ..run_chosen()
    };
    let found: Vec<u32> = chosen
        .by_word(&all)
        .iter()
        .map(|one| one.identity.pid)
        .collect();
    assert_eq!(found, [301]);
}

/// A process of the test's own, begun after the run's window, that watches for the run's command:
/// `grep -F '<the command>'` (on Windows `findstr /C:"<the command>"`) reading a pipe that stays
/// empty, as an owner's log filter does. Its command line holds the whole command with the run's
/// word between blanks (`ps` shows `grep -F sleep 4711.<digits>`, T-1557's probe), and it is none
/// of the run's: it is not chosen for ending, the word does not find it, and it is not said to have
/// escaped. The table the run reads is cut to it, so that nothing else on the machine is reached;
/// the test ends it by its own handle.
#[test]
fn an_observer_begun_after_the_window_that_quotes_the_whole_command_with_the_runs_word_is_none_of_the_run(
) {
    use cf_e2e::process::Run;

    let began = ThisMachine
        .identity(std::process::id())
        .unwrap()
        .expect("this process is there")
        .started;
    let line = command();
    // Held by its handle from here: dropped, it is ended.
    let observer = if cfg!(windows) {
        Run::new("findstr").arg(format!("/C:{line}"))
    } else {
        Run::new("grep").args(["-F", line.as_str()])
    }
    .inheriting_env()
    .spawn()
    .unwrap();
    let ours = |all: &[Running]| -> Vec<Running> {
        all.iter()
            .filter(|one| one.identity.pid == observer.id())
            .cloned()
            .collect()
    };
    let mut table = Vec::new();
    for _ in 0..100 {
        table = ours(&ThisMachine.processes().unwrap());
        if !table.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(table.len(), 1, "the observer runs: {table:#?}");
    assert!(
        has_arg(&table[0].command, word()),
        "the observer's command line does not hold the run's word between blanks, and the test proves nothing: {}",
        table[0].command
    );
    let mut chosen = Chosen {
        window_began: Some(began),
        ..run_chosen()
    };
    let wanted = wanted(Who::Harness(Harness::Claude));
    chosen.choose(&table, None, &[], &wanted);
    assert!(
        chosen.processes.is_empty(),
        "an observer that quotes the run's command is chosen for ending: {:#?}",
        chosen.processes
    );
    assert!(
        chosen.by_word(&table).is_empty(),
        "the run's word finds an observer that quotes it"
    );
    let escaped =
        on_the_machine(|all| chosen.of_the_run(&ours(all), (None, &wanted, None), working_folder));
    assert!(
        escaped.is_empty(),
        "an observer that quotes the run's command is said to have escaped: {escaped:#?}"
    );
    drop(observer);
}

/// The run's own command, as the member is told to run it, under no window of the run's (a
/// harness detached it, and its shell has gone): the word finds it, the run does not end it, and
/// it is said to have escaped, which fails the case, as a line that says the kit left it to end by
/// itself. It still runs after the run's ending; the test ends it by its own handle.
#[test]
fn a_detached_command_of_the_runs_word_is_said_to_have_escaped_and_runs_on_after_the_runs_ending() {
    use cf_e2e::process::Run;

    let began = ThisMachine
        .identity(std::process::id())
        .unwrap()
        .expect("this process is there")
        .started;
    let line = command();
    let mut words = line.split_whitespace();
    let mut own = Run::new(words.next().unwrap())
        .args(words)
        .inheriting_env()
        .spawn()
        .unwrap();
    let pid = own.id();
    let ours = |all: &[Running]| -> Vec<Running> {
        all.iter()
            .filter(|one| one.identity.pid == pid)
            .cloned()
            .collect()
    };
    let mut table = Vec::new();
    for _ in 0..100 {
        table = ours(&ThisMachine.processes().unwrap());
        if !table.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(table.len(), 1, "the command runs: {table:#?}");
    let mut chosen = Chosen {
        window_began: Some(began),
        ..run_chosen()
    };
    chosen.choose(&table, None, &[], &wanted(Who::Harness(Harness::Claude)));
    let ended = end_chosen(&chosen.processes);
    let runs_on = own.wait(Duration::from_millis(500)).unwrap().is_none();
    assert!(
        runs_on,
        "the run ended a command it found by its word alone: {ended:#?}"
    );
    let said = chosen.escaped_the_ending(&ours(&ThisMachine.processes().unwrap()));
    own.kill();
    assert_eq!(
        said,
        [format!(
            "the command escaped the ending: pid {pid}, `{line}`; the kit did not end it, it ends by itself within {} s",
            seconds()
        )],
    );
}

/// What the run chooses with, as `run_asked` makes it for a harness's worker.
fn run_chosen() -> Chosen {
    Chosen {
        word: Some(word().to_owned()),
        ..Chosen::default()
    }
}

/// What the run's word finds once the windows have closed: a command the run chose (by its window)
/// and could not end is failed by its ending ([`Chosen::problems`]) and not said again; one it never
/// chose escaped, and is said with its number and line. Neither is ended here.
#[test]
fn what_escaped_the_ending_is_what_the_word_finds_that_the_run_never_chose() {
    let line = command();
    let chosen_one = running(300, 20, 1, &line);
    let all = vec![chosen_one.clone(), running(301, 21, 1, &line)];
    let chosen = Chosen {
        window_began: Some(10),
        processes: vec![chosen_one],
        ..run_chosen()
    };
    assert_eq!(
        chosen.escaped_the_ending(&all),
        [format!(
            "the command escaped the ending: pid 301, `{line}`; the kit did not end it, it ends by itself within {} s",
            seconds()
        )]
    );
}

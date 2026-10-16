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

const ENDED: &str = "T-1's turn left background work running. Ended: sleep 4711.";

/// A worker whose command was listed, ended, and told to the chief.
fn good() -> Left {
    Left {
        state: "done".to_owned(),
        took: Duration::from_secs(20),
        listed: vec![proc(41, "sleep 4711")],
        result: Some("I started it.".to_owned()),
        notes: vec![ENDED.to_owned()],
        alive: Vec::new(),
        escaped: Vec::new(),
        after_close: None,
    }
}

#[test]
fn a_task_done_with_the_command_listed_gone_and_the_chief_told_it_was_ended_passes() {
    assert_eq!(judge(&good()), Ok(()));
    assert_eq!(
        summary(&good(), &judge(&good())),
        "T-1 done after 20.0 s; the command (41) was ended and the chief told: T-1's turn left background work running. Ended: sleep 4711."
    );
}

#[test]
fn a_command_the_harness_ended_with_its_tool_call_leaves_nothing_to_end_and_passes() {
    let mut left = good();
    left.listed.clear();
    left.notes.clear();
    assert_eq!(judge(&left), Ok(()));
    assert_eq!(
        summary(&left, &judge(&left)),
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
    let problems = judge(&left).unwrap_err();
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
    let problems = judge(&left).unwrap_err();
    assert!(
        problems[0].ends_with("4242 sleep 4711; the window's close ended it, unsaid"),
        "{}",
        problems[0]
    );
    // Or it found nothing of it, and it runs on.
    left.after_close = Some(vec!["4242 sleep 4711".to_owned()]);
    let problems = judge(&left).unwrap_err();
    assert!(
        problems[0].ends_with(
            "it still runs after the window closed, which found nothing of it: 4242 sleep 4711"
        ),
        "{}",
        problems[0]
    );
}

#[test]
fn what_escaped_is_found_by_its_command_and_ended_by_the_number_a_line_begins_with() {
    assert_eq!(pid_of("4242 sleep 4711"), Some(4242));
    assert_eq!(pid_of("  77 /bin/zsh -c sleep 4711"), Some(77));
    assert_eq!(
        pid_of("sleep 4711"),
        None,
        "a line with no number is no process"
    );
    assert_eq!(pid_of(""), None);
    // The command of a harness is found by the command, and the rig's stand-in, which starts
    // a job of its own, by the job's words.
    assert!(wanted(Who::Harness(Harness::Pi)).ends_with(&SECONDS.to_string()));
    assert_eq!(wanted(Who::StandIn), format!("--background-job {SECONDS}"));
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
    let problems = judge(&left).unwrap_err();
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
    // What the number of such a line begins with is still the process's.
    assert_eq!(
        pid_of(&standing("51 4 50 ??  sleep 4711").unwrap()),
        Some(51)
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
    let said = judge(&left).unwrap_err().remove(0);
    assert!(
        said.contains(
            "86330 sleep 4711 (parent 1, group 86301, terminal ??); the window had not closed when the run last looked (terminal ??: it has none, so it is in a session of its own"
        ),
        "{said}"
    );
    // One that has a terminal is not said so, nor is one a Windows machine lists.
    left.escaped = vec![standing("86330 1 86301 ttys004 sleep 4711").unwrap()];
    assert!(!judge(&left).unwrap_err()[0].contains("session of its own"));
    left.escaped = vec!["4242 sleep 4711".to_owned()];
    assert!(!judge(&left).unwrap_err()[0].contains("session of its own"));
}

#[cfg(unix)]
#[test]
fn what_runs_the_command_is_ended_and_nothing_else_is() {
    let (words, other) = ("leaving-test-4711-to-end", "leaving-test-4711-to-keep");
    // bash's `exec -a` names a process by the words it is given, as a command line holds
    // them (the `sh` of some systems has no such option).
    let run = |name: &str| {
        cf_e2e::process::Run::new("/bin/bash")
            .args(["-c", &format!("exec -a {name} sleep 30")])
            .inheriting_env()
            .spawn()
            .unwrap()
    };
    let (mut ended, mut kept) = (run(words), run(other));
    let found = |name: &str| {
        // The process may not have begun by the time it is looked for.
        (0..50).any(|_| {
            std::thread::sleep(Duration::from_millis(100));
            !on_the_machine(name).is_empty()
        })
    };
    assert!(found(words) && found(other), "both run");

    end_what_runs_on(words);

    let gone = (0..50).any(|_| {
        std::thread::sleep(Duration::from_millis(100));
        on_the_machine(words).is_empty()
    });
    assert!(gone, "what runs the command is ended");
    assert!(
        !on_the_machine(other).is_empty(),
        "and what does not is not"
    );
    ended.kill();
    kept.kill();
}

#[test]
fn a_command_still_running_once_the_chief_was_told_is_named_with_its_command_line() {
    let mut left = good();
    left.listed.push(proc(42, "/bin/zsh -c sleep 4711"));
    left.alive = vec![42];
    let problems = judge(&left).unwrap_err();
    assert_eq!(
        problems,
        ["the command is still running after the task was done and the chief told: 42 /bin/zsh -c sleep 4711"]
    );
}

#[test]
fn a_chief_told_nothing_or_told_nothing_was_ended_is_a_run_that_fails() {
    let mut left = good();
    left.notes.clear();
    assert_eq!(
        judge(&left).unwrap_err(),
        ["the chief was told nothing of the work the turn left"]
    );
    left.notes = vec!["T-1's turn left background agents running.".to_owned()];
    let problems = judge(&left).unwrap_err();
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert!(
        problems[0]
            .starts_with("the chief was told the turn left work, but not that any of it was ended"),
        "{}",
        problems[0]
    );
}

#[test]
fn a_chief_told_part_of_it_was_not_ended_gets_a_failure_that_says_so_beside_what_was_ended() {
    let mut left = good();
    left.notes = vec![
        "T-1's turn left background work running. Ended: sleep 4711. Still running, not ended: node server.js."
            .to_owned(),
    ];
    let problems = judge(&left).unwrap_err();
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert!(problems[0].contains("part of what the turn left was not ended, and still runs"));
    // Said alone, it is not that anything was ended.
    left.notes = vec![
        "T-1's turn left background work running. Still running, not ended: sleep 4711.".to_owned(),
    ];
    let problems = judge(&left).unwrap_err();
    assert_eq!(problems.len(), 2, "{problems:#?}");
}

#[test]
fn a_task_that_did_not_end_or_gave_no_result_says_so_first() {
    let left = Left {
        state: "working".to_owned(),
        took: Duration::from_secs(300),
        ..Left::default()
    };
    let problems = judge(&left).unwrap_err();
    assert_eq!(problems[0], "T-1 is working, not done after 300.0 s");
    assert_eq!(problems[1], "the chief was given no result");
    let mut empty = good();
    empty.result = Some("  \n".to_owned());
    assert_eq!(
        judge(&empty).unwrap_err(),
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
        format!("what the chief was told of work the turn left:\n    {ENDED}")
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

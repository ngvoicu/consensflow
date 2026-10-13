use cf_proto::panes::Background;

use super::*;

fn process(pid: u32, command: &str) -> BackgroundProcess {
    BackgroundProcess {
        pid,
        command: command.to_owned(),
    }
}

/// What a host lists: all of `processes`.
fn all_of(processes: Vec<BackgroundProcess>) -> Background {
    Background {
        total: processes.len(),
        processes,
    }
}

fn ending(ended: Vec<BackgroundProcess>, left: Vec<BackgroundProcess>) -> Ending {
    Ending {
        ended: all_of(ended),
        left: all_of(left),
    }
}

#[test]
fn the_requester_is_told_what_was_ended_in_one_line() {
    let ended = ending(
        vec![
            process(1, "python3 lock.py record --run 7"),
            process(2, "tail -f run.log"),
        ],
        Vec::new(),
    );
    assert_eq!(
        left_note(3, &ended, false).as_deref(),
        Some("T-3's turn left background work running. Ended: python3 lock.py record --run 7; tail -f run.log.")
    );
}

#[test]
fn what_could_not_be_ended_is_said_to_still_run_beside_what_was() {
    let both = ending(
        vec![process(1, "msedge --headless --dump-dom")],
        vec![process(2, "node server.js")],
    );
    assert_eq!(
        left_note(15, &both, false).as_deref(),
        Some("T-15's turn left background work running. Ended: msedge --headless --dump-dom. Still running, not ended: node server.js.")
    );
    let left = ending(Vec::new(), vec![process(2, "node server.js")]);
    assert_eq!(
        left_note(15, &left, false).as_deref(),
        Some("T-15's turn left background work running. Still running, not ended: node server.js.")
    );
}

#[test]
fn background_agents_are_said_to_have_been_left_with_the_work_or_alone() {
    let ended = ending(vec![process(1, "sleep 600")], Vec::new());
    assert_eq!(
        left_note(4, &ended, true).as_deref(),
        Some("T-4's turn left background work running. Ended: sleep 600. Its background agents were left running too.")
    );
    assert_eq!(
        left_note(4, &Ending::default(), true).as_deref(),
        Some("T-4's turn left background agents running.")
    );
}

#[test]
fn a_turn_that_left_nothing_says_nothing() {
    assert_eq!(left_note(1, &Ending::default(), false), None);
}

#[test]
fn a_note_quotes_the_first_commands_and_says_how_many_it_leaves_out() {
    let left: Vec<_> = (1..=7)
        .map(|n| process(n, &format!("python3 job.py {n}")))
        .collect();
    assert_eq!(
        quote(&left),
        "python3 job.py 1; python3 job.py 2; python3 job.py 3; python3 job.py 4; python3 job.py 5; and 2 more"
    );
    // A host that lists the first sixteen of twenty counts the other four too.
    let listed: Vec<_> = (1..=16)
        .map(|n| process(n, &format!("python3 job.py {n}")))
        .collect();
    let said = left_note(
        2,
        &Ending {
            ended: Background {
                total: 20,
                processes: listed,
            },
            left: Background::default(),
        },
        false,
    )
    .unwrap();
    assert!(said.ends_with("python3 job.py 5; and 15 more."), "{said}");
    // A host that lists none of the processes it names still says how many.
    let counted = Background {
        total: 2,
        processes: Vec::new(),
    };
    assert_eq!(quote_of(&counted.processes, counted.total), "2 processes");
}

#[test]
fn a_command_is_one_line_and_bounded_and_a_secret_in_it_is_hidden() {
    let long = format!("tool {}", "x".repeat(400));
    let quoted = quote(&[process(1, &long)]);
    assert_eq!(quoted.chars().count(), COMMAND_CHARS);
    assert!(quoted.ends_with('…'));

    let quoted = quote(&[process(
        2,
        "curl -H 'Authorization: Bearer abcdef0123456789abcdef' --data  a\nb",
    )]);
    assert!(!quoted.contains("abcdef0123456789abcdef"), "{quoted}");
    assert!(quoted.contains("[masked]"), "{quoted}");
    assert!(
        quoted.ends_with("(1 key- or token-like string masked)"),
        "{quoted}"
    );
    assert!(!quoted.contains('\n'));
    // In the line the requester reads the mark of what was masked is inside it.
    let note = left_note(
        5,
        &ending(
            vec![process(
                2,
                "curl -H 'Authorization: Bearer abcdef0123456789abcdef' --data  a\nb",
            )],
            Vec::new(),
        ),
        false,
    )
    .unwrap();
    assert!(!note.contains("abcdef0123456789abcdef"), "{note}");
    assert!(
        note.ends_with("(1 key- or token-like string masked)."),
        "{note}"
    );
}

#[test]
fn a_process_whose_command_could_not_be_read_is_named_by_its_number() {
    assert_eq!(quote(&[process(42, "")]), "process 42");
}

#[test]
fn a_program_named_for_the_harness_and_a_dash_is_the_harnesss_own() {
    // What Codex 0.162 starts when its agent first runs a command.
    let helper = "/opt/homebrew/Caskroom/codex/0.162.1/bin/codex-code-mode-host";
    assert!(harness_own(Harness::Codex, helper));
    assert!(harness_own(
        Harness::Codex,
        &format!("{helper} --serve --port 7")
    ));
    // On Windows, by its own separators and with its extension.
    assert!(harness_own(
        Harness::Codex,
        r"C:\tools\codex\bin\codex-code-mode-host.exe"
    ));
    // Not another harness's, nor a program that only starts with its name,
    // nor the agent's own jobs.
    assert!(!harness_own(Harness::Claude, helper));
    assert!(!harness_own(Harness::Codex, "/usr/bin/codexec --watch"));
    assert!(!harness_own(
        Harness::Codex,
        "python3 lock.py record --run 7"
    ));
    assert!(!harness_own(Harness::Codex, "tail -f codex-notes.log"));
    assert!(!harness_own(Harness::Codex, ""));
    // The harness's own program, with no dash and nothing after, is the baseline's.
    assert!(!harness_own(Harness::Codex, "/opt/homebrew/bin/codex"));
}

#[test]
fn only_the_agents_work_is_asked_to_end() {
    let helper = process(
        1,
        "/opt/homebrew/Caskroom/codex/0.162.1/bin/codex-code-mode-host",
    );
    let job = process(2, "python3 lock.py record --run 7");
    let listed = [helper, job.clone()];
    assert_eq!(
        agent_processes(Some("codex"), &listed),
        std::slice::from_ref(&job)
    );
    // A harness the roster does not know, or none, owns nothing.
    assert_eq!(agent_processes(Some("claude-code"), &listed).len(), 2);
    assert_eq!(agent_processes(Some("image"), &listed).len(), 2);
    assert_eq!(agent_processes(None, &listed).len(), 2);
}

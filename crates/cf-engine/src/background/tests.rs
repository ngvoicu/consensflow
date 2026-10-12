use super::*;

fn process(pid: u32, command: &str) -> BackgroundProcess {
    BackgroundProcess {
        pid,
        command: command.to_owned(),
    }
}

fn running(count: usize, named: &str) -> Left {
    Left::Running {
        count,
        named: named.to_owned(),
    }
}

#[test]
fn the_board_says_how_many_processes_the_window_waits_for_and_which() {
    assert_eq!(Left::Nothing.reason(), None);
    assert_eq!(
        running(1, "python3 lock.py record").reason().as_deref(),
        Some("background work running (1 process): python3 lock.py record")
    );
    assert_eq!(
        running(3, "npm exec @playwright/mcp; node server.js")
            .reason()
            .as_deref(),
        Some("background work running (3 processes): npm exec @playwright/mcp; node server.js")
    );
    // Of a list the host left empty there is a count and nothing to name.
    assert_eq!(
        running(2, "").reason().as_deref(),
        Some("background work running (2 processes)")
    );
    assert!(Left::Ended.reason().is_some());
    assert_eq!(
        Left::Agents.reason().as_deref(),
        Some("background agents running")
    );
}

#[test]
fn a_look_that_keeps_what_it_found_keeps_the_work_it_was_held_for_before_the_agents() {
    let held = |ended_at| Held {
        running: 2,
        named: "tail -f run.log".to_owned(),
        ended_at,
    };
    let (running_held, ended_held) = (held(None), held(Some(5)));
    let work = running(2, "tail -f run.log");
    assert_eq!(remembered(Some(&running_held), false), work);
    assert_eq!(remembered(Some(&running_held), true), work);
    assert_eq!(remembered(Some(&ended_held), true), Left::Ended);
    // Held for no work, a window whose turn left agents is held for them.
    assert_eq!(remembered(None, true), Left::Agents);
    assert_eq!(remembered(None, false), Left::Nothing);
}

#[test]
fn the_board_names_the_first_two_commands_held_to_a_short_line_with_a_secret_hidden() {
    let left: Vec<_> = (1..=4)
        .map(|n| process(n, &format!("python3 job.py {n}")))
        .collect();
    assert_eq!(first_named(&left), "python3 job.py 1; python3 job.py 2");
    assert_eq!(first_named(&left[..1]), "python3 job.py 1");
    assert_eq!(first_named(&[]), "");

    let long = format!("tool {}", "x".repeat(400));
    let named = first_named(&[process(1, &long)]);
    assert_eq!(named.chars().count(), NAMED_CHARS);
    assert!(named.ends_with('…'));

    let named = first_named(&[process(
        2,
        "curl -H 'Authorization: Bearer abcdef0123456789abcdef' --data  a\nb",
    )]);
    assert!(!named.contains("abcdef0123456789abcdef"), "{named}");
    assert!(named.contains("[masked]"), "{named}");
    assert!(!named.contains('\n'));
    assert_eq!(first_named(&[process(42, "")]), "process 42");
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
fn only_the_agents_work_is_counted_and_a_list_the_host_cut_short_counts_what_it_left_out() {
    let helper = process(
        1,
        "/opt/homebrew/Caskroom/codex/0.162.1/bin/codex-code-mode-host",
    );
    let job = process(2, "python3 lock.py record --run 7");
    let listed = [helper.clone(), job.clone()];
    assert_eq!(
        agent_processes(Some("codex"), &listed),
        std::slice::from_ref(&job)
    );
    // A harness the roster does not know, or none, owns nothing.
    assert_eq!(agent_processes(Some("claude-code"), &listed).len(), 2);
    assert_eq!(agent_processes(Some("image"), &listed).len(), 2);
    assert_eq!(agent_processes(None, &listed).len(), 2);

    let work = |total, processes: Vec<BackgroundProcess>| Background { total, processes };
    let counted =
        |work: Background| agent_work(&work, &agent_processes(Some("codex"), &work.processes));
    assert_eq!(counted(work(2, listed.to_vec())), 1);
    assert_eq!(counted(work(1, vec![helper])), 0);
    // Twenty processes and sixteen listed, one of them the harness's: the
    // four the host left out are the agent's.
    let mut cut_short = vec![job; 15];
    cut_short.push(process(
        3,
        "/opt/homebrew/Caskroom/codex/0.162.1/bin/codex-code-mode-host",
    ));
    assert_eq!(counted(work(20, cut_short)), 19);
}

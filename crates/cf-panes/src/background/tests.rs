use cf_proto::panes::Servers;

use super::*;

fn key(pid: u32) -> ProcessKey {
    ProcessKey {
        pid,
        began: u64::from(pid),
    }
}

/// A process of `program` started by `parent`, which began when its number says.
fn member(pid: u32, command: &str, program: &str, parent: Option<ProcessKey>) -> Member {
    Member {
        key: key(pid),
        command: command.to_owned(),
        program: program.to_owned(),
        exe: String::new(),
        group: None,
        parent,
    }
}

fn of(pid: u32) -> Option<ProcessKey> {
    Some(key(pid))
}

/// The window's program.
fn program() -> Member {
    member(10, "claude", "claude", None)
}

/// The numbers of the turn's work in `tree`, as `baseline` tells it.
fn turns(baseline: &Baseline, tree: &[Member]) -> Vec<u32> {
    baseline
        .turn_work(tree)
        .iter()
        .map(|member| member.key.pid)
        .collect()
}

/// A baseline marked where the window had `before`.
fn marked(before: &[Member], kept: Option<Kept>, harness: Option<&str>) -> Baseline {
    let mut baseline = Baseline::default();
    baseline.mark(before, kept, harness.map(str::to_owned));
    baseline
}

#[test]
fn a_pane_never_marked_has_no_background_work() {
    let mut baseline = Baseline::default();
    assert!(!baseline.is_marked());
    assert_eq!(
        baseline.left_running(&[program(), member(42, "sleep 600", "sh", of(10))]),
        Background::default()
    );
    baseline.mark(&[], None, None);
    assert!(baseline.is_marked());
}

#[test]
fn what_the_tree_had_when_the_turn_began_is_not_the_turns() {
    let before = [program(), member(11, "npx mcp-server", "node", of(10))];
    let baseline = marked(&before, None, None);
    let mut now = before.to_vec();
    now.extend([
        member(41, "bash -c python3 lock.py record", "bash", of(10)),
        member(42, "python3 lock.py record", "python3", of(41)),
    ]);
    let left = baseline.left_running(&now);
    assert_eq!(left.total, 2);
    assert_eq!(
        left.processes[1],
        BackgroundProcess {
            pid: 42,
            command: "python3 lock.py record".to_owned()
        }
    );
}

#[test]
fn a_process_that_took_the_number_of_one_that_ended_is_a_new_process() {
    let baseline = marked(
        &[program(), member(11, "npx mcp-server", "bash", of(10))],
        None,
        None,
    );
    let reused = Member {
        key: ProcessKey { pid: 11, began: 7 },
        ..member(11, "sleep 600", "bash", of(10))
    };
    let left = baseline.left_running(&[program(), reused]);
    assert_eq!(left.total, 1, "the number is the same and the start is not");
}

#[test]
fn a_new_mark_moves_what_came_before_to_where_the_tree_is() {
    let baseline = marked(&[program()], None, None);
    let tree = vec![program(), member(42, "sleep 600", "sh", of(10))];
    assert_eq!(baseline.left_running(&tree).total, 1);
    let baseline = marked(&tree, None, None);
    assert_eq!(baseline.left_running(&tree).total, 0);
}

#[test]
fn a_new_mark_says_again_which_harness_it_is_and_how_it_keeps_its_servers() {
    let wrapped = vec![
        program(),
        Member {
            group: Some(10),
            ..member(20, "sh -c npx mcp", "sh", of(10))
        },
    ];
    let before = [program()];
    let told = marked(&before, Kept::of(Servers::InGroup, Some(10)), None);
    assert_eq!(turns(&told, &wrapped), Vec::<u32>::new());
    // A mark that does not say is a window told by what a shell started, alone.
    let not_told = marked(&before, None, None);
    assert_eq!(turns(&not_told, &wrapped), [20]);
}

#[test]
fn the_oldest_processes_come_first_and_only_so_many_are_named_of_a_longer_list() {
    let baseline = marked(&[program()], None, None);
    // More jobs than a frame names, each its own shell's, the newest first.
    let tree: Vec<Member> = (0..(BACKGROUND_PROCESSES as u32 + 4))
        .rev()
        .map(|n| Member {
            key: ProcessKey {
                pid: 100 + n,
                began: u64::from(n),
            },
            ..member(100 + n, &format!("job {n}"), "sh", of(10))
        })
        .collect();
    assert_eq!(
        baseline.turn_work(&tree).len(),
        BACKGROUND_PROCESSES + 4,
        "all of it is the turn's, whatever the daemon is told"
    );
    let left = baseline.left_running(&tree);
    assert_eq!(left.total, BACKGROUND_PROCESSES + 4);
    assert_eq!(left.processes.len(), BACKGROUND_PROCESSES);
    assert_eq!(left.processes[0].command, "job 0");
    assert_eq!(
        left.processes
            .last()
            .map(|process| process.command.as_str()),
        Some(format!("job {}", BACKGROUND_PROCESSES - 1).as_str())
    );
}

#[test]
fn a_server_the_harness_starts_late_is_no_work_of_the_turn_and_is_not_named() {
    let baseline = marked(&[program()], None, None);
    let tree = vec![
        program(),
        // Claude Code's MCP servers begin as it takes its first message, after the
        // mark: no shell started them, and nothing proves them anyone's work.
        member(20, "node server.js", "node", of(10)),
        member(21, "chrome --headless", "chrome", of(20)),
        // The agent's command beside them, and what it started.
        member(40, "/bin/bash -c sleep 600", "bash", of(10)),
        member(41, "sleep 600", "sleep", of(40)),
    ];
    assert_eq!(turns(&baseline, &tree), [40, 41]);
    assert_eq!(baseline.left_running(&tree).total, 2);
}

/// A harness that ends the script of the agent's command by its number (Pi, on a stop) leaves the
/// script's child, which no shell the harness started leads to any longer: it is left running, not
/// named and not ended, until its window closes (T-142, P3-7, seen on zeewin with the `stuck` case:
/// the sleep outlived the script by up to its 200 s). What the host saw to be the turn's work while
/// the chain was whole is forgotten at the next look, as every look is made on the tree alone, and
/// the daemon asks for none during a turn. Remembering it (what was seen the turn's, and runs
/// still, is the turn's at the end of it) is a change of what the host ends, and it is not made
/// here: it needs a look the host makes itself during the turn.
#[test]
#[ignore = "issues/a-command-whose-shell-the-harness-ended-leaves-its-children-running.md: the host forgets what it saw to be the turn's work, so the child of a script the harness ended by its number is left until the window closes"]
fn what_was_seen_to_be_the_turns_work_is_still_it_when_the_shell_that_started_it_was_ended() {
    let baseline = marked(&[program()], None, None);
    let running = vec![
        program(),
        member(40, "/bin/bash -c sh ./script", "bash", of(10)),
        member(41, "sh ./script", "sh", of(40)),
        member(42, "sleep 200", "sleep", of(41)),
    ];
    // A look while the script runs: the chain is whole, and the sleep is the turn's.
    assert_eq!(turns(&baseline, &running), [40, 41, 42]);
    // The harness ended the script by its number: the shells are gone and the sleep runs on, with
    // no parent that runs. It was the turn's, and still runs.
    let after = vec![program(), member(42, "sleep 200", "sleep", None)];
    assert_eq!(turns(&baseline, &after), [42]);
}

#[test]
fn a_job_that_has_lost_its_shell_is_no_work_of_the_turn_and_is_not_named() {
    let baseline = marked(&[program()], None, None);
    let tree = vec![
        program(),
        // `nohup sleep 600 &` from a shell that ended, and a shell that lost its own
        // parent with what it runs: no chain of processes that run leads to a shell the
        // harness started.
        member(70, "sleep 600", "sleep", None),
        member(71, "bash -c sleep 700", "bash", None),
        member(72, "sleep 700", "sleep", of(71)),
        // The agent's command, whose shell runs.
        member(40, "/bin/bash -c sleep 800", "bash", of(10)),
        member(41, "sleep 800", "sleep", of(40)),
    ];
    assert_eq!(turns(&baseline, &tree), [40, 41]);
    let left = baseline.left_running(&tree);
    assert_eq!(left.total, 2, "the others are not even named");
}

#[test]
fn where_the_harness_keeps_its_servers_in_a_known_way_a_server_wrapped_in_a_shell_is_still_the_harnesss(
) {
    let tree = vec![
        program(),
        // A server the harness wrapped in a shell, in the window's group, and its
        // child.
        Member {
            group: Some(10),
            ..member(20, "sh -c npx mcp", "sh", of(10))
        },
        Member {
            group: Some(10),
            ..member(21, "node mcp.js", "node", of(20))
        },
        // The agent's command, in a group of its own.
        Member {
            group: Some(40),
            ..member(40, "bash -c make", "bash", of(10))
        },
    ];
    let known = marked(&[program()], Kept::of(Servers::InGroup, Some(10)), None);
    assert_eq!(turns(&known, &tree), [40]);
    let unknown = marked(&[program()], None, None);
    assert_eq!(turns(&unknown, &tree), [20, 21, 40]);
}

#[test]
fn a_program_of_the_harnesss_own_and_what_it_started_are_never_the_turns_though_a_shell_ran_them() {
    let at = |exe: &str, member: Member| Member {
        exe: exe.to_owned(),
        ..member
    };
    let helper = at(
        "/opt/codex/bin/codex-code-mode-host",
        member(50, "codex-code-mode-host", "codex-code-mode-host", of(11)),
    );
    let agents = [
        // The agent's command.
        at("/bin/bash", member(40, "bash -c sleep 600", "bash", of(11))),
        at("/bin/sleep", member(41, "sleep 600", "sleep", of(40))),
    ];
    let before = [
        at("/usr/local/bin/cf", program()),
        at("/opt/codex/bin/codex", member(11, "codex", "codex", of(10))),
    ];

    // The helper Codex starts as its agent first runs a command, and keeps for the
    // session, was there before this turn began: a shell it runs is the harness's.
    let kept = [before.to_vec(), vec![helper.clone()]].concat();
    let mut tree = kept.clone();
    tree.push(at("/bin/bash", member(51, "bash -c x", "bash", of(50))));
    tree.extend(agents.clone());
    assert_eq!(turns(&marked(&kept, None, Some("codex")), &tree), [40, 41]);
    // Where the daemon said no harness, it is a shell that a process from before the
    // mark started, which the host cannot tell from the agent's, and takes for it.
    assert_eq!(turns(&marked(&kept, None, None), &tree), [40, 41, 51]);

    // The helper began in this turn: it is no shell, so no shell the harness started is
    // in the chain of what it runs, named or not.
    let mut tree = before.to_vec();
    tree.extend([
        helper,
        at("/bin/bash", member(51, "bash -c x", "bash", of(50))),
    ]);
    tree.extend(agents);
    assert_eq!(
        turns(&marked(&before, None, Some("codex")), &tree),
        [40, 41]
    );
    assert_eq!(turns(&marked(&before, None, None), &tree), [40, 41]);
}

#[test]
fn what_was_seen_the_harnesss_stays_so_when_what_protected_it_has_gone() {
    let at = |exe: &str, member: Member| Member {
        exe: exe.to_owned(),
        ..member
    };
    let helper = at(
        "/opt/codex/bin/codex-code-mode-host",
        member(50, "codex-code-mode-host", "codex-code-mode-host", of(11)),
    );
    let codex = at("/opt/codex/bin/codex", member(11, "codex", "codex", of(10)));
    // A server of the harness's, begun under the helper in a turn before, which the
    // window had when this one began.
    let server = member(52, "node mcp.js", "node", of(50));
    let before = [program(), codex.clone(), helper.clone(), server.clone()];
    let baseline = marked(&before, None, Some("codex"));

    // The helper has gone, and the server, which was the helper's, runs a shell for a
    // tool of its own: none of it is the turn's, for the host saw the server the
    // harness's while the helper ran.
    let later = vec![
        program(),
        codex,
        Member {
            parent: None,
            ..server
        },
        member(60, "bash -c git status", "bash", of(52)),
        member(61, "git status", "git", of(60)),
    ];
    assert_eq!(turns(&baseline, &later), Vec::<u32>::new());

    // A host that had not seen it would take the shell for the agent's: the server
    // is from before the mark, and a shell it started has a parent that is the
    // harness's, as far as the host knows.
    let unseen = Baseline {
        own: Arc::default(),
        ..baseline.clone()
    };
    assert_eq!(turns(&unseen, &later), [60, 61]);
}

#[test]
fn what_ended_is_let_go_of_and_what_runs_is_kept_for_the_next_look() {
    let at = |exe: &str, member: Member| Member {
        exe: exe.to_owned(),
        ..member
    };
    let codex = at("/opt/codex/bin/codex", member(11, "codex", "codex", of(10)));
    let helper = at(
        "/opt/codex/bin/codex-code-mode-host",
        member(50, "codex-code-mode-host", "codex-code-mode-host", of(11)),
    );
    let server = member(52, "node mcp.js", "node", of(50));
    let baseline = marked(&[program()], None, Some("codex"));
    let remembered = |baseline: &Baseline| {
        baseline
            .own
            .lock()
            .expect("the memory is not poisoned")
            .len()
    };

    baseline.turn_work(&[program(), codex.clone(), helper.clone(), server.clone()]);
    assert_eq!(
        remembered(&baseline),
        1,
        "what the helper started, and not the helper, which a look tells by its name"
    );
    // The helper is gone and the server runs: it is kept, with no parent.
    let orphan = Member {
        parent: None,
        ..server
    };
    baseline.turn_work(&[program(), codex.clone(), orphan]);
    assert_eq!(remembered(&baseline), 1);
    // Neither runs.
    baseline.turn_work(&[program(), codex]);
    assert_eq!(remembered(&baseline), 0);
}

#[test]
fn a_process_that_was_the_harnesss_by_where_it_was_and_has_left_is_the_turns_by_what_started_it() {
    // A child begins in its parent's group, which is the window's, and moves out a
    // moment after: a look that caught it there took it for a server, and the next
    // look, which finds it in a group of its own, does not.
    let baseline = marked(&[program()], Kept::of(Servers::InGroup, Some(10)), None);
    let at = |group| Member {
        group: Some(group),
        ..member(20, "sh -c make", "sh", of(10))
    };
    assert_eq!(turns(&baseline, &[program(), at(10)]), Vec::<u32>::new());
    assert_eq!(turns(&baseline, &[program(), at(20)]), [20]);
}

#[test]
fn a_run_of_the_harnesss_status_line_is_the_turns_work_to_one_look_and_none_of_it_to_two() {
    // The window's program started the status line's `bash` after the mark, in a group
    // of its own, and the `git` it runs: a shell the harness started, and its job. That
    // is all a chain of processes asks of the turn's work, so a look that comes while a
    // run lives names both.
    let status_line = |pid, command: &str, program, parent| Member {
        group: Some(i32::try_from(pid).expect("a number")),
        ..member(pid, command, program, parent)
    };
    let baseline = marked(&[program()], Kept::of(Servers::InGroup, Some(10)), None);
    let run = vec![
        program(),
        status_line(60, "bash ~/.claude/statusline-command.sh", "bash", of(10)),
        status_line(
            61,
            "git -C /work --no-optional-locks branch --show-current",
            "git",
            of(60),
        ),
    ];
    let first = baseline.turn_work(&run);
    assert_eq!(
        first.len(),
        2,
        "a look that comes while a run lives names it"
    );

    // A moment later that run has ended, and the next is another process; a look that
    // comes then names it too, and the two looks have no process in common.
    let next = vec![
        program(),
        status_line(70, "bash ~/.claude/statusline-command.sh", "bash", of(10)),
    ];
    let later = baseline.turn_work(&next);
    assert_eq!(later.len(), 1);
    assert_eq!(steady(&first, later), Vec::<Member>::new());

    // The agent's own command lives through both, and is still there.
    let command = member(80, "bash -c make", "bash", of(10));
    let (mut during, mut after) = (run, next);
    during.push(command.clone());
    after.push(command);
    let steady_work = steady(&baseline.turn_work(&during), baseline.turn_work(&after));
    assert_eq!(
        steady_work
            .iter()
            .map(|member| member.key.pid)
            .collect::<Vec<_>>(),
        [80]
    );
}

#[test]
fn what_is_still_there_a_moment_later_is_what_both_looks_find_by_number_and_start() {
    let shell = member(41, "bash -c sleep 600", "bash", of(10));
    let first = vec![
        shell.clone(),
        member(42, "bash -c sleep 1", "bash", of(10)),
        member(43, "bash -c sleep 2", "bash", of(10)),
    ];
    // The second ended, and the third's number is another process's now: it began later.
    let reused = Member {
        key: ProcessKey { pid: 43, began: 99 },
        ..member(43, "bash -c sleep 3", "bash", of(10))
    };
    let new = member(44, "bash -c sleep 4", "bash", of(10));
    let later = vec![shell.clone(), reused, new];

    assert_eq!(steady(&first, later), vec![shell]);
    assert_eq!(steady(&first, Vec::new()), Vec::<Member>::new());
    assert_eq!(steady(&[], first), Vec::<Member>::new());
}

#[test]
fn what_an_ending_ended_started_is_what_still_runs_of_it_and_a_process_that_ended_by_itself_is_no_part_of_it(
) {
    // What a look of the handles an ending holds lists, a moment after its first look: the
    // two shells it ended (`40`, and `50`, which it could not end and runs), the ping the
    // first of them started that ended by itself meanwhile (`41`), which is still the
    // parent it names to what it started, and what the shells started while they were
    // given the grace (`42`, `51`, and `43`, which the ping started).
    let tree = [
        program(),
        member(40, "powershell -Command ping -n 2", "powershell", of(10)),
        member(41, "PING.EXE -n 2 127.0.0.1", "ping", of(40)),
        member(42, "PING.EXE -n 7 127.0.0.1", "ping", of(40)),
        member(43, "sleep 600", "sleep", of(41)),
        member(50, "powershell -Command ping -n 1006", "powershell", of(10)),
        member(51, "PING.EXE -n 1006 127.0.0.1", "ping", of(50)),
        // The window's own, which no shell of the roots started.
        member(60, "node server.js", "node", of(10)),
    ];
    let keys = |pids: &[u32]| -> HashSet<ProcessKey> { pids.iter().map(|pid| key(*pid)).collect() };
    let (roots, running) = (keys(&[40, 50]), keys(&[10, 42, 43, 50, 51, 60]));

    let mut found: Vec<u32> = started_by(&roots, &tree, &running)
        .iter()
        .map(|key| key.pid)
        .collect();
    found.sort_unstable();

    assert_eq!(
        found,
        [42, 43, 51],
        "what runs, and none that ended or is a root"
    );
    assert!(
        descending_from(&roots, &tree).contains(&key(41)),
        "a look that asks only who started whom takes the ping that ended for theirs"
    );
}

#[test]
fn what_an_ending_could_not_end_is_left_and_said_so() {
    let chosen = vec![
        member(1, "sleep 1", "sleep", of(10)),
        member(2, "sleep 2", "sleep", of(10)),
        member(3, "node server.js", "node", of(10)),
    ];
    // The second and the third are still there once the first is gone.
    let alive: HashSet<ProcessKey> = [key(2), key(3)].into_iter().collect();

    let ending = report(chosen, &alive);

    assert_eq!((ending.ended.total, ending.ended.processes[0].pid), (1, 1));
    let left: Vec<u32> = ending.left.processes.iter().map(|p| p.pid).collect();
    assert_eq!((ending.left.total, left), (2, vec![2, 3]));
}

#[test]
fn what_descends_from_a_process_is_told_by_the_parents_that_are_of_the_tree() {
    let tree = [
        program(),
        member(20, "a", "x", of(10)),
        member(21, "b", "x", of(20)),
        member(22, "c", "x", of(21)),
        // One that lost its parent descends from nothing.
        member(30, "d", "x", None),
        member(31, "e", "x", of(30)),
    ];
    let roots: HashSet<ProcessKey> = [key(20)].into_iter().collect();
    let found = descending_from(&roots, &tree);
    let mut pids: Vec<u32> = found.iter().map(|key| key.pid).collect();
    pids.sort_unstable();
    assert_eq!(pids, [20, 21, 22]);
    assert!(descending_from(&HashSet::new(), &tree).is_empty());
}

#[test]
fn a_command_line_is_held_to_one_line_and_to_the_bound() {
    assert_eq!(bounded("sh  -c\n  'sleep 5'"), "sh -c 'sleep 5'");
    let long = format!("python3 {}", "x".repeat(COMMAND_CHARS * 2));
    let cut = bounded(&long);
    assert_eq!(cut.chars().count(), COMMAND_CHARS);
    assert!(cut.ends_with('…'));
    assert!(cut.starts_with("python3 xxx"));
}

/// `member` in the process group `group`.
fn in_group(member: Member, group: i32) -> Member {
    Member {
        group: Some(group),
        ..member
    }
}

/// The numbers of the commands `baseline` tells in `tree`, or none where it cannot.
fn commands(baseline: &Baseline, tree: &[Member]) -> Option<Vec<u32>> {
    baseline
        .commands(tree, None, &|_| false)
        .map(|found| found.processes.iter().map(|process| process.pid).collect())
}

#[test]
fn a_command_is_what_the_program_started_in_a_group_of_its_own_and_no_server_or_helper_is() {
    // The shape of T-203's probe of Claude Code 2.1.307 in a pane: the program leads the
    // window's group, where it keeps its MCP server and its `caffeinate`.
    let baseline = marked(&[], Kept::of(Servers::InGroup, Some(10)), None);
    let resting = [
        in_group(program(), 10),
        in_group(member(11, "python3 server.py", "Python", of(10)), 10),
        in_group(member(12, "caffeinate -i -t 300", "caffeinate", of(10)), 10),
    ];
    assert_eq!(commands(&baseline, &resting), Some(vec![]));
    let mut running = resting.to_vec();
    running.extend([
        // A Monitor's command that `exec`ed: its shell's group, its shell's place.
        in_group(member(41, "sleep 300", "sleep", of(10)), 41),
        // A Monitor's loop, and what it runs, in its group.
        in_group(
            member(42, "/bin/zsh -c source … && eval 'while true; do sleep 1; done' < /dev/null && pwd -P >| /tmp/claude-3f1a-cwd", "zsh", of(10)),
            42,
        ),
        in_group(member(43, "sleep 1", "sleep", of(42)), 42),
        // A browser an MCP server launched, in a group of its own: the server's.
        in_group(member(44, "chrome --headless", "chrome", of(11)), 44),
        // What a command left when its shell ended: in the command's group.
        in_group(member(45, "python3 -m http.server", "python3", None), 42),
        // A hook, for the moment it runs.
        in_group(member(46, "/bin/sh -c printf '{}'", "sh", of(10)), 46),
        // One whose group could not be read.
        member(47, "sleep 9", "sleep", of(10)),
        // A daemon a command left that leads a session of its own.
        in_group(member(48, "dockerd", "dockerd", None), 48),
    ]);
    assert_eq!(
        commands(&baseline, &running),
        Some(vec![41, 42, 46, 47, 48])
    );
}

#[test]
fn the_commands_are_not_told_of_a_window_whose_servers_are_not_in_its_group() {
    let tree = [
        in_group(program(), 10),
        in_group(member(41, "sleep 300", "sleep", of(10)), 41),
    ];
    for kept in [
        None,
        Kept::of(Servers::AtLaunch, Some(10)),
        Kept::of(Servers::InGroup, None),
    ] {
        assert_eq!(commands(&marked(&[], kept, None), &tree), None, "{kept:?}");
    }
    assert_eq!(commands(&Baseline::default(), &tree), None);
}

#[test]
fn what_ran_at_the_rest_a_look_names_is_no_command_and_what_began_after_it_is() {
    // Astraeus on 472b1d96: an MCP server that left the window's group (`setsid`) leads a
    // group of its own and was started by the program, as a command is; it ran when the
    // window was at rest before the monitor began, and no command the monitor runs did.
    let mut baseline = marked(&[], Kept::of(Servers::InGroup, Some(10)), None);
    let server = in_group(member(11, "python3 server.py", "Python", of(10)), 11);
    let resting = [in_group(program(), 10), server.clone()];
    let rest = baseline.rest(&resting, &[]).expect("a rest kept");
    let mut running = resting.to_vec();
    running.push(in_group(member(41, "sleep 300", "sleep", of(10)), 41));
    assert_eq!(
        commands_since(&baseline, &running, Some(rest)),
        Some(vec![41])
    );
    // A look that names no rest, or one not kept, leaves nothing out: the server counts.
    assert_eq!(
        commands_since(&baseline, &running, None),
        Some(vec![11, 41])
    );
    assert_eq!(
        commands_since(&baseline, &running, Some(rest + 1)),
        Some(vec![11, 41])
    );
    // The last rests are kept, and the oldest let go.
    for _ in 0..RESTS {
        baseline.rest(&resting, &[]);
    }
    assert_eq!(
        commands_since(&baseline, &running, Some(rest)),
        Some(vec![11, 41])
    );
    // A window whose commands are not told keeps no rest.
    let mut untold = marked(&[], None, None);
    assert_eq!(untold.rest(&resting, &[]), None);
    assert!(!untold.tells_commands());
}

/// [`commands`], less what ran at the rest `since`.
fn commands_since(baseline: &Baseline, tree: &[Member], since: Option<u64>) -> Option<Vec<u32>> {
    baseline
        .commands(tree, since, &|_| false)
        .map(|found| found.processes.iter().map(|process| process.pid).collect())
}

#[test]
fn a_rest_a_monitor_names_is_kept_however_many_rests_come_after_it() {
    // Astraeus on 136a7158: a monitor whose task was woken more than sixteen times after it
    // was armed named a rest no longer kept, and the server that left the group counted
    // again, for good.
    let mut baseline = marked(&[], Kept::of(Servers::InGroup, Some(10)), None);
    let server = in_group(member(11, "python3 server.py", "Python", of(10)), 11);
    let resting = [in_group(program(), 10), server.clone()];
    let rest = baseline.rest(&resting, &[]).expect("a rest kept");
    let mut running = resting.to_vec();
    running.push(in_group(member(41, "sleep 300", "sleep", of(10)), 41));
    for _ in 0..(RESTS + 4) {
        baseline.rest(&running, &[rest]);
    }
    // The monitor's command is gone; the server runs on.
    assert_eq!(
        commands_since(&baseline, &resting, Some(rest)),
        Some(vec![])
    );
}

/// A pane opened at `opened` (microseconds since the epoch) whose clock was read every
/// 100 ms for `seconds`, its wall clock `off(t)` ahead of where it was, `t` microseconds
/// after the open; and the awake start of a process begun `t` after the open.
fn clock_from(opened: i64, seconds: i64, off: impl Fn(i64) -> i64) -> PaneClock {
    let reading = |t: i64| Reading {
        wall: opened + t + off(t),
        awake: 1_000_000 + t,
        continuous: 3_000_000 + t,
    };
    let mut clock = PaneClock::new(Some(reading(0)));
    for tick in 1..=seconds * 10 {
        clock.observe(Some(reading(tick * 100_000)));
    }
    clock
}

/// `member`, begun `t` microseconds after a pane opened at `opened`, its wall clock `off`
/// ahead then.
fn begun(member: Member, opened: i64, t: i64, off: i64) -> Member {
    Member {
        key: ProcessKey {
            pid: member.key.pid,
            began: u64::try_from(opened + t + off).unwrap(),
        },
        ..member
    }
}

/// The numbers of the commands `baseline` tells in `tree`, less what `clock` proves began
/// before `after` (milliseconds), the awake start of each by `awake` (microseconds after the
/// open, as [`clock_from`] has them).
fn commands_after(
    baseline: &Baseline,
    tree: &[Member],
    clock: &PaneClock,
    after: i64,
    awake: &HashMap<u32, i64>,
) -> Option<Vec<u32>> {
    let before = began_before(clock, after, |pid| awake.get(&pid).map(|t| 1_000_000 + t));
    baseline.commands(tree, None, &before).map(|found| {
        let mut pids: Vec<u32> = found.processes.iter().map(|process| process.pid).collect();
        pids.sort_unstable();
        pids
    })
}

#[test]
fn a_process_that_began_before_the_monitors_were_called_is_no_command() {
    // The chief on T-301: a worker's first turn has no rest before it (its message is on
    // the command line), and a monitor's command begins after its call. A server that left
    // the group at launch began before the call; one begun after it may be a command, and
    // counts; one whose start was not read counts, the safe way.
    let baseline = marked(&[], Kept::of(Servers::InGroup, Some(10)), None);
    // The pane opened ten seconds before the call, whose record says 1_000_000_000 ms.
    let call_ms: i64 = 1_000_000_000;
    let opened = call_ms * 1_000 - 10_000_000;
    let clock = clock_from(opened, 20, |_| 0);
    let tree = [
        in_group(program(), 10),
        begun(
            in_group(member(11, "python3 server.py", "Python", of(10)), 11),
            opened,
            5_000_000,
            0,
        ),
        begun(
            in_group(member(41, "sleep 300", "sleep", of(10)), 41),
            opened,
            10_040_000,
            0,
        ),
        begun(
            in_group(member(42, "python3 late.py", "Python", of(10)), 42),
            opened,
            10_900_000,
            0,
        ),
        in_group(member(43, "unread", "", of(10)), 43),
    ];
    let awake: HashMap<u32, i64> = [(11, 5_000_000), (41, 10_040_000), (42, 10_900_000)].into();
    assert_eq!(
        commands(&baseline, &tree),
        Some(vec![43, 11, 41, 42]),
        "oldest first: the one whose start was not read, then by start"
    );
    assert_eq!(
        commands_after(&baseline, &tree, &clock, call_ms, &awake),
        Some(vec![41, 42, 43])
    );
    // A server whose awake start was not read is counted.
    assert_eq!(
        commands_after(&baseline, &tree, &clock, call_ms, &HashMap::new()),
        Some(vec![11, 41, 42, 43])
    );
}

#[test]
fn a_command_begun_after_its_call_while_the_clock_was_set_back_is_counted() {
    // Astraeus on 02a6cd4f: the call is stamped 12:00:10.000; before its command starts
    // the clock is set back two seconds, and the command begins at 12:00:08.020 by it. A
    // look that names the call less the margin left the command out, and the task ended.
    let baseline = marked(&[], Kept::of(Servers::InGroup, Some(10)), None);
    let call_ms: i64 = 1_792_670_410_000;
    let opened = call_ms * 1_000 - 10_000_000;
    let clock = clock_from(opened, 20, |t| if t > 10_010_000 { -2_000_000 } else { 0 });
    let tree = [
        in_group(program(), 10),
        begun(
            in_group(member(11, "python3 server.py", "Python", of(10)), 11),
            opened,
            1_000_000,
            0,
        ),
        begun(
            in_group(member(41, "sleep 600", "sleep", of(10)), 41),
            opened,
            10_020_000,
            -2_000_000,
        ),
    ];
    let awake: HashMap<u32, i64> = [(11, 1_000_000), (41, 10_020_000)].into();
    assert_eq!(
        commands_after(&baseline, &tree, &clock, call_ms - 1_000, &awake),
        Some(vec![11, 41]),
        "the command runs, and the server cannot be told from one"
    );
}

use cf_proto::panes::Servers;

use super::*;

fn key(pid: u32) -> ProcessKey {
    ProcessKey {
        pid,
        began: u64::from(pid),
    }
}

/// A process of `program` started by `parent`, which began when its number says.
fn member(pid: u32, command: &str, program: &str, parent: Parent) -> Member {
    Member {
        key: key(pid),
        command: command.to_owned(),
        program: program.to_owned(),
        exe: String::new(),
        group: None,
        parent,
    }
}

fn of(pid: u32) -> Parent {
    Parent::Of(key(pid))
}

/// The window's program.
fn program() -> Member {
    member(10, "claude", "claude", Parent::Gone)
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
        baseline.left_running(&[program(), member(42, "sleep 600", "sh", Parent::Gone)]),
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
    now.push(member(
        42,
        "python3 lock.py record",
        "python3",
        Parent::Gone,
    ));
    let left = baseline.left_running(&now);
    assert_eq!(left.total, 1);
    assert_eq!(
        left.processes,
        vec![BackgroundProcess {
            pid: 42,
            command: "python3 lock.py record".to_owned()
        }]
    );
}

#[test]
fn a_process_that_took_the_number_of_one_that_ended_is_a_new_process() {
    let baseline = marked(
        &[member(11, "npx mcp-server", "node", Parent::Gone)],
        None,
        None,
    );
    let reused = Member {
        key: ProcessKey { pid: 11, began: 7 },
        ..member(11, "sleep 600", "sleep", Parent::Gone)
    };
    let left = baseline.left_running(&[reused]);
    assert_eq!(left.total, 1, "the number is the same and the start is not");
}

#[test]
fn a_new_mark_moves_what_came_before_to_where_the_tree_is() {
    let baseline = marked(&[program()], None, None);
    let tree = vec![program(), member(42, "sleep 600", "sleep", Parent::Gone)];
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
    // A mark that does not say is a window told by what a shell started and by
    // what lost its parent, alone.
    let not_told = marked(&before, None, None);
    assert_eq!(turns(&not_told, &wrapped), [20]);
}

#[test]
fn the_oldest_processes_come_first_and_only_so_many_are_named_of_a_longer_list() {
    let baseline = marked(&[], None, None);
    // More jobs than a frame names, each its own shell's, the newest first.
    let tree: Vec<Member> = (0..(BACKGROUND_PROCESSES as u32 + 4))
        .rev()
        .map(|n| Member {
            key: ProcessKey {
                pid: 100 + n,
                began: u64::from(n),
            },
            ..member(100 + n, &format!("job {n}"), "sh", Parent::Gone)
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
    let before = [
        at("/usr/local/bin/cf", program()),
        at("/opt/codex/bin/codex", member(11, "codex", "codex", of(10))),
    ];
    let baseline = marked(&before, None, Some("codex"));
    let mut tree = before.to_vec();
    tree.extend([
        // The helper Codex starts as its agent first runs a command, a shell it
        // runs, and one that lost its parent: all the harness's.
        at(
            "/opt/codex/bin/codex-code-mode-host",
            member(50, "codex-code-mode-host", "codex-code-mode-host", of(11)),
        ),
        at("/bin/bash", member(51, "bash -c x", "bash", of(50))),
        at(
            "/opt/codex/bin/codex-code-mode-host",
            member(
                53,
                "codex-code-mode-host",
                "codex-code-mode-host",
                Parent::Gone,
            ),
        ),
        // The agent's command.
        at("/bin/bash", member(40, "bash -c sleep 600", "bash", of(11))),
        at("/bin/sleep", member(41, "sleep 600", "sleep", of(40))),
    ]);
    assert_eq!(turns(&baseline, &tree), [40, 41]);
    // Where the daemon said no harness, the shell the helper ran and the helper
    // that lost its parent are taken for jobs; the helper itself, which no shell
    // started and which has its parent, is left alone all the same.
    let nameless = marked(&before, None, None);
    assert_eq!(turns(&nameless, &tree), [40, 41, 51, 53]);
}

#[test]
fn what_an_ending_could_not_end_is_left_and_said_so() {
    let chosen = vec![
        member(1, "sleep 1", "sleep", Parent::Gone),
        member(2, "sleep 2", "sleep", Parent::Gone),
        member(3, "node server.js", "node", Parent::Gone),
    ];
    // The second and the third are still there once the first is gone.
    let alive: HashSet<ProcessKey> = [key(2), key(3)].into_iter().collect();

    let ending = report(chosen, &alive);

    assert_eq!((ending.ended.total, ending.ended.processes[0].pid), (1, 1));
    let left: Vec<u32> = ending.left.processes.iter().map(|p| p.pid).collect();
    assert_eq!((ending.left.total, left), (2, vec![2, 3]));
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

use super::*;
use crate::background::Parent;

/// The window's program, which leads the window's group.
const PANE: i32 = 10;

fn key(pid: u32) -> ProcessKey {
    ProcessKey {
        pid,
        began: u64::from(pid),
    }
}

fn of(pid: u32) -> Parent {
    Parent::Of(key(pid))
}

/// A process in the process group `group`, started by the process `parent`.
fn started(pid: u32, command: &str, group: i32, parent: u32) -> Member {
    Member {
        key: key(pid),
        command: command.to_owned(),
        program: String::new(),
        exe: String::new(),
        group: Some(group),
        parent: of(parent),
    }
}

/// The window's program.
fn program() -> Member {
    Member {
        key: key(10),
        command: "claude".to_owned(),
        program: "claude".to_owned(),
        exe: String::new(),
        group: Some(PANE),
        parent: Parent::Gone,
    }
}

/// The numbers of the harness's own servers among `tree`, less the window's
/// program, which is in its own group: where the window had `launch` when its first
/// turn began, and the daemon said `servers`.
fn own_of(servers: Servers, launch: &[Member], tree: &[Member]) -> Vec<u32> {
    let kept = Kept::of(servers, Some(PANE)).expect("a known way");
    let launch: HashSet<ProcessKey> = launch.iter().map(|member| member.key).collect();
    let mut own: Vec<u32> = own(kept, &launch, tree)
        .iter()
        .map(|key| key.pid)
        .filter(|pid| *pid != 10)
        .collect();
    own.sort_unstable();
    own
}

// The servers in the window's group: Claude Code's and Pi's.

#[test]
fn what_a_server_in_the_windows_group_started_is_the_harnesss_whatever_group_it_is_in() {
    // The tree of a worker's window after its agent used the browser tool.
    let tree = vec![
        program(),
        started(20, "npm exec chrome-devtools-mcp", PANE, 10),
        started(21, "chrome-devtools-mcp", PANE, 20),
        // The browser the server launches, and what the browser starts: each in a
        // group of its own.
        started(30, "Google Chrome", 30, 21),
        started(31, "Google Chrome Helper", 30, 30),
        started(32, "chrome_crashpad_handler", 32, 30),
    ];
    assert_eq!(
        own_of(Servers::InGroup, &[program()], &tree),
        [20, 21, 30, 31, 32]
    );
}

#[test]
fn what_a_tools_shell_started_is_not_the_harnesss_and_the_windows_program_is_no_ancestor_that_counts(
) {
    let tree = vec![
        program(),
        // The shell the Bash tool runs a command in: a group of its own, the
        // window's program its parent.
        started(40, "/bin/zsh -c source snapshot.sh && eval 'make'", 40, 10),
        started(41, "make", 40, 40),
        // A job the command left, in a group of its own too.
        started(42, "npm run dev", 42, 41),
    ];
    assert_eq!(
        own_of(Servers::InGroup, &[program()], &tree),
        Vec::<u32>::new()
    );
}

#[test]
fn a_server_and_a_tools_shell_side_by_side_are_told_apart_by_the_group_and_what_each_started() {
    let tree = vec![
        program(),
        started(20, "npm exec @playwright/mcp@latest", PANE, 10),
        started(21, "playwright-mcp", PANE, 20),
        started(22, "watchdog", 22, 21),
        started(40, "/bin/zsh -c eval 'sleep 600'", 40, 10),
        started(41, "sleep 600", 40, 40),
    ];
    assert_eq!(own_of(Servers::InGroup, &[program()], &tree), [20, 21, 22]);
}

#[test]
fn ancestry_reaches_through_as_many_processes_as_there_are() {
    let tree = vec![
        program(),
        started(20, "npm exec x", PANE, 10),
        started(21, "sh -c x", 21, 20),
        started(22, "node x", 22, 21),
        started(23, "chrome", 23, 22),
        started(24, "chrome helper", 23, 23),
    ];
    assert_eq!(
        own_of(Servers::InGroup, &[program()], &tree),
        [20, 21, 22, 23, 24]
    );
}

#[test]
fn a_process_whose_parent_has_gone_is_told_by_its_own_group_alone() {
    let orphan = |pid, group| Member {
        parent: Parent::Gone,
        ..started(pid, "node browser", group, 20)
    };
    // Its group is its own: not told to be the harness's, though a server in the
    // window's group started it.
    assert_eq!(
        own_of(Servers::InGroup, &[program()], &[program(), orphan(30, 30)]),
        Vec::<u32>::new()
    );
    // Its group is the window's: the harness's.
    assert_eq!(
        own_of(
            Servers::InGroup,
            &[program()],
            &[program(), orphan(31, PANE)]
        ),
        [31]
    );
}

#[test]
fn a_process_whose_group_cannot_be_read_is_told_by_its_ancestry_and_by_nothing_else() {
    let unread = |pid, parent: Parent| Member {
        group: None,
        parent,
        ..started(pid, "node x", 0, 0)
    };
    let tree = vec![
        program(),
        started(20, "npm exec x", PANE, 10),
        unread(30, of(20)),
        unread(31, Parent::Gone),
        unread(32, of(10)),
    ];
    assert_eq!(own_of(Servers::InGroup, &[program()], &tree), [20, 30]);
}

#[test]
fn a_group_the_platform_does_not_say_is_no_way_of_telling_them() {
    assert_eq!(Kept::of(Servers::InGroup, None), None);
    assert_eq!(Kept::of(Servers::AtLaunch, None), None);
    assert_eq!(Kept::of(Servers::Unknown, Some(PANE)), None);
    assert_eq!(
        Kept::of(Servers::AtLaunch, Some(PANE)),
        Some(Kept {
            group: PANE,
            servers: Servers::AtLaunch
        })
    );
}

#[test]
fn a_list_that_names_its_processes_as_each_others_parents_is_no_tree_and_ends() {
    let mut a = started(50, "a", 50, 51);
    let mut b = started(51, "b", 51, 50);
    a.group = Some(50);
    b.group = Some(51);
    assert_eq!(
        own_of(Servers::InGroup, &[program()], &[program(), a, b]),
        Vec::<u32>::new()
    );
}

// The servers a window opens with, outside its group: Codex's.

/// The tree of a Codex window as it opens: the supervisor that opens it (the
/// window's program), Codex's own process and its TUI in the window's group, and
/// two servers in groups of their own, the second with a process of its.
fn codex_at_launch() -> Vec<Member> {
    vec![
        Member {
            command: "cf codex-session codex".to_owned(),
            ..program()
        },
        started(11, "codex", PANE, 10),
        started(12, "codex --remote ws://127.0.0.1:1", PANE, 10),
        started(20, "SkyComputerUseClient mcp", 20, 11),
        started(21, "cua-repl.mjs", 21, 11),
        started(22, "node_repl", 22, 11),
        started(23, "node_repl", 21, 21),
    ]
}

#[test]
fn what_a_server_the_window_opened_with_starts_later_is_the_harnesss() {
    let before = codex_at_launch();
    let mut tree = before.clone();
    tree.extend([
        // Two processes of the REPL that the agent's tool call started.
        started(30, "node --experimental-vm-modules kernel.js", 22, 22),
        started(31, "node trusted-worker.js", 22, 22),
        started(32, "node a worker's own", 32, 30),
    ]);
    assert_eq!(
        own_of(Servers::AtLaunch, &before, &tree),
        [20, 21, 22, 23, 30, 31, 32]
    );
}

#[test]
fn what_the_harnesss_own_process_starts_is_no_servers_though_it_was_there_from_the_launch() {
    let before = codex_at_launch();
    let mut tree = before.clone();
    tree.extend([
        // A command the agent's tool ran, in a session of its own, and what it left.
        started(40, "/bin/zsh -lc make", 40, 11),
        started(41, "make", 40, 40),
        started(42, "npm run dev", 42, 41),
        // What the harness's own process starts besides, in a group of its own: no
        // server of the launch, which this shape has no way to say is its own.
        started(50, "codex-code-mode-host", 50, 11),
    ]);
    assert_eq!(own_of(Servers::AtLaunch, &before, &tree), [20, 21, 22, 23]);
}

#[test]
fn a_process_that_began_after_the_first_turn_is_no_server_of_the_launch() {
    let launch = codex_at_launch();
    // The first turn's agent opened a session in a group of its own, which the next
    // turn's mark finds there: what it starts is not a server's.
    let session = started(40, "bash", 40, 11);
    let mut now = launch.clone();
    now.push(session);
    now.extend([
        started(41, "sleep 600", 40, 40),
        // And a server of the launch still starts what it starts.
        started(42, "node kernel.js", 22, 22),
    ]);
    assert_eq!(
        own_of(Servers::AtLaunch, &launch, &now),
        [20, 21, 22, 23, 42]
    );
}

#[test]
fn a_server_whose_group_cannot_be_read_is_not_known_to_be_outside_the_windows() {
    let unread = |pid, parent| Member {
        group: None,
        ..started(pid, "node x", 0, parent)
    };
    let before = vec![program(), unread(20, 10)];
    let tree = vec![program(), unread(20, 10), started(30, "child", 30, 20)];
    assert_eq!(own_of(Servers::AtLaunch, &before, &tree), Vec::<u32>::new());
}

#[test]
fn what_began_in_the_windows_group_is_not_a_servers_where_they_are_kept_at_launch() {
    let before = codex_at_launch();
    let mut tree = before.clone();
    tree.push(started(60, "a helper of codex's own", PANE, 11));
    assert_eq!(own_of(Servers::AtLaunch, &before, &tree), [20, 21, 22, 23]);
}

#[test]
fn the_two_shapes_take_what_a_process_outside_the_group_that_was_there_from_the_launch_starts_differently(
) {
    // The window's program started a process in a group of its own before
    // the first turn, which starts a process after it.
    let before = vec![program(), started(20, "a process of its own", 20, 10)];
    let mut tree = before.clone();
    tree.push(started(30, "what it started", 20, 20));
    // Where the harness keeps its servers in the window's group it is nothing of
    // theirs: it may be what the agent told it to start.
    assert_eq!(own_of(Servers::InGroup, &before, &tree), Vec::<u32>::new());
    // Where it opens them with the window outside it, it is a server's.
    assert_eq!(own_of(Servers::AtLaunch, &before, &tree), [20, 30]);
}

// The programs a harness is named for.

/// A program run from `exe`, whose parent is the process `parent`.
fn run(pid: u32, exe: &str, parent: Parent) -> Member {
    let program = exe.rsplit(['/', '\\']).next().unwrap_or_default();
    Member {
        exe: exe.to_owned(),
        program: program.to_owned(),
        parent,
        ..started(pid, program, 0, 0)
    }
}

/// The numbers `programs` says are the harness `harness`'s, in order.
fn programs_of(harness: &str, tree: &[Member]) -> Vec<u32> {
    let mut own: Vec<u32> = programs(harness, tree).iter().map(|key| key.pid).collect();
    own.sort_unstable();
    own
}

#[test]
fn a_program_named_for_the_harness_that_runs_beside_its_binary_is_the_harnesss_and_so_is_what_descends_from_it(
) {
    let tree = vec![
        run(10, "/usr/local/bin/cf", Parent::Gone),
        run(11, "/opt/codex/bin/codex", of(10)),
        // The helper Codex starts the first time its agent runs a command.
        run(50, "/opt/codex/bin/codex-code-mode-host", of(11)),
        // A shell it runs, and a job that shell left: the harness's, whatever they are.
        run(51, "/bin/bash", of(50)),
        run(52, "/bin/sleep", of(51)),
        // The agent's command, started by the harness's own binary.
        run(40, "/bin/bash", of(11)),
        run(41, "/bin/sleep", of(40)),
    ];
    assert_eq!(programs_of("codex", &tree), [50, 51, 52]);
}

#[test]
fn a_path_with_spaces_in_it_is_whole_and_no_matter() {
    let tree = vec![
        run(
            11,
            "/Applications/Codex App/Contents/MacOS/codex",
            Parent::Gone,
        ),
        run(
            50,
            "/Applications/Codex App/Contents/MacOS/codex-code-mode-host",
            of(11),
        ),
        run(
            51,
            r"C:\Program Files\Codex App\bin\codex-code-mode-host.exe",
            of(11),
        ),
    ];
    assert_eq!(programs_of("codex", &tree), [50]);
}

#[test]
fn windows_names_have_an_extension_and_no_case() {
    let tree = vec![
        run(
            11,
            r"C:\Program Files\Codex App\bin\CODEX.EXE",
            Parent::Gone,
        ),
        run(
            51,
            r"C:\Program Files\Codex App\bin\Codex-Code-Mode-Host.exe",
            of(11),
        ),
        // Its directories have no case either, on Windows.
        run(
            52,
            r"C:\PROGRAM FILES\codex app\BIN\codex-code-mode-host.exe",
            of(11),
        ),
    ];
    let expected: &[u32] = if cfg!(windows) { &[51, 52] } else { &[51] };
    assert_eq!(programs_of("codex", &tree), expected);
}

#[test]
fn a_program_named_for_the_harness_that_is_not_beside_its_binary_is_anybodys() {
    let tree = vec![
        run(11, "/opt/codex/bin/codex", Parent::Gone),
        run(50, "/tmp/work/codex-code-mode-host", of(11)),
        // Not named for it: beside the binary, and not a helper of it.
        run(51, "/opt/codex/bin/node", of(11)),
        run(52, "/opt/codex/bin/codexify", of(11)),
    ];
    assert_eq!(programs_of("codex", &tree), Vec::<u32>::new());
}

#[test]
fn where_the_harnesss_binary_is_not_among_what_the_tree_has_no_program_is_proved_its() {
    let tree = vec![run(50, "/opt/codex/bin/codex-code-mode-host", Parent::Gone)];
    assert_eq!(programs_of("codex", &tree), Vec::<u32>::new());
}

#[test]
fn a_program_named_for_another_harness_is_no_helper_of_this_one() {
    let tree = vec![
        run(11, "/opt/codex/bin/codex", Parent::Gone),
        run(50, "/opt/codex/bin/claude-code-mode-host", of(11)),
    ];
    assert_eq!(programs_of("codex", &tree), Vec::<u32>::new());
    assert_eq!(programs_of("claude", &tree), Vec::<u32>::new());
}

#[test]
fn a_binary_run_under_the_harnesss_name_has_its_directory_for_the_programs_beside_it() {
    // A launcher the system keeps the name of: the program is `claude`, the file is
    // a version.
    let tree = vec![
        Member {
            program: "claude".to_owned(),
            ..run(
                11,
                "/home/me/.local/share/claude/versions/2.1.296",
                Parent::Gone,
            )
        },
        run(
            50,
            "/home/me/.local/share/claude/versions/claude-helper",
            of(11),
        ),
    ];
    assert_eq!(programs_of("claude", &tree), [50]);
}

#[test]
fn the_name_of_the_harness_is_told_in_lower_case() {
    let tree = vec![
        run(11, "/opt/Codex/bin/Codex", Parent::Gone),
        run(50, "/opt/Codex/bin/Codex-Helper", of(11)),
    ];
    assert_eq!(programs_of("Codex", &tree), [50]);
}

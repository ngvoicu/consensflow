//! Which of the processes a window's tree began after a turn's mark are its
//! harness's own, and so no work the turn started. The host cannot tell them
//! from the agent's work by anything the processes are, and by when they began
//! ([`super::Baseline`]) it is wrong for a server that begins late and for what a
//! server starts. What the harness is known to do with its servers (`pane.mark`'s
//! `servers`) gives it more to go by, in one of two shapes ([`Servers`]), and both
//! go by ancestry: [`Member::parent`] says which process of the tree started each.
//!
//! The window's program leads the window's group. It is the harness, or the
//! supervisor that opens it, and the agent's commands are started by the harness's
//! own process: the window's program (Claude Code, Pi) or one beside it in the
//! window's group (Codex's own process, under the supervisor). So a walk up from a
//! process, through the parents that are processes of the tree, stops at the
//! window's program: it is in the window's group, and it would make everything of
//! the window the harness's. The process is the harness's if it, or a process the
//! walk meets, is a server:
//!
//! - [`Servers::InGroup`]: a server is a process in the window's group. What a
//!   server starts is the harness's whatever group it starts it in, which a
//!   browser a server launches is: a tool's command is not, for its shell is in a
//!   group of its own and its parent is the window's program.
//! - [`Servers::AtLaunch`]: a server is a process the window had when its first
//!   turn began and that is outside the window's group. The harness's own process
//!   is there from the launch too, but in the window's group: what it starts is
//!   the agent's. A process that began after the first turn is no server of the
//!   launch, whatever the mark after it took for the turn's past: a session the
//!   agent opened in a turn, and what it starts in the next, are the agent's.
//!
//! A process whose parent has gone has no ancestry to look at, and is told by its
//! own place alone, as a process of an unknown harness is told by time: the system
//! keeps naming the process that started it (macOS 26.x, seen with a `sleep` and a
//! Python that `sh` left), which is no process of the tree, and `Member::parent`
//! says none. A browser that outlives the server that started it is the turn's.

use std::collections::{HashMap, HashSet};

use cf_proto::panes::Servers;

use super::{Member, ProcessKey};

/// What the daemon said of the window's harness at a mark, with the process
/// group of the window's program, which leads it: the window's group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Kept {
    /// The window's group.
    pub group: i32,
    /// How its harness keeps its servers, which is known: a window whose harness
    /// is not known to keep them in a way has no `Kept`.
    pub servers: Servers,
}

impl Kept {
    /// What `servers` says of a window whose program leads `group`: none where it
    /// says it is not known, and where the platform has no groups.
    pub fn of(servers: Servers, group: Option<i32>) -> Option<Self> {
        let group = group?;
        (!servers.is_unknown()).then_some(Self { group, servers })
    }
}

/// The processes of `tree` that are the harness's own. `launch` is the processes
/// the window had when its first turn began.
pub(super) fn own(
    kept: Kept,
    launch: &HashSet<ProcessKey>,
    tree: &[Member],
) -> HashSet<ProcessKey> {
    let by_key: HashMap<ProcessKey, &Member> =
        tree.iter().map(|member| (member.key, member)).collect();
    tree.iter()
        .filter(|member| descends_from_a_server(kept, launch, &by_key, member))
        .map(|member| member.key)
        .collect()
}

/// Whether `member` is a server, or a process one started, by the walk up from it.
fn descends_from_a_server(
    kept: Kept,
    launch: &HashSet<ProcessKey>,
    by_key: &HashMap<ProcessKey, &Member>,
    member: &Member,
) -> bool {
    let mut at = member;
    // A tree names no process twice among a process's ancestors: the bound is
    // only for a list that is no tree.
    for _ in 0..=by_key.len() {
        if is_server(kept, launch, at) {
            return true;
        }
        match at.parent.and_then(|parent| by_key.get(&parent)) {
            Some(parent) if !leads(kept, parent) => at = parent,
            _ => return false,
        }
    }
    false
}

/// Whether `member` is a server in the way the harness keeps them.
fn is_server(kept: Kept, launch: &HashSet<ProcessKey>, member: &Member) -> bool {
    match kept.servers {
        Servers::InGroup => member.group == Some(kept.group),
        // A process whose group could not be read is not known to be outside the window's.
        Servers::AtLaunch => {
            launch.contains(&member.key) && member.group.is_some_and(|group| group != kept.group)
        }
        Servers::Unknown => false,
    }
}

/// Whether `member` is the window's program: it leads the window's group, so the
/// group's number is its own.
fn leads(kept: Kept, member: &Member) -> bool {
    i32::try_from(member.key.pid) == Ok(kept.group)
}

#[cfg(test)]
mod tests {
    use cf_proto::panes::BackgroundProcess;

    use super::super::Baseline;
    use super::*;

    /// The window's program, which leads the window's group.
    const PANE: i32 = 10;

    fn key(pid: u32) -> ProcessKey {
        ProcessKey {
            pid,
            began: u64::from(pid),
        }
    }

    /// A process in the process group `group`, started by the process `parent`.
    fn started(pid: u32, command: &str, group: i32, parent: u32) -> Member {
        Member {
            key: key(pid),
            command: command.to_owned(),
            group: Some(group),
            parent: Some(key(parent)),
        }
    }

    /// The window's program.
    fn program() -> Member {
        Member {
            key: key(10),
            command: "claude".to_owned(),
            group: Some(PANE),
            parent: None,
        }
    }

    /// What `tree` has that began after the mark, where the tree had `before`
    /// when the turn began, and the daemon said `servers`.
    fn left(servers: Servers, before: &[Member], tree: Vec<Member>) -> Vec<u32> {
        let mut baseline = Baseline::default();
        baseline.mark(before, Kept::of(servers, Some(PANE)));
        pids(baseline.left_running(tree).processes)
    }

    fn pids(processes: Vec<BackgroundProcess>) -> Vec<u32> {
        processes.iter().map(|process| process.pid).collect()
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
            left(Servers::InGroup, &[program()], tree),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn what_a_tools_shell_started_is_the_turns_and_the_window_s_program_is_no_ancestor_that_counts()
    {
        let tree = vec![
            program(),
            // The shell the Bash tool runs a command in: a group of its own, the
            // window's program its parent.
            started(40, "/bin/zsh -c source snapshot.sh && eval 'make'", 40, 10),
            started(41, "make", 40, 40),
            // A job the command left, in a group of its own too.
            started(42, "npm run dev", 42, 41),
        ];
        assert_eq!(left(Servers::InGroup, &[program()], tree), vec![40, 41, 42]);
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
        assert_eq!(left(Servers::InGroup, &[program()], tree), vec![40, 41]);
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
            left(Servers::InGroup, &[program()], tree),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn a_server_the_window_already_had_is_an_ancestor_all_the_same() {
        let server = started(20, "npm exec chrome-devtools-mcp", PANE, 10);
        let tree = vec![
            program(),
            server.clone(),
            started(30, "node watchdog/main.js", 30, 20),
        ];
        // The server came before the mark and is not the turn's; nor is what it starts.
        assert_eq!(
            left(Servers::InGroup, &[program(), server], tree),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn a_process_whose_parent_has_gone_is_told_by_its_own_group_alone() {
        let orphan = |pid, group| Member {
            parent: None,
            ..started(pid, "node browser", group, 20)
        };
        // Its group is its own: the turn's, though a server in the window's group
        // started it.
        assert_eq!(
            left(
                Servers::InGroup,
                &[program()],
                vec![program(), orphan(30, 30)]
            ),
            vec![30]
        );
        // Its group is the window's: the harness's.
        assert_eq!(
            left(
                Servers::InGroup,
                &[program()],
                vec![program(), orphan(31, PANE)]
            ),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn a_process_whose_group_cannot_be_read_is_told_by_its_ancestry_and_by_nothing_else() {
        let unread = |pid, parent: Option<u32>| Member {
            group: None,
            parent: parent.map(key),
            ..started(pid, "node x", 0, 0)
        };
        let server = started(20, "npm exec x", PANE, 10);
        let tree = vec![
            program(),
            server.clone(),
            unread(30, Some(20)),
            unread(31, None),
            unread(32, Some(10)),
        ];
        assert_eq!(
            left(Servers::InGroup, &[program(), server], tree),
            vec![31, 32]
        );
    }

    #[test]
    fn where_it_is_not_said_how_the_harness_keeps_its_servers_what_they_start_is_told_by_time() {
        let tree = vec![
            program(),
            started(20, "npm exec x", PANE, 10),
            started(30, "chrome", 30, 20),
        ];
        let mut baseline = Baseline::default();
        baseline.mark(&[program()], Kept::of(Servers::Unknown, Some(PANE)));
        assert_eq!(pids(baseline.left_running(tree).processes), vec![20, 30]);
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
        let tree = vec![
            program(),
            started(50, "a", 50, 51),
            started(51, "b", 51, 50),
        ];
        assert_eq!(left(Servers::InGroup, &[program()], tree), vec![50, 51]);
    }

    #[test]
    fn a_new_mark_says_again_how_the_harness_keeps_its_servers() {
        let server = started(20, "npm exec x", PANE, 10);
        let child = started(30, "node x", 30, 20);
        let tree = vec![program(), server.clone(), child.clone()];
        let mut baseline = Baseline::default();
        baseline.mark(&[program()], Kept::of(Servers::InGroup, Some(PANE)));
        assert_eq!(baseline.left_running(tree.clone()).total, 0);

        // A mark that does not say is a window told by time again.
        baseline.mark(&[program(), server.clone()], None);
        assert_eq!(
            pids(baseline.left_running(tree.clone()).processes),
            vec![30]
        );
        baseline.mark(&tree, Kept::of(Servers::InGroup, Some(PANE)));
        let later = started(31, "node y", 31, 20);
        let mut now = tree;
        now.push(later);
        assert_eq!(baseline.left_running(now).total, 0);
    }

    // The servers a window opens with, outside its group: Codex's.

    /// The tree of a Codex window as it opens: the supervisor that opens it (the
    /// window's program), Codex's own process and its TUI in the window's group,
    /// and two servers in groups of their own, the second with a process of its.
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
        assert_eq!(left(Servers::AtLaunch, &before, tree), Vec::<u32>::new());
    }

    #[test]
    fn what_the_harnesss_own_process_starts_is_the_agents_though_it_was_there_from_the_launch() {
        let before = codex_at_launch();
        let mut tree = before.clone();
        tree.extend([
            // A command the agent's tool ran, in a session of its own, and what it left.
            started(40, "/bin/zsh -lc make", 40, 11),
            started(41, "make", 40, 40),
            started(42, "npm run dev", 42, 41),
            // What the harness's own process starts besides, in a group of its own.
            started(50, "codex-code-mode-host", 50, 11),
        ]);
        assert_eq!(left(Servers::AtLaunch, &before, tree), vec![40, 41, 42, 50]);
    }

    #[test]
    fn a_process_that_began_after_the_first_turn_is_no_server_of_the_launch() {
        let launch = codex_at_launch();
        let mut baseline = Baseline::default();
        baseline.mark(&launch, Kept::of(Servers::AtLaunch, Some(PANE)));

        // The first turn's agent opened a session in a group of its own, which the
        // next turn's mark finds there: what it starts is the agent's.
        let session = started(40, "bash", 40, 11);
        let mut then = launch.clone();
        then.push(session.clone());
        baseline.mark(&then, Kept::of(Servers::AtLaunch, Some(PANE)));
        let mut now = then;
        now.extend([
            started(41, "sleep 600", 40, 40),
            // And a server of the launch still starts what it starts.
            started(42, "node kernel.js", 22, 22),
        ]);
        assert_eq!(pids(baseline.left_running(now).processes), vec![41]);
    }

    #[test]
    fn a_server_whose_group_cannot_be_read_is_not_known_to_be_outside_the_windows() {
        let unread = |pid, parent| Member {
            group: None,
            ..started(pid, "node x", 0, parent)
        };
        let before = vec![program(), unread(20, 10)];
        let tree = vec![program(), unread(20, 10), started(30, "child", 30, 20)];
        assert_eq!(left(Servers::AtLaunch, &before, tree), vec![30]);
    }

    #[test]
    fn what_began_in_the_windows_group_is_not_a_servers_where_they_are_kept_at_launch() {
        let before = codex_at_launch();
        let mut tree = before.clone();
        tree.push(started(60, "a helper of codex's own", PANE, 11));
        assert_eq!(left(Servers::AtLaunch, &before, tree), vec![60]);
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
        assert_eq!(left(Servers::InGroup, &before, tree.clone()), vec![30]);
        // Where it opens them with the window outside it, it is a server's.
        assert_eq!(left(Servers::AtLaunch, &before, tree), Vec::<u32>::new());
    }
}

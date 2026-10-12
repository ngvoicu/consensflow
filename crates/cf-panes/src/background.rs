//! What a window's program started and left running. An agent can start work
//! that outlives its turn (a job in the background, a monitor it armed) and
//! end the turn; whether the turn's end is the task's result depends on whether
//! such work is still running, and closing the window ends it: on Unix once it
//! was asked to end and given time to release what it holds, on Windows at once
//! with no chance to (`pty::close` says why).
//!
//! The host cannot tell a program from the work it started by anything the
//! program is: an MCP server and a recording run are both children of the
//! harness. It tells them by when they began. The daemon says a turn is about
//! to begin (`pane.mark`), and the host keeps the processes its tree has then:
//! the harness, its servers, its language servers. What the tree has later that
//! is not among them is what the turn started ([`Baseline::left_running`]).
//! A process is known by its number and by when it began, so a process that
//! took the number of one that ended is not mistaken for it.
//!
//! Time alone is wrong for a server that begins after the mark, and for what a
//! server starts. Claude Code starts its MCP servers as it takes its first
//! message, which the daemon marks once it shows in the record, so they began
//! after the mark and were taken for the turn's work (found on 3.0.1: a worker's
//! task was never done); and what a server starts (a watchdog, the browser a tool
//! launches, each in a group of its own) is the same wherever it began, as is
//! what Codex's plugins started with the window start later. Where the daemon
//! says how the window's harness keeps its servers (`pane.mark`'s `servers`) the
//! host also goes by process group and by which process started which
//! ([`harness`] says how); a process of a harness that is not known to keep them
//! in a way is told by time alone, as it always was.
//!
//! Only looking at the tree is the platform's: the macOS process tree and the
//! Windows job object each list their members ([`Member`]), with the group each is
//! in and the process that started it where the platform says (macOS; a job
//! object has neither, so a Windows window is told by time alone); the rest is
//! here.

mod harness;

use std::collections::HashSet;

use cf_proto::panes::{Background, BackgroundProcess, BACKGROUND_PROCESSES, COMMAND_CHARS};

pub(crate) use harness::Kept;

/// A process by what tells it from a later one of the same number: the number,
/// and when it began, in whatever unit its platform keeps that in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ProcessKey {
    pub pid: u32,
    pub began: u64,
}

/// A process of a pane's tree and the command line it was started with, as far
/// as the platform let it be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Member {
    pub key: ProcessKey,
    pub command: String,
    /// The process group it is in; none where the platform says none (a Windows
    /// job object has no groups).
    pub group: Option<i32>,
    /// The process that started it, where that is another process of the tree:
    /// none for the window's program, for a process whose parent has gone (the
    /// system keeps naming the parent it was started by, which is no process of
    /// the tree), and where the platform says none (a Windows job object).
    pub parent: Option<ProcessKey>,
}

/// The processes of a pane's tree when its turn began; none until the daemon
/// says one does, and a pane with none has no background work, since nothing
/// says what came before.
#[derive(Debug, Default, Clone)]
pub(crate) struct Baseline {
    /// The processes the tree had when the latest turn began.
    before: Option<HashSet<ProcessKey>>,
    /// The processes it had when the first turn began, and has for good: what the
    /// window opened with.
    launch: HashSet<ProcessKey>,
    /// How the window's harness keeps its servers, as the daemon said at the
    /// latest mark: none where it did not say, or the platform has no groups.
    kept: Option<Kept>,
}

impl Baseline {
    /// A turn begins: every process of the tree now came before it, and `kept`
    /// says how the window's harness keeps its servers ([`Kept::of`]).
    pub fn mark(&mut self, tree: &[Member], kept: Option<Kept>) {
        let now: HashSet<ProcessKey> = tree.iter().map(|member| member.key).collect();
        if self.before.is_none() {
            self.launch.clone_from(&now);
        }
        self.before = Some(now);
        self.kept = kept;
    }

    /// Whether a turn was ever said to begin: where it was not, the tree is
    /// not worth looking at.
    pub fn is_marked(&self) -> bool {
        self.before.is_some()
    }

    /// What `tree`, the pane's tree now, has that its turn started: the
    /// processes not in the baseline and not the harness's own, the first of them
    /// by when they began.
    pub fn left_running(&self, tree: Vec<Member>) -> Background {
        let Some(before) = &self.before else {
            return Background::default();
        };
        let harness = self
            .kept
            .map(|kept| harness::own(kept, &self.launch, &tree))
            .unwrap_or_default();
        let mut started: Vec<Member> = tree
            .into_iter()
            .filter(|member| !before.contains(&member.key) && !harness.contains(&member.key))
            .collect();
        started.sort_by_key(|member| (member.key.began, member.key.pid));
        Background {
            total: started.len(),
            processes: started
                .into_iter()
                .take(BACKGROUND_PROCESSES)
                .map(|member| BackgroundProcess {
                    pid: member.key.pid,
                    command: bounded(&member.command),
                })
                .collect(),
        }
    }
}

/// A command line held to what a frame carries: its blanks made one, cut at
/// [`COMMAND_CHARS`] characters with a mark that says it was.
fn bounded(command: &str) -> String {
    let words = command.split_whitespace().collect::<Vec<_>>().join(" ");
    if words.chars().count() <= COMMAND_CHARS {
        return words;
    }
    let kept: String = words.chars().take(COMMAND_CHARS - 1).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A process whose group and parent the platform does not say.
    fn member(pid: u32, began: u64, command: &str) -> Member {
        Member {
            key: ProcessKey { pid, began },
            command: command.to_owned(),
            group: None,
            parent: None,
        }
    }

    #[test]
    fn a_pane_never_marked_has_no_background_work() {
        let mut baseline = Baseline::default();
        assert!(!baseline.is_marked());
        assert_eq!(
            baseline.left_running(vec![member(10, 1, "claude")]),
            Background::default()
        );
        baseline.mark(&[], None);
        assert!(baseline.is_marked());
    }

    #[test]
    fn what_the_tree_had_when_the_turn_began_is_not_the_turns() {
        let mut baseline = Baseline::default();
        baseline.mark(
            &[member(10, 1, "claude"), member(11, 2, "npx mcp-server")],
            None,
        );
        let now = vec![
            member(10, 1, "claude"),
            member(11, 2, "npx mcp-server"),
            member(42, 9, "python3 lock.py record"),
        ];
        let left = baseline.left_running(now);
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
        let mut baseline = Baseline::default();
        baseline.mark(&[member(11, 2, "npx mcp-server")], None);
        let left = baseline.left_running(vec![member(11, 7, "sleep 600")]);
        assert_eq!(left.total, 1, "the number is the same and the start is not");
    }

    #[test]
    fn a_new_mark_moves_what_came_before_to_where_the_tree_is() {
        let mut baseline = Baseline::default();
        baseline.mark(&[member(10, 1, "claude")], None);
        let tree = vec![member(10, 1, "claude"), member(42, 9, "sleep 600")];
        assert_eq!(baseline.left_running(tree.clone()).total, 1);
        baseline.mark(&tree, None);
        assert_eq!(baseline.left_running(tree).total, 0);
    }

    #[test]
    fn the_oldest_processes_come_first_and_only_so_many_are_named_of_a_longer_list() {
        let mut baseline = Baseline::default();
        baseline.mark(&[], None);
        let tree: Vec<Member> = (0..(BACKGROUND_PROCESSES as u32 + 4))
            .rev()
            .map(|n| member(100 + n, u64::from(n), &format!("job {n}")))
            .collect();
        let left = baseline.left_running(tree);
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
    fn a_command_line_is_held_to_one_line_and_to_the_bound() {
        assert_eq!(bounded("sh  -c\n  'sleep 5'"), "sh -c 'sleep 5'");
        let long = format!("python3 {}", "x".repeat(COMMAND_CHARS * 2));
        let cut = bounded(&long);
        assert_eq!(cut.chars().count(), COMMAND_CHARS);
        assert!(cut.ends_with('…'));
        assert!(cut.starts_with("python3 xxx"));
    }
}

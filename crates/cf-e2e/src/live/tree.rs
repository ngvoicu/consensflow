//! What the pane host says of the processes a window's tree started in its turn
//! (`pane.background`): the processes its tree has that began after the daemon said
//! the turn did, each by its number and the command line it was started with. The host
//! sees the tree whatever the platform, so this is where a check reads whether a command
//! its member ran is still there, with no `ps` to ask (Windows has none), and what the
//! daemon holds a worker's result back for.

use serde_json::{json, Value};

use crate::rig::Rig;

/// How many characters of a command line [`Tree::line`] says.
pub const WIDTH: usize = 90;

/// A process the turn started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proc {
    pub pid: u64,
    pub command: String,
}

/// What the window's tree has that its turn started: how many processes, and the first
/// of them (the host names a few, and says how many there are besides).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tree {
    pub total: usize,
    pub processes: Vec<Proc>,
}

/// What a reading of the tree came to: the tree, or in words why it could not be told.
pub type Read = Result<Tree, String>;

/// A reading as lines for a report: the tree's block, or why it could not be told.
pub fn show(read: &Read) -> String {
    match read {
        Ok(tree) => tree.block(),
        Err(said) => said.clone(),
    }
}

/// What the pane host said the window `pane` (its id) left running when it closed: the
/// `background` of its `pane.exit`, the processes of its tree that began after the turn
/// did and that the close ended, as the host took them just before it did. None while
/// the window has not closed. A look once a second may miss what a window leaves in the
/// moment between the end of its turn and its close; this does not.
pub fn left_at_exit(rig: &Rig, pane: &str) -> Option<Tree> {
    left_in(&rig.exits(), pane)
}

/// What the `exits` the host told say the window `pane` left running when it closed:
/// the last of its exits (a window may be opened again), none where it has none.
fn left_in(exits: &[Value], pane: &str) -> Option<Tree> {
    let exit = exits.iter().rev().find(|exit| exit["id"] == pane)?;
    Some(Tree::of(&json!({ "processes": exit["background"] })))
}

/// The tree of the window `pane` (`{ id, generation }`) as the host says it, or in
/// words why it could not be told: the host said no (the pane is gone), or could not
/// be asked.
pub fn read(rig: &Rig, pane: &Value) -> Read {
    let asked = rig.host(
        "pane.background",
        json!({ "id": pane["id"], "generation": pane["generation"] }),
    );
    match asked {
        Ok(answer) if answer["ok"] == true => Ok(Tree::of(&answer)),
        Ok(answer) => Err(format!("(the host said: {answer})")),
        Err(failed) => Err(format!("(could not be asked: {failed})")),
    }
}

impl Tree {
    /// The tree an answer of the host to `pane.background` holds.
    pub fn of(answer: &Value) -> Self {
        let processes: Vec<Proc> = answer["processes"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|process| Proc {
                pid: process["pid"].as_u64().unwrap_or_default(),
                command: process["command"].as_str().unwrap_or_default().to_owned(),
            })
            .collect();
        Self {
            total: answer["total"]
                .as_u64()
                .and_then(|total| usize::try_from(total).ok())
                .unwrap_or(processes.len()),
            processes,
        }
    }

    /// The processes whose command line holds `mark`.
    pub fn holding(&self, mark: &str) -> Vec<&Proc> {
        self.processes
            .iter()
            .filter(|process| process.command.contains(mark))
            .collect()
    }

    /// The tree as a line: each process by its number and its command line (the first
    /// [`WIDTH`] characters of it), `; ` between them, and how many there are in all
    /// after them in brackets; `none` where it has none.
    pub fn line(&self) -> String {
        if self.processes.is_empty() {
            return "none".to_owned();
        }
        let found: Vec<String> = self
            .processes
            .iter()
            .map(|process| {
                format!(
                    "{} {}",
                    process.pid,
                    process.command.chars().take(WIDTH).collect::<String>()
                )
            })
            .collect();
        format!("{} ({})", found.join("; "), self.total)
    }

    /// The tree as lines for a report: a line to each process, its number and the whole
    /// of the command line the host gave (it cut what is long, and said so), and a last
    /// line that says how many more there were than the host named; `none` where it
    /// has none.
    pub fn block(&self) -> String {
        if self.processes.is_empty() {
            return "none".to_owned();
        }
        let mut lines: Vec<String> = self
            .processes
            .iter()
            .map(|process| {
                if process.command.is_empty() {
                    format!("{} (its command line could not be read)", process.pid)
                } else {
                    format!("{} {}", process.pid, process.command)
                }
            })
            .collect();
        if self.total > self.processes.len() {
            lines.push(format!(
                "and {} more the host did not name",
                self.total - self.processes.len()
            ));
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer() -> Value {
        json!({
            "ok": true,
            "total": 3,
            "processes": [
                { "pid": 4101, "command": "bash -c ping -c 61 127.0.0.1" },
                { "pid": 4102, "command": "ping -c 61 127.0.0.1" },
            ],
        })
    }

    #[test]
    fn the_tree_is_the_processes_the_host_names_and_how_many_there_are_in_all() {
        let tree = Tree::of(&answer());
        assert_eq!(tree.total, 3);
        assert_eq!(
            tree.processes,
            [
                Proc {
                    pid: 4101,
                    command: "bash -c ping -c 61 127.0.0.1".to_owned()
                },
                Proc {
                    pid: 4102,
                    command: "ping -c 61 127.0.0.1".to_owned()
                },
            ]
        );
        // An answer that names none, or no total, is a tree of what it names.
        let none = Tree::of(&json!({ "ok": true, "total": 0, "processes": [] }));
        assert_eq!((none.total, none.processes.len()), (0, 0));
        let unsaid = Tree::of(&json!({ "ok": true, "processes": [{ "pid": 1, "command": "x" }] }));
        assert_eq!(unsaid.total, 1);
    }

    #[test]
    fn the_processes_that_hold_a_mark_are_the_ones_whose_command_line_says_it() {
        let tree = Tree::of(&answer());
        let held: Vec<u64> = tree
            .holding("-c 61 127.0.0.1")
            .iter()
            .map(|process| process.pid)
            .collect();
        assert_eq!(held, [4101, 4102]);
        assert!(tree.holding("sleep 119").is_empty());
    }

    #[test]
    fn a_tree_is_a_line_of_its_processes_each_cut_with_the_total_after_them_and_none_for_none() {
        assert_eq!(
            Tree::of(&answer()).line(),
            "4101 bash -c ping -c 61 127.0.0.1; 4102 ping -c 61 127.0.0.1 (3)"
        );
        let long = Tree::of(&json!({
            "total": 1,
            "processes": [{ "pid": 7, "command": "x".repeat(200) }],
        }));
        assert_eq!(long.line(), format!("7 {} (1)", "x".repeat(WIDTH)));
        assert_eq!(Tree::of(&json!({ "ok": true, "total": 0 })).line(), "none");
    }

    #[test]
    fn a_tree_is_a_block_of_lines_with_each_command_line_whole_and_the_count_the_host_left_out() {
        assert_eq!(
            Tree::of(&answer()).block(),
            "4101 bash -c ping -c 61 127.0.0.1\n4102 ping -c 61 127.0.0.1\nand 1 more the host did not name"
        );
        let long = Tree::of(&json!({
            "total": 1,
            "processes": [{ "pid": 7, "command": "x".repeat(200) }],
        }));
        assert_eq!(long.block(), format!("7 {}", "x".repeat(200)));
        // A process whose command line the host could not read is said by its number.
        let unread = Tree::of(&json!({ "total": 1, "processes": [{ "pid": 9, "command": "" }] }));
        assert_eq!(unread.block(), "9 (its command line could not be read)");
        assert_eq!(Tree::of(&json!({ "ok": true, "total": 0 })).block(), "none");
    }

    #[test]
    fn what_a_window_left_when_it_closed_is_the_background_of_its_last_exit_and_none_before() {
        let exits = [
            json!({ "id": "p1-a", "generation": 1, "exitCode": 0 }),
            json!({ "id": "p1-b", "generation": 4, "background": [
                { "pid": 71, "command": "msedge.exe --headless" },
                { "pid": 72, "command": "conhost.exe" },
            ] }),
            json!({ "id": "p1-b", "generation": 5, "background": [{ "pid": 80, "command": "node.exe" }] }),
        ];
        // The window that closed with nothing left says none, and one that has not closed says nothing.
        let none = left_in(&exits, "p1-a").unwrap();
        assert_eq!((none.total, none.block().as_str()), (0, "none"));
        assert_eq!(left_in(&exits, "p1-c"), None);
        assert_eq!(left_in(&[], "p1-a"), None);
        // A window opened again has the exit of its last generation.
        let last = left_in(&exits, "p1-b").unwrap();
        assert_eq!(last.processes.len(), 1);
        assert_eq!(last.block(), "80 node.exe");
        let both = left_in(&exits[..2], "p1-b").unwrap();
        assert_eq!(both.total, 2);
        assert_eq!(both.block(), "71 msedge.exe --headless\n72 conhost.exe");
    }

    #[test]
    fn a_reading_is_said_as_its_block_or_as_the_words_that_say_why_it_could_not_be_told() {
        let tree = Tree::of(&answer());
        assert_eq!(show(&Ok(tree.clone())), tree.block());
        assert_eq!(
            show(&Err("(the host said: no such pane)".to_owned())),
            "(the host said: no such pane)"
        );
    }
}

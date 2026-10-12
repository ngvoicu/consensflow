//! What the pane host says of the processes a window's tree started in its turn
//! (`pane.background`): the processes its tree has that began after the daemon said
//! the turn did, each by its number and the command line it was started with. The host
//! sees the tree whatever the platform, so this is where a case reads whether a command
//! its member ran is still there, with no `ps` to ask (Windows has none).

use cf_e2e::live::members::Members;
use serde_json::{json, Value};

/// How many characters of a command line a line of the case says.
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

/// The tree of the window `pane` (`{ id, generation }`) as the host says it, or in
/// words why it could not be told: the host said no (the pane is gone), or could not
/// be asked.
pub fn read(members: &Members, pane: &Value) -> Read {
    let asked = members.rig().host(
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
}

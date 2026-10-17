//! Ending a project's rig, and what is said when it will not end. The rig's own
//! close waits five seconds for the daemon and the pane host to end once their input
//! has, which suits the stand-in agents (they leave when asked). A real harness
//! takes its time (OpenCode's window needs a little over five seconds to leave once
//! the host asks it to, after a command of its was stopped), and the host gives a
//! program that does not leave ten before it kills it. So a project of real
//! harnesses is given [`EXIT`] to end in, and one that does not end in it says why:
//! "process did not exit" does not say which process it was, so the rig's processes
//! are looked at while it ends, and what was still there goes into the failure.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use super::shown;
use crate::live::claude::Process;
use crate::process::Run;
use crate::rig::Rig;
use crate::{Error, Result};

/// How long the daemon and the pane host each have to end once their input has: the
/// host's own grace for a program that ignores the request to end, and the time it
/// takes to kill it, twice over.
pub const EXIT: Duration = Duration::from_secs(20);

/// How often the processes are looked at while the rig ends.
const LOOK: Duration = Duration::from_millis(500);

/// A process of the machine and the process group it is in, as `ps` lists them.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Listed {
    process: Process,
    group: u32,
}

/// The processes `ps -axo pid=,ppid=,pgid=,command=` listed, but for the listing's own.
fn parsed(listing: &str) -> Vec<Listed> {
    listing
        .lines()
        .filter_map(|line| {
            let (pid, rest) = line.trim_start().split_once(char::is_whitespace)?;
            let (ppid, rest) = rest.trim_start().split_once(char::is_whitespace)?;
            let (group, command) = rest.trim_start().split_once(char::is_whitespace)?;
            Some(Listed {
                process: Process {
                    pid: pid.parse().ok()?,
                    ppid: ppid.parse().ok()?,
                    command: command.trim_start().to_owned(),
                },
                group: group.parse().ok()?,
            })
        })
        .filter(|listed| !listed.process.command.contains("ps -axo"))
        .collect()
}

/// The processes of the machine now, with their groups: none on a machine with no
/// `ps`.
fn listing() -> Vec<Listed> {
    Run::new("ps")
        .args(["-axo", "pid=,ppid=,pgid=,command="])
        .inheriting_env()
        .run()
        .ok()
        .filter(|ran| ran.code == Some(0))
        .map(|ran| parsed(&ran.stdout))
        .unwrap_or_default()
}

/// The processes of `all` that descend from `root`, by the parent each was started
/// by: what a rig started, and what that started in turn.
fn descendants(all: &[Listed], root: u32) -> Vec<Listed> {
    let mut found: Vec<Listed> = Vec::new();
    let mut parents = vec![root];
    while let Some(parent) = parents.pop() {
        for child in all.iter().filter(|listed| listed.process.ppid == parent) {
            if !found
                .iter()
                .any(|known| known.process.pid == child.process.pid)
            {
                parents.push(child.process.pid);
                found.push(child.clone());
            }
        }
    }
    found.sort_by_key(|listed| listed.process.pid);
    found
}

/// What of `alive` the rig left: the processes it started, and whatever is in the
/// group of one of them. A window's program is the leader of a group of its own, and
/// the jobs it started stay in it after their parent has gone, which a look by the
/// parents alone does not find.
fn left_of(alive: &[Listed], started: &BTreeSet<u32>) -> Vec<Process> {
    alive
        .iter()
        .filter(|listed| started.contains(&listed.process.pid) || started.contains(&listed.group))
        .map(|listed| listed.process.clone())
        .collect()
}

/// One look at what this process started, its processes written into `seen`.
fn look(seen: &Mutex<BTreeSet<u32>>) {
    let tree = descendants(&listing(), std::process::id());
    seen.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .extend(tree.iter().map(|listed| listed.process.pid));
}

/// Ends `rig`, giving its daemon and its pane host `within` each. One that does
/// not end is an error that says which of the processes it started were left.
pub fn end_within(mut rig: Rig, within: Duration) -> Result {
    let seen: Arc<Mutex<BTreeSet<u32>>> = Arc::default();
    let done = Arc::new(AtomicBool::new(false));
    // The first look is made before the rig begins to end: a window that leaves the
    // moment it is asked (a stand-in does) and the jobs it leaves to the machine are
    // out of reach of the parents by the time a look in a thread of its own is made.
    look(&seen);
    let looking = {
        let (seen, done) = (Arc::clone(&seen), Arc::clone(&done));
        thread::spawn(move || loop {
            thread::sleep(LOOK);
            if done.load(Ordering::Relaxed) {
                break;
            }
            look(&seen);
        })
    };
    let closed = rig.shut_down_within(within);
    done.store(true, Ordering::Relaxed);
    let _ = looking.join();
    // What is left is listed before the rig is dropped, which kills the daemon and the
    // pane host it could not end: a listing after that would not have the host that
    // was still waiting for its windows.
    let said = closed.map_err(|failed| {
        let seen = seen.lock().unwrap_or_else(PoisonError::into_inner);
        let mut left = left_of(&listing(), &seen);
        left.sort_by_key(|process| process.pid);
        Error::Timeout(format!(
            "{failed}; the processes it started that were left: {}",
            if left.is_empty() {
                "none".to_owned()
            } else {
                shown(&left).join("; ")
            }
        ))
    });
    drop(rig);
    said
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(pid: u32, ppid: u32, group: u32, command: &str) -> Listed {
        Listed {
            process: Process {
                pid,
                ppid,
                command: command.to_owned(),
            },
            group,
        }
    }

    #[test]
    fn a_listing_is_read_as_a_process_its_parent_its_group_and_the_command_as_it_is_spaced() {
        let text = "    1     0     1 /sbin/launchd\n  \
                    501   500   500 node  -e   \"x\"\n\
                    ps   -axo pid=,ppid=,pgid=,command=\n  \
                    7   1   7 ps -axo pid=,ppid=,pgid=,command=\n\
                    not a process\n";
        let all = parsed(text);
        assert_eq!(
            all,
            [
                listed(1, 0, 1, "/sbin/launchd"),
                listed(501, 500, 500, "node  -e   \"x\""),
            ]
        );
        assert!(parsed("").is_empty());
    }

    #[test]
    fn what_a_rig_started_is_what_descends_from_it_by_the_parents_whatever_their_depth() {
        let all = [
            listed(1, 0, 1, "launchd"),
            listed(10, 1, 10, "cargo test"),
            listed(20, 10, 10, "cf ui --json"),
            listed(21, 10, 10, "consensflow-bridge"),
            listed(30, 21, 30, "opencode"),
            listed(31, 30, 30, "sh ./script"),
            listed(32, 31, 30, "sleep 200"),
            // Another's, which no parent of this rig started.
            listed(40, 1, 40, "vim notes.md"),
            listed(41, 40, 40, "claude"),
        ];
        let found: Vec<u32> = descendants(&all, 10)
            .iter()
            .map(|listed| listed.process.pid)
            .collect();
        assert_eq!(found, [20, 21, 30, 31, 32]);
        assert!(descendants(&all, 999).is_empty());
        assert!(descendants(&[], 10).is_empty());
    }

    #[test]
    fn a_parent_that_is_its_own_child_does_not_loop() {
        let all = [listed(10, 10, 10, "odd"), listed(11, 10, 10, "child")];
        let found: Vec<u32> = descendants(&all, 10)
            .iter()
            .map(|listed| listed.process.pid)
            .collect();
        assert_eq!(found, [10, 11]);
    }

    #[test]
    fn what_is_left_of_a_rig_is_what_it_started_and_what_stayed_in_the_group_of_a_window() {
        // The window (30) went; its job (33) was left to launchd, in the window's group.
        let alive = [
            listed(21, 10, 10, "consensflow-bridge"),
            listed(33, 1, 30, "fake-agent --background-job 120 job --deaf"),
            listed(40, 1, 40, "vim notes.md"),
            listed(41, 40, 10, "a stranger in the test's own group"),
        ];
        let started: BTreeSet<u32> = [20, 21, 30, 31].into();
        let left: Vec<u32> = left_of(&alive, &started)
            .iter()
            .map(|process| process.pid)
            .collect();
        assert_eq!(left, [21, 33]);
        assert!(left_of(&alive, &BTreeSet::new()).is_empty());
    }
}

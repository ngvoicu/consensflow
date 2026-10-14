//! macOS pane ownership survives setsid and ordinary parent exit. Never match
//! executable names, working directories, or arbitrary command-line substrings
//! to say what is the pane's: the tree has no process that those add. A tree is
//! looked at ([`ProcessTree::members`]), asked to end
//! ([`ProcessTree::ask_to_exit`]) and, last, ended ([`ProcessTree::terminate`], and
//! [`ProcessTree::finish`] for what a turn left), the whole of it when a window
//! closes and only the turn's work, which the baseline proves, when that is what
//! is ended ([`Subject`]).
//!
//! What is the pane's is found by the parent each process was started by, by its
//! session and by the marker its environment carries where one shows it. The pane's
//! program is a session leader (the pty makes it one), and every process it starts
//! is in its session unless it leaves it with `setsid`, whoever started it and
//! whether or not the process that started it is still there. The session says a
//! process is the window's, which a close ends; it says nothing of whose work it is,
//! and the turn's is proved by a chain of processes that run ([`crate::background`]):
//! Claude Code's and Pi's shells are session leaders themselves, and what they leave
//! is in a session no pane has. What the system says of a process is read in
//! [`native`].
//!
//! An ending of a turn's work is bound to the turn: [`ProcessTree::lineage`] chooses
//! what it ends once, by process identity, and what the ending looks for after that is
//! what those processes started, never what a look of the whole tree takes for the
//! turn's work by the baseline, which a window's new work is.
//!
//! What is selected is not what is signalled. The selection reads the whole table of the
//! machine's processes, and a wrong one (a bug, or a plant of one) would signal anyone's:
//! every signal the tree sends is sent by [`bound`], after a check of its own that shares
//! nothing with the selection. A process that is not the pane's program, nor under it by
//! the process that started each, nor seen so at a look before, is not signalled and is
//! not named as ended; so a close ends no job that no look saw under the window's program
//! and that its session or its marker alone says is the window's, and leaves it running.
//!
//! Limits. The last signal is not bound to the process it was meant for: `signal` (in
//! [`native`]) looks at the process and then signals its number, and a process that
//! ended in between, whose number another took, would be signalled. The system gives
//! numbers out one after the other, and the window between the look and the signal is
//! microseconds; the kernel can bind a signal to one process by its audit token (xnu's
//! `proc_info`), which this does not use. The pane's process group, which a close signals
//! whole (`pty::close`), is the pane's program's own, a number the system keeps from every
//! other process while the group has a member; it is no part of this bound.
mod bound;
mod native;
mod selection;
#[cfg(test)]
mod testing;

use std::collections::{HashMap, HashSet};
use std::io;
use std::time::{Duration, Instant};

use crate::background::{report, Baseline, Member, ProcessKey};
use bound::{Bound, Native, System, Table};
use cf_proto::panes::Ending;
use native::{command_line, exe_of, has_marker, procargs, process};
pub(crate) use selection::Lineage;

pub(crate) const OWNER_ENV: &str = "CF_PANE_PROCESS_OWNER";

/// How long a process that was sent SIGKILL is given to be gone: the signal is
/// delivered at once, and the process is gone when its system call returns.
const AFTER_KILL: Duration = Duration::from_secs(2);

/// What a tree is asked to end, or ended.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Subject<'a> {
    /// All of it: a window closes. The pane's own process is part of it.
    Whole,
    /// The processes a turn's work was when its ending began, and what they start
    /// while it goes on: what a turn left. The pane's own process is never part of
    /// it, nor is any process a later look would take for the turn's.
    Lineage(&'a Lineage),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Identity {
    pid: i32,
    born: (u64, u64),
    unique: u64,
}

#[derive(Clone)]
struct Process {
    identity: Identity,
    parent: u64,
    /// The process group it is in.
    group: i32,
    /// The session it is in, where the system says it.
    session: Option<i32>,
    /// What the system calls it, for a process whose command line cannot be read.
    name: String,
}

/// The processes of a tree that were asked to end, and are waited for.
pub(crate) struct Asked {
    identities: Vec<Identity>,
    /// Whether any of the signals could not be sent: a process that is ending, or
    /// that is not ours to signal.
    failed: bool,
}

impl Asked {
    /// Whether any of them is still there: the process itself, not one that
    /// took its number.
    pub fn any_alive(&self) -> bool {
        self.identities
            .iter()
            .any(|asked| process(asked.pid).map(|p| p.identity) == Some(*asked))
    }

    /// Whether some process could not be asked.
    pub fn failed(&self) -> bool {
        self.failed
    }
}

/// What forcing an ending made of the tree: the members it told the processes it
/// stopped by, and the failure that last stopped it from doing all it was asked.
struct Forced {
    members: Vec<Member>,
    failure: Option<io::Error>,
}

#[derive(Clone)]
pub(crate) struct ProcessTree {
    pub marker: String,
    root: Option<Identity>,
    /// The session the pane's program leads.
    session: Option<i32>,
    /// What every signal of the tree passes.
    bound: Bound,
}

impl ProcessTree {
    pub fn new() -> Self {
        let mut random = [0u8; 16];
        // SAFETY: the buffer is writable for its full declared length.
        unsafe { libc::arc4random_buf(random.as_mut_ptr().cast(), random.len()) };
        Self {
            marker: random.iter().map(|byte| format!("{byte:02x}")).collect(),
            root: None,
            session: None,
            bound: Bound::default(),
        }
    }

    pub fn attach(&mut self, pid: i32) {
        let root = process(pid);
        self.root = root.as_ref().map(|p| p.identity);
        self.session = root.and_then(|p| p.session);
        self.bound = Bound::new(self.root);
    }

    /// The processes of the tree now, each with the command line it was
    /// started with, the program and the path it runs from, the process group it is
    /// in and the process of the tree that started it: a look, nothing is signalled
    /// or stopped. What is under the pane's program is seen ([`bound`]).
    pub fn members(&self) -> io::Result<Vec<Member>> {
        self.members_by(&Native)
    }

    fn members_by(&self, system: &impl System) -> io::Result<Vec<Member>> {
        let snapshot = system.processes()?;
        self.bound.observe(&Table::of(&snapshot));
        let tree = self.select(&snapshot, &HashMap::new());
        Ok(self.members_in(&snapshot, &tree))
    }

    /// The members of `snapshot` that `tree` holds.
    ///
    /// The system names the parent a process was started by with a number that
    /// is the parent's alone, and keeps naming it after the parent has gone (seen
    /// on macOS 26.x with a `sleep` and a Python that `sh` left running: their
    /// ordinary parent number becomes launchd's, and this one does not). Such a
    /// process names a parent that is no process of the tree (as one that named
    /// launchd would), and has none here: its parent is gone.
    fn members_in(&self, snapshot: &[Process], tree: &HashMap<i32, Identity>) -> Vec<Member> {
        let held: Vec<&Process> = snapshot
            .iter()
            .filter(|p| tree.get(&p.identity.pid) == Some(&p.identity))
            .collect();
        let keys: HashMap<u64, ProcessKey> = held
            .iter()
            .map(|p| (p.identity.unique, key_of(p.identity)))
            .collect();
        held.iter()
            .map(|p| Member {
                key: key_of(p.identity),
                command: procargs(p.identity.pid)
                    .and_then(|bytes| command_line(&bytes))
                    .unwrap_or_else(|| p.name.clone()),
                program: p.name.clone(),
                exe: exe_of(p.identity.pid),
                group: Some(p.group),
                parent: keys.get(&p.parent).copied(),
            })
            .collect()
    }

    /// The turn's work as `baseline` proves it now, chosen once, in one look at the
    /// tree: what an ending of the turn is bound to ([`Lineage`]).
    pub fn lineage(&self, baseline: &Baseline) -> io::Result<Lineage> {
        self.lineage_by(&Native, baseline)
    }

    /// [`ProcessTree::lineage`], less what the bound would refuse: none of that is the
    /// turn's, whatever chose it.
    fn lineage_by(&self, system: &impl System, baseline: &Baseline) -> io::Result<Lineage> {
        let snapshot = system.processes()?;
        let table = Table::of(&snapshot);
        self.bound.observe(&table);
        let tree = self.select(&snapshot, &HashMap::new());
        let members = self.members_in(&snapshot, &tree);
        let work = baseline.turn_work(&members);
        let keys: HashSet<ProcessKey> = work.iter().map(|member| member.key).collect();
        let identities: Vec<Identity> = snapshot
            .iter()
            .filter(|p| tree.get(&p.identity.pid) == Some(&p.identity))
            .filter(|p| keys.contains(&key_of(p.identity)))
            .map(|p| p.identity)
            .filter(|identity| self.bound.allows(&table, *identity))
            .collect();
        let allowed: HashSet<ProcessKey> = identities.iter().map(|id| key_of(*id)).collect();
        Ok(Lineage {
            identities,
            members: work
                .into_iter()
                .filter(|member| allowed.contains(&member.key))
                .collect(),
        })
    }

    /// Asks `subject` to end as a closing terminal does: TERM, then HUP, to
    /// every process of it outside `group`, the pane's own process group, which
    /// a close signals whole (an ending has none). Nothing is stopped first: a
    /// stopped process cannot answer a signal. The processes asked, which are kept
    /// for the ending's end, and whether some could not be. What the bound refuses
    /// is not asked, and is none of what is kept.
    pub fn ask_to_exit(&self, subject: Subject<'_>, group: Option<i32>) -> io::Result<Asked> {
        self.ask_by(&Native, subject, group)
    }

    fn ask_by(
        &self,
        system: &impl System,
        subject: Subject<'_>,
        group: Option<i32>,
    ) -> io::Result<Asked> {
        let snapshot = system.processes()?;
        let table = Table::of(&snapshot);
        self.bound.observe(&table);
        let chosen = self.chosen(subject, &snapshot, &HashMap::new());
        let outside: Vec<Identity> = snapshot
            .iter()
            .filter(|p| chosen.get(&p.identity.pid) == Some(&p.identity))
            .filter(|p| Some(p.group) != group)
            .map(|p| p.identity)
            .collect();
        let mut asked = Vec::new();
        let mut failed = false;
        for identity in outside {
            match self.bound.signal(system, &table, identity, libc::SIGTERM) {
                Ok(false) => {}
                Ok(true) => asked.push(identity),
                Err(_) => {
                    failed = true;
                    asked.push(identity);
                }
            }
        }
        for identity in &asked {
            failed |= self
                .bound
                .signal(system, &table, *identity, libc::SIGHUP)
                .is_err();
        }
        Ok(Asked {
            identities: asked,
            failed,
        })
    }

    /// Ends `subject` by force. The pane's own process is ended with a `Whole`
    /// tree and never with what an ending was asked for.
    pub fn terminate(&self, subject: Subject<'_>) -> io::Result<()> {
        match self.force_by(&Native, subject, &[]).failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Ends what a turn left, by force, once `asked` has had the grace: the lineage,
    /// and what it started while it was given the grace (a handler asked to end may
    /// have started a process before it ended, whose parent is gone: it is still its
    /// parent's), then waits for every process it ended to be gone. What it chose and
    /// signalled, told by whether it is: ended, or not ended. What was never signalled
    /// (the bound refused it, or it was gone before it was asked) is not named.
    pub fn finish(&self, lineage: &Lineage, asked: Asked) -> Ending {
        self.finish_by(&Native, lineage, asked)
    }

    fn finish_by(&self, system: &impl System, lineage: &Lineage, asked: Asked) -> Ending {
        let forced = self.force_by(system, Subject::Lineage(lineage), &asked.identities);
        let signalled: HashSet<ProcessKey> = asked
            .identities
            .iter()
            .map(|identity| key_of(*identity))
            .chain(forced.members.iter().map(|member| member.key))
            .collect();
        let mut chosen: Vec<Member> = lineage
            .members
            .iter()
            .filter(|member| signalled.contains(&member.key))
            .cloned()
            .collect();
        for member in forced.members {
            if !chosen.iter().any(|known| known.key == member.key) {
                chosen.push(member);
            }
        }
        let deadline = Instant::now() + AFTER_KILL;
        let alive = loop {
            let alive: HashSet<ProcessKey> = chosen
                .iter()
                .filter(|member| {
                    process(i32::try_from(member.key.pid).unwrap_or(0))
                        .is_some_and(|p| key_of(p.identity) == member.key)
                })
                .map(|member| member.key)
                .collect();
            if alive.is_empty() || Instant::now() >= deadline {
                break alive;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        report(chosen, &alive)
    }

    /// Stops what `subject` names, over and over until a look finds nothing of it
    /// that is not stopped, and kills what it stopped: `seeds`, the processes already
    /// asked, are parents of what it finds though they are gone. What a lineage
    /// finds is its own and what descends from it, which a stopped process starts no
    /// more of. What the bound refuses is not stopped nor killed, and not looked for
    /// again.
    fn force_by(&self, system: &impl System, subject: Subject<'_>, seeds: &[Identity]) -> Forced {
        let mut known: HashMap<i32, Identity> =
            seeds.iter().map(|seed| (seed.pid, *seed)).collect();
        let mut frozen: HashMap<i32, Identity> = HashMap::new();
        let mut refused: HashMap<i32, Identity> = HashMap::new();
        let mut members: Vec<Member> = Vec::new();
        let mut failure = None;
        let mut snapshot: Vec<Process> = Vec::new();
        // Freeze proven parents before rescanning, so they cannot keep forking
        // through shutdown. Always kill already frozen processes on an error.
        for pass in 0..16 {
            snapshot = match system.processes() {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            };
            let table = Table::of(&snapshot);
            self.bound.observe(&table);
            let chosen = self.chosen(subject, &snapshot, &known);
            let fresh: HashMap<i32, Identity> = chosen
                .into_iter()
                .filter(|(pid, identity)| {
                    frozen.get(pid) != Some(identity) && refused.get(pid) != Some(identity)
                })
                .collect();
            if fresh.is_empty() {
                break;
            }
            let mut stopping: HashMap<i32, Identity> = HashMap::new();
            for identity in fresh.values() {
                match self.bound.signal(system, &table, *identity, libc::SIGSTOP) {
                    Ok(false) => {
                        refused.insert(identity.pid, *identity);
                    }
                    Ok(true) => {
                        stopping.insert(identity.pid, *identity);
                    }
                    Err(error) => {
                        failure = Some(error);
                        stopping.insert(identity.pid, *identity);
                    }
                }
            }
            if matches!(subject, Subject::Lineage(_)) {
                members.extend(self.members_in(&snapshot, &stopping));
            }
            for (pid, identity) in stopping {
                frozen.insert(pid, identity);
                known.insert(pid, identity);
            }
            if pass == 15 {
                failure = Some(io::Error::other("pane process tree did not stabilize"));
            }
        }
        let table = Table::of(&snapshot);
        for identity in frozen.values() {
            if let Err(error) = self.bound.signal(system, &table, *identity, libc::SIGKILL) {
                failure = Some(error);
            }
        }
        // A failed process enumeration must still stop the exact root we own.
        if let (Subject::Whole, Some(root)) = (subject, self.root) {
            if let Err(error) = self.bound.signal(system, &table, root, libc::SIGKILL) {
                failure = Some(error);
            }
        }
        Forced { members, failure }
    }
}

/// A process as the baseline knows it: its number and when it began.
fn key_of(identity: Identity) -> ProcessKey {
    ProcessKey {
        pid: identity.pid.unsigned_abs(),
        began: identity.born.0 * 1_000_000 + identity.born.1,
    }
}

#[cfg(test)]
mod tests;

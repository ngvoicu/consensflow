//! macOS pane ownership survives setsid and ordinary parent exit. Never match
//! executable names, working directories, or arbitrary command-line substrings
//! to say what is the pane's: the tree has no process that those add. A tree is
//! looked at ([`ProcessTree::members`]), and, when its window closes, asked to end
//! ([`ProcessTree::ask_to_exit`]) and, last, ended ([`ProcessTree::terminate`]), the
//! whole of it. Nothing ends a turn's work but that close (turn-end: a turn ends nothing
//! it left running, and Finish ends no process).
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
//! What is selected is not what is signalled. The selection reads the whole table of the
//! machine's processes, and a wrong one (a bug, or a plant of one) would signal anyone's:
//! every signal the tree sends, to a process or to the pane's group, is sent by [`bound`],
//! after a check of its own that shares nothing with the selection. A process that is not
//! the pane's program, nor under it by the process that started each, nor in the session
//! the program leads (while it runs, and once it has gone for as long as nobody has its
//! number), nor seen so at a look before, is not signalled and is
//! not named as ended; so a close ends no job that only the marker its environment
//! carries says is the window's, which no look saw under the window's program, and leaves
//! it running; the window's exit may name it as possibly the window's
//! ([`ProcessTree::possibly_left`]), and never signals it.
//!
//! In every build that tests with real signals (this crate under `cfg(test)`, or with the
//! `test-support` feature) the real system's senders ask [`guard`] first, which reads the
//! system itself, at the moment of sending, and stops a test that would signal a process
//! that is none of its own: a fault in the table every look is made of would otherwise reach
//! the user's programs before any assertion could say so. An audit of the sources
//! (`audit`) holds that nothing but the bound sends a signal, and names the exceptions.
//!
//! Limits. The last signal is not bound to the process it was meant for: `signal` (in
//! [`native`]) checks that the process is the one this tree discovered, and then signals its
//! number, and a process that ended between the check and the call (it exited, was reaped,
//! and another took its number) would be signalled. The system gives numbers out one after
//! the other, and the window between the check and the call is microseconds, which the guard
//! of a test build narrows (it reads the target again) and does not close; the kernel can
//! bind a signal to one process by its audit token (xnu's `proc_info`), which this does not
//! use.
//!
//! A close ends less than the bound permits. What the bound allows is the window's by what
//! a look saw (the process that started it, or the session its program leads), and the
//! close attempts only what the selection names: by the parents of the tree, by the pane's
//! session and by the marker. A job a look saw under the program that left the session
//! (`setsid`) after its shell had gone, with no marker to show it, is the bound's to allow
//! and no candidate of the close: it is left running.
#[cfg(test)]
mod audit;
mod bound;
#[cfg(any(test, feature = "test-support"))]
pub(crate) mod guard;
#[cfg(test)]
mod left_tests;
#[cfg(test)]
mod machine_tests;
mod native;
mod selection;
#[cfg(test)]
mod testing;

use std::collections::HashMap;
use std::io;

use crate::background::{Member, ProcessKey};
use bound::{Bound, Native, System, Table};
use cf_proto::panes::PossiblyLeft;
use native::process;
pub(crate) use native::{awake_start, clocks};

pub(crate) const OWNER_ENV: &str = "CF_PANE_PROCESS_OWNER";

/// How many times a look that must read all of the tree is made where a process was born
/// while it looked ([`ProcessTree::readable_members`]).
const LOOKS: usize = 3;

/// What a look that must read all of the tree found ([`ProcessTree::readable_members`]).
#[derive(Debug, PartialEq)]
pub(crate) enum Readable {
    /// Every process of the tree, each read.
    All(Vec<Member>),
    /// A process of the tree the system will not say what it is (another user's).
    Unreadable,
    /// The tree could not be read whole: a child was born since the table at every one of
    /// [`LOOKS`] looks, or it has no program (Astraeus on 02a6cd4f: no other user's).
    Unsettled,
}

/// What the system says of a process another named (`bound::System::looked`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Looked {
    /// One of ours that runs, which a look reads.
    Readable,
    /// It ended (reaped or not), or is no process now.
    Ended,
    /// Another user's, as its owner says.
    Unreadable,
    /// One of ours that changed while it was read, or one no read said the owner of: the
    /// look is made again ([`LOOKS`]).
    Changed,
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
        // A test build notes the programs it starts, for the guard of its real signals.
        #[cfg(any(test, feature = "test-support"))]
        guard::note_program(pid);
        let root = process(pid);
        self.root = root.as_ref().map(|p| p.identity);
        self.session = root.and_then(|p| p.session);
        self.bound = Bound::new(self.root, self.session);
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

    /// What may be the window's and runs on, not ended, for the exit to name: each
    /// process that carries the window's marker, began at or after its program, is in a
    /// session whose leader has gone (no process of any user has the session's number), and
    /// that the bound refuses, so that no close or ending signals it. Its number and the
    /// name the system gives its program, nothing more. A job a tool shell detached is one
    /// (`(server &)` from Claude Code's or Pi's shell, which leads a session of its own and
    /// returns); so is a job the user started through a server that the window started and
    /// that hands its environment on (a `screen` or `tmux` server, an editor), which is why
    /// this names and never signals (T-406, Astraeus's review T-411). Nothing looked at here
    /// is seen ([`Bound::observe`] is not asked). A process whose number another took by the
    /// time its marker was read is not named. None where the program was never read, or the
    /// system's table cannot be.
    pub fn possibly_left(&self) -> Vec<PossiblyLeft> {
        self.possibly_left_by(&Native)
    }

    fn possibly_left_by(&self, system: &impl System) -> Vec<PossiblyLeft> {
        let Some(root) = self.root else {
            return Vec::new();
        };
        let Ok(snapshot) = system.processes() else {
            return Vec::new();
        };
        let table = Table::of(&snapshot);
        snapshot
            .iter()
            .filter(|p| p.identity.born >= root.born)
            .filter(|p| {
                p.session
                    .is_some_and(|session| session > 1 && system.nobody_holds(session))
            })
            .filter(|p| !self.bound.allows(&table, p.identity))
            .filter(|p| system.carries(p.identity.pid, &self.marker))
            // The marker was read by number: what answered is the process of the table only
            // if it is still there, as a signal is sent to none that took its number.
            .filter(|p| system.runs(key_of(p.identity)))
            .map(|p| PossiblyLeft {
                pid: p.identity.pid.unsigned_abs(),
                program: p.name.clone(),
            })
            .collect()
    }

    /// [`ProcessTree::members`], where every process the pane's program and each of them
    /// started is one of them ([`Readable`]): hidden where one is not and the system will
    /// not say what it is (another user's, as a command that `exec`s `sudo` becomes), or
    /// where the tree has no program. A look that must not take what it cannot read for
    /// nothing running (`pane.commands`) asks this. A process the system names that ended
    /// since (reaped or not) is none of it; one born since the table was read (Calliope on
    /// 136a7158: a race, which would make the look say it cannot tell) has the look made
    /// again, [`LOOKS`] times at most.
    pub fn readable_members(&self) -> io::Result<Readable> {
        self.readable_members_by(&Native)
    }

    fn readable_members_by(&self, system: &impl System) -> io::Result<Readable> {
        let Some(root) = self.root else {
            return Ok(Readable::Unsettled);
        };
        for _ in 0..LOOKS {
            let snapshot = system.processes()?;
            self.bound.observe(&Table::of(&snapshot));
            let tree = self.select(&snapshot, &HashMap::new());
            let mut born = false;
            for parent in std::iter::once(root.pid).chain(tree.keys().copied()) {
                for child in system.children(parent)? {
                    if tree.contains_key(&child) {
                        continue;
                    }
                    match system.looked(child) {
                        Looked::Unreadable => return Ok(Readable::Unreadable),
                        Looked::Readable | Looked::Changed => born = true,
                        Looked::Ended => {}
                    }
                }
            }
            if !born {
                return Ok(Readable::All(self.members_in(&snapshot, &tree)));
            }
        }
        Ok(Readable::Unsettled)
    }

    /// Asks the tree to end as a closing terminal does: TERM, then HUP, to every process
    /// of it outside `group`, the pane's own process group, which a close signals whole.
    /// Nothing is stopped first: a stopped process cannot answer a signal. The processes
    /// asked, which the close waits for, and whether some could not be. What the bound
    /// refuses is not asked, and is none of what is kept.
    pub fn ask_to_exit(&self, group: Option<i32>) -> io::Result<Asked> {
        self.ask_by(&Native, group)
    }

    fn ask_by(&self, system: &impl System, group: Option<i32>) -> io::Result<Asked> {
        let snapshot = system.processes()?;
        let table = Table::of(&snapshot);
        self.bound.observe(&table);
        let chosen = self.select(&snapshot, &HashMap::new());
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

    /// Signals the pane's process group `group` with `signal`, as a close does: whole
    /// while the pane's program runs and leads it, and else member by member, each through
    /// the bound ([`Bound::signal_group`]).
    pub fn signal_group(&self, group: i32, signal: i32) -> io::Result<()> {
        self.signal_group_by(&Native, group, signal)
    }

    fn signal_group_by(&self, system: &impl System, group: i32, signal: i32) -> io::Result<()> {
        let snapshot = system.processes()?;
        let table = Table::of(&snapshot);
        self.bound.observe(&table);
        self.bound.signal_group(system, &table, group, signal)
    }

    /// Ends the tree by force, the pane's own process with it.
    pub fn terminate(&self) -> io::Result<()> {
        match self.force_by(&Native, &[]) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Stops the tree, over and over until a look finds nothing of it that is not
    /// stopped, and kills what it stopped and the pane's own process: `seeds`, the
    /// processes already asked, are parents of what it finds though they are gone. What
    /// the bound refuses is not stopped nor killed, and not looked for again. The failure
    /// that last stopped it from doing all it was asked, if one did.
    fn force_by(&self, system: &impl System, seeds: &[Identity]) -> Option<io::Error> {
        let mut known: HashMap<i32, Identity> =
            seeds.iter().map(|seed| (seed.pid, *seed)).collect();
        let mut frozen: HashMap<i32, Identity> = HashMap::new();
        let mut refused: HashMap<i32, Identity> = HashMap::new();
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
            let chosen = self.select(&snapshot, &known);
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
        if let Some(root) = self.root {
            if let Err(error) = self.bound.signal(system, &table, root, libc::SIGKILL) {
                failure = Some(error);
            }
        }
        failure
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

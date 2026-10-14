//! The bound every signal to a process of a pane's tree passes. Who the tree signals is
//! chosen by what [`super`] selects (what the pane's tree is, what a turn's work is),
//! which reads the whole table of the system's processes and can be wrong: a bug in it,
//! or a plant of one, once chose every process of the machine, and the ending signalled
//! them, the user's own programs among them. A wrong selection must not be able to end
//! anyone's but the window's, so a signal is sent here, after a check of its own that
//! shares nothing with the selection (not its roots, its parents nor its descendants).
//!
//! **The host only ever signals a process that is the pane's program, or runs under it
//! by the process each was started by, in the table being acted on; or that was seen so
//! at a look before, or runs under one that was.** A process the bound refuses is not
//! signalled, and what asked for it does not name it as ended. When in doubt it ends
//! less: a job an agent detached from its shell, which no look saw under it, is the
//! window's by its session or by the marker its environment carries, and the bound
//! leaves it running as an ending does. A stranger (the user's shell, a build, the
//! process that started the pane's program) is under the program by no parent, and is
//! never signalled.
//!
//! Each look at the tree ([`Bound::observe`]) records what is under the program,
//! told by the identifier the system gives each process, which no later process takes,
//! never by its number alone. What was seen stays the window's while it runs, whatever
//! becomes of the process that started it; what has gone is let go of.

use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use super::{native, Identity, Process};

/// What a tree asks of the system: the processes there are, and a signal sent to one.
/// The tests of the tree ask a machine made up, which signals nothing.
pub(super) trait System {
    /// Every process of ours there is now.
    fn processes(&self) -> io::Result<Vec<Process>>;

    /// Sends `signal` to the process `to` names, and to none that has taken its
    /// number: one that is gone is no error. Only [`Bound::signal`] calls it.
    fn send(&self, to: Identity, signal: i32) -> io::Result<()>;
}

/// The system itself.
pub(super) struct Native;

impl System for Native {
    fn processes(&self) -> io::Result<Vec<Process>> {
        native::processes()
    }

    fn send(&self, to: Identity, signal: i32) -> io::Result<()> {
        native::signal(to, signal)
    }
}

/// The system's processes of one look, by the identifier the system gives each, held for
/// the checks and the signals that look makes: one table is made of them, not one for each
/// signal.
pub(super) struct Table<'a> {
    processes: &'a [Process],
    by_identifier: HashMap<u64, &'a Process>,
}

impl<'a> Table<'a> {
    pub fn of(processes: &'a [Process]) -> Self {
        Self {
            processes,
            by_identifier: processes
                .iter()
                .map(|process| (process.identity.unique, process))
                .collect(),
        }
    }
}

/// What a pane's tree may signal. Clones are one bound: what one has seen the others
/// know, as the pane's tree is looked at from several places.
#[derive(Clone, Default)]
pub(super) struct Bound {
    /// The pane's program, which the tree starts at: none where it could not be read.
    program: Option<Identity>,
    /// The identifiers of what the latest look saw under the program, and runs still.
    seen: Arc<Mutex<HashSet<u64>>>,
}

impl Bound {
    /// The bound of a tree whose program is `program`.
    pub fn new(program: Option<Identity>) -> Self {
        Self {
            program,
            seen: Arc::default(),
        }
    }

    /// A look at the tree: `table` is the system's processes now. What is under the
    /// program, or under what was seen before, is seen, and what was seen and has gone is
    /// no longer: a process under one that was seen and has gone is seen itself now.
    pub fn observe(&self, table: &Table<'_>) {
        let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        let now: HashSet<u64> = table
            .processes
            .iter()
            .filter(|process| self.under(table, &seen, process))
            .map(|process| process.identity.unique)
            .collect();
        *seen = now;
    }

    /// Whether `target` may be signalled, `table` being the processes there are now: it
    /// is the pane's program, or is under it or under what was seen, by the processes
    /// that started each. A process the table does not hold (it has gone, or the system
    /// will not say) is the window's only if it was seen, and the signal itself goes
    /// to none that has taken its number.
    pub fn allows(&self, table: &Table<'_>, target: Identity) -> bool {
        if self.program == Some(target) {
            return true;
        }
        let seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        match table.by_identifier.get(&target.unique) {
            None => seen.contains(&target.unique),
            Some(process) if process.identity != target => false,
            Some(process) => self.under(table, &seen, process),
        }
    }

    /// Signals `target` with `signal` through `system`, where the bound allows it:
    /// whether it was. A refusal sends nothing.
    pub fn signal(
        &self,
        system: &impl System,
        table: &Table<'_>,
        target: Identity,
        signal: i32,
    ) -> io::Result<bool> {
        if !self.allows(table, target) {
            return Ok(false);
        }
        system.send(target, signal)?;
        Ok(true)
    }

    /// Whether `process` is the program or was seen, or the process that started it is,
    /// or the one that started that, up as far as `table` names them. The system names
    /// the process that started a process by its identifier, which it keeps naming after
    /// that process has gone, so a process under one that was seen and has gone is found
    /// by the identifier alone.
    fn under(&self, table: &Table<'_>, seen: &HashSet<u64>, process: &Process) -> bool {
        let program = self.program.map(|program| program.unique);
        let known = |identifier: u64| Some(identifier) == program || seen.contains(&identifier);
        let mut at = process;
        // A table names no process twice among a process's ancestors: the bound on the
        // walk is only for one that is no tree.
        for _ in 0..=table.by_identifier.len() {
            if known(at.identity.unique) || known(at.parent) {
                return true;
            }
            match table.by_identifier.get(&at.parent) {
                Some(parent) => at = parent,
                None => return false,
            }
        }
        false
    }
}

#[cfg(test)]
mod tests;

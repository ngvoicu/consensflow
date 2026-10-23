//! The bound every signal to a process of a pane's tree passes. Who the tree signals is
//! chosen by what [`super`] selects (what the pane's tree is, what a turn's work is),
//! which reads the whole table of the system's processes and can be wrong: a bug in it,
//! or a plant of one, once chose every process of the machine, and the ending signalled
//! them, the user's own programs among them. A wrong selection must not be able to end
//! anyone's but the window's, so a signal is sent here, after a check of its own that
//! shares nothing with the selection (not its roots, its parents nor its descendants).
//!
//! **The host only ever signals a process that is the pane's program, or that runs
//! under it by the process each was started by, or that is in the session the program
//! leads, in the table being acted on; or that was seen so at a look before, or runs
//! under one that was.** A process the bound refuses is not signalled, and what asked for
//! it does not name it as ended. When in doubt it ends less. A stranger (the user's shell,
//! a build, the process that started the pane's program) is under the program by no
//! parent, and in no session of its, and is never signalled.
//!
//! The session is the kernel's own rule: a process is in a session only by being started
//! by one that is, and a new session is named for the process that makes it, so while the
//! pane's program leads its session (it is the pane's program, by its unique identifier
//! and not its number, it runs, and it is its own session's leader) every process in that
//! session is the window's, whoever started it and whether or not the process that
//! started it is still there: a job an agent detached from its shell is one. So it is once
//! the program has gone, for as long as nobody has its number: the session keeps the name
//! of its leader while it has a member, and what the system says of who started a job
//! cannot be relied on after that (below). A session whose number another process has
//! now allows nothing, nor does one the program was not the leader of: a number is given
//! again once its process and its session are gone.
//!
//! **What the system says of who started a process is no proof once that process has
//! ended.** It records the identifier of a process's parent when the process is forked and
//! again at each exec, from the parent of that moment (T-384), and keeps it when that parent
//! ends: a job that has exec'd by then names its starter for good, and one that execs after
//! the starter has gone names init's (1). Load does not change what is recorded; it makes the
//! starter's exit beat the job's exec more often, a process just forked waiting for a
//! processor. T-375: of 300 windows opened beside computing threads, 11 left a job that named
//! init when the host looked, and ended nothing of it (`under_load_…` in `tests/headless.rs`;
//! the kernel's rule is held there by `the_identifier_recorded_…`). A job the program left
//! that no look saw (a window never marked, or a job begun after the last look) is then under
//! no process that is known, and its session is all that says it is the window's.
//!
//! Each look at the tree ([`Bound::observe`]) records what the window's is by these
//! rules, told by the identifier the system gives each process, which no later process
//! takes, never by its number alone. What was seen stays the window's while it runs,
//! whatever becomes of the process that started it or of the session's leader; what has
//! gone is let go of.
//!
//! The bound only permits: it chooses nothing, and what it allows is not what is signalled.
//! A close attempts what the selection names, and an ending what the turn's lineage names,
//! so a job a look saw under the program that then left its session (`setsid`), whose shell
//! has gone, is permitted here and is no candidate of a close unless the selection names it
//! (by the process that started it, the session or the marker): it is left running, which
//! ends less, and is the limit written down in [`super`].
//!
//! The pane's process group, which a close signals whole, is the pane's program's own
//! while the program runs ([`Bound::signal_group`]); once it has gone its number may have
//! been given to another process that leads a group of its own, and the group's members
//! are each signalled through the bound instead.
//!
//! Every place that can send a signal is held to a table by an audit of the sources
//! (`audit`), the bound's own senders among them. In a test build the real system's
//! senders ask a guard (`guard`) that reads the system itself, which does not depend on the
//! table the bound judges by: a fault in that table, which the bound would trust, is the
//! guard's to stop.

use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use super::{key_of, native, Identity, Process};
use crate::background::ProcessKey;

/// What a tree asks of the system: the processes there are, whether one runs, and a signal
/// sent to one. The tests of the tree ask a machine made up, which signals nothing.
pub(super) trait System {
    /// Every process of ours there is now.
    fn processes(&self) -> io::Result<Vec<Process>>;

    /// Whether the process `key` names (its number and when it began) runs now: none that
    /// has taken its number does.
    fn runs(&self, key: ProcessKey) -> bool;

    /// Sends `signal` to the process `to` names, and to none that has taken its
    /// number: one that is gone is no error. Only [`Bound::signal`] calls it.
    fn send(&self, to: Identity, signal: i32) -> io::Result<()>;

    /// Sends `signal` to every process of the group `group`: one that is gone is no
    /// error. Only [`Bound::signal_group`] calls it.
    fn send_group(&self, group: i32, signal: i32) -> io::Result<()>;

    /// Whether the process `pid` carries `marker`, a pane's, in the environment it was
    /// started with: what [`super::ProcessTree::possibly_left`] names by, which the bound
    /// never asks.
    fn carries(&self, pid: i32, marker: &str) -> bool;

    /// Whether no process of any user has the number `pid` ([`native::nobody_holds`]): what
    /// [`super::ProcessTree::possibly_left`] tells a session whose leader has gone by, which
    /// the bound never asks.
    fn nobody_holds(&self, pid: i32) -> bool;
}

/// The system itself.
pub(super) struct Native;

impl System for Native {
    fn processes(&self) -> io::Result<Vec<Process>> {
        native::processes()
    }

    fn runs(&self, key: ProcessKey) -> bool {
        i32::try_from(key.pid)
            .ok()
            .and_then(native::process)
            .is_some_and(|process| key_of(process.identity) == key)
    }

    fn send(&self, to: Identity, signal: i32) -> io::Result<()> {
        native::signal(to, signal)
    }

    fn send_group(&self, group: i32, signal: i32) -> io::Result<()> {
        native::signal_group(group, signal)
    }

    fn carries(&self, pid: i32, marker: &str) -> bool {
        native::has_marker(pid, marker)
    }

    fn nobody_holds(&self, pid: i32) -> bool {
        native::nobody_holds(pid)
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
    /// The session the program was in when the tree attached to it: its own number if it led
    /// one, as the pty makes a pane's program do.
    session: Option<i32>,
    /// The identifiers of what the latest look saw to be the window's, and runs still.
    seen: Arc<Mutex<HashSet<u64>>>,
}

impl Bound {
    /// The bound of a tree whose program is `program`, in the session `session`.
    pub fn new(program: Option<Identity>, session: Option<i32>) -> Self {
        Self {
            program,
            session,
            seen: Arc::default(),
        }
    }

    /// A look at the tree: `table` is the system's processes now. What is the window's by
    /// the rules above, or is under what was seen before, is seen, and what was seen and
    /// has gone is no longer: a process under one that was seen and has gone is seen itself
    /// now.
    pub fn observe(&self, table: &Table<'_>) {
        let led = self.led_session(table);
        let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        let now: HashSet<u64> = table
            .processes
            .iter()
            .filter(|process| self.under(table, &seen, led, process))
            .map(|process| process.identity.unique)
            .collect();
        *seen = now;
    }

    /// Whether `target` may be signalled, `table` being the processes there are now: it
    /// is the pane's program, or is under it, or in the session it leads, or under what
    /// was seen. A process the table does not hold (it has gone, or the system will not
    /// say) is the window's only if it was seen, and the signal itself goes to none that
    /// has taken its number.
    pub fn allows(&self, table: &Table<'_>, target: Identity) -> bool {
        if self.program == Some(target) {
            return true;
        }
        let led = self.led_session(table);
        let seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        match table.by_identifier.get(&target.unique) {
            None => seen.contains(&target.unique),
            Some(process) if process.identity != target => false,
            Some(process) => self.under(table, &seen, led, process),
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

    /// Signals the process group `group` with `signal` through `system`, as a close
    /// signals the pane's own. While the pane's program runs, with the identity it was
    /// attached with, and leads the group, the group is the pane's own whole: no other
    /// process has its number, and none can join it from outside its session. Otherwise
    /// the number may have been given again, to a process that leads a group of its own,
    /// and the group's members are signalled one by one, through the bound, which
    /// refuses the strangers among them. A number that names more than a group (0 is the
    /// caller's own, 1 is none, and -1 is every process) is refused.
    pub fn signal_group(
        &self,
        system: &impl System,
        table: &Table<'_>,
        group: i32,
        signal: i32,
    ) -> io::Result<()> {
        if group <= 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{group} names no process group of a pane"),
            ));
        }
        if self.leads(table, group) {
            return system.send_group(group, signal);
        }
        let mut failure = None;
        for member in table
            .processes
            .iter()
            .filter(|process| process.group == group)
        {
            if let Err(error) = self.signal(system, table, member.identity, signal) {
                failure = Some(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    /// The session the pane's program leads, or led: its own number, while the program runs
    /// with the identity it was attached with and leads it; and once the program has gone,
    /// while none of ours has that number (the session is named for its leader and keeps the
    /// name while it has a member; the table is of our processes, `native::processes` leaves
    /// out those of other users, so a holder of another user's is not seen). None where
    /// another process of ours has the number now (another identifier), and where the
    /// program was no session's leader.
    fn led_session(&self, table: &Table<'_>) -> Option<i32> {
        let program = self.program?;
        let leads = match table.by_identifier.get(&program.unique) {
            Some(process) => process.identity == program && process.session == Some(program.pid),
            None => {
                self.session == Some(program.pid)
                    && !table
                        .processes
                        .iter()
                        .any(|process| process.identity.pid == program.pid)
            }
        };
        leads.then_some(program.pid)
    }

    /// Whether the pane's program runs, with the identity it was attached with, and leads
    /// the process group `group`.
    fn leads(&self, table: &Table<'_>, group: i32) -> bool {
        let Some(program) = self.program else {
            return false;
        };
        program.pid == group
            && table
                .by_identifier
                .get(&program.unique)
                .is_some_and(|process| process.identity == program && process.group == group)
    }

    /// Whether `process` is the program or was seen, or is in the session `led`, or the
    /// process that started it is one of these, or the one that started that, up as far
    /// as `table` names them. The system names the process that started a process by its
    /// identifier, which it keeps naming after that process has gone, so a process under
    /// one that was seen and has gone is found by the identifier alone.
    fn under(
        &self,
        table: &Table<'_>,
        seen: &HashSet<u64>,
        led: Option<i32>,
        process: &Process,
    ) -> bool {
        let program = self.program.map(|program| program.unique);
        let known = |identifier: u64| Some(identifier) == program || seen.contains(&identifier);
        let mut at = process;
        // A table names no process twice among a process's ancestors: the bound on the
        // walk is only for one that is no tree.
        for _ in 0..=table.by_identifier.len() {
            if known(at.identity.unique)
                || known(at.parent)
                || led.is_some_and(|session| at.session == Some(session))
            {
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

//! What a tree selects of the system's table of processes: the processes that are the
//! pane's (the whole of its tree, which a close ends), and the members the daemon is told
//! them by. It reads the whole
//! table, and what it names is a choice that can be wrong: it signals nothing, and what
//! it names is signalled only through [`super::bound`], which shares none of this code.

use std::collections::{HashMap, HashSet};

use super::native::{command_line, exe_of, has_marker, procargs};
use super::{key_of, Identity, Process, ProcessTree};
use crate::background::{Member, ProcessKey};

impl ProcessTree {
    /// The processes of `snapshot` that are the pane's: the root, its children, what
    /// is in its session, whatever carries the pane's marker in its environment (a
    /// process that left its session), `owned` ones found before, and every
    /// descendant of those, by the parent each was started by. A system binary
    /// (`sleep`, `bash`, `tail`) shows no environment to anyone, so it carries no
    /// marker to be found by, and one whose parent has gone is found by the parent it
    /// was started by only where the system still names it (it names the starter if the
    /// job had exec'd before the starter ended, and launchd if it exec'd after, T-384: an
    /// earlier look found a `sleep` that `sh` left behind naming launchd, a later one, on
    /// Darwin 25.5 (macOS 26.5), found it naming the process that started it for as long as
    /// it was watched, which `members` says; which is told by when the job exec'd, not by
    /// how long its starter had run). What the
    /// session holds is found whichever the system does: a process stays in the
    /// session its parent started it in, and a parent that goes changes nothing of it.
    /// The session is the pane's while its number is nobody else's: when the pane's
    /// program has gone and nothing holds the number, it may be given to another
    /// process, which may lead a session of its own, and then what is in a session of
    /// that number is not the pane's to tell.
    pub(super) fn select(
        &self,
        snapshot: &[Process],
        owned: &HashMap<i32, Identity>,
    ) -> HashMap<i32, Identity> {
        let session = self.session.filter(|session| {
            snapshot
                .iter()
                .find(|p| p.identity.pid == *session)
                .is_none_or(|holder| Some(holder.identity) == self.root)
        });
        let mut selected = HashMap::new();
        for p in snapshot {
            if self.root == Some(p.identity)
                || self.root.is_some_and(|root| p.parent == root.unique)
                || session.is_some_and(|session| p.session == Some(session))
                || owned.get(&p.identity.pid) == Some(&p.identity)
                || self.carries_marker(p)
            {
                selected.insert(p.identity.pid, p.identity);
            }
        }
        let parents: HashSet<u64> = owned.values().map(|identity| identity.unique).collect();
        descendants(snapshot, selected, &parents)
    }

    /// The members of `snapshot` that `tree` holds.
    ///
    /// The system names the parent a process was started by with a number that
    /// is the parent's alone, and keeps naming it after the parent has gone (seen
    /// on macOS 26.x with a `sleep` and a Python that `sh` left running: their
    /// ordinary parent number becomes launchd's, and this one does not). Such a
    /// process names a parent that is no process of the tree (as one that named
    /// launchd would), and has none here: its parent is gone.
    pub(super) fn members_in(
        &self,
        snapshot: &[Process],
        tree: &HashMap<i32, Identity>,
    ) -> Vec<Member> {
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

    /// Whether `p` carries the pane's marker. One that began before the root
    /// cannot have got it: the marker was made with the pane.
    fn carries_marker(&self, p: &Process) -> bool {
        self.root.is_none_or(|root| p.identity.born >= root.born)
            && has_marker(p.identity.pid, &self.marker)
    }
}

/// `selected` and whatever of `snapshot` descends from it or from the processes
/// `parents` are the unique identifiers of, by the process each was started by.
fn descendants(
    snapshot: &[Process],
    mut selected: HashMap<i32, Identity>,
    parents: &HashSet<u64>,
) -> HashMap<i32, Identity> {
    loop {
        let before = selected.len();
        for p in snapshot {
            if parents.contains(&p.parent)
                || selected.values().any(|parent| parent.unique == p.parent)
            {
                selected.insert(p.identity.pid, p.identity);
            }
        }
        if selected.len() == before {
            return selected;
        }
    }
}

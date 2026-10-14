//! What a tree selects of the system's table of processes: the processes that are the
//! pane's (the whole of its tree, which a close ends) and the turn's lineage (what an
//! ending of a turn ends). It reads the whole table, and what it names is a choice that
//! can be wrong: it signals nothing, and what it names is signalled only through
//! [`super::bound`], which shares none of this code.

use std::collections::{HashMap, HashSet};

use super::{has_marker, Identity, Process, ProcessTree, Subject};
use crate::background::Member;

/// The turn's work as it was when its ending began, chosen once: each process by
/// identity (its number, when it began and the identifier the system gives it, which no
/// later process takes), and the members the daemon is told it by. What an ending of the
/// turn goes on to end is these and what they started, found by the identifier of the
/// process that started it, which names a parent that has gone as it does one that
/// runs.
#[derive(Debug)]
pub(crate) struct Lineage {
    pub(super) identities: Vec<Identity>,
    pub(super) members: Vec<Member>,
}

impl Lineage {
    /// What of `snapshot` is the lineage's: its processes that run, by identity, those
    /// of `known` (what was found under it before, which may have gone), and
    /// whatever descends from any of them, by the process each was started by.
    pub(super) fn found(
        &self,
        snapshot: &[Process],
        known: &HashMap<i32, Identity>,
    ) -> HashMap<i32, Identity> {
        let roots: HashMap<i32, Identity> = snapshot
            .iter()
            .filter(|p| {
                self.identities.contains(&p.identity)
                    || known.get(&p.identity.pid) == Some(&p.identity)
            })
            .map(|p| (p.identity.pid, p.identity))
            .collect();
        let parents: HashSet<u64> = self
            .identities
            .iter()
            .chain(known.values())
            .map(|identity| identity.unique)
            .collect();
        descendants(snapshot, roots, &parents)
    }
}

impl ProcessTree {
    /// The processes of `snapshot` that are the pane's: the root, its children, what
    /// is in its session, whatever carries the pane's marker in its environment (a
    /// process that left its session), `owned` ones found before, and every
    /// descendant of those, by the parent each was started by. A system binary
    /// (`sleep`, `bash`, `tail`) shows no environment to anyone, so it carries no
    /// marker to be found by, and one whose parent has gone is found by the parent it
    /// was started by only where the system still names it (an earlier look found it
    /// naming launchd once launchd had taken the job, a `sleep` that `sh` left behind;
    /// a later one, on Darwin 25.5 (macOS 26.5), found it naming the process that
    /// started it for as long as it was watched, which `members` says). What the
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

    /// What `subject` names of `snapshot`, each told by when it began and not by its
    /// number alone: the pane's whole tree, or the turn's lineage; `known` is what
    /// was found before, which may have gone.
    pub(super) fn chosen(
        &self,
        subject: Subject<'_>,
        snapshot: &[Process],
        known: &HashMap<i32, Identity>,
    ) -> HashMap<i32, Identity> {
        match subject {
            Subject::Whole => self.select(snapshot, known),
            Subject::Lineage(lineage) => lineage.found(snapshot, known),
        }
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

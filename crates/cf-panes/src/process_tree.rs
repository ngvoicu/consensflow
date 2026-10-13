//! macOS pane ownership survives setsid and ordinary parent exit. Never match
//! executable names, working directories, or arbitrary command-line substrings
//! to say what is the pane's: the tree has no process that those add. A tree is
//! looked at ([`ProcessTree::members`]), asked to end
//! ([`ProcessTree::ask_to_exit`]) and, last, ended ([`ProcessTree::terminate`], and
//! [`ProcessTree::finish`] for what a turn left), the whole of it when a window
//! closes and the turn's work, which the baseline proves ([`Subject`]), when only
//! that is.
//!
//! The pane's program is a session leader (the pty makes it one), and every process
//! it starts is in its session unless it leaves it with `setsid`, whoever started it
//! and whether or not the process that started it is still there: a job an agent
//! detached with `nohup … &` from a shell that ended is left to launchd, and the
//! system names no parent for it that the tree has, but it is in the session still.
//! A harness that gives each command a shell of its own with `setsid` (Pi's Bash
//! tool does) leaves the job in the session of that shell, which the pane's does not
//! hold: the system is asked to say what the pane's program starts as it starts it
//! ([`tracker`]), and the tree keeps what it names. So the tree finds a process by the
//! parent it was started by where that is still named, by its session, by having
//! been tracked, and by the marker its environment carries where one shows it. What
//! the system says of a process is read in [`native`].
mod native;
mod tracker;

use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::background::{report, Baseline, Member, Parent, ProcessKey};
use cf_proto::panes::Ending;
use native::{command_line, exe_of, has_marker, procargs, process, processes, signal};
use tracker::Tracker;

pub(crate) const OWNER_ENV: &str = "CF_PANE_PROCESS_OWNER";

/// How long a process that was sent SIGKILL is given to be gone: the signal is
/// delivered at once, and the process is gone when its system call returns.
const AFTER_KILL: Duration = Duration::from_secs(2);

/// What a tree is asked to end, or ended.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Subject<'a> {
    /// All of it: a window closes. The pane's own process is part of it.
    Whole,
    /// What `baseline` proves the turn's work, found again at each step so that what
    /// the processes asked to end started while they were given the grace is in it:
    /// what a turn left. The pane's own process is never part of it.
    Turn(&'a Baseline),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Identity {
    pid: i32,
    born: (u64, u64),
    unique: u64,
}

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
    /// What was chosen to be asked, for a turn's work: the members themselves,
    /// which is what an ending tells of.
    members: Vec<Member>,
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

/// What a step of an ending chose: the processes, and for a turn's work the members
/// it told them by.
struct Chosen {
    identities: HashMap<i32, Identity>,
    members: Vec<Member>,
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
    /// What the pane's program has started, as it started it, where the system could
    /// be asked to say it.
    tracker: Option<Arc<Tracker>>,
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
            tracker: None,
        }
    }

    pub fn attach(&mut self, pid: i32) {
        let root = process(pid);
        self.root = root.as_ref().map(|p| p.identity);
        self.session = root.and_then(|p| p.session);
        self.tracker = Tracker::follow(pid).ok().map(Arc::new);
    }

    /// The processes of `snapshot` that are the pane's: the root, its children, what
    /// is in its session, what the tracking has seen it start, whatever carries the
    /// pane's marker in its environment (a process that left its session), `owned`
    /// ones found before, and every descendant of those, by the parent each was
    /// started by. A system binary (`sleep`, `bash`, `tail`) shows no environment to
    /// anyone, so it carries no marker to be found by, and one whose parent has gone
    /// is found by the parent it was started by only where the system still names it
    /// (an earlier look found it naming launchd once launchd had taken the job, a
    /// `sleep` that `sh` left behind; a later one, on Darwin 25.5 (macOS 26.5), found
    /// it naming the process that started it for as long as it was watched, which
    /// `members` says). What the session holds is found whichever the system does: a
    /// process stays in the session its parent started it in, and a parent that goes
    /// changes nothing of it. What left the session too, with its shell gone, is what
    /// the tracking has kept.
    fn select(
        &self,
        snapshot: &[Process],
        owned: &HashMap<i32, Identity>,
    ) -> HashMap<i32, Identity> {
        let tracked = self.tracked(snapshot);
        let mut selected = HashMap::new();
        for p in snapshot {
            if self.root == Some(p.identity)
                || self.root.is_some_and(|root| p.parent == root.unique)
                || self
                    .session
                    .is_some_and(|session| p.session == Some(session))
                || tracked.get(&p.identity.unique) == Some(&p.identity)
                || owned.get(&p.identity.pid) == Some(&p.identity)
                || self.carries_marker(p)
            {
                selected.insert(p.identity.pid, p.identity);
            }
        }
        // A process that has gone is still the parent of what it started: the ones
        // found before and the ones the tracking saw start.
        let parents: HashSet<u64> = owned
            .values()
            .chain(tracked.values())
            .map(|identity| identity.unique)
            .collect();
        descendants(snapshot, selected, &parents)
    }

    /// What the tracking has seen the pane's program start, told by unique
    /// identifier: the ones that run and the ones that have gone. What has gone and is
    /// the parent of nothing that runs is let go of here, for it will never be again.
    fn tracked(&self, snapshot: &[Process]) -> HashMap<u64, Identity> {
        let Some(tracker) = &self.tracker else {
            return HashMap::new();
        };
        let seen = tracker.seen();
        let running: HashSet<u64> = snapshot.iter().map(|p| p.identity.unique).collect();
        let named: HashSet<u64> = snapshot.iter().map(|p| p.parent).collect();
        tracker.forget(|identity| {
            running.contains(&identity.unique) || named.contains(&identity.unique)
        });
        seen
    }

    /// What `subject` names of `snapshot`: the pane's tree, or the processes of it
    /// that its baseline proves the turn's work, each told by when it began and not
    /// by its number alone.
    fn subject(
        &self,
        subject: Subject<'_>,
        snapshot: &[Process],
        owned: &HashMap<i32, Identity>,
    ) -> Chosen {
        let tree = self.select(snapshot, owned);
        match subject {
            Subject::Whole => Chosen {
                identities: tree,
                members: Vec::new(),
            },
            Subject::Turn(baseline) => {
                let members = self.members_in(snapshot, &tree);
                let work = baseline.turn_work(&members);
                let keys: HashSet<ProcessKey> = work.iter().map(|member| member.key).collect();
                let identities = snapshot
                    .iter()
                    .filter(|p| tree.get(&p.identity.pid) == Some(&p.identity))
                    .filter(|p| keys.contains(&key_of(p)))
                    .map(|p| (p.identity.pid, p.identity))
                    .collect();
                Chosen {
                    identities,
                    members: work,
                }
            }
        }
    }

    /// Whether `p` carries the pane's marker. One that began before the root
    /// cannot have got it: the marker was made with the pane.
    fn carries_marker(&self, p: &Process) -> bool {
        self.root.is_none_or(|root| p.identity.born >= root.born)
            && has_marker(p.identity.pid, &self.marker)
    }

    /// The processes of the tree now, each with the command line it was
    /// started with, the program and the path it runs from, the process group it is
    /// in and the process of the tree that started it: a look, nothing is signalled
    /// or stopped.
    pub fn members(&self) -> io::Result<Vec<Member>> {
        let snapshot = processes()?;
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
            .map(|p| (p.identity.unique, key_of(p)))
            .collect();
        held.iter()
            .map(|p| Member {
                key: key_of(p),
                command: procargs(p.identity.pid)
                    .and_then(|bytes| command_line(&bytes))
                    .unwrap_or_else(|| p.name.clone()),
                program: p.name.clone(),
                exe: exe_of(p.identity.pid),
                group: Some(p.group),
                // The system names no parent by a number that is nobody's.
                parent: match p.parent {
                    0 => Parent::Unknown,
                    number => keys.get(&number).copied().map_or(Parent::Gone, Parent::Of),
                },
            })
            .collect()
    }

    /// Asks `subject` to end as a closing terminal does: TERM, then HUP, to
    /// every process of it outside `group`, the pane's own process group, which
    /// a close signals whole (an ending has none). Nothing is stopped first: a
    /// stopped process cannot answer a signal. The processes asked, which are kept
    /// for the ending's end, and whether some could not be.
    pub fn ask_to_exit(&self, subject: Subject<'_>, group: Option<i32>) -> io::Result<Asked> {
        let snapshot = processes()?;
        let chosen = self.subject(subject, &snapshot, &HashMap::new());
        let outside: Vec<Identity> = snapshot
            .iter()
            .filter(|p| chosen.identities.get(&p.identity.pid) == Some(&p.identity))
            .filter(|p| Some(p.group) != group)
            .map(|p| p.identity)
            .collect();
        let mut failed = false;
        for asking in [libc::SIGTERM, libc::SIGHUP] {
            for identity in &outside {
                failed |= signal(*identity, asking).is_err();
            }
        }
        Ok(Asked {
            identities: outside,
            members: chosen.members,
            failed,
        })
    }

    /// Ends `subject` by force. The pane's own process is ended with a `Whole`
    /// tree and never with what an ending was asked for.
    pub fn terminate(&self, subject: Subject<'_>) -> io::Result<()> {
        match self.force(subject, &[]).failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Ends what a turn left, by force, once `asked` has had the grace: the
    /// processes it chooses now and what is in the tree beyond what it chose then (a
    /// handler asked to end may have started a process before it ended, and that
    /// process's parent is gone, but it is in the session and it is told from the
    /// harness's as any is), then waits for every process it ended to be gone. What
    /// it chose, told by whether it is: ended, or not ended.
    pub fn finish(&self, subject: Subject<'_>, asked: Asked) -> Ending {
        let seeds: Vec<Identity> = asked.identities.clone();
        let forced = self.force(subject, &seeds);
        let mut chosen = asked.members;
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
                        .is_some_and(|p| key_of(&p) == member.key)
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
    /// asked, are parents of what it finds though they are gone.
    fn force(&self, subject: Subject<'_>, seeds: &[Identity]) -> Forced {
        let mut known: HashMap<i32, Identity> =
            seeds.iter().map(|seed| (seed.pid, *seed)).collect();
        let mut frozen: HashMap<i32, Identity> = HashMap::new();
        let mut members: Vec<Member> = Vec::new();
        let mut failure = None;
        // Freeze proven parents before rescanning, so they cannot keep forking
        // through shutdown. Always kill already frozen processes on an error.
        for pass in 0..16 {
            let snapshot = match processes() {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            };
            let chosen = self.subject(subject, &snapshot, &known);
            let fresh: Vec<_> = chosen
                .identities
                .values()
                .filter(|p| frozen.get(&p.pid) != Some(p))
                .copied()
                .collect();
            members.extend(chosen.members);
            if fresh.is_empty() {
                break;
            }
            for identity in fresh {
                if let Err(error) = signal(identity, libc::SIGSTOP) {
                    failure = Some(error);
                }
                frozen.insert(identity.pid, identity);
                known.insert(identity.pid, identity);
            }
            if pass == 15 {
                failure = Some(io::Error::other("pane process tree did not stabilize"));
            }
        }
        for identity in frozen.values() {
            if let Err(error) = signal(*identity, libc::SIGKILL) {
                failure = Some(error);
            }
        }
        // A failed process enumeration must still stop the exact root we own.
        if let (Subject::Whole, Some(root)) = (subject, self.root) {
            if let Err(error) = signal(root, libc::SIGKILL) {
                failure = Some(error);
            }
        }
        Forced { members, failure }
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

/// A process as the baseline knows it: its number and when it began.
fn key_of(p: &Process) -> ProcessKey {
    ProcessKey {
        pid: p.identity.pid.unsigned_abs(),
        began: p.identity.born.0 * 1_000_000 + p.identity.born.1,
    }
}

#[cfg(test)]
mod tests;

//! The processes of the run's windows, and the strangers among them: what a
//! window's tree ran (the pane host's word, `pane.background`, look by look, and
//! what a window's `pane.exit` said its close ended) and the program each window was
//! opened on, followed after the window is gone. A window ends what it ran when it
//! closes, and the app's quit ends every window; a process still there a grace after
//! its window went belongs to no window (6, B2: "no process of the run belongs to
//! none"). Each stranger is said once, and left running: the run ends only what it
//! started itself, and the identity is what the report says it by.
//!
//! A process is followed by the window that ran it, by its id and its generation: a
//! window opened again under its id is another window. A look takes the trees of the
//! open windows before it reads the exits the host told since the last, so a tree it
//! took of a window that then went is no word that the window still runs: only
//! another window's having a process takes it back from its gone window, and asks
//! again who it is, for a number another window has may be another process's.
//!
//! Whether a process is there is asked of the system by its number and when it began
//! (its identity), so that a number taken by another process is not taken for it. A
//! process's identity is taken only while its window shows it: at the look that saw
//! its number under the window, by that look's one asking. Where that asking failed,
//! or named a process that began after the look saw the number, or the process was
//! first known from a window's exit (whose list names a number and a command, nothing
//! that ties the number to a process), its identity is never taken: if its number is
//! there when it is judged, it is unresolved, and said so. One known already whose
//! asking failed at a later look that saw its number is doubted: the window may have
//! run another process under the number then, which the system never named; a
//! process there at the end begun at another time is unresolved, never another's. A
//! number no process has is gone, whoever it was. The asking is the caller's: the
//! tracker says which numbers, and is told what the system said or why it could not.

use std::collections::{BTreeMap, BTreeSet};

use super::look::{Process, Unresolved};
use super::processes;

/// How long a process may outlive its window before it is a stranger: a close
/// asks the work to end and gives it up to ten seconds, and the run looks every few.
pub const GRACE: i64 = 60_000;

/// A window, by its id and its generation.
pub type Owner = (String, i64);

/// Who a followed process is, as far as the system said.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Identity {
    /// Seen under its window by the look that is asking now.
    Asking,
    /// When it began, as the system said to the look that saw it.
    Began(String),
    /// When it began, as the system said to a look that saw it; but a look that saw
    /// its number since could not ask who was under it (why): another process may have
    /// had the number then.
    Doubted { began: String, why: String },
    /// Never to be taken, and why.
    Never(String),
}

/// A process of a window, followed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Tracked {
    command: String,
    owner: Owner,
    identity: Identity,
    /// When its window went, in milliseconds.
    gone: Option<i64>,
    /// When its window's exit listed its number among what its close ended.
    listed: Option<i64>,
}

/// A process of the run found there after its window went, the same process it was:
/// a stranger, to be said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leftover {
    pub process: Process,
    pub pane: String,
    pub since: i64,
    /// When the system says it began: what it is known by.
    pub began: String,
}

/// What a check of the processes found: the leftovers, and the processes it could
/// not judge.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub leftovers: Vec<Leftover>,
    pub unresolved: Vec<Unresolved>,
}

/// The processes the run's windows ran.
#[derive(Debug, Default)]
pub struct Tracker {
    tracked: BTreeMap<u32, Tracked>,
    /// The numbers the look asking now saw under their open windows.
    shown: BTreeSet<u32>,
}

impl Tracker {
    /// A look found `processes` in the open window `owner`. A process new to the
    /// tracker is followed from now; one another window had is this window's now; one
    /// its own window had is its still (a tree the look took before that window's exit
    /// was told brings nothing back). Every one the open window shows is asked who it
    /// is, at every look ([`identify`]): the same window may have run another process
    /// under the same number since. The numbers this look is to ask.
    ///
    /// [`identify`]: Tracker::identify
    pub fn saw(&mut self, owner: &Owner, processes: &[Process]) -> Vec<u32> {
        let mut asked = Vec::new();
        for process in processes {
            match self.tracked.get_mut(&process.pid) {
                Some(tracked) if tracked.owner == *owner => {
                    if tracked.gone.is_some() {
                        continue;
                    }
                    tracked.command.clone_from(&process.command);
                }
                _ => {
                    self.tracked
                        .insert(process.pid, Tracked::new(process, owner, Identity::Asking));
                }
            }
            self.shown.insert(process.pid);
            asked.push(process.pid);
        }
        asked
    }

    /// What the look that saw the numbers [`saw`](Tracker::saw) gave it found when
    /// it asked who they are, the trees having been taken at `shown_at`: when each
    /// that is there began, or why the system could not say. One new to the tracker is
    /// known by what it named, unless that began after `shown_at`: then, or where the
    /// system could not say, never. One known already that began at another time is
    /// another process its window runs under the same number, known from now by when it
    /// began; or, begun after `shown_at`, never known. A process begun after `shown_at`
    /// may be another, given the number since, or the one the tree showed, which was
    /// taken a moment after: which, the system does not say. One it did not name had
    /// ended, and is followed no more. One known already that the system could not be
    /// asked of is doubted: the window may run another process under its number since,
    /// which nothing said; a later look that asks resolves it.
    pub fn identify(&mut self, shown_at: i64, answer: Result<&BTreeMap<u32, String>, &str>) {
        for pid in std::mem::take(&mut self.shown) {
            let Some(tracked) = self.tracked.get_mut(&pid) else {
                continue;
            };
            let said = match answer {
                Err(why) => {
                    match &tracked.identity {
                        Identity::Asking => {
                            tracked.identity = Identity::Never(format!(
                                "the system could not say who it was at the look that saw it ({why})"
                            ));
                        }
                        Identity::Began(began) => {
                            tracked.identity = Identity::Doubted {
                                began: began.clone(),
                                why: format!(
                                    "the system could not say who was under its number at a look that saw it there since ({why})"
                                ),
                            };
                        }
                        Identity::Doubted { .. } | Identity::Never(_) => {}
                    }
                    continue;
                }
                Ok(said) => said,
            };
            let Some(began) = said.get(&pid) else {
                self.tracked.remove(&pid);
                continue;
            };
            let after = processes::began_at(began).is_some_and(|began| began > shown_at);
            match &tracked.identity {
                Identity::Never(_) => {}
                Identity::Began(known) if known == began => {}
                Identity::Asking | Identity::Began(_) | Identity::Doubted { .. } if after => {
                    tracked.identity = Identity::Never(
                        "the process the system named under its number began after the look saw it there".to_owned(),
                    );
                }
                Identity::Asking | Identity::Began(_) | Identity::Doubted { .. } => {
                    tracked.identity = Identity::Began(began.clone());
                }
            }
        }
    }

    /// The window `owner` went at `at`, its close having ended `ended` (what its
    /// `pane.exit` lists): those are followed from then, gone with it, and so is
    /// everything else the window had. One the window's looks never showed is known
    /// by the exit's word alone, which ties its number to no process: its identity is
    /// never taken. Each number the exit listed is kept as listed then.
    pub fn gone(&mut self, owner: &Owner, at: i64, ended: &[Process]) {
        for process in ended {
            let tracked = self
                .tracked
                .entry(process.pid)
                .and_modify(|tracked| {
                    if tracked.owner != *owner {
                        *tracked = Tracked::new(process, owner, Tracked::from_an_exit());
                    }
                })
                .or_insert_with(|| Tracked::new(process, owner, Tracked::from_an_exit()));
            tracked.listed = Some(at);
        }
        for tracked in self.tracked.values_mut() {
            if tracked.owner == *owner && tracked.gone.is_none() {
                tracked.gone = Some(at);
            }
        }
    }

    /// Every window went at `at`: the app quit, or the run is over.
    pub fn all_gone(&mut self, at: i64) {
        for tracked in self.tracked.values_mut() {
            tracked.gone.get_or_insert(at);
        }
    }

    /// The numbers of the processes whose window went more than [`GRACE`] before
    /// `now`: whose presence is to be asked.
    pub fn due(&self, now: i64) -> Vec<u32> {
        self.tracked
            .iter()
            .filter(|(_, tracked)| tracked.gone.is_some_and(|gone| now - gone > GRACE))
            .map(|(pid, _)| *pid)
            .collect()
    }

    /// The numbers of every process followed: whose presence is asked at the end.
    pub fn followed(&self) -> Vec<u32> {
        self.tracked.keys().copied().collect()
    }

    /// What the due at `now` are, the system having said what is there (`there`):
    /// each said once and followed no more.
    pub fn strangers(&mut self, now: i64, there: &BTreeMap<u32, String>) -> Found {
        let due = self.due(now);
        self.take(&due, now, there)
    }

    /// What every process still followed is, once the run is over: said once.
    pub fn left(&mut self, now: i64, there: &BTreeMap<u32, String>) -> Found {
        let all = self.followed();
        self.take(&all, now, there)
    }

    /// Every process still followed, unresolved: the system could not be asked, at
    /// the end, whether it still runs (`why`).
    pub fn unresolved_all(&mut self, now: i64, why: &str) -> Vec<Unresolved> {
        std::mem::take(&mut self.tracked)
            .into_iter()
            .map(|(pid, tracked)| Unresolved {
                process: Process {
                    pid,
                    command: tracked.command,
                },
                pane: tracked.owner.0,
                since: tracked.gone.unwrap_or(now),
                why: format!("the system could not be asked whether it still runs: {why}"),
            })
            .collect()
    }

    /// `pids`, judged by what is `there`: one there as it was is a leftover; one
    /// whose identity was never taken and whose number is there is unresolved (who is
    /// under the number cannot be told). One that began at another time than the look
    /// knew is another process: where its window's exit listed the number after the
    /// look knew it, the process under it may be the window's own, begun since, which
    /// it is where the system says it began before the exit, none of the window's where
    /// after, and unresolved where the system says the second alone; else another's.
    /// Unless its identity is doubted: then it may be the one a look that could not ask
    /// saw, and it is unresolved. The rest are gone.
    fn take(&mut self, pids: &[u32], now: i64, there: &BTreeMap<u32, String>) -> Found {
        let mut found = Found::default();
        for pid in pids {
            let Some(tracked) = self.tracked.remove(pid) else {
                continue;
            };
            let Some(now_said) = there.get(pid) else {
                continue;
            };
            let judged = match &tracked.identity {
                Identity::Began(began) => match under(began, now_said, tracked.listed) {
                    Under::Its => Ok(()),
                    Under::Another => continue,
                    Under::Untold => Err("its number holds a process begun at another time than the one the look knew, and its window's exit listed the number: whether that is the window's own, begun since, the system does not say to the moment".to_owned()),
                },
                // A process begun at another time may be the one a look that could not
                // ask saw under the number: never another to drop.
                Identity::Doubted { began, why } => match under(began, now_said, tracked.listed) {
                    Under::Its => Ok(()),
                    Under::Another | Under::Untold => Err(why.clone()),
                },
                Identity::Never(why) => Err(why.clone()),
                Identity::Asking => Err("its identity was never asked".to_owned()),
            };
            let process = Process {
                pid: *pid,
                command: tracked.command,
            };
            let since = tracked.gone.unwrap_or(now);
            let Err(never) = judged else {
                found.leftovers.push(Leftover {
                    process,
                    pane: tracked.owner.0,
                    since,
                    began: now_said.clone(),
                });
                continue;
            };
            found.unresolved.push(Unresolved {
                process,
                pane: tracked.owner.0,
                since,
                why: format!(
                    "{never}, so whether the process under its number is it cannot be told; it was left running"
                ),
            });
        }
        found
    }
}

/// Whose the process under a followed number is, by when the system says it began.
enum Under {
    /// The window's: the process the look knew, or one its exit listed.
    Its,
    /// None of the window's.
    Another,
    /// The system says it to the second alone, which cannot tell.
    Untold,
}

/// Whose the process under a number is: the look knew one begun at `known`, the system
/// says the one there now began at `now`, and the window's exit listed the number at
/// `listed`, if it did. Another process than the one known is the window's own only
/// where the exit listed its number and the system says it began before that.
fn under(known: &str, now: &str, listed: Option<i64>) -> Under {
    if known == now {
        return Under::Its;
    }
    let Some(listed) = listed else {
        return Under::Another;
    };
    match processes::began_at(now) {
        Some(began) if began <= listed => Under::Its,
        Some(_) => Under::Another,
        None => Under::Untold,
    }
}

impl Tracked {
    /// `process`, followed as the window `owner`'s, with `identity`.
    fn new(process: &Process, owner: &Owner, identity: Identity) -> Self {
        Self {
            command: process.command.clone(),
            owner: owner.clone(),
            identity,
            gone: None,
            listed: None,
        }
    }

    /// The identity of a process first known from its window's exit: never taken.
    fn from_an_exit() -> Identity {
        Identity::Never(
            "it was first known from its window's exit, which says no more of it than its number"
                .to_owned(),
        )
    }
}

#[cfg(test)]
mod tests;

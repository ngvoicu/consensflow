//! What the run learns of the command's processes from the machine, ordered against the turn's end.
//!
//! The pane host lists a worker's command while the turn works, and the window's close after the
//! result ends what it finds still running and tells the chief so. A process the run finds gone
//! once the task is seen over may have ended of itself in the turn, which needs no note, or have
//! been ended by the close, which the chief is owed one of. The two look alike afterwards (T-1339
//! killed a real `sleep` as the close would and showed the verdict taking it for the first), so
//! only what was seen *before* the close tells them apart.
//!
//! Each look of the run takes the machine's processes twice, the pane host's own look between
//! them, and reads the task last. A process is the one that began at a time, not the number it had
//! ([`Identity`]: a number another process takes afterwards is another). One seen with the
//! command's words and found absent from the second listing of a look whose task, read after both,
//! was not over, ended while the turn ran: the task being over is final, and the window closes
//! only after it. An absence read from a look the task was over at, one the machine did not
//! answer, and a process never seen by identity say nothing.

use cf_e2e::live::members::{Identity, Running};
use cf_e2e::live::tree::Proc;

/// A process the run saw with the command's words, and whether the machine was found without it
/// at a look the turn still ran through.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    identity: Identity,
    ended_in_turn: bool,
}

/// What the run's looks have shown of the command's processes.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Looks {
    seen: Vec<Seen>,
}

impl Looks {
    /// One look: the machine's processes `before` and `after` the pane host's own (none where
    /// the machine did not answer), and `in_turn` for the task read as not over once both were
    /// taken.
    pub fn look(
        &mut self,
        before: Option<&[Running]>,
        after: Option<&[Running]>,
        in_turn: bool,
        mark: &str,
    ) {
        // Seen alive, by identity, wherever the look fell: an identity is no claim of when.
        for running in before.into_iter().chain(after).flatten() {
            if running.command.contains(mark)
                && !self
                    .seen
                    .iter()
                    .any(|known| known.identity == running.identity)
            {
                self.seen.push(Seen {
                    identity: running.identity,
                    ended_in_turn: false,
                });
            }
        }
        // Found gone only by a listing the machine gave after the pane host's look and before
        // the task was read as not over: a look the task was over at may be one the close has
        // already ended the process at. The process is told by its identity alone: the same one
        // under other words, or under another parent, is there.
        if let (true, Some(after)) = (in_turn, after) {
            for seen in &mut self.seen {
                if !after
                    .iter()
                    .any(|running| running.identity == seen.identity)
                {
                    seen.ended_in_turn = true;
                }
            }
        }
    }

    /// The numbers of the processes of `listed` that ended in the turn: those the run saw by
    /// identity and found, each, gone from a look of the turn. A listed number the run never saw
    /// a process of has none to follow, and is not among them; one that two processes had, in
    /// turn, is where both ended.
    pub fn gone_of(&self, listed: &[Proc]) -> Vec<u64> {
        listed
            .iter()
            .filter(|process| {
                let had: Vec<&Seen> = self
                    .seen
                    .iter()
                    .filter(|seen| u64::from(seen.identity.pid) == process.pid)
                    .collect();
                !had.is_empty() && had.iter().all(|seen| seen.ended_in_turn)
            })
            .map(|process| process.pid)
            .collect()
    }

    /// The numbers of the processes of `listed` that are still on the machine, `all` of its
    /// processes: where the run saw the process, the one that began when it did (a number another
    /// process took afterwards is none of them); where it never did, any process of that number
    /// that holds the command's words.
    pub fn alive_of(&self, listed: &[Proc], all: &[Running], mark: &str) -> Vec<u64> {
        listed
            .iter()
            .filter(|process| {
                let began: Vec<Identity> = self
                    .seen
                    .iter()
                    .filter(|seen| u64::from(seen.identity.pid) == process.pid)
                    .map(|seen| seen.identity)
                    .collect();
                all.iter().any(|running| {
                    u64::from(running.identity.pid) == process.pid
                        && if began.is_empty() {
                            running.command.contains(mark)
                        } else {
                            began.contains(&running.identity)
                        }
                })
            })
            .map(|process| process.pid)
            .collect()
    }
}

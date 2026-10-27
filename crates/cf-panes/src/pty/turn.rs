//! Where a pane's turn began, and what it started and left running (see
//! [`crate::background`]). The daemon says a turn is about to begin
//! (`pane.mark`: [`PaneTable::mark`]), how its harness keeps its servers and which
//! harness it is, and asks what the pane's tree has running that the turn's work is told
//! by ([`PaneTable::background`]). Nothing here ends it: a turn ends nothing it left
//! running (turn-end), and the window's close ends what is the window's ([`super::close`]). A
//! close lists what it is about to end for the exit it sends: what the window left
//! running ([`Pane::keep_background`]). The window's group is the pane's own process
//! group (`Pane::process_group_id`), which the close signals whole.
//!
//! What a close and the exit of a window's program name as the turn's work is what the
//! host finds at two looks, [`SETTLE`] apart: a short run of the harness's own, which the
//! host cannot tell from the agent's, is no process at the second
//! ([`crate::background`]). The look the daemon asks for while a turn runs is one look.

use std::io;
#[cfg(windows)]
use std::sync::Arc;
use std::thread;

use cf_proto::panes::{Background, BackgroundProcess, CommandsAnswer, Servers};

use super::{Pane, PaneError, PaneKey, PaneTable};
use crate::background::{listed, steady, Baseline, Kept, Member, SETTLE};
use crate::ended::EndedSlot;

/// A pane's tree, held where the table's lock is not: looking at it walks the
/// system's processes, which no lock of the table is held for.
#[derive(Clone)]
struct Tree {
    #[cfg(target_os = "macos")]
    tree: crate::process_tree::ProcessTree,
    #[cfg(windows)]
    job: Arc<crate::job_object::JobObject>,
}

/// What a look that must read all of a pane's tree found ([`Tree::readable_members`]).
// Only a Mac reads a tree whole: a Windows job is asked of no commands.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
enum Read {
    /// Every process of it, each read.
    All(Vec<Member>),
    /// A process of it the system will not say what it is (another user's).
    Unreadable,
    /// It could not be read whole: it kept changing as it was read.
    Unsettled,
}

impl Tree {
    /// [`Tree::members`], where every process the window's program and each of them
    /// started can be read ([`Read`]; `ProcessTree::readable_members`): unreadable where the
    /// platform cannot say (a Windows job is asked of no commands).
    fn readable_members(&self) -> io::Result<Read> {
        #[cfg(target_os = "macos")]
        {
            use crate::process_tree::Readable;
            Ok(match self.tree.readable_members()? {
                Readable::All(members) => Read::All(members),
                Readable::Unreadable => Read::Unreadable,
                Readable::Unsettled => Read::Unsettled,
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            Ok(Read::Unreadable)
        }
    }

    /// The processes it has now. A platform that cannot look has none.
    fn members(&self) -> io::Result<Vec<Member>> {
        #[cfg(target_os = "macos")]
        {
            self.tree.members()
        }
        #[cfg(windows)]
        {
            self.job.members()
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            Ok(Vec::new())
        }
    }

    /// What may be the window's and runs outside its tree, which nothing signals
    /// (`ProcessTree::possibly_left`): on macOS; elsewhere none, and on Windows the pane's
    /// job holds what its processes start wherever it goes.
    fn possibly_left(&self) -> Vec<cf_proto::panes::PossiblyLeft> {
        #[cfg(target_os = "macos")]
        {
            self.tree.possibly_left()
        }
        #[cfg(not(target_os = "macos"))]
        {
            Vec::new()
        }
    }

    /// The turn's work that `baseline` proves, and still is [`SETTLE`] after it was
    /// first found ([`steady`]): looked at again when the first look finds any, so that
    /// a run of the harness's own, which has ended by then, is none of it.
    fn steady_work(&self, baseline: &Baseline) -> io::Result<Vec<Member>> {
        let first = baseline.turn_work(&self.members()?);
        if first.is_empty() {
            return Ok(first);
        }
        thread::sleep(SETTLE);
        Ok(steady(&first, baseline.turn_work(&self.members()?)))
    }
}

/// What `tree` has running that is the turn's work, where the turn `baseline` was
/// marked at began. A pane never marked has none, and its tree is not looked at.
fn looked_at(baseline: &Baseline, tree: &Tree) -> io::Result<Background> {
    if !baseline.is_marked() {
        return Ok(Background::default());
    }
    Ok(baseline.left_running(&tree.members()?))
}

/// [`looked_at`], and what it finds is looked for again [`SETTLE`] later: what is still
/// there. What a close and the exit of a window's program say the window left running.
/// The host's watch of the clocks, which every pane's clock is read by
/// (`background::clock`), started with the first pane.
#[cfg(target_os = "macos")]
pub(super) fn watch() -> &'static std::sync::Arc<crate::background::Watch> {
    static WATCH: std::sync::OnceLock<std::sync::Arc<crate::background::Watch>> =
        std::sync::OnceLock::new();
    WATCH.get_or_init(|| crate::background::Watch::start(crate::process_tree::clocks))
}

/// What a look at a pane's commands proves of `member`'s start: that it began before the
/// time the look names, by the pane's clock as it reads now (`seen`), which watched the wall
/// clock keep its order.
#[cfg(target_os = "macos")]
fn proved_before(seen: Option<&(i64, crate::background::PaneClock)>, member: &Member) -> bool {
    seen.is_some_and(|(after, clock)| {
        crate::background::began_before(clock, *after, crate::process_tree::awake_start)(member)
    })
}

fn steadily_looked_at(baseline: &Baseline, tree: &Tree) -> io::Result<Background> {
    if !baseline.is_marked() {
        return Ok(Background::default());
    }
    Ok(listed(tree.steady_work(baseline)?))
}

impl Pane {
    fn tree(&self) -> Tree {
        Tree {
            #[cfg(target_os = "macos")]
            tree: self.process_tree.clone(),
            #[cfg(windows)]
            job: Arc::clone(&self.job),
        }
    }

    /// What this pane's close is about to end, kept for the exit it will send, once:
    /// where the exit of the window's program was told it already, that is what is
    /// kept, and the tree is not looked at again. A tree that cannot be looked at has
    /// none, and nothing is made up for it.
    pub(super) fn keep_background(&self) {
        if self.ended.background().is_some() {
            return;
        }
        let left = steadily_looked_at(&self.baseline, &self.tree()).unwrap_or_default();
        self.ended.keep_background(left.processes);
    }
}

impl PaneTable {
    /// A turn begins in the pane: every process its tree has now came before
    /// it, and is not the turn's. Where `servers` says how the pane's harness
    /// keeps its own, what they are, and what they start, is not the turn's
    /// either, whenever it began, and so is what `harness` names the programs of
    /// ([`crate::background`]).
    pub fn mark(
        &self,
        key: &PaneKey,
        servers: Servers,
        harness: Option<&str>,
    ) -> Result<(), PaneError> {
        let (tree, group) = {
            let panes = self.lock_panes()?;
            let pane = panes
                .get(key)
                .ok_or_else(|| PaneError::NotFound(key.clone()))?;
            (pane.tree(), pane.process_group_id)
        };
        let members = tree.members()?;
        self.lock_panes()?
            .get_mut(key)
            .ok_or_else(|| PaneError::NotFound(key.clone()))?
            .baseline
            .mark(
                &members,
                Kept::of(servers, group),
                harness.map(str::to_owned),
            );
        Ok(())
    }

    /// What the pane's tree has running that is the turn's work, as it was marked
    /// (a pane never marked has none), and what may be the window's and runs outside its
    /// tree, marked or not.
    pub fn background(&self, key: &PaneKey) -> Result<Background, PaneError> {
        let (tree, baseline) = {
            let panes = self.lock_panes()?;
            let pane = panes
                .get(key)
                .ok_or_else(|| PaneError::NotFound(key.clone()))?;
            (pane.tree(), pane.baseline.clone())
        };
        let mut background = looked_at(&baseline, &tree)?;
        background.possibly_left = tree.possibly_left();
        Ok(background)
    }

    /// The commands of the window's harness that run in the pane's tree now
    /// ([`Baseline::commands`]): none where the host cannot tell them, and none, said
    /// unreadable, where a process of the window cannot be read.
    pub fn commands(
        &self,
        key: &PaneKey,
        since: Option<u64>,
        after: Option<i64>,
    ) -> Result<CommandsAnswer, PaneError> {
        let (tree, baseline) = {
            let panes = self.lock_panes()?;
            let pane = panes
                .get(key)
                .ok_or_else(|| PaneError::NotFound(key.clone()))?;
            (pane.tree(), pane.baseline.clone())
        };
        // Where it cannot tell them the tree is not walked.
        if !baseline.tells_commands() {
            return Ok(CommandsAnswer::default());
        }
        // The pane's clock read now, where a look names a time: only a Mac tells commands.
        #[cfg(target_os = "macos")]
        let seen = match after {
            Some(after) => {
                let clock = self
                    .lock_panes()?
                    .get(key)
                    .map(|pane| std::sync::Arc::clone(&pane.clock))
                    .ok_or_else(|| PaneError::NotFound(key.clone()))?;
                Some((after, watch().now(&clock)))
            }
            None => None,
        };
        #[cfg(target_os = "macos")]
        let before = |member: &Member| proved_before(seen.as_ref(), member);
        // Elsewhere no start is read against a call.
        #[cfg(not(target_os = "macos"))]
        let before = {
            let _ = after;
            |_: &Member| false
        };
        Ok(match tree.readable_members()? {
            Read::All(members) => CommandsAnswer {
                commands: baseline.commands(&members, since, &before),
                ..CommandsAnswer::default()
            },
            Read::Unreadable => CommandsAnswer {
                unreadable: true,
                ..CommandsAnswer::default()
            },
            Read::Unsettled => CommandsAnswer {
                unsettled: true,
                ..CommandsAnswer::default()
            },
        })
    }

    /// The wall clock as the pane's clock reads it next is `by` microseconds off: a test's
    /// clock set back or forward.
    #[cfg(all(test, target_os = "macos"))]
    pub(crate) fn set_clock(&self, key: &PaneKey, by: i64) -> Result<(), PaneError> {
        let clock = self
            .lock_panes()?
            .get(key)
            .map(|pane| std::sync::Arc::clone(&pane.clock))
            .ok_or_else(|| PaneError::NotFound(key.clone()))?;
        watch().set(&clock, by);
        Ok(())
    }

    /// The pane is at rest, the daemon says: its tree now is kept, under the number
    /// answered, for a look at its commands to leave out ([`Baseline::rest`]). None where
    /// the host tells no commands of it, and its tree is not walked then.
    pub fn rest(&self, key: &PaneKey, keep: &[u64]) -> Result<Option<u64>, PaneError> {
        let (tree, tells) = {
            let panes = self.lock_panes()?;
            let pane = panes
                .get(key)
                .ok_or_else(|| PaneError::NotFound(key.clone()))?;
            (pane.tree(), pane.baseline.tells_commands())
        };
        if !tells {
            return Ok(None);
        }
        let members = tree.members()?;
        Ok(self
            .lock_panes()?
            .get_mut(key)
            .ok_or_else(|| PaneError::NotFound(key.clone()))?
            .baseline
            .rest(&members, keep))
    }

    /// What the window left running when its program exited, for the exit that
    /// says so: what its close kept if a close ended it, else what its tree
    /// has at two looks, [`SETTLE`] apart, kept so that the close that follows does
    /// not look again.
    pub(crate) fn left_running_at_exit(
        &self,
        key: &PaneKey,
        slot: &EndedSlot,
    ) -> Vec<BackgroundProcess> {
        if let Some(kept) = slot.background() {
            return kept;
        }
        let Ok(panes) = self.lock_panes() else {
            return Vec::new();
        };
        let Some(pane) = panes.get(key) else {
            return Vec::new();
        };
        let (tree, baseline) = (pane.tree(), pane.baseline.clone());
        drop(panes);
        let left = steadily_looked_at(&baseline, &tree)
            .unwrap_or_default()
            .processes;
        slot.keep_background(left);
        slot.background().unwrap_or_default()
    }
}

#[cfg(test)]
mod leaving_tests;
#[cfg(test)]
mod second_look_tests;
#[cfg(test)]
mod support;
#[cfg(test)]
mod tests;

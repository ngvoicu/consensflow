//! Where a pane's turn began, and what it started and left running (see
//! [`crate::background`]). The daemon says a turn is about to begin
//! (`pane.mark`: [`PaneTable::mark`]), how its harness keeps its servers and which
//! harness it is, and asks, when the turn ends, what the pane's tree has running
//! that the turn's work is told by ([`PaneTable::background`]), and to end it
//! ([`PaneTable::end_background`]), the window staying open. A close lists the
//! same for the exit it sends: what is about to be ended is what the window left
//! running ([`Pane::keep_background`]). The window's group is the pane's own
//! process group (`Pane::process_group_id`), which the close signals whole and an
//! ending never does.
//!
//! An ending asks what it ends to end as a close does (TERM, then HUP, on Unix) and
//! gives it the table's grace to do so, and what is left after is ended by force:
//! the same asking, the same grace and the same ending of what is left as a close
//! makes of a tree ([`close`]), of the turn's work where a close makes it of the
//! whole tree and of the group. What the turn's work started while it was given the
//! grace is found again before the force, whoever started it and wherever it went.
//! On Windows they are ended at once: a job object ends all of its tree or none of
//! it, and each process an ending chooses is ended by itself, and waited for.
//!
//! What a close, the exit of a window's program and an ending name as the turn's work
//! is what the host finds at two looks, [`SETTLE`] apart: a short run of the harness's
//! own, which the host cannot tell from the agent's, is no process at the second
//! ([`crate::background`]). The look the daemon asks for while a turn runs is one look.

use std::io;
#[cfg(windows)]
use std::sync::Arc;
use std::thread;
use std::time::Duration;
#[cfg(target_os = "macos")]
use std::time::Instant;

use cf_proto::panes::{Background, BackgroundProcess, Ending, Servers};

#[cfg(target_os = "macos")]
use super::close;
use super::{Pane, PaneError, PaneKey, PaneTable};
use crate::background::{listed, steady, Baseline, Kept, Member, SETTLE};
use crate::ended::EndedSlot;
#[cfg(target_os = "macos")]
use crate::process_tree::Subject;

/// A pane's tree, held where the table's lock is not: looking at it walks the
/// system's processes, which no lock of the table is held for.
#[derive(Clone)]
struct Tree {
    #[cfg(target_os = "macos")]
    tree: crate::process_tree::ProcessTree,
    #[cfg(windows)]
    job: Arc<crate::job_object::JobObject>,
}

impl Tree {
    /// [`Tree::members`], where every process the window's program and each of them
    /// started can be read: none where one cannot, or where the platform cannot say
    /// (`ProcessTree::readable_members`; a Windows job is asked of no commands).
    fn readable_members(&self) -> io::Result<Option<Vec<Member>>> {
        #[cfg(target_os = "macos")]
        {
            self.tree.readable_members()
        }
        #[cfg(not(target_os = "macos"))]
        {
            Ok(None)
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

    /// Ends the turn's work that `baseline` proves, and still is [`SETTLE`] after it was
    /// first found, the rest of the tree staying as it is: where there is anyone to ask
    /// they are asked to end and given `grace` to, and what is left after is ended by
    /// force. What it ends is chosen once, at the second look, and is those processes
    /// and what they start meanwhile: the window's new work, which a human typed in or
    /// a message began while the grace went, is none of it. It says what it ended and
    /// what it could not, told by looking at what it chose when it was done.
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
    fn end(&self, baseline: &Baseline, grace: Duration) -> io::Result<Ending> {
        #[cfg(target_os = "macos")]
        {
            let lineage = self.tree.lineage(baseline, SETTLE)?;
            let asked = self.tree.ask_to_exit(Subject::Lineage(&lineage), None)?;
            close::wait_until_gone(Instant::now() + grace, || !asked.any_alive());
            Ok(self.tree.finish(&lineage, asked))
        }
        #[cfg(windows)]
        {
            self.job.end(baseline, SETTLE)
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            Ok(Ending::default())
        }
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

    /// What the pane's tree has running that is the turn's work, as it was marked.
    /// A pane never marked has none.
    pub fn background(&self, key: &PaneKey) -> Result<Background, PaneError> {
        let (tree, baseline) = {
            let panes = self.lock_panes()?;
            let pane = panes
                .get(key)
                .ok_or_else(|| PaneError::NotFound(key.clone()))?;
            (pane.tree(), pane.baseline.clone())
        };
        Ok(looked_at(&baseline, &tree)?)
    }

    /// The commands of the window's harness that run in the pane's tree now
    /// ([`Baseline::commands`]): none where the host cannot tell them.
    pub fn commands(
        &self,
        key: &PaneKey,
        since: Option<u64>,
    ) -> Result<Option<Background>, PaneError> {
        let (tree, baseline) = {
            let panes = self.lock_panes()?;
            let pane = panes
                .get(key)
                .ok_or_else(|| PaneError::NotFound(key.clone()))?;
            (pane.tree(), pane.baseline.clone())
        };
        // Where it cannot tell them the tree is not walked.
        if !baseline.tells_commands() {
            return Ok(None);
        }
        Ok(tree
            .readable_members()?
            .and_then(|members| baseline.commands(&members, since)))
    }

    /// The pane is at rest, the daemon says: its tree now is kept, under the number
    /// answered, for a look at its commands to leave out ([`Baseline::rest`]). None where
    /// the host tells no commands of it, and its tree is not walked then.
    pub fn rest(&self, key: &PaneKey) -> Result<Option<u64>, PaneError> {
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
            .rest(&members))
    }

    /// Ends what the pane's turn left running, and nothing of the pane's own, the
    /// window staying open: all of what the host tells for the turn's work with no
    /// doubt ([`Baseline::turn_work`]), at two looks [`SETTLE`] apart, and what it
    /// started. They are asked to end and given the table's grace, so this waits as
    /// long as a close would, and the settle more. What it ended is said, and so is
    /// what it chose and could not end. A pane never marked has none.
    pub fn end_background(&self, key: &PaneKey) -> Result<Ending, PaneError> {
        let (tree, baseline) = {
            let panes = self.lock_panes()?;
            let pane = panes
                .get(key)
                .ok_or_else(|| PaneError::NotFound(key.clone()))?;
            (pane.tree(), pane.baseline.clone())
        };
        if !baseline.is_marked() {
            return Ok(Ending::default());
        }
        Ok(tree.end(&baseline, self.close_grace)?)
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
mod ending_tests;
#[cfg(test)]
mod second_look_tests;
#[cfg(test)]
mod support;
#[cfg(test)]
mod tests;

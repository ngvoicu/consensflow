//! Where a pane's turn began, and what it started and left running (see
//! [`crate::background`]). The daemon says a turn is about to begin
//! (`pane.mark`: [`PaneTable::mark`]), and how its harness keeps its servers, and
//! asks, when the turn ends, what the pane's tree has running that was not there
//! then ([`PaneTable::background`]), and to end it ([`PaneTable::end_background`]),
//! the window staying open. A close lists the same for the exit it sends: what
//! is about to be ended is what the window left running ([`Pane::keep_background`]).
//! The window's group is the pane's own process group (`Pane::process_group_id`),
//! which the close signals whole and an ending never does.
//!
//! An ending asks what it ends to end as a close does (TERM, then HUP, on Unix) and
//! gives it the table's grace to do so, and what is left after is ended by force:
//! the same asking, the same grace and the same ending of what is left as a close
//! makes of a tree ([`close`]), of the processes it names and what descends from
//! them, where a close makes it of the whole tree and of the group. On Windows
//! they are ended at once: a job object ends all of its tree or none of it, and
//! each process an ending names is ended by itself.

use std::collections::HashSet;
use std::io;
#[cfg(windows)]
use std::sync::Arc;
use std::time::Duration;
#[cfg(target_os = "macos")]
use std::time::Instant;

use cf_proto::panes::{Background, BackgroundProcess, Ending, Servers};

#[cfg(target_os = "macos")]
use super::close;
use super::{Pane, PaneError, PaneKey, PaneTable};
use crate::background::{report, Baseline, Kept, Member, ProcessKey};
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

    /// Ends the processes `keys` name and what descends from them, the rest of the
    /// tree staying as it is: where there is anyone to ask they are asked to end
    /// and given `grace` to, and what is left after is ended by force. What could
    /// not be ended is for the caller to find out by looking again.
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
    fn end(&self, keys: &[ProcessKey], grace: Duration) {
        #[cfg(target_os = "macos")]
        {
            let subject = Subject::Under(keys);
            if let Ok(asked) = self.tree.ask_to_exit(subject, None) {
                close::wait_until_gone(Instant::now() + grace, || !asked.any_alive());
            }
            let _ = self.tree.terminate(subject);
        }
        #[cfg(windows)]
        {
            let _ = self.job.end(keys);
        }
    }
}

/// What `tree` has running that began after the turn `baseline` was marked at
/// did. A pane never marked has none, and its tree is not looked at.
fn looked_at(baseline: &Baseline, tree: &Tree) -> io::Result<Background> {
    if !baseline.is_marked() {
        return Ok(Background::default());
    }
    Ok(baseline.left_running(tree.members()?))
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

    /// What the tree has running that began after the turn did. A tree that
    /// cannot be looked at has none, and nothing is made up for it.
    fn left_running(&self) -> Background {
        looked_at(&self.baseline, &self.tree()).unwrap_or_default()
    }

    /// What this pane's close is about to end, kept for the exit it will send.
    pub(super) fn keep_background(&self) {
        self.ended.keep_background(self.left_running().processes);
    }
}

impl PaneTable {
    /// A turn begins in the pane: every process its tree has now came before
    /// it, and is not the turn's. Where `servers` says how the pane's harness
    /// keeps its own, what they are, and what they start, is not the turn's
    /// either, whenever it began ([`crate::background`]).
    pub fn mark(&self, key: &PaneKey, servers: Servers) -> Result<(), PaneError> {
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
            .mark(&members, Kept::of(servers, group));
        Ok(())
    }

    /// What the pane's tree has running that began after its last mark. A pane
    /// never marked has none.
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

    /// Ends what the pane's turn left running, and nothing of the pane's own, the
    /// window staying open: of the processes `asked` names, those the host tells
    /// for the turn's with no doubt, and what descends from them
    /// ([`Baseline::ending`]). They are asked to end and given the table's grace,
    /// so this waits as long as a close would. What it ended is said, and so is
    /// what it was asked to end and leaves running, or could not end. A pane never
    /// marked has none.
    pub fn end_background(&self, key: &PaneKey, asked: &[u32]) -> Result<Ending, PaneError> {
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
        let plan = baseline.ending(&tree.members()?, asked);
        if plan.end.is_empty() {
            // Nothing is to end: what it left running, if anything, runs as it did.
            return Ok(report(plan, &HashSet::new()));
        }
        let keys: Vec<ProcessKey> = plan.end.iter().map(|member| member.key).collect();
        tree.end(&keys, self.close_grace);
        // What of it is there now is what it could not end.
        let alive: HashSet<ProcessKey> = tree.members()?.into_iter().map(|m| m.key).collect();
        Ok(report(plan, &alive))
    }

    /// What the window left running when its program exited, for the exit that
    /// says so: what its close kept if a close ended it, else what its tree
    /// has now, kept so that the close that follows does not look again.
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
        let left = looked_at(&baseline, &tree).unwrap_or_default().processes;
        slot.keep_background(left);
        slot.background().unwrap_or_default()
    }
}

#[cfg(test)]
mod ending_tests;
#[cfg(test)]
mod tests;

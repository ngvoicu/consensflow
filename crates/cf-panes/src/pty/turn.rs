//! Where a pane's turn began, and what it started and left running (see
//! [`crate::background`]). The daemon says a turn is about to begin
//! (`pane.mark`: [`PaneTable::mark`]) and asks, when the turn ends, what the
//! pane's tree has running that was not there then ([`PaneTable::background`]).
//! A close lists the same for the exit it sends: what is about to be ended is
//! what the window left running ([`Pane::keep_background`]).

use std::io;
#[cfg(windows)]
use std::sync::Arc;

use cf_proto::panes::{Background, BackgroundProcess};

use super::{Pane, PaneError, PaneKey, PaneTable};
use crate::background::{Baseline, Member};
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
    /// it, and is not the turn's.
    pub fn mark(&self, key: &PaneKey) -> Result<(), PaneError> {
        let tree = self
            .lock_panes()?
            .get(key)
            .ok_or_else(|| PaneError::NotFound(key.clone()))?
            .tree();
        let members = tree.members()?;
        self.lock_panes()?
            .get_mut(key)
            .ok_or_else(|| PaneError::NotFound(key.clone()))?
            .baseline
            .mark(&members);
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
mod tests;

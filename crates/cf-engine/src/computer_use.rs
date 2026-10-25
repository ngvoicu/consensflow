//! Claude Code's computer use, as a launch meets it (T-540, the owner's decision of 2026-10-25).
//!
//! The human turns it on for a project's repository in Claude Code itself (`/mcp`),
//! and Claude Code's computer use then acts through ConsensFlow's own macOS
//! permissions, which only the human can give. A Claude window that starts in a
//! repository where it is on, while a permission is missing, gets the human one
//! note: once for each missing state of each project, not at every launch; a state
//! the human mends and loses again is said again.

use std::cell::RefCell;
use std::collections::HashSet;

use cf_ledger::{NewNote, ParticipantView, ProjectView};

use crate::dispatcher::Dispatcher;
use crate::seams::EngineError;
use crate::seams::ScreenPermissions;

/// The harness a note is for.
const CLAUDE: &str = "claude-code";

/// How the note opens: the page knows it by these words, and gives it the
/// buttons to System Settings (`app/ui/core/board.js`, `computerUseNote`).
pub const NOTE_OPENING: &str = "Claude Code's computer use is on for this project, but ConsensFlow";

/// What the engine keeps across launches: the missing states each project was
/// told of already.
#[derive(Debug, Default)]
pub(crate) struct ComputerUseState {
    told: RefCell<HashSet<(i64, bool, bool)>>,
}

/// The note for `missing`, in plain words.
pub fn note_for(missing: ScreenPermissions) -> String {
    let needs = match (missing.accessibility, missing.screen_recording) {
        (true, true) => "Accessibility and Screen & System Audio Recording",
        (true, false) => "Accessibility",
        _ => "Screen & System Audio Recording",
    };
    format!(
        "{NOTE_OPENING} can't control the screen until you allow it {needs} in System Settings → \
         Privacy & Security."
    )
}

impl Dispatcher {
    /// Tells the human once when a Claude window of `participant` starts where
    /// Claude Code's computer use is on and ConsensFlow lacks a permission it needs.
    pub(crate) fn tell_computer_use_once(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
    ) -> Result<(), EngineError> {
        if participant.harness.as_deref() != Some(CLAUDE) {
            return Ok(());
        }
        let Some(granted) = self.seams.computer_use.permissions() else {
            return Ok(());
        };
        if granted.all() {
            // Mended: a state lost again is said again.
            self.computer_use
                .told
                .borrow_mut()
                .retain(|(id, ..)| *id != project.id);
            return Ok(());
        }
        if !self.seams.computer_use.claude(&project.directory).on {
            return Ok(());
        }
        let missing = ScreenPermissions {
            accessibility: !granted.accessibility,
            screen_recording: !granted.screen_recording,
        };
        if !self.computer_use.told.borrow_mut().insert((
            project.id,
            missing.accessibility,
            missing.screen_recording,
        )) {
            return Ok(());
        }
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: "human".to_owned(),
                task: None,
                body: note_for(missing),
            },
        )?;
        self.changed();
        Ok(())
    }
}

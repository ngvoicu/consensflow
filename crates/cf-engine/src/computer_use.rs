//! Claude Code's computer use, as a launch meets it (T-540, the owner's decision of 2026-10-25).
//!
//! The human turns it on for a project's repository in Claude Code itself (`/mcp`),
//! and Claude Code's computer use then acts through ConsensFlow's own macOS
//! permissions, which only the human can give. A Claude window that starts in a
//! repository where it is on, while a permission is missing, gets the human one
//! note: once for each missing state of each project, not at every launch nor at
//! every start of the app (the project's last note of it, in the ledger, says what
//! was said); a state the human mends and loses again while the app runs is said
//! again. A mend and a loss both made while the app is closed go unseen, and the
//! state, the same as the last note's, is not said again.

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
/// told of already, and the projects seen mended since the app started (whose last
/// note says what is no longer so).
#[derive(Debug, Default)]
pub(crate) struct ComputerUseState {
    told: RefCell<HashSet<(i64, bool, bool)>>,
    mended: RefCell<HashSet<i64>>,
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
            self.computer_use.mended.borrow_mut().insert(project.id);
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
        let body = note_for(missing);
        if !self.computer_use.mended.borrow().contains(&project.id)
            && self.last_said(project)?.as_deref() == Some(body.as_str())
        {
            // Said before the app last started, and nothing mended since.
            return Ok(());
        }
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: "human".to_owned(),
                task: None,
                body,
            },
        )?;
        self.changed();
        Ok(())
    }

    /// What the project's last note of computer use said to the human: ConsensFlow's
    /// own note (no sender), none where there is none.
    fn last_said(&self, project: &ProjectView) -> Result<Option<String>, EngineError> {
        let Some(human) = project.participants.iter().find(|p| p.role == "human") else {
            return Ok(None);
        };
        Ok(self
            .seams
            .ledger
            .borrow()
            .newest_own_note(human.id, NOTE_OPENING)?
            .map(|note| note.body))
    }
}

//! What a window that did not come up says of itself. It said why on its own
//! screen (Pi without a login: "No API key found … Use /login"), and the
//! screen goes with the window, so the engine asks the pane host for it before
//! it closes a window itself, and takes it from the host's `pane.exit` when
//! the window closed first. The words go into the failure the requester and
//! the human are told ([`Shown::after`]), and one line of the daemon's log.

use cf_harness::contract::Pane;
use cf_ledger::{NewNote, ProjectView};
use cf_proto::panes::{SnapshotRequest, TAIL_LINES};

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::runtime::returning;
use crate::seams::EngineError;
use crate::shown::Shown;

impl Dispatcher {
    /// What the window in `pane` shows now, and not how it ended. A host that
    /// cannot answer (the window is gone already, the host does not know the
    /// request) says nothing.
    async fn look_at(&self, pane: &Pane) -> Shown {
        let request = SnapshotRequest {
            id: pane.id.clone(),
            generation: pane.generation,
            tail: Some(TAIL_LINES),
        };
        let Ok(body) = serde_json::to_value(&request) else {
            return Shown::default();
        };
        // Its answer is taken a turn after the host's, as the engine's other
        // snapshot requests' are (`drawn`).
        match returning(self.seams.host.request("pane.snapshot", body)).await {
            Ok(answer) => Shown::from_snapshot(&answer),
            Err(_) => Shown::default(),
        }
    }

    /// A launch about to be given up on, and its window closed. What the
    /// window shows is asked first, for the screen goes with it, and the
    /// delivery the launch was for is taken out of the record. Says what the
    /// window showed, and whether the delivery was there to take: one is not
    /// where an exit came while the host answered, which settled it and said
    /// what it knew.
    pub(crate) async fn look_before_giving_up(
        &self,
        record: &Record,
        pane: Option<&Pane>,
    ) -> (Shown, bool) {
        let shown = match pane {
            Some(pane) => self.look_at(pane).await,
            None => Shown::default(),
        };
        let settles = record.delivery.borrow_mut().delivering.take().is_some();
        (shown, settles)
    }

    /// `because`, with what the window showed said after it, for the failure
    /// of a window that did not come up. Where the host said anything, one
    /// line of the daemon's log says the same.
    pub(crate) fn unstarted_because(
        &self,
        record: &Record,
        because: &str,
        shown: &Shown,
    ) -> String {
        let said = shown.after(because);
        if shown.is_known() {
            if let Some(window) = self.window_name(record) {
                self.seams
                    .log
                    .warn(&format!("the launch of {window} failed: {said}"));
            }
        }
        said
    }

    /// The name a window's pane has, which the log knows it by: the project's
    /// and the participant's. None for a participant the ledger forgot.
    fn window_name(&self, record: &Record) -> Option<String> {
        let project = self.project_of(record.id).ok().flatten()?;
        let participant = project.participants.iter().find(|p| p.id == record.id)?;
        Some(format!("p{}-{}", project.id, participant.handle))
    }

    /// The chief's window closed before its first message showed, and the
    /// project closes with it as it does when the human closes the chief:
    /// the human is told why, where the host said.
    pub(crate) fn tell_chief_closed(
        &self,
        project: &ProjectView,
        said: &str,
    ) -> Result<(), EngineError> {
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: "human".to_owned(),
                task: None,
                body: format!(
                    "{said}. The project is closed: resume it to try again; what comes for the chief waits for it."
                ),
            },
        )?;
        Ok(())
    }
}

//! What a window that did not come up says of itself. It said why on its own
//! screen (Pi without a login: "No API key found … Use /login"), and the
//! screen goes with the window, so the engine asks the pane host for it before
//! it closes a window itself, and takes it from the host's `pane.exit` when
//! the window closed first. The words go into the failure the requester and
//! the human are told ([`Shown::after`]), and one line of the daemon's log;
//! where they name a secret (Pi's do: "No API key found"), the failure says
//! that its lines were left out, and quotes none of them.
//!
//! A window can come up and be no window to work in, and say so when it is asked
//! whether it may take a delivery (`Readiness::Unfit`: a Devin left in its normal
//! permission mode, where every call would wait for a permission nobody gives).
//! Its launch is given up as one that did not come up ([`Dispatcher::give_up_unfit`]),
//! on the board, and never left to wait: a window that says so is asked before it has
//! drawn too, where its harness bounds its readiness whether or not it draws
//! ([`Dispatcher::give_up_unfit_drawing`]).

use std::rc::Rc;

use cf_harness::contract::{Pane, PaneHost, Readiness};
use cf_ledger::{MessageView, NewNote, ProjectView};
use cf_proto::panes::{SnapshotRequest, TAIL_LINES};

use crate::deliveries::Delivering;
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::role_change::Prelude;
use crate::runtime::{begin, returning};
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

    /// A launch that was to say the role first, about to be given up on, and its
    /// window closed. What the window shows is asked first, for the screen goes
    /// with it, and what the launch owed is taken out of the record. Says what the
    /// window showed, and what the launch owed: none where an exit came while the
    /// host answered, which settled it and said what it knew.
    pub(crate) async fn look_before_abandoning(
        &self,
        record: &Record,
        pane: Option<&Pane>,
    ) -> (Shown, Option<Prelude>) {
        let shown = match pane {
            Some(pane) => self.look_at(pane).await,
            None => Shown::default(),
        };
        let owed = record.delivery.borrow_mut().prelude.take();
        (shown, owed)
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

    /// A window that says it is unfit for work, and will not be fit, as `message`
    /// was to go into it (`why`, its own words): its launch did not come up, and is
    /// given up as one that did not. What the window shows is asked first, for the
    /// screen goes with it, and the window is closed. A chief's goes without its
    /// project, and the chief is tried again at the backoff, the human told once
    /// ([`Dispatcher::launch_failed`]); a chief's window the host would not kill is
    /// looked at again at the next pass, and nothing goes in meanwhile. A member's
    /// goes with the message it was to be given, which fails as a launch's first
    /// message does, so that whoever gave it hears why and no launch follows at the
    /// next pass. A launch that was to say the role first, and has not, is given up
    /// as one that cannot ([`Dispatcher::abandon_launch`]): what fails is the message
    /// the role went ahead of, and not the note that says the role, which goes with
    /// the launch (a member's task stayed queued, and was launched for again at
    /// every pass: Astra on the B1 fix).
    pub(crate) async fn give_up_unfit(
        self: &Rc<Self>,
        record: &Rc<Record>,
        message: &MessageView,
        why: &str,
    ) -> Result<(), EngineError> {
        let Some(project) = self.project_of(record.id)? else {
            return Ok(());
        };
        let Some(participant) = project
            .participants
            .iter()
            .find(|p| p.id == record.id)
            .cloned()
        else {
            return Ok(());
        };
        if self.owed(record)?.is_some() {
            return self
                .abandon_launch(&project, &participant, record, why)
                .await;
        }
        let pane = record.window.borrow().pane.clone();
        let (shown, _) = self.look_before_giving_up(record, pane.as_ref()).await;
        let said = self.unstarted_because(record, why, &shown);
        let failing = Delivering {
            message: message.id,
            since: self.now(),
            rested_since: None,
            launch: true,
            chief: participant.role == "chief",
            queued: false,
            free_since: None,
            entered_again: false,
        };
        if participant.role == "chief" {
            let closed = match &pane {
                Some(pane) => self.close_own(record, pane).await?,
                None => true,
            };
            if !closed {
                return Ok(());
            }
            return self.launch_failed(record, &project, &participant, Some(failing), &said);
        }
        let (this, held) = (Rc::clone(self), Rc::clone(record));
        let closing = begin(&*self.seams.spawn, async move { this.retire(&held).await }).await;
        self.launch_failed(record, &project, &participant, Some(failing), &said)?;
        closing.await?;
        Ok(())
    }

    /// Whether `record`'s window is asked whether it is ready before it has drawn
    /// its screen ([`Window::ready_while_drawing`]).
    ///
    /// [`Window::ready_while_drawing`]: cf_harness::contract::Window::ready_while_drawing
    pub(crate) fn ready_while_drawing(&self, record: &Record) -> bool {
        self.window_of(record)
            .0
            .is_some_and(|window| window.ready_while_drawing())
    }

    /// A window that has not drawn its screen, with `message` next for it, asked
    /// whether it may take it: nothing goes in, and one that says it never will be fit
    /// is given up as [`Dispatcher::give_up_unfit`] gives one up. Asked only once it
    /// had drawn, a Devin left in its normal mode that never drew was never asked, and
    /// its bound never ran (Astra on the B1 fix). Any other answer waits for the look
    /// that finds it drawn.
    pub(crate) async fn give_up_unfit_drawing(
        self: &Rc<Self>,
        record: &Rc<Record>,
        message: MessageView,
    ) -> Result<(), EngineError> {
        let host: &dyn PaneHost = &*self.seams.host;
        let (Some(window), Some(pane)) = self.window_of(record) else {
            return Ok(());
        };
        let ready = window
            .ready(host, &pane)
            .await
            .map_err(|error| EngineError::said("not-ready", error))?;
        if self.forgotten(record) {
            return Ok(());
        }
        // What waits for it may have gone meanwhile, as for a delivery.
        let next = self.next_in_line(record)?;
        match ready {
            Readiness::Unfit(why) if next.is_some_and(|next| next.id == message.id) => {
                self.give_up_unfit(record, &message, &why).await
            }
            _ => Ok(()),
        }
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

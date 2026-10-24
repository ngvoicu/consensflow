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
use cf_ledger::{MessageView, NewNote, ParticipantView, ProjectView};
use cf_proto::panes::{SnapshotRequest, TAIL_LINES};

use crate::deliveries::Delivering;
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::role_change::Prelude;
use crate::runtime::{begin, returning};
use crate::seams::EngineError;
use crate::shown::Shown;

/// A launch whose window did not come up in the time it had, about to be given
/// up: its pane, the message the launch fails with where it has one, why its
/// harness says it did not come up, and what the window showed.
pub(crate) struct GivenUp {
    pub(crate) pane: Pane,
    pub(crate) failing: Option<Delivering>,
    pub(crate) error: String,
    pub(crate) shown: Shown,
}

impl Dispatcher {
    /// What the window in `pane` shows now, and not how it ended. A host that
    /// cannot answer (the window is gone already, the host does not know the
    /// request) says nothing.
    pub(crate) async fn look_at(&self, pane: &Pane) -> Shown {
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
    /// looked at again at the next pass, and nothing goes in meanwhile. A member's exit
    /// is settled at the close, which pauses its task, and then the message it was to
    /// be given goes, failing as a launch's first message does, so that whoever gave it
    /// hears why and no launch follows at the next pass; words told to the window about
    /// the task are kept for its resume instead, the requester and the human told why.
    /// A member's window the host would not kill has no exit to pause its task: its
    /// message goes all the same, and fails. A launch that was to say the role first, and has not, is given up
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
            // A launch's first message is never pressed for.
            carried: Vec::new(),
            owed: None,
            late: false,
        };
        // The window's exit is settled at its close, before what it was to be given:
        // the host answers a kill before the exit comes, so a member's task paused by
        // the exit only once the message had failed it (the merge's review, T-16 F1).
        let closed = match &pane {
            Some(pane) => self.close_own(record, pane).await?,
            None => true,
        };
        // A chief's window the host would not kill is looked at again; a member's
        // message goes all the same, its task never paused by an exit that never came
        // (T-47 P3-1: left at work, asked and killed again at every pass, nobody told).
        if !closed && participant.role == "chief" {
            return Ok(());
        }
        self.launch_failed(record, &project, &participant, Some(failing), &said)
    }

    /// A launch whose window did not come up in the time it had, its harness asked
    /// once more after its screen was: the window is closed and the launch settled as
    /// one that did not come up ([`Dispatcher::launch_failed`]), its participant's next
    /// window given longer (`starts_given_up`). One whose exit came meanwhile went as
    /// any window goes, and its exit settled it: it is not given up again, whether or
    /// not it had a first message (Astra on round 1 of the chief's restart: a chief's
    /// was, and the human told it is tried again after its exit had closed the project).
    /// A chief's first message that the window's record shows it took went in, as at any
    /// window's end ([`Dispatcher::settle_gone`]), and is confirmed, never given again
    /// (Sol and Astra on round 1: one Codex recorded as its window was given up was
    /// given back, and sent again on the same thread). The window may have begun a turn
    /// as it was closed, and no look can tell before the close: the chief opens again at
    /// the backoff whatever waits for it, told the turn was cut where its record holds one
    /// (Sol and Astra on round 2: closed with nothing left queued, it stayed closed).
    pub(crate) async fn give_up_unstarted(
        self: &Rc<Self>,
        record: &Rc<Record>,
        project: &ProjectView,
        participant: &ParticipantView,
        given_up: GivenUp,
    ) -> Result<(), EngineError> {
        let GivenUp {
            pane,
            failing,
            error,
            shown,
        } = given_up;
        if record.window.borrow().pane.as_ref() != Some(&pane) {
            return Ok(());
        }
        let first = failing.is_some();
        // A member's message that the role went ahead of is no record's: it was given back.
        let settles = record.delivery.borrow_mut().delivering.take().is_some();
        record.window.borrow_mut().starts_given_up += 1;
        // The chief's window goes without taking its project with it: the chief is tried again.
        if participant.role == "chief" {
            self.close_own(record, &pane).await?;
        } else {
            let this = Rc::clone(self);
            drop(
                begin(&*self.seams.spawn, async move {
                    let _ = this.seams.host.kill(&pane).await;
                })
                .await,
            );
        }
        let because = if first {
            format!("the window could not take its first message: {error}")
        } else {
            format!("the window never came up: {error}")
        };
        let said = if settles || !first {
            self.unstarted_because(record, &because, &shown)
        } else {
            shown.after(&because)
        };
        let failing = match failing {
            Some(delivering) if delivering.chief => {
                let mut back = None;
                self.settle_gone(delivering, |delivering| {
                    back = Some(delivering);
                    Ok(())
                })
                .await?;
                back
            }
            failing => failing,
        };
        self.launch_failed(record, project, participant, failing, &said)?;
        // The chief opens again at its time whatever waits for it, told its turn was cut
        // where the window had begun one (`Dispatcher::step`).
        if let Some(relaunch) = record.window.borrow_mut().relaunch.as_mut() {
            relaunch.reopens = true;
        }
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

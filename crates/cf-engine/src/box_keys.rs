//! The engine's acts on a window's input box, and the one observation each is licensed by (N17).
//!
//! A delivery decides what it does to the box (paste into it, press an Enter that sends what it
//! holds) on one look at the window ([`cf_harness::Look`]): the pane host's snapshot of its
//! screen's last lines, read whole and current. **Another snapshot may say wait; only a look says
//! go.** The look's `seen` goes with the act and the host checks it again where it writes, with
//! the window's writer held, so a box that changed between the look and the write is refused
//! there and not written to.
//!
//! This is the one place a delivery's Enter goes out ([`Dispatcher::enter`]). `pane.input`'s
//! callers are a dialog's answer, a trust question's and an interrupt's: a test holds the list.

use std::rc::Rc;

use cf_harness::contract::{Held, Input, Pane, Readiness, Window};
use cf_harness::{licence_paste, BoxState, Look, NoLook, PasteLicence, Sent, NO_LOOK};
use serde_json::Value;

use crate::arrival::SLOW_READ_MS;
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::runtime::returning;
use crate::seams::EngineError;
use crate::windows::pane_body_drawn;

/// What a delivery waits for where it has no look.
pub(crate) fn no_look_wait(no_look: NoLook) -> String {
    no_look.words().to_owned()
}

impl Dispatcher {
    /// What `look` licenses of a paste into `pane`'s window: the human's unsent text, a key of
    /// theirs too young to be drawn, a box that holds words and a stale paste the screen cannot
    /// show hold it ([`licence_paste`]); a paste of ours left in the box (one whose Enter was
    /// withheld, one an interrupt put back) is emptied first, by the key that goes with the paste.
    pub(crate) fn licence_of(
        &self,
        record: &Record,
        pane: &Pane,
        window: &Rc<dyn Window>,
        look: &Look,
    ) -> PasteLicence {
        let Input::Pasted { clear } = record.window.borrow().input else {
            return PasteLicence::Wait(Held::Because(
                "its window takes its messages through a channel".to_owned(),
            ));
        };
        let uncleared = record
            .delivery
            .borrow()
            .uncleared
            .as_ref()
            .is_some_and(|stale| &stale.pane == pane);
        let state = BoxState {
            stale_paste: uncleared || window.stale_paste(),
        };
        licence_paste(look, window.harness(), state, clear)
    }

    /// Whether the window of `record` in `pane` may take a delivery now, on one look at it where it
    /// takes pastes: the adapter's readiness of that look, and what the look licenses of a
    /// paste ([`Dispatcher::licence_of`]). No look is a wait for one, never a go; a window with a
    /// channel of its own is given none. The look is kept for the attempt's licence
    /// ([`crate::deliveries::DeliveryPart::looked`]).
    pub(crate) async fn readiness_on_a_look(
        &self,
        record: &Record,
        window: &Rc<dyn Window>,
        pane: &Pane,
    ) -> Result<Readiness, EngineError> {
        let host: &dyn cf_harness::contract::PaneHost = &*self.seams.host;
        let pasted = matches!(record.window.borrow().input, Input::Pasted { .. });
        let looked = if pasted {
            Some(self.look_quietly(pane).await)
        } else {
            None
        };
        let look = looked.as_ref().and_then(|looked| looked.as_ref().ok());
        record.delivery.borrow_mut().looked = look.cloned();
        let ready = window
            .ready_on(host, pane, look)
            .await
            .map_err(|error| EngineError::said("not-ready", error))?;
        match (ready, &looked) {
            // What the host said of why there is no look is the wait's words.
            (Readiness::Held(Held::Because(why)), Some(Err(no_look))) if why == NO_LOOK => {
                Ok(Readiness::Held(Held::Because(no_look_wait(*no_look))))
            }
            (Readiness::Ready, Some(Ok(look))) => Ok(match self.licence_of(record, pane, window, look)
            {
                PasteLicence::Wait(held) => Readiness::Held(held),
                PasteLicence::Go { .. } => Readiness::Ready,
            }),
            (ready, _) => Ok(ready),
        }
    }

    /// The licence of the look the attempt kept, for the paste of a launch's first message;
    /// none for a window with a channel.
    pub(crate) fn first_licence(
        &self,
        record: &Record,
        window: &Rc<dyn Window>,
        pane: &Pane,
    ) -> Option<PasteLicence> {
        let look = record.delivery.borrow().looked.clone()?;
        Some(self.licence_of(record, pane, window, &look))
    }

    /// A look at `pane` through a request the Node traces hold (a `pane.snapshot` that asks for
    /// the lines): where the old engine asked the host for a snapshot at the point of decision,
    /// that request is the look.
    pub(crate) async fn look_recorded(&self, pane: &Pane) -> Result<Look, NoLook> {
        let asked = self.now();
        let answer: Result<Value, _> =
            returning(self.seams.host.request("pane.snapshot", pane_body_drawn(pane))).await;
        match answer {
            // An answer that came after a stall says what the window did, not what it does.
            Ok(_) if self.now() - asked > SLOW_READ_MS => Err(NoLook::NoAnswer),
            Ok(answer) => Look::of(&answer),
            Err(_) => Err(NoLook::NoAnswer),
        }
    }

    /// A look at `pane` through the engine's own seam ([`crate::host::EngineHost::look`]), which
    /// the Node traces hold no call of: where the old engine asked the host nothing.
    pub(crate) async fn look_quietly(&self, pane: &Pane) -> Result<Look, NoLook> {
        let asked = self.now();
        let looked = returning(self.seams.host.look(pane)).await;
        // A look that came after a stall says what the window did, not what it does.
        match looked {
            Ok(_) if self.now() - asked > SLOW_READ_MS => Err(NoLook::NoAnswer),
            looked => looked,
        }
    }

    /// An Enter that sends what waits in `pane`'s input box, on the look that licensed it: the
    /// host writes it only if the box on its screen is still the look's, nothing of the output
    /// it read is undrawn, and the human has typed nothing since. A refusal wrote nothing.
    pub(crate) async fn enter(&self, pane: &Pane, at: &cf_harness::At) -> Sent {
        returning(cf_harness::enter(&*self.seams.host, pane, at)).await
    }
}

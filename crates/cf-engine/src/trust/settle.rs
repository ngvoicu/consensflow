//! What is left to do once the watch of a window is over: the human's yes banked, the
//! notes forgotten that are the window's alone, the servers told of, the window let go.

use cf_harness::contract::ServersChosen;

use crate::dispatcher::Dispatcher;
use crate::seams::EngineError;

use super::watch::Watch;
use super::{Ended, Trusting};

impl Dispatcher {
    /// The watch of a window that is past the question, or no longer the record's, ends.
    pub(super) fn settle_trust(&self, watch: Watch<'_>, ended: Ended) -> Result<(), EngineError> {
        let Watch {
            record,
            project,
            participant,
            pane,
            window,
            folder,
            chosen,
            into_servers,
            using,
            grants,
            servers,
            ..
        } = watch;
        let asked = record.window.borrow().trust.asks();
        // The question left a window now at its prompt, its choices gone from the
        // screen, and no key of ConsensFlow's answered it: the human said yes. Nothing
        // else says so: not the question's absence, which a No leaves on the way out,
        // nor the record's growth, which may be a process before the window's, nor a
        // prompt below a conversation that quoted the choices. Only the last lines are
        // read: a quote of the dialog, its cursor and all, that more of the conversation
        // pushes out of them before the prompt is drawn still banks a yes nobody gave.
        let (keys, sure) = {
            let part = record.window.borrow();
            (part.trust_keys, part.trust_sure)
        };
        let Ended { at_prompt, cleared } = ended;
        if asked && keys == 0 && at_prompt && cleared {
            self.trust.said_yes(project.id, &folder, sure);
        }
        // A no to the grants is the window's alone, and the next window asks again; so
        // is a window that went while it asked.
        if grants {
            self.forget_grants_told(project, &folder);
        }
        if servers {
            self.forget_servers_told(project, &folder);
        }
        // A server the window's folder left out while only ConsensFlow pressed into the
        // dialog: a key that answered what it was not pressed for (T-415). No later window
        // asks of it, so the human is told.
        let now = window.servers_chosen();
        let (refused, used) = servers_changed(&chosen, now);
        if into_servers && !servers && !refused.is_empty() {
            self.tell_left_out(project, &folder, participant, &refused)?;
        }
        // The servers ConsensFlow's Enter used, once the window's folder says it used them
        // (the window may be gone by then, its task done): each told of once, in the words
        // the dialog named it in.
        if let (Some(named), true, true) = (using, used, refused.is_empty()) {
            self.tell_used(project, &folder, participant, named)?;
        }
        if !self.shows(record, pane) {
            return Ok(());
        }
        record.window.borrow_mut().trust = Trusting::Past;
        // A first message the question held gets its whole time from now.
        if asked {
            if let Some(delivering) = record.delivery.borrow_mut().delivering.as_mut() {
                delivering.since = self.now();
            }
        }
        self.changed();
        Ok(())
    }
}

/// The servers the window's folder left out since the watch began, and whether it used
/// one it had not.
fn servers_changed(chosen: &ServersChosen, now: ServersChosen) -> (Vec<String>, bool) {
    let refused: Vec<String> = now
        .left_out
        .into_iter()
        .filter(|name| !chosen.left_out.contains(name))
        .collect();
    let used = now.used.iter().any(|name| !chosen.used.contains(name));
    (refused, used)
}

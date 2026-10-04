//! Devin's record of a session (`devinReader`, `hosts/lib/completion/devin.js`):
//! its store, and each launch's wire log. Devin persists revisions, including
//! cancelled assistant text. Only its main chain plus a matching native
//! request/complete boundary proves a reply.
//!
//! A look reads the store (the `store` module) and every launch's wire log
//! (`wires`), each on from where the last look left it. When neither said
//! anything new, the look answers what the last did, the same reading. A look
//! that fails forgets all it read: the next reads everything again.
//!
//! Kept from Node on purpose:
//! - Devin's data folder is under `HOME`, else `USERPROFILE`, when its own
//!   variable is not set: with neither, the look fails with `missing home in
//!   env` (`shared::paths::home`), where Node read the process's own home.
//! - A message whose JSON nests past what serde_json reads, or holds a number
//!   past a double's range, fails the look, which Node read.
//! - Rows below the main chain's head that lead back to themselves fail the
//!   look, where Node followed them for good and never answered.

mod answer;
mod chain;
mod comparable;
mod store;
mod wires;

use std::sync::Arc;

use cf_base::env::Env;

use crate::shared::record::cache::{Look, Options};
use crate::shared::record::reading::Reading;
use store::Store;
use wires::Wires;

/// The reader of the session `session`, in the Devin folders and the
/// ConsensFlow home `env` names.
pub fn reader(session: &str, env: &Env) -> Box<dyn Look + Send> {
    Box::new(Reader {
        session: session.to_owned(),
        env: env.clone(),
        store: None,
        wires: Wires::default(),
        answer: None,
    })
}

/// A session's reader: what its store and its wire logs said so far.
struct Reader {
    session: String,
    env: Env,
    /// The store as the last look left it: none to read it whole.
    store: Option<Store>,
    wires: Wires,
    /// The last look's answer, while nothing it rests on changes.
    answer: Option<Arc<Reading>>,
}

impl Look for Reader {
    /// Takes no options and reads no clock: the store and the logs alone
    /// answer.
    fn look(&mut self, _options: &Options, _now_ms: i64) -> Arc<Reading> {
        self.read().unwrap_or_else(|reason| {
            self.store = None;
            self.wires = Wires::default();
            self.answer = None;
            Arc::new(Reading::unreadable(&reason))
        })
    }
}

impl Reader {
    /// What the session says now, or why it cannot be read.
    fn read(&mut self) -> Result<Arc<Reading>, String> {
        let (stored, store) = Store::read(&mut self.store, &self.session, &self.env)?;
        let wired = self.wires.read(&self.session, &self.env)?;
        if let (false, false, Some(answer)) = (stored, wired, &self.answer) {
            return Ok(Arc::clone(answer));
        }
        let answer = Arc::new(Reading::Known(answer::answer(&store.chain, &self.wires)));
        self.answer = Some(Arc::clone(&answer));
        Ok(answer)
    }
}

#[cfg(test)]
mod tests;

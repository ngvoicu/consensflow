//! The models each harness lists, **read when the page asks** (A4-28, T-1321): a dialog opens
//! and asks, and what the harness offers at that moment is what it shows, whatever an earlier
//! ask read. Nothing is read in the background, and nothing is answered from a read before: a
//! harness that cannot be read lists no model and says why ([`ModelListing::failed`]); the list
//! it offered an hour ago is not put in its place. Reads of different harnesses run side by
//! side, each under the bound its adapter gives it; one harness is read one ask at a time (two
//! probes of one harness have hung), and an ask that waited for the read in flight is answered
//! from it, as it was asked for while that read ran.
//!
//! The ledger keeps the last read of each harness, for a submission to be checked against
//! the list the human was shown; a daemon forgets those rows as it starts ([`crate::start`]).
//!
//! Read at the ask, not as the daemon starts: a daemon a test starts would run every
//! harness its PATH and the system's folders find, and the human who opens no picker
//! waits for nothing.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use cf_base::env::Env;
use cf_engine::seams::Adapters;
use cf_harness::detect::harness_path;
use cf_ledger::Ledger;
use cf_proto::agents::Harness;
use cf_proto::models::{Freshness, ModelListing};
use tokio::sync::Mutex;

/// The harnesses' model lists of this daemon's life.
pub struct ModelLists {
    adapters: Rc<dyn Adapters>,
    env: Env,
    /// One turn for each harness, held through each read of it: two probes of one harness at
    /// once have hung (two Pi's). The harnesses do not wait for one another.
    reading: RefCell<HashMap<Harness, Rc<Mutex<()>>>>,
    /// How many reads of each harness have ended, and what the last one answered: an ask that
    /// began before the last read ended waited for it, and takes its answer.
    ended: RefCell<HashMap<Harness, (u64, ModelListing)>>,
}

impl ModelLists {
    /// The lists of a daemon that has read none.
    pub fn new(adapters: Rc<dyn Adapters>, env: Env) -> Self {
        Self {
            adapters,
            env,
            reading: RefCell::new(HashMap::new()),
            ended: RefCell::new(HashMap::new()),
        }
    }

    /// The listing of `harness`, read from it now (`refresh` is the human's Refresh, which also
    /// has a harness fetch the catalog it keeps), and kept as the last read. A read that fails
    /// is a listing of no model that says why. Fails where the harness is not installed here,
    /// or the ledger fails.
    pub async fn get(
        &self,
        ledger: &RefCell<Ledger>,
        harness: Harness,
        refresh: bool,
    ) -> Result<ModelListing, String> {
        if harness_path(harness, &self.env).is_none() {
            return Err(format!("{} is not installed here", harness.as_str()));
        }
        let asked_after = self.ended_reads(harness);
        let turn = Rc::clone(self.reading.borrow_mut().entry(harness).or_default());
        let _turn = turn.lock().await;
        // A read that ended while this ask waited for the turn is the read it waited for: it was
        // asked for while that read ran. A Refresh reads for itself.
        if !refresh {
            if let Some((ended, listing)) = self.ended.borrow().get(&harness) {
                if *ended > asked_after {
                    return Ok(listing.clone());
                }
            }
        }
        let adapter = self
            .adapters
            .adapter(harness.kind())
            .ok_or_else(|| format!("{} has no adapter", harness.as_str()))?;
        let listing = without_credentials(
            adapter
                .models(refresh)
                .await
                .unwrap_or_else(ModelListing::failed),
        );
        ledger
            .borrow_mut()
            .keep_model_listing(harness.as_str(), &listing)
            .map_err(|failed| failed.to_string())?;
        let ended = self.ended_reads(harness) + 1;
        self.ended
            .borrow_mut()
            .insert(harness, (ended, listing.clone()));
        Ok(listing)
    }

    /// How many reads of `harness` have ended in this daemon's life.
    fn ended_reads(&self, harness: Harness) -> u64 {
        self.ended
            .borrow()
            .get(&harness)
            .map_or(0, |(ended, _)| *ended)
    }
}

/// `listing` with its stale reason as it may be kept and told ([`cf_engine::quotable`]). A harness
/// quotes what its program said when a read fails, and what a program said can carry what it
/// was given: Pi's words (after the mark a failure of Pi's read carries) are screened whole and
/// withheld whole if anything in them may hold a credential, then cut; the reason of any other
/// harness, which has none, is only masked, and says what it said (`Invalid API key · Please run
/// /login`). The reason is kept in the ledger, which the page reads, and told to the human.
fn without_credentials(mut listing: ModelListing) -> ModelListing {
    if let Freshness::Stale { why } = &listing.freshness {
        listing.freshness = Freshness::Stale {
            why: cf_engine::quotable(why),
        };
    }
    listing
}

impl ModelLists {
    /// How the human switches `harness`'s model in its own window ([`Adapter::model_switch`]):
    /// none where it has no adapter, or ConsensFlow has not probed it.
    ///
    /// [`Adapter::model_switch`]: cf_harness::contract::Adapter::model_switch
    pub fn switch_help(&self, harness: Harness) -> Option<&'static str> {
        self.adapters
            .adapter(harness.kind())
            .and_then(|adapter| adapter.model_switch())
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use std::rc::Rc;

    use cf_engine::seams::Adapters;
    use cf_harness::contract::Adapter;

    /// The adapters of a page that reads no model: none.
    pub(crate) struct NoAdapters;

    impl Adapters for NoAdapters {
        fn adapter(&self, _harness: &str) -> Option<Rc<dyn Adapter>> {
            None
        }
    }
}

#[cfg(test)]
mod tests;

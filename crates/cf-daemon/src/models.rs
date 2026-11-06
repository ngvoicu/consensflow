//! The models each harness lists, as the home keeps them (CHF-03): read from the harness
//! itself ([`Adapter::models`]) at the first ask of this daemon's life and again at the
//! human's Refresh, never in the background, one read of a harness at a time; between, what
//! the ledger kept answers, **at once, whatever is being read**: a Pi Refresh takes as long as
//! `pi update --models` and the read take, and a Claude list the home kept does not wait for it
//! (the review T-1270, second reading, F8). A read that fails keeps the listing read before,
//! stale for its reason ([`ModelListing::after`]).
//!
//! Read at the first ask, not as the daemon starts: a daemon a test starts would run every
//! harness its PATH and the system's folders find, and the human who opens no picker
//! waits for nothing.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
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
    /// The harnesses read since the daemon started.
    read: RefCell<HashSet<Harness>>,
    /// One turn for each harness, held through each read of it: two probes of one harness at
    /// once have hung (two Pi's). The harnesses do not wait for one another, and a list the
    /// home kept waits for no turn at all.
    reading: RefCell<HashMap<Harness, Rc<Mutex<()>>>>,
}

impl ModelLists {
    pub fn new(adapters: Rc<dyn Adapters>, env: Env) -> Self {
        Self {
            adapters,
            env,
            read: RefCell::new(HashSet::new()),
            reading: RefCell::new(HashMap::new()),
        }
    }

    /// The listing of `harness`, read from it first where this daemon has not read it yet or
    /// `refresh` asks (the human's Refresh, which also has a harness fetch the catalog it
    /// keeps), and kept. Fails where the harness is not installed here, or the ledger
    /// fails.
    pub async fn get(
        &self,
        ledger: &RefCell<Ledger>,
        harness: Harness,
        refresh: bool,
    ) -> Result<ModelListing, String> {
        if harness_path(harness, &self.env).is_none() {
            return Err(format!("{} is not installed here", harness.as_str()));
        }
        // What the home kept answers a list asked for with no Refresh, with no turn to wait for:
        // a read of this harness, or of another, may be in flight for as long as a Pi update.
        if let Some(kept) = self.cached(ledger, harness, refresh)? {
            return Ok(kept);
        }
        let turn = Rc::clone(self.reading.borrow_mut().entry(harness).or_default());
        let _turn = turn.lock().await;
        // The read this ask waited for may have kept the list it wants.
        if let Some(kept) = self.cached(ledger, harness, refresh)? {
            return Ok(kept);
        }
        let kept = Self::kept(ledger, harness)?;
        let adapter = self
            .adapters
            .adapter(harness.kind())
            .ok_or_else(|| format!("{} has no adapter", harness.as_str()))?;
        let listing = without_credentials(ModelListing::after(kept, adapter.models(refresh).await));
        ledger
            .borrow_mut()
            .keep_model_listing(harness.as_str(), &listing)
            .map_err(|failed| failed.to_string())?;
        self.read.borrow_mut().insert(harness);
        Ok(listing)
    }

    /// What the home kept of `harness`, for an ask with no Refresh that this daemon has read the
    /// harness for already: none where it asks for one, has not read it, or kept nothing.
    fn cached(
        &self,
        ledger: &RefCell<Ledger>,
        harness: Harness,
        refresh: bool,
    ) -> Result<Option<ModelListing>, String> {
        if refresh || !self.read.borrow().contains(&harness) {
            return Ok(None);
        }
        Ok(Self::kept(ledger, harness)?.map(without_credentials))
    }

    /// What the home kept of `harness`.
    fn kept(ledger: &RefCell<Ledger>, harness: Harness) -> Result<Option<ModelListing>, String> {
        ledger
            .borrow()
            .model_listing(harness.as_str())
            .map_err(|failed| failed.to_string())
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

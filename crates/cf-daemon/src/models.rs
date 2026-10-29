//! The models each harness lists, as the home keeps them (CHF-03): read from the harness
//! itself ([`Adapter::models`]) at the first ask of this daemon's life and again at the
//! human's Refresh, never in the background, one harness at a time; between, what the
//! ledger kept answers. A read that fails keeps the listing read before, stale for its
//! reason ([`ModelListing::after`]).
//!
//! Read at the first ask, not as the daemon starts: a daemon a test starts would run every
//! harness its PATH and the system's folders find, and the human who opens no picker
//! waits for nothing.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use cf_base::env::Env;
use cf_engine::seams::Adapters;
use cf_harness::detect::harness_path;
use cf_ledger::Ledger;
use cf_proto::agents::Harness;
use cf_proto::models::ModelListing;
use tokio::sync::Mutex;

/// The harnesses' model lists of this daemon's life.
pub struct ModelLists {
    adapters: Rc<dyn Adapters>,
    env: Env,
    /// The harnesses read since the daemon started.
    read: RefCell<HashSet<Harness>>,
    /// Held through each read: one harness at a time (two Pi probes at once have hung).
    reading: Mutex<()>,
}

impl ModelLists {
    pub fn new(adapters: Rc<dyn Adapters>, env: Env) -> Self {
        Self {
            adapters,
            env,
            read: RefCell::new(HashSet::new()),
            reading: Mutex::new(()),
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
        let _turn = self.reading.lock().await;
        let kept = ledger
            .borrow()
            .model_listing(harness.as_str())
            .map_err(|failed| failed.to_string())?;
        if !refresh && self.read.borrow().contains(&harness) {
            if let Some(kept) = kept {
                return Ok(kept);
            }
        }
        let adapter = self
            .adapters
            .adapter(harness.kind())
            .ok_or_else(|| format!("{} has no adapter", harness.as_str()))?;
        let listing = ModelListing::after(kept, adapter.models(refresh).await);
        ledger
            .borrow_mut()
            .keep_model_listing(harness.as_str(), &listing)
            .map_err(|failed| failed.to_string())?;
        self.read.borrow_mut().insert(harness);
        Ok(listing)
    }
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

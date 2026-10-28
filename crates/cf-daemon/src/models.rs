//! The models each harness lists, as the home keeps them (CHF-03): read from the harness
//! itself ([`Adapter::models`]) at the first ask of this daemon's life and again at the
//! human's Refresh, never in the background, one harness at a time; between, what the
//! ledger kept answers. A read that fails keeps the list read before, marked stale, with
//! the reason ([`ModelList::after`]).
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
use cf_harness::seams::Time;
use cf_ledger::Ledger;
use cf_proto::agents::Harness;
use cf_proto::models::ModelList;
use tokio::sync::Mutex;

/// The harnesses' model lists of this daemon's life.
pub struct ModelLists {
    adapters: Rc<dyn Adapters>,
    env: Env,
    time: Rc<dyn Time>,
    /// The harnesses read since the daemon started.
    read: RefCell<HashSet<Harness>>,
    /// Held through each read: one harness at a time (two Pi probes at once have hung).
    reading: Mutex<()>,
}

impl ModelLists {
    pub fn new(adapters: Rc<dyn Adapters>, env: Env, time: Rc<dyn Time>) -> Self {
        Self {
            adapters,
            env,
            time,
            read: RefCell::new(HashSet::new()),
            reading: Mutex::new(()),
        }
    }

    /// The list of `harness`, read from it first where this daemon has not read it yet or
    /// `refresh` asks (the human's Refresh, which also has a harness fetch the catalog it
    /// keeps), and kept. Fails where the harness is not installed here, or the ledger
    /// fails.
    pub async fn get(
        &self,
        ledger: &RefCell<Ledger>,
        harness: Harness,
        refresh: bool,
    ) -> Result<ModelList, String> {
        if harness_path(harness, &self.env).is_none() {
            return Err(format!("{} is not installed here", harness.as_str()));
        }
        let _turn = self.reading.lock().await;
        let kept = ledger
            .borrow()
            .model_list(harness.as_str())
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
        let at = self.time.wall_ms();
        let read = adapter.models(refresh).await;
        let list = ModelList::after(harness.as_str(), kept, at, read);
        ledger
            .borrow_mut()
            .keep_model_list(&list)
            .map_err(|failed| failed.to_string())?;
        self.read.borrow_mut().insert(harness);
        Ok(list)
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

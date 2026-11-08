//! The models each harness lists, **read when the page asks** (A4-28, T-1321): a dialog opens
//! and asks, and what the harness offers at that moment is what it shows, whatever an earlier
//! ask read. Nothing is read in the background, and nothing is answered from a read before: a
//! harness that cannot be read lists no model and says why ([`ModelListing::failed`]); the list
//! it offered an hour ago is not put in its place. Reads of different harnesses run side by
//! side, each under the bound its adapter gives it.
//!
//! **A read serves only the asks that came before it began.** One harness is read one ask at
//! a time (two probes of one harness have hung); an ask that arrives while a read of its
//! harness is on its way waits for the next read, which it begins itself, and the asks already
//! waiting when that read begins share it. Each read has an id, which the page sends back with
//! what it submits from the list it showed: **a submission is checked against that read**
//! ([`ModelLists::checked`]), kept for this daemon's life (the last [`KEPT_READS`] of each
//! harness), never against whichever read was made last. Where there is no such read (none in
//! this daemon's life, one evicted, or one that failed) the harness is read again and the
//! submission is checked against that; nothing is accepted unchecked.
//!
//! Nothing is kept in the ledger: what an earlier daemon's life wrote of the lists is forgotten
//! as it starts ([`crate::start`]).
//!
//! Read at the ask, not as the daemon starts: a daemon a test starts would run every
//! harness its PATH and the system's folders find, and the human who opens no picker
//! waits for nothing.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use cf_base::env::Env;
use cf_engine::seams::Adapters;
use cf_harness::detect::harness_path;
use cf_proto::agents::Harness;
use cf_proto::models::{Freshness, ModelListing};
use tokio::sync::Mutex;

/// How many of the reads of each harness a submission can still be checked against.
pub const KEPT_READS: usize = 8;

/// One read of a harness: the listing it answered, and the id the page sends back with a
/// submission made from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRead {
    pub id: u64,
    pub listing: ModelListing,
}

/// The harnesses' model lists of this daemon's life.
pub struct ModelLists {
    adapters: Rc<dyn Adapters>,
    env: Env,
    /// One turn for each harness, held through each read of it: two probes of one harness at
    /// once have hung (two Pi's). The harnesses do not wait for one another.
    reading: RefCell<HashMap<Harness, Rc<Mutex<()>>>>,
    /// How many reads of each harness have begun.
    begun: RefCell<HashMap<Harness, u64>>,
    /// The last read of each harness that ended, and which read it was (by how many had begun
    /// when it did): an ask that arrived before that read began, and waited, takes it.
    last: RefCell<HashMap<Harness, (u64, ModelRead)>>,
    /// The id the next read takes.
    next: Cell<u64>,
    /// The last reads of each harness, oldest first, for a submission to be checked against.
    kept: RefCell<HashMap<Harness, VecDeque<ModelRead>>>,
}

impl ModelLists {
    /// The lists of a daemon that has read none.
    pub fn new(adapters: Rc<dyn Adapters>, env: Env) -> Self {
        Self {
            adapters,
            env,
            reading: RefCell::new(HashMap::new()),
            begun: RefCell::new(HashMap::new()),
            last: RefCell::new(HashMap::new()),
            next: Cell::new(1),
            kept: RefCell::new(HashMap::new()),
        }
    }

    /// The listing of `harness`, read from it now ([`ModelLists::read`]).
    pub async fn get(&self, harness: Harness, refresh: bool) -> Result<ModelListing, String> {
        self.read(harness, refresh).await.map(|read| read.listing)
    }

    /// `harness`, read from it now (`refresh` is the human's Refresh, which also has a harness
    /// fetch the catalog it keeps), and kept by the id it is given. A read that fails is a
    /// listing of no model that says why. Fails where the harness is not installed here.
    pub async fn read(&self, harness: Harness, refresh: bool) -> Result<ModelRead, String> {
        if harness_path(harness, &self.env).is_none() {
            return Err(format!("{} is not installed here", harness.as_str()));
        }
        let arrived = self.begun_reads(harness);
        let turn = Rc::clone(self.reading.borrow_mut().entry(harness).or_default());
        let _turn = turn.lock().await;
        // A read that began after this ask came, and ended while it waited for the turn, is the
        // read begun for the asks waiting then: this ask is one of them. A Refresh reads for
        // itself.
        if !refresh {
            if let Some((began, read)) = self.last.borrow().get(&harness) {
                if *began > arrived {
                    return Ok(read.clone());
                }
            }
        }
        let began = self.begun_reads(harness) + 1;
        self.begun.borrow_mut().insert(harness, began);
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
        let read = self.keep(harness, listing);
        self.last
            .borrow_mut()
            .insert(harness, (began, read.clone()));
        Ok(read)
    }

    /// Keeps `listing` as a read of `harness`, which takes the next id.
    fn keep(&self, harness: Harness, listing: ModelListing) -> ModelRead {
        let read = ModelRead {
            id: self.next.get(),
            listing,
        };
        self.next.set(read.id + 1);
        let mut kept = self.kept.borrow_mut();
        let reads = kept.entry(harness).or_default();
        reads.push_back(read.clone());
        while reads.len() > KEPT_READS {
            reads.pop_front();
        }
        read
    }

    /// How many reads of `harness` have begun in this daemon's life.
    fn begun_reads(&self, harness: Harness) -> u64 {
        self.begun.borrow().get(&harness).copied().unwrap_or(0)
    }

    /// The read `id` of `harness`, if this daemon still keeps it.
    pub fn shown(&self, harness: Harness, id: u64) -> Option<ModelListing> {
        self.kept
            .borrow()
            .get(&harness)?
            .iter()
            .find(|read| read.id == id)
            .map(|read| read.listing.clone())
    }

    /// The listing a submission on `harness` is checked against: the read `read` names (the one
    /// its picker showed) where this daemon keeps it and it listed models; else a read made now
    /// (the harness is read again, an ask that began after any read before it). Fails, with why,
    /// where that read lists no model: nothing is checked against no list, and nothing is
    /// accepted unchecked. None where ConsensFlow has no adapter to read the harness with (the
    /// five harnesses all have one), so that there is no list to check against at all.
    pub async fn checked(
        &self,
        harness: Harness,
        read: Option<u64>,
    ) -> Result<Option<ModelListing>, String> {
        if let Some(listing) = read
            .and_then(|id| self.shown(harness, id))
            .filter(|listing| !listing.models.is_empty())
        {
            return Ok(Some(listing));
        }
        if self.adapters.adapter(harness.kind()).is_none() {
            return Ok(None);
        }
        let listing = self.get(harness, false).await?;
        if listing.models.is_empty() {
            return Err(match &listing.freshness {
                Freshness::Stale { why } => why.clone(),
                Freshness::Fresh => "it lists no model".to_owned(),
            });
        }
        Ok(Some(listing))
    }

    /// Records `listing` as a read of `harness`, as the page would have been shown it: what a
    /// test checks a submission against with no adapter to read from.
    #[cfg(test)]
    pub(crate) fn record(&self, harness: Harness, listing: ModelListing) -> u64 {
        self.keep(harness, listing).id
    }
}

/// `listing` with its stale reason as it may be kept and told ([`cf_engine::quotable`]). A harness
/// quotes what its program said when a read fails, and what a program said can carry what it
/// was given: Pi's words (after the mark a failure of Pi's read carries) are screened whole and
/// withheld whole if anything in them may hold a credential, then cut; the reason of any other
/// harness, which has none, is only masked, and says what it said (`Invalid API key · Please run
/// /login`). The reason is kept with the read and told to the human.
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
pub(crate) mod tests;

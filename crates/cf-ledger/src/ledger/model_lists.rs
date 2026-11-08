//! The harnesses' model listings, as the ledger's callers hold them.

use cf_proto::models::ModelListing;

use super::Ledger;
use crate::{model_lists, LedgerError};

impl Ledger {
    /// The model listing kept of the last read of `harness` (`claude`), if one was tried.
    pub fn model_listing(&self, harness: &str) -> Result<Option<ModelListing>, LedgerError> {
        model_lists::model_listing(&self.store, harness)
    }

    /// Keeps `listing` as `harness`'s, in place of what was kept.
    pub fn keep_model_listing(
        &mut self,
        harness: &str,
        listing: &ModelListing,
    ) -> Result<(), LedgerError> {
        model_lists::keep_model_listing(&mut self.store, harness, listing)
    }

    /// Forgets every kept listing, as a daemon does when it starts.
    pub fn forget_model_listings(&mut self) -> Result<(), LedgerError> {
        model_lists::forget_model_listings(&mut self.store)
    }
}

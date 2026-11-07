//! The harnesses' model listings, as the ledger's callers hold them.

use cf_proto::models::ModelListing;

use super::Ledger;
use crate::{model_lists, LedgerError};

impl Ledger {
    /// The model listing kept for `harness` (`claude`), if a read was ever tried.
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
}

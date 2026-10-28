//! The harnesses' model lists, as the ledger's callers hold them.

use cf_proto::models::ModelList;

use super::Ledger;
use crate::{model_lists, LedgerError};

impl Ledger {
    /// The list of models kept for `harness` (`claude`), if one was ever tried.
    pub fn model_list(&self, harness: &str) -> Result<Option<ModelList>, LedgerError> {
        model_lists::model_list(&self.store, harness)
    }

    /// Keeps `list` as its harness's, in place of what was kept.
    pub fn keep_model_list(&mut self, list: &ModelList) -> Result<(), LedgerError> {
        model_lists::keep_model_list(&mut self.store, list)
    }
}

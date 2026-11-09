//! The harnesses' model listings an older build kept, which a daemon forgets as it starts.

use super::Ledger;
use crate::{model_lists, LedgerError};

impl Ledger {
    /// Forgets every kept listing, as a daemon does when it starts.
    pub fn forget_model_listings(&mut self) -> Result<(), LedgerError> {
        model_lists::forget_model_listings(&mut self.store)
    }
}

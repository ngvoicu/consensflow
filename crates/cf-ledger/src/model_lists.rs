//! What older builds kept of each harness's models, in the home ledger (migration 0027's table,
//! `model_list`): nothing writes or reads a row now. A model list is read when it is asked for
//! (A4-28, T-1321) and a submission is checked against the read its picker was shown, which the
//! daemon keeps in memory for its life (`cf_daemon::models`), so no list is ever shown from the
//! home. The rows an older build left are forgotten when the daemon starts
//! ([`forget_model_listings`]); migration 0033 reads them before that, at the ledger's opening,
//! for the names it gives members. No event is logged: a listing is no change to any project.

use crate::model::LedgerError;
use crate::store::Store;

/// Forgets every kept listing: what a harness listed in an earlier life of the daemon is no
/// record of what it lists now.
pub(crate) fn forget_model_listings(store: &mut Store) -> Result<(), LedgerError> {
    store.write(|store| {
        store.db.execute("DELETE FROM model_list", [])?;
        Ok(())
    })
}

//! What each harness listed at its last read, kept in the home (CHF-03): one row a harness,
//! its [`ModelListing`] written as a whole each time a list is read or a read fails
//! ([`ModelListing::failed`]). It is the record a submission is checked against, and nothing
//! the page is shown: a list is read when it is asked for, and the daemon forgets the rows
//! when it starts ([`forget_model_listings`]). No event is logged: a listing is no change to
//! any project.

use cf_proto::models::{Freshness, ModelChoice, ModelListing};
use rusqlite::{params, OptionalExtension};

use crate::model::LedgerError;
use crate::store::Store;

/// The listing kept for `harness` (`claude`), if a read was ever tried.
pub(crate) fn model_listing(
    store: &Store,
    harness: &str,
) -> Result<Option<ModelListing>, LedgerError> {
    let row = store
        .db
        .query_row(
            "SELECT models, read_at, how, stale_why FROM model_list WHERE harness = ?",
            [harness],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((models, read_at, how, stale_why)) = row else {
        return Ok(None);
    };
    let models: Vec<ModelChoice> = serde_json::from_str(&models).map_err(|error| {
        LedgerError::refused_with(
            "model-list-unreadable",
            format!("the models kept for {harness} are not readable: {error}"),
            500,
        )
    })?;
    Ok(Some(ModelListing {
        models,
        read_at,
        freshness: match stale_why {
            Some(why) => Freshness::Stale { why },
            None => Freshness::Fresh,
        },
        how,
    }))
}

/// Keeps `listing` as `harness`'s, in place of what was kept.
pub(crate) fn keep_model_listing(
    store: &mut Store,
    harness: &str,
    listing: &ModelListing,
) -> Result<(), LedgerError> {
    if harness.is_empty() || harness.len() > 64 {
        return Err(LedgerError::refused_with(
            "harness-invalid",
            format!("{harness:?} names no harness"),
            400,
        ));
    }
    let models = serde_json::to_string(&listing.models)
        .map_err(|error| LedgerError::refused_with("model-list-invalid", error.to_string(), 400))?;
    let stale_why = match &listing.freshness {
        Freshness::Stale { why } => Some(why.as_str()),
        Freshness::Fresh => None,
    };
    store.write(|store| {
        store.db.execute(
            "INSERT INTO model_list (harness, models, read_at, how, stale_why)
               VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (harness) DO UPDATE SET models = ?2, read_at = ?3, how = ?4,
               stale_why = ?5",
            params![harness, models, listing.read_at, listing.how, stale_why],
        )?;
        Ok(())
    })
}

/// Forgets every kept listing: what a harness listed in an earlier life of the daemon is no
/// record of what it lists now.
pub(crate) fn forget_model_listings(store: &mut Store) -> Result<(), LedgerError> {
    store.write(|store| {
        store.db.execute("DELETE FROM model_list", [])?;
        Ok(())
    })
}

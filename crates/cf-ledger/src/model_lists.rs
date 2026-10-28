//! What each harness lists of its models, kept in the home (CHF-03): one row a harness,
//! written as a whole each time a list is read or a read fails ([`ModelList::after`]),
//! read back for the pickers. No event is logged: a list is no change to any project.

use cf_proto::models::{ModelChoice, ModelList};
use rusqlite::{params, OptionalExtension};

use crate::model::LedgerError;
use crate::store::Store;

/// The list kept for `harness` (`claude`), if one was ever tried.
pub(crate) fn model_list(store: &Store, harness: &str) -> Result<Option<ModelList>, LedgerError> {
    let row = store
        .db
        .query_row(
            "SELECT models, stale, how, read_at, tried_at, failure FROM model_list WHERE harness = ?",
            [harness],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()?;
    let Some((models, stale, how, read_at, tried_at, failure)) = row else {
        return Ok(None);
    };
    let models: Vec<ModelChoice> = serde_json::from_str(&models).map_err(|error| {
        LedgerError::refused_with(
            "model-list-unreadable",
            format!("the models kept for {harness} are not readable: {error}"),
            500,
        )
    })?;
    Ok(Some(ModelList {
        harness: harness.to_owned(),
        models,
        stale: stale != 0,
        how,
        read_at,
        tried_at,
        failure,
    }))
}

/// Keeps `list` as its harness's, in place of what was kept.
pub(crate) fn keep_model_list(store: &mut Store, list: &ModelList) -> Result<(), LedgerError> {
    if list.harness.is_empty() || list.harness.len() > 64 {
        return Err(LedgerError::refused_with(
            "harness-invalid",
            format!("{:?} names no harness", list.harness),
            400,
        ));
    }
    let models = serde_json::to_string(&list.models)
        .map_err(|error| LedgerError::refused_with("model-list-invalid", error.to_string(), 400))?;
    store.write(|store| {
        store.db.execute(
            "INSERT INTO model_list (harness, models, stale, how, read_at, tried_at, failure)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (harness) DO UPDATE SET models = ?2, stale = ?3, how = ?4,
               read_at = ?5, tried_at = ?6, failure = ?7",
            params![
                list.harness,
                models,
                i64::from(list.stale),
                list.how,
                list.read_at,
                list.tried_at,
                list.failure
            ],
        )?;
        Ok(())
    })
}

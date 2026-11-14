//! Open a project from staff rows that carry backup links and Vacation marks.
//! Their ids identify rows in the source staff only; new participant ids are
//! made in the new project, and all links are rewritten in the same transaction.

use std::collections::{HashMap, HashSet};

use cf_base::time;
use rusqlite::params;
use serde_json::{json, Value};

use super::{create_project, known_project, NewMember, NewProject};
use crate::model::LedgerError;
use crate::staff::add_backup;
use crate::store::Store;
use crate::ProjectView;

struct SourceRow {
    id: Option<i64>,
    backup_of: Option<i64>,
    vacation: Option<String>,
    value: Value,
}

fn row_id(row: &Value, name: &str) -> Result<Option<i64>, LedgerError> {
    match row.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_i64()
            .filter(|number| *number > 0)
            .map(Some)
            .ok_or_else(|| {
                LedgerError::refused(
                    "invalid-staff-link",
                    format!("{name} is a positive staff row id"),
                )
            }),
    }
}

fn vacation(row: &Value) -> Result<Option<String>, LedgerError> {
    match row.get("vacationSince") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => time::canonical(value)
            .map(Some)
            .ok_or_else(|| LedgerError::refused("invalid-time", "vacationSince is a time")),
        Some(_) => Err(LedgerError::refused(
            "invalid-time",
            "vacationSince is a time",
        )),
    }
}

pub(crate) fn create_project_with_staff(
    store: &mut Store,
    value: &Value,
) -> Result<ProjectView, LedgerError> {
    let Some(staff) = value.get("staff") else {
        return create_project(store, &NewProject::from_json(value)?);
    };
    let staff = staff
        .as_array()
        .ok_or_else(|| LedgerError::refused("invalid-staff", "staff is a list of members"))?;
    let mut ids = HashSet::new();
    let mut rows = Vec::with_capacity(staff.len());
    for value in staff {
        let id = row_id(value, "id")?;
        if id.is_some_and(|id| !ids.insert(id)) {
            return Err(LedgerError::refused(
                "invalid-staff-link",
                "staff row ids must be distinct",
            ));
        }
        rows.push(SourceRow {
            id,
            backup_of: row_id(value, "backupOf")?,
            vacation: vacation(value)?,
            value: value.clone(),
        });
    }
    let mut request = value.clone();
    let object = request
        .as_object_mut()
        .ok_or_else(|| LedgerError::refused("invalid-project", "a project is an object"))?;
    object.insert(
        "staff".to_owned(),
        Value::Array(
            rows.iter()
                .filter(|row| row.backup_of.is_none())
                .map(|row| row.value.clone())
                .collect(),
        ),
    );
    let parsed = NewProject::from_json(&request)?;
    let mut backups = Vec::new();
    for row in rows.iter().filter(|row| row.backup_of.is_some()) {
        let Some(source_owner) = row.backup_of else {
            continue;
        };
        let mut config = row.value.clone();
        let object = config
            .as_object_mut()
            .ok_or_else(|| LedgerError::refused("invalid-staff", "a staff row is an object"))?;
        // A backup is not an independent holder of the source row's roles.
        // Validate its model configuration; add_backup takes roles from its owner.
        let designer = object.get("designer") == Some(&Value::Bool(true));
        object.insert(
            "roles".into(),
            json!([if designer { "designer" } else { "worker" }]),
        );
        if object.get("tier").is_none_or(Value::is_null) {
            object.insert("tier".into(), json!("light"));
        }
        backups.push((
            source_owner,
            NewMember::from_json(&config)?,
            row.vacation.clone(),
        ));
    }
    store.write(|store| {
        let opened = create_project(store, &parsed)?;
        let primaries: Vec<_> = opened
            .participants
            .iter()
            .filter(|p| crate::roles::is_role(&p.role))
            .collect();
        let source_primaries: Vec<_> = rows.iter().filter(|row| row.backup_of.is_none()).collect();
        if primaries.len() != source_primaries.len() {
            return Err(LedgerError::refused(
                "invalid-staff-link",
                "the copied staff is incomplete",
            ));
        }
        let mut owners = HashMap::new();
        for (source, member) in source_primaries.into_iter().zip(primaries) {
            if let Some(id) = source.id {
                owners.insert(id, member.handle.clone());
            }
            if let Some(since) = &source.vacation {
                store.db.execute(
                    "UPDATE participant SET vacation_since = ? WHERE id = ?",
                    params![since, member.id],
                )?;
                store.log(
                    opened.id,
                    "member.vacation",
                    json!({ "handle": member.handle, "since": since }),
                )?;
            }
        }
        for (source_owner, backup, since) in &backups {
            let owner = owners.get(source_owner).ok_or_else(|| {
                LedgerError::refused(
                    "invalid-staff-link",
                    "a backup must name a primary in the copied staff",
                )
            })?;
            let created = add_backup(store, opened.id, owner, backup)?;
            if let Some(since) = since {
                store.db.execute(
                    "UPDATE participant SET vacation_since = ? WHERE id = ?",
                    params![since, created.id],
                )?;
                store.log(
                    opened.id,
                    "member.vacation",
                    json!({ "handle": created.handle, "since": since }),
                )?;
            }
        }
        known_project(store, opened.id)
    })
}

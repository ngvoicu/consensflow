//! What the owner's own setup of a harness is at a moment, as far as a run could have
//! changed it: the hash of each file that holds a login or a config, and the entries of
//! each folder of records that name a folder of a run. Taken when a run's home is made
//! and again when the run is over; a difference is the run's (or the owner's own harness,
//! which could have written it as well), said with the file named and never its contents.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::super::login::digest;

/// Whether `name` is the name a harness gives (in its folder of sessions) to a folder of a
/// run: the live suites' (`~/.consensflow-candidate/live/…`) or the home of a rig
/// (`consensflow-integration-…`).
/// A harness writes a path with its separators, and its dot, as `-`, or as `--` around it.
pub fn names_a_run(name: &str) -> bool {
    name.contains("consensflow-candidate-live-") || name.contains("consensflow-integration-")
}

/// What the owner's setup is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    /// The hash of each file watched; none for a file that is not there.
    files: BTreeMap<PathBuf, Option<String>>,
    /// Names only from the Codex config, from the same bytes as its hash.
    tables: BTreeMap<PathBuf, Option<BTreeSet<String>>>,
    /// The entries of each folder of records that name a folder of a run.
    records: BTreeMap<PathBuf, BTreeSet<String>>,
}

impl Seen {
    /// What `files` and `records` are now.
    pub fn look(
        files: &[PathBuf],
        records: &[PathBuf],
        codex_config: Option<&Path>,
        pi_auth: Option<&Path>,
    ) -> Self {
        let mut tables = BTreeMap::new();
        let files = files
            .iter()
            .map(|file| {
                crate::standin_process::require_home(file, "harness isolation file lookup");
                let read = if pi_auth == Some(file.as_path()) {
                    super::copy_pi_login(file).map_err(|_| std::io::ErrorKind::Other)
                } else {
                    fs::read(file).map_err(|error| error.kind())
                };
                if codex_config == Some(file.as_path()) {
                    tables.insert(
                        file.clone(),
                        match &read {
                            Ok(bytes) => table_names(bytes),
                            Err(std::io::ErrorKind::NotFound) => Some(BTreeSet::new()),
                            Err(_) => None,
                        },
                    );
                }
                (file.clone(), read.as_deref().ok().map(digest))
            })
            .collect();
        Self {
            files,
            tables,
            records: records
                .iter()
                .map(|folder| {
                    crate::standin_process::require_home(
                        folder,
                        "harness isolation records lookup",
                    );
                    (folder.clone(), run_entries(folder))
                })
                .collect(),
        }
    }

    /// The files watched, each with the first twelve digits of its hash (`none` for one that is not
    /// there), below `~` where it is below `home`: what a run that left the setup as it was says it
    /// checked. A hash says nothing of what the file holds.
    pub fn summary(&self, home: &Path) -> String {
        self.files
            .iter()
            .map(|(file, hash)| {
                format!(
                    "{} {}",
                    shown(file, home),
                    hash.as_deref()
                        .map_or("none", |hash| &hash[..12.min(hash.len())])
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// What differs between `self` (before the run) and `after`, in words that name the file
    /// or the folder (below `~` where it is below `home`); nothing where nothing does.
    pub fn changes_to(&self, after: &Self, home: &Path) -> Vec<String> {
        let mut found = Vec::new();
        for (file, was) in &self.files {
            let now = after.files.get(file).cloned().flatten();
            match (was, &now) {
                (Some(was), Some(now)) if was == now => {}
                (Some(_), Some(_)) => found.push(format!("{} is not as it was", shown(file, home))),
                (Some(_), None) => found.push(format!("{} is gone", shown(file, home))),
                (None, Some(_)) => found.push(format!("{} was made", shown(file, home))),
                (None, None) => {}
            }
        }
        for (folder, before) in &self.records {
            for name in after
                .records
                .get(folder)
                .into_iter()
                .flatten()
                .filter(|name| !before.contains(*name))
            {
                found.push(format!("{} gained {name}", shown(folder, home)));
            }
        }
        found
    }

    /// The changed Codex table names, and which added names are also in the run's config.
    /// Config values and parse errors are never included in the output.
    pub fn codex_tables_to(&self, after: &Self, owner: &Path, run: &Path) -> Option<String> {
        if self.files.get(owner) == after.files.get(owner) {
            return None;
        }
        let before = self.tables.get(owner).and_then(Option::as_ref);
        let now = after.tables.get(owner).and_then(Option::as_ref);
        let (Some(before), Some(now)) = (before, now) else {
            return Some("Codex config table names could not be compared".to_owned());
        };
        let added: BTreeSet<_> = now.difference(before).cloned().collect();
        let removed: BTreeSet<_> = before.difference(now).cloned().collect();
        let show = |names: &BTreeSet<String>| {
            if names.is_empty() {
                "none".to_owned()
            } else {
                names.iter().cloned().collect::<Vec<_>>().join(", ")
            }
        };
        let run_tables = fs::read(run).ok().and_then(|bytes| table_names(&bytes));
        let in_run = run_tables.map_or_else(
            || "unavailable".to_owned(),
            |tables| show(&added.intersection(&tables).cloned().collect()),
        );
        Some(format!(
            "Codex config.toml tables added: {}; removed: {}; added tables also in the run's config.toml: {in_run}",
            show(&added), show(&removed)
        ))
    }
}

/// Parse the document and retain table paths only, never scalar values or errors.
fn table_names(bytes: &[u8]) -> Option<BTreeSet<String>> {
    fn walk(table: &toml::Table, path: &mut Vec<String>, names: &mut BTreeSet<String>) {
        for (key, value) in table {
            path.push(format!("{key:?}"));
            let mut add = |nested: &toml::Table| {
                names.insert(format!("[{}]", path.join(".")));
                walk(nested, path, names);
            };
            match value {
                toml::Value::Table(nested) => add(nested),
                toml::Value::Array(values) => {
                    for nested in values.iter().filter_map(toml::Value::as_table) {
                        add(nested);
                    }
                }
                _ => {}
            }
            path.pop();
        }
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let parsed = text.parse::<toml::Table>().ok()?;
    let mut names = BTreeSet::new();
    walk(&parsed, &mut Vec::new(), &mut names);
    Some(names)
}

/// The entries of `folder` that name a folder of a run.
fn run_entries(folder: &Path) -> BTreeSet<String> {
    fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| names_a_run(name))
        .collect()
}

/// `path` as a run says it: below `~` where it is below the owner's `home`.
pub fn shown(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        // With `/` between the parts, where the system's separator is another: a path on Windows
        // is said `~/AppData/Roaming/devin/…`, not `~/AppData/Roaming\devin\…`.
        Ok(below) => {
            let parts: Vec<_> = below
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect();
            format!("~/{}", parts.join("/"))
        }
        Err(_) => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t1744_codex_table_names_compare_on_either_platform_without_values() {
        let root = crate::door::made("cf-codex-tables-").unwrap();
        let owner = root.path().join("owner.toml");
        let run = root.path().join("run.toml");
        crate::door::write(&owner, "[old]\nkey = 'synthetic-old-value'\n").unwrap();
        crate::door::write(&run, "[new]\nkey = 'synthetic-run-value'\n").unwrap();
        let files = [owner.clone()];
        let before = Seen::look(&files, &[], Some(&owner), None);
        crate::door::write(&owner, "[new]\nkey = 'synthetic-new-value'\n").unwrap();
        let after = Seen::look(&files, &[], Some(&owner), None);
        let said = before.codex_tables_to(&after, &owner, &run).unwrap();
        assert!(said.contains("[\"new\"]"));
        assert!(said.contains("[\"old\"]"));
        assert!(said.contains("also in the run"));
        assert!(!said.contains("synthetic-"));
    }

    #[test]
    fn t1744_an_unreadable_codex_config_is_not_said_to_have_lost_its_tables() {
        let root = crate::door::made("cf-codex-unreadable-").unwrap();
        let owner = root.path().join("owner.toml");
        crate::door::write(&owner, "[old]\nkey = 'synthetic'\n").unwrap();
        let files = [owner.clone()];
        let before = Seen::look(&files, &[], Some(&owner), None);
        std::fs::remove_file(&owner).unwrap();
        std::fs::create_dir(&owner).unwrap();
        let after = Seen::look(&files, &[], Some(&owner), None);
        let said = before
            .codex_tables_to(&after, &owner, &root.path().join("run.toml"))
            .unwrap();
        assert!(said.contains("could not be compared"), "{said}");
        assert!(!said.contains("removed"), "{said}");
    }
}

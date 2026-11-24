//! What the owner's own setup of a harness is at a moment, as far as a run could have
//! changed it: the metadata of each login file, the hash of each config, and the entries of
//! each folder of records that name a folder of a run. Taken when a run's home is made
//! and again when the run is over; a difference is the run's (or the owner's own harness,
//! which could have written it as well), said with the file named and never its contents.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::door::source::Source;

use super::super::login::digest;

/// Whether `name` is the name a harness gives (in its folder of sessions) to a folder of a
/// run: the live suites' (`~/.consensflow-candidate/live/…`) or the home of a rig
/// (`consensflow-integration-…`).
/// A harness writes a path with its separators, and its dot, as `-`, or as `--` around it.
pub fn names_a_run(name: &str) -> bool {
    name.contains("consensflow-candidate-live-") || name.contains("consensflow-integration-")
}

/// What the owner's setup is.
#[derive(Debug, Clone, PartialEq)]
enum Observed {
    Content(Option<String>),
    Metadata(Option<cf_base::file::Stat>),
    Unreadable,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Seen {
    /// Login files are never read: their size, modification time and identity suffice.
    /// The other files are compared by their content hashes.
    files: BTreeMap<Source, Observed>,
    /// Names only from the Codex config, from the same bytes as its hash.
    tables: BTreeMap<Source, Option<BTreeSet<String>>>,
    /// The entries of each folder of records that name a folder of a run.
    records: BTreeMap<Source, BTreeSet<String>>,
}

impl Seen {
    /// What `files` and `records` are now.
    pub fn look(
        files: &[Source],
        records: &[Source],
        codex_config: Option<&Source>,
        login_files: &[Source],
    ) -> Self {
        let mut tables = BTreeMap::new();
        let files = files
            .iter()
            .map(|file| {
                file.require_home("harness isolation file lookup");
                if login_files.contains(file) {
                    // On Windows stat opens a handle for FILE_READ_ATTRIBUTES only, never bytes.
                    let seen = match file.stat() {
                        Ok(stat) => Observed::Metadata(Some(stat)),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            Observed::Metadata(None)
                        }
                        Err(_) => Observed::Unreadable,
                    };
                    return (file.clone(), seen);
                }
                let read = file.read();
                if codex_config == Some(file) {
                    tables.insert(
                        file.clone(),
                        match &read {
                            Ok(bytes) => table_names(bytes),
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                                Some(BTreeSet::new())
                            }
                            Err(_) => None,
                        },
                    );
                }
                let seen = match read {
                    Ok(bytes) => Observed::Content(Some(digest(&bytes))),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        Observed::Content(None)
                    }
                    Err(_) => Observed::Unreadable,
                };
                (file.clone(), seen)
            })
            .collect();
        Self {
            files,
            tables,
            records: records
                .iter()
                .map(|folder| {
                    folder.require_home("harness isolation records lookup");
                    (folder.clone(), run_entries(folder))
                })
                .collect(),
        }
    }

    /// What was compared: a hash of config bytes or a login file's metadata only.
    pub fn summary(&self, home: &Source) -> String {
        self.files
            .iter()
            .map(|(file, seen)| {
                let what = match seen {
                    Observed::Content(Some(hash)) => &hash[..12.min(hash.len())],
                    Observed::Metadata(Some(_)) => "metadata",
                    Observed::Content(None) | Observed::Metadata(None) => "none",
                    Observed::Unreadable => "unreadable",
                };
                format!("{} {}", shown(file, home), what)
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// What differs, in words that name paths and never a file's contents.
    pub fn changes_to(&self, after: &Self, home: &Source) -> Vec<String> {
        let mut found = Vec::new();
        for (file, was) in &self.files {
            let now = after.files.get(file);
            let shown = shown(file, home);
            match (was, now) {
                (Observed::Unreadable, _) | (_, None | Some(Observed::Unreadable)) => {
                    found.push(format!("{shown} could not be compared"));
                }
                (was, Some(now)) if was == now => {}
                (Observed::Content(Some(_)), Some(Observed::Content(None)))
                | (Observed::Metadata(Some(_)), Some(Observed::Metadata(None))) => {
                    found.push(format!("{shown} is gone"));
                }
                (Observed::Content(None), Some(Observed::Content(Some(_))))
                | (Observed::Metadata(None), Some(Observed::Metadata(Some(_)))) => {
                    found.push(format!("{shown} was made"));
                }
                _ => found.push(format!("{shown} is not as it was")),
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
    pub fn codex_tables_to(&self, after: &Self, owner: &Source, run: &Path) -> Option<String> {
        let before = self.tables.get(owner).and_then(Option::as_ref);
        let now = after.tables.get(owner).and_then(Option::as_ref);
        if self.files.get(owner) == after.files.get(owner) {
            // Matching hashes prove identical bytes, even when those bytes are not TOML.
            // A missing file is known to have no tables; an unreadable file is not.
            if matches!(self.files.get(owner), Some(Observed::Content(Some(_))))
                || (before.is_some() && now.is_some())
            {
                return None;
            }
            return Some("Codex config table names could not be compared".to_owned());
        }
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
        let run_tables = crate::door::reads::read(run).ok().and_then(|bytes| table_names(&bytes));
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
fn run_entries(folder: &Source) -> BTreeSet<String> {
    folder
        .names()
        .into_iter()
        .flatten()
        .filter_map(|name| name.into_string().ok())
        .filter(|name| names_a_run(name))
        .collect()
}

/// `path` as a run says it: below `~` where it is below the owner's `home`.
pub fn shown(path: &Source, home: &Source) -> String {
    match path.below(home) {
        // With `/` between the parts, where the system's separator is another: a path on Windows
        // is said `~/AppData/Roaming/devin/…`, not `~/AppData/Roaming\devin\…`.
        Some(below) => format!("~/{below}"),
        None => path.name(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t1744_a_login_replaced_at_the_same_size_and_time_is_detected_by_identity() {
        let root = crate::door::made("cf-login-metadata-").unwrap();
        let path = root.path().join("auth.json");
        let replacement = root.path().join("replacement.json");
        crate::door::write(&path, b"one").unwrap();
        crate::door::write(&replacement, b"two").unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        crate::door::set_modified(&replacement, modified).unwrap();
        let files = [Source::from(&path)];
        let before = Seen::look(&files, &[], None, &files);
        let original = cf_base::file::stat(&path).unwrap();
        crate::door::rename(&path, root.path().join("old.json")).unwrap();
        crate::door::rename(&replacement, &path).unwrap();
        let replaced = cf_base::file::stat(&path).unwrap();
        assert_eq!(original.size, replaced.size);
        assert_eq!(original.mtime_ms, replaced.mtime_ms);
        assert_ne!(original.identity, replaced.identity);
        let after = Seen::look(&files, &[], None, &files);
        let changed = before.changes_to(&after, &Source::from(root.path()));
        assert_eq!(changed.len(), 1);
        assert!(changed[0].contains("auth.json"));
    }

    #[cfg(unix)]
    #[test]
    fn t1744_an_unstatable_login_cannot_be_said_unchanged() {
        let root = crate::door::made("cf-login-unstatable-").unwrap();
        let path = root.path().join("auth.json");
        crate::door::symlink("auth.json", &path).unwrap();
        let files = [Source::from(&path)];
        let before = Seen::look(&files, &[], None, &files);
        let after = Seen::look(&files, &[], None, &files);
        assert!(before.changes_to(&after, &Source::from(root.path()))[0].contains("could not be compared"));
    }

    #[test]
    fn t1744_codex_table_names_compare_on_either_platform_without_values() {
        let root = crate::door::made("cf-codex-tables-").unwrap();
        let owner = root.path().join("owner.toml");
        let run = root.path().join("run.toml");
        crate::door::write(&owner, "[old]\nkey = 'synthetic-old-value'\n").unwrap();
        crate::door::write(&run, "[new]\nkey = 'synthetic-run-value'\n").unwrap();
        let files = [Source::from(&owner)];
        let owner_source = Source::from(&owner);
        let before = Seen::look(&files, &[], Some(&owner_source), &[]);
        crate::door::write(&owner, "[new]\nkey = 'synthetic-new-value'\n").unwrap();
        let after = Seen::look(&files, &[], Some(&owner_source), &[]);
        let said = before.codex_tables_to(&after, &owner_source, &run).unwrap();
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
        let files = [Source::from(&owner)];
        let owner_source = Source::from(&owner);
        let before = Seen::look(&files, &[], Some(&owner_source), &[]);
        crate::door::remove(&owner).unwrap();
        crate::door::make_dir(&owner).unwrap();
        let after = Seen::look(&files, &[], Some(&owner_source), &[]);
        let said = before
            .codex_tables_to(&after, &owner_source, &root.path().join("run.toml"))
            .unwrap();
        assert!(said.contains("could not be compared"), "{said}");
        assert!(!said.contains("removed"), "{said}");
    }

    #[test]
    fn t1744_two_unreadable_codex_snapshots_cannot_prove_the_config_unchanged() {
        let root = crate::door::made("cf-codex-unreadable-both-").unwrap();
        let owner = root.path().join("owner.toml");
        crate::door::make_dir(&owner).unwrap();
        let files = [Source::from(&owner)];
        let owner_source = Source::from(&owner);
        let before = Seen::look(&files, &[], Some(&owner_source), &[]);
        let after = Seen::look(&files, &[], Some(&owner_source), &[]);
        let said = before
            .codex_tables_to(&after, &owner_source, &root.path().join("run.toml"))
            .expect("unreadable snapshots cannot prove the config unchanged");
        assert!(said.contains("could not be compared"), "{said}");
    }

    #[test]
    fn t1744_two_absent_or_identical_invalid_codex_configs_are_unchanged() {
        let root = crate::door::made("cf-codex-unchanged-").unwrap();
        let owner = root.path().join("owner.toml");
        let files = [Source::from(&owner)];
        let owner_source = Source::from(&owner);
        let before = Seen::look(&files, &[], Some(&owner_source), &[]);
        let after = Seen::look(&files, &[], Some(&owner_source), &[]);
        assert_eq!(before.codex_tables_to(&after, &owner_source, &owner), None);

        crate::door::write(&owner, "[unfinished").unwrap();
        let before = Seen::look(&files, &[], Some(&owner_source), &[]);
        let after = Seen::look(&files, &[], Some(&owner_source), &[]);
        assert_eq!(before.codex_tables_to(&after, &owner_source, &owner), None);
    }

    #[test]
    fn t1744_a_missing_config_that_becomes_unreadable_is_not_said_unchanged() {
        let root = crate::door::made("cf-codex-missing-then-unreadable-").unwrap();
        let owner = root.path().join("owner.toml");
        let files = [Source::from(&owner)];
        let owner_source = Source::from(&owner);
        let before = Seen::look(&files, &[], Some(&owner_source), &[]);
        crate::door::make_dir(&owner).unwrap();
        let after = Seen::look(&files, &[], Some(&owner_source), &[]);
        let said = before.codex_tables_to(&after, &owner_source, &owner).unwrap();
        assert!(said.contains("could not be compared"), "{said}");
    }
}

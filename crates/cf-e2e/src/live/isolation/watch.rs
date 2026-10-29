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
/// run: the live suites' (`~/.consensflow-candidate/live/…`), the evals' workspace
/// (`~/.consensflow-candidate/evals/…`), or the home of a rig (`consensflow-integration-…`).
/// A harness writes a path with its separators, and its dot, as `-`, or as `--` around it.
pub fn names_a_run(name: &str) -> bool {
    name.contains("consensflow-candidate-live-")
        || name.contains("consensflow-candidate-evals-")
        || name.contains("consensflow-integration-")
}

/// What the owner's setup is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    /// The hash of each file watched; none for a file that is not there.
    files: BTreeMap<PathBuf, Option<String>>,
    /// The entries of each folder of records that name a folder of a run.
    records: BTreeMap<PathBuf, BTreeSet<String>>,
}

impl Seen {
    /// What `files` and `records` are now.
    pub fn look(files: &[PathBuf], records: &[PathBuf]) -> Self {
        Self {
            files: files
                .iter()
                .map(|file| {
                    (
                        file.clone(),
                        fs::read(file).ok().map(|bytes| digest(&bytes)),
                    )
                })
                .collect(),
            records: records
                .iter()
                .map(|folder| (folder.clone(), run_entries(folder)))
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
        Ok(below) => format!("~/{}", below.display()),
        Err(_) => path.display().to_string(),
    }
}

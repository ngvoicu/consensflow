//! The renewal's door (K3, `live-kit-isolation-doors.md` section 2): the one place the kit runs a
//! harness itself on the account's own homes, to renew a login it keeps a copy of, and the one
//! that lists the records folder the harness will write in.
//!
//! - **[`start`]** runs the harness once, on the account's homes by name, in a folder this run
//!   made. It takes the test machine's token, and refuses without one before anything is made or
//!   started: a renewal writes the account's login and can rotate its refresh token, which on an
//!   account that is its owner's own is theirs. Only Claude Code and Pi are renewed.
//! - **[`Listing`]** is the records folder as it is before the harness runs, read whole: a folder
//!   or an entry that cannot be read gives no listing, so no renewal runs on a baseline that
//!   would take a record that was the owner's for the harness's own (T-1997 finding 9). A name
//!   in the listing is compared as the file system compares names.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::live::renew::TestMachine;
use crate::live::Harness;
use crate::process::{Ran, Run};
use crate::{Error, Result};

/// Runs `program`, the installed program of `harness`, once with `words`, on the account's homes
/// (`account`, by name) and the switches that keep it from updating ([`crate::live::launcher`]),
/// in `cwd`, a folder this run made, ended after `limit`. `machine` is the test machine's token.
pub fn start(
    machine: &TestMachine,
    harness: Harness,
    program: &Path,
    words: &[String],
    account: &[(String, String)],
    cwd: &Path,
    limit: Duration,
) -> Result<Ran> {
    let _ = machine;
    if !matches!(harness, Harness::Claude | Harness::Pi) {
        return Err(Error::Daemon(format!(
            "{harness:?} is not renewed by the kit: only Claude Code's and Pi's logins are"
        )));
    }
    if !crate::door::is_made_or_below(cwd) {
        return Err(Error::Daemon(format!(
            "the renewal runs in a folder this run made, and {} is none",
            cwd.display()
        )));
    }
    Run::operation("renewal", program)
        .args(words)
        .vars(account.iter().cloned())
        .vars(crate::live::launcher::updates_off_of(harness).iter().copied())
        .cwd(cwd)
        .closed_input()
        .limit(limit)
        .run()
}

/// The records folders as they are before the harness runs: each folder, and every name in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    folders: Vec<(PathBuf, BTreeSet<OsString>)>,
}

impl Listing {
    /// Reads each of `records` whole. A folder that is not there holds no names; one that cannot
    /// be listed, or an entry of it that cannot be read, is an error: nothing is run on it.
    pub fn of(records: &[&Path]) -> Result<Listing> {
        let mut folders = Vec::new();
        for folder in records {
            let names = match fs::read_dir(folder) {
                Ok(entries) => entries
                    .map(|entry| entry.map(|entry| entry.file_name()))
                    .collect::<std::io::Result<BTreeSet<OsString>>>()
                    .map_err(Error::file("list", folder))?,
                Err(failed) if failed.kind() == std::io::ErrorKind::NotFound => BTreeSet::new(),
                Err(failed) => return Err(Error::file("list", folder)(failed)),
            };
            folders.push(((*folder).to_path_buf(), names));
        }
        Ok(Listing { folders })
    }

    /// Whether the `at`th folder held `name` before the harness ran, as the file system compares
    /// names: the same letters in another case are the same name, whatever the volume does.
    pub fn held(&self, at: usize, name: &std::ffi::OsStr) -> bool {
        let wanted = name.to_string_lossy().to_lowercase();
        self.folders.get(at).is_some_and(|(_, names)| {
            names
                .iter()
                .any(|held| held.to_string_lossy().to_lowercase() == wanted)
        })
    }
}

//! The owner's launchers of the harnesses, which a live run never moves (T-1082).
//!
//! A live run keeps the person's `HOME` for its windows (their project folders are below it,
//! [`super::isolation`]), so a harness that installs itself writes where the person's own
//! installer wrote: `~/.local/bin`. Claude Code's native updater does it ten seconds after its
//! screen mounts, and it divides its paths: the launcher is `$HOME/.local/bin/claude`, the
//! versions are in `$XDG_DATA_HOME/claude/versions`. A rig that gives its windows a throwaway
//! `XDG_DATA_HOME` (a seat for Muse does, Devin's home does) therefore had an updater that found
//! no version of its own, downloaded the newest, put it in the throwaway folder and re-pointed the
//! person's `claude` at it, because their link is "a symlink into a claude/versions/ directory".
//! It did that for the version the person already ran, so not one byte of any binary differed; and
//! when the run ended the folder it pointed into was gone. The alpha.1 Mac release check found
//! their `claude` command dangling (T-1068). The kit hashed their login, settings and projects, and
//! not their launchers. Muse's launcher (`~/.local/bin/muse`) is the same hazard another way: asked
//! for `--version` by the kit with the person's own environment, it ran its hourly check, which
//! may rewrite the launcher and the binary.
//!
//! Two rules keep a run off them:
//!
//! - **Every window and every probe of the kit is told to update no harness** ([`updates_off`]):
//!   Claude Code's two switches, `DISABLE_AUTOUPDATER` and `DISABLE_UPDATES`, and Muse's launcher's
//!   `MUSE_NO_AUTO_UPDATE`. The live case `live launcher` shows that a Claude Code honours them,
//!   on the machine's own, in a pretend owner's home: they are honoured by 2.1.263, 2.1.314 and
//!   2.1.316 (each run alone in one, T-1082), so a version that stops honouring them is found by
//!   that case, and a switch that is only set proves nothing.
//! - **The launchers are hashed around every run** ([`Guard`], a [`HarnessHome`] of every harness a
//!   rig seats): where each one is, the link it reads, where it ends and the hash of the file it
//!   ends at, the entries of the folder the harness installs its versions in, and Muse's version
//!   line. A difference fails the rig that ends, with the link the owner had, so that it can be
//!   put back by hand. The kit never puts it back.
//!
//! The link and where it ends are recorded as well as the bytes of the file, because the
//! updater's copy of the version the owner already ran is the same file.
//!
//! **The Keychain** is the owner's too (T-1102): the guard of Claude Code also lists the names of
//! the login Keychain's items of `Claude Code-credentials` around the run, and an item gained fails
//! the rig that ends ([`super::keychain`] says what writes one, and what keeps a run from it).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use sha2::{Digest, Sha256};

use super::keychain;
use super::login::digest;
use super::machine::Harness;
use crate::process::own_var;
use crate::rig::HarnessHome;
use crate::{Error, Result};

/// What Claude Code is told, in every window and probe of the kit, to keep its native updater
/// from installing a version and re-pointing `$HOME/.local/bin/claude`. Each of the two is
/// read by the binary's `gy` (2.1.314, 2.1.316: either one stops the installer), and the second
/// also refuses `claude update`.
pub const CLAUDE_UPDATES_OFF: [(&str, &str); 2] =
    [("DISABLE_AUTOUPDATER", "1"), ("DISABLE_UPDATES", "1")];

/// What Muse's launcher is told, so that it does not run its hourly check, which rewrites the
/// launcher, the binary and the version line beside it.
pub const MUSE_UPDATES_OFF: [(&str, &str); 1] = [("MUSE_NO_AUTO_UPDATE", "1")];

/// The variables that keep `harness` from updating itself, as the kit gives them to it. Codex,
/// Pi and Devin have none here: Codex and Devin are told by their own configuration
/// (`check_for_update_on_startup=false`, `auto_update: false`), and Pi installs only on `pi update`.
pub const fn updates_off_of(harness: Harness) -> &'static [(&'static str, &'static str)] {
    match harness {
        Harness::Claude => &CLAUDE_UPDATES_OFF,
        Harness::Muse => &MUSE_UPDATES_OFF,
        Harness::Codex | Harness::Pi | Harness::Devin => &[],
    }
}

/// The variables of every harness's, which every window of a rig has (a harmless variable to
/// the others).
pub fn updates_off() -> impl Iterator<Item = (&'static str, &'static str)> {
    CLAUDE_UPDATES_OFF.into_iter().chain(MUSE_UPDATES_OFF)
}

/// What to look at for one harness.
#[derive(Debug, Clone)]
pub struct Watch {
    /// The launchers: each is looked at as a link and as the file it ends at.
    launchers: Vec<PathBuf>,
    /// The folders where the harness installs its versions, whose entries are listed, those that
    /// begin with the word where there is one.
    folders: Vec<(PathBuf, Option<&'static str>)>,
    /// The small files that name the version in use.
    files: Vec<PathBuf>,
    /// How to list the names of Claude Code's Keychain items, for the harness that keeps its login
    /// there; none for the others, and for a watch that is only about the launchers.
    keychain: Option<fn() -> Option<BTreeSet<String>>>,
}

impl Watch {
    /// What to look at for `harness`, whose program the kit runs is `command`, on the machine of
    /// the person whose home is `home` (Codex's `home` is its own folder, which holds no launcher).
    pub fn of(harness: Harness, command: &Path, home: &Path) -> Self {
        let program = if cfg!(windows) {
            format!("{}.exe", harness.name())
        } else {
            harness.name().to_owned()
        };
        let mut launchers = vec![command.to_path_buf()];
        if harness != Harness::Codex {
            // Where the harness's own installer puts it: the link it re-points.
            let installed = home.join(".local").join("bin").join(program);
            if !launchers.contains(&installed) {
                launchers.push(installed);
            }
        }
        let data = own_var("XDG_DATA_HOME")
            .filter(|folder| !folder.is_empty())
            .map_or_else(|| home.join(".local").join("share"), PathBuf::from);
        let mut folders = Vec::new();
        let mut files = Vec::new();
        match harness {
            Harness::Claude => {
                folders.push((data.join("claude").join("versions"), None));
            }
            Harness::Devin => {
                folders.push((data.join("devin").join("cli").join("_versions"), None));
            }
            Harness::Muse => {
                // The launcher keeps its binaries and its version line beside itself.
                let install = fs::canonicalize(command)
                    .ok()
                    .and_then(|real| real.parent().map(Path::to_path_buf))
                    .or_else(|| command.parent().map(Path::to_path_buf));
                if let Some(install) = install {
                    folders.push((install.clone(), Some("muse")));
                    files.push(install.join(".muse-version"));
                }
            }
            Harness::Codex | Harness::Pi => {}
        }
        Self {
            launchers,
            folders,
            files,
            keychain: None,
        }
    }

    /// This watch, which also lists the names of Claude Code's Keychain items with `names` (none
    /// where the machine has no Keychain, or it cannot be listed, which a run then cannot check).
    #[must_use]
    pub fn with_keychain(mut self, names: fn() -> Option<BTreeSet<String>>) -> Self {
        self.keychain = Some(names);
        self
    }

    /// What it is now.
    pub fn look(&self) -> Seen {
        Seen {
            keychain: self.keychain.and_then(|names| names()),
            launchers: self
                .launchers
                .iter()
                .map(|path| (path.clone(), Launcher::look(path)))
                .collect(),
            folders: self
                .folders
                .iter()
                .map(|(folder, prefix)| (folder.clone(), entries(folder, *prefix)))
                .collect(),
            files: self
                .files
                .iter()
                .map(|file| {
                    (
                        file.clone(),
                        fs::read(file).ok().map(|bytes| digest(&bytes)),
                    )
                })
                .collect(),
        }
    }
}

/// The names in `folder`, those that begin with `prefix` where there is one; none for a folder
/// that is not there.
fn entries(folder: &Path, prefix: Option<&str>) -> Option<BTreeSet<String>> {
    let read = fs::read_dir(folder).ok()?;
    Some(
        read.flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| prefix.is_none_or(|prefix| name.starts_with(prefix)))
            .collect(),
    )
}

/// The size and the hash of a file's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Bytes {
    size: u64,
    hash: String,
}

impl Bytes {
    /// The file `path`'s, read once through (the binary of a harness is hundreds of megabytes).
    fn of(path: &Path) -> Option<Self> {
        let mut file = fs::File::open(path).ok()?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; 1 << 20];
        let mut size = 0_u64;
        loop {
            let read = file.read(&mut buffer).ok()?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            size += read as u64;
        }
        Some(Self {
            size,
            hash: hasher
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        })
    }
}

/// A launcher as it is at a moment.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Launcher {
    /// Whether anything is at the path (a link that dangles is something).
    present: bool,
    /// The link the path reads, where it is one.
    link: Option<PathBuf>,
    /// Where it ends, every link followed; none for one that dangles.
    ends_at: Option<PathBuf>,
    /// The bytes of the file it ends at.
    bytes: Option<Bytes>,
}

impl Launcher {
    fn look(path: &Path) -> Self {
        let present = fs::symlink_metadata(path).is_ok();
        let ends_at = fs::canonicalize(path).ok();
        Self {
            present,
            link: fs::read_link(path).ok(),
            bytes: ends_at
                .as_deref()
                .filter(|real| real.is_file())
                .and_then(Bytes::of),
            ends_at,
        }
    }
}

/// The owner's launchers and install folders at a moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    launchers: BTreeMap<PathBuf, Launcher>,
    folders: BTreeMap<PathBuf, Option<BTreeSet<String>>>,
    files: BTreeMap<PathBuf, Option<String>>,
    /// The names of Claude Code's Keychain items; none where they were not listed.
    keychain: Option<BTreeSet<String>>,
}

/// `path` as a run says it: below `~` where it is below the owner's `home`.
fn shown(path: &Path, home: &Path) -> String {
    // A path that was followed through a link is spelled as the system spells it (macOS's
    // temporary folders are `/private/var/…`), the home as it was given.
    let real = fs::canonicalize(home).ok();
    match path.strip_prefix(home).ok().or_else(|| {
        real.as_deref()
            .and_then(|real| path.strip_prefix(real).ok())
    }) {
        Some(below) => format!("~/{}", below.display()),
        None => path.display().to_string(),
    }
}

impl Seen {
    /// What the launchers are, in a line, for a run that left them as they were: where each is
    /// and the first twelve digits of the hash of the file it ends at (`none` for one that is
    /// not there).
    pub fn summary(&self, home: &Path) -> String {
        let present = self
            .launchers
            .iter()
            .filter(|(_, launcher)| launcher.present)
            .map(|(path, launcher)| {
                let hash = launcher
                    .bytes
                    .as_ref()
                    .map_or("none", |bytes| &bytes.hash[..12.min(bytes.hash.len())]);
                format!("{} {hash}", shown(path, home))
            })
            .collect::<Vec<_>>();
        if present.is_empty() {
            "none is installed".to_owned()
        } else {
            present.join(", ")
        }
    }

    /// What differs between `self` (before the run) and `after`, in words, with the way back
    /// for a link that moved; nothing where nothing does. `home` is the owner's.
    pub fn changes_to(&self, after: &Self, home: &Path) -> Vec<String> {
        let mut found = Vec::new();
        for (path, was) in &self.launchers {
            let Some(now) = after.launchers.get(path) else {
                continue;
            };
            let name = shown(path, home);
            match (was.present, now.present) {
                (false, false) => continue,
                (true, false) => {
                    found.push(format!("{name} is gone"));
                    continue;
                }
                (false, true) => {
                    found.push(format!("{name} was made"));
                    continue;
                }
                (true, true) => {}
            }
            match (&was.link, &now.link) {
                (Some(old), Some(new)) if old != new => found.push(format!(
                    "{name} now points at {} (it pointed at {}); put it back with \
                     `ln -sfn {} {name}`",
                    new.display(),
                    old.display(),
                    old.display()
                )),
                (Some(old), None) => found.push(format!(
                    "{name} is a file now, and was a link to {}; put the link back with \
                     `ln -sfn {} {name}`",
                    old.display(),
                    old.display()
                )),
                (None, Some(new)) => {
                    found.push(format!(
                        "{name} is a link to {} now, and was a file",
                        new.display()
                    ));
                }
                _ => {}
            }
            if was.link == now.link && was.ends_at != now.ends_at {
                found.push(format!(
                    "{name} now ends at {} (it ended at {}): a link it goes through moved",
                    now.ends_at
                        .as_deref()
                        .map_or("nothing (it dangles)".to_owned(), |end| shown(end, home)),
                    was.ends_at
                        .as_deref()
                        .map_or("nothing".to_owned(), |end| shown(end, home)),
                ));
            } else if was.ends_at == now.ends_at && was.bytes != now.bytes {
                let end = now.ends_at.as_deref().unwrap_or(path);
                found.push(format!("{} is not as it was", shown(end, home)));
            }
        }
        for (folder, was) in &self.folders {
            let now = after.folders.get(folder).cloned().flatten();
            match (was, now) {
                (Some(was), Some(now)) => {
                    for gained in now.difference(was) {
                        found.push(format!("{} gained {gained}", shown(folder, home)));
                    }
                    for lost in was.difference(&now) {
                        found.push(format!("{} lost {lost}", shown(folder, home)));
                    }
                }
                (Some(_), None) => found.push(format!("{} is gone", shown(folder, home))),
                (None, Some(_)) => found.push(format!("{} was made", shown(folder, home))),
                (None, None) => {}
            }
        }
        for (file, was) in &self.files {
            if after.files.get(file).is_some_and(|now| now != was) {
                found.push(format!("{} is not as it was", shown(file, home)));
            }
        }
        found
    }
}

impl Seen {
    /// The Keychain items of Claude Code that `after` has and this look did not, in words; none
    /// where either look could not list the Keychain.
    pub fn keychain_gained(&self, after: &Self) -> Vec<String> {
        let (Some(was), Some(now)) = (&self.keychain, &after.keychain) else {
            return Vec::new();
        };
        now.difference(was)
            .map(|name| keychain::gained_words(name))
            .collect()
    }
}

/// The guard of one harness's launchers: a [`HarnessHome`] that sets no variable, looks at the
/// owner's launchers when the rig starts and again when it ends, and fails the rig that ends if
/// they differ (the owner's own harness could have written them as well, while the run went:
/// the words say it).
#[derive(Debug)]
pub struct Guard {
    harness: Harness,
    /// The owner's home, which a run says its paths below as `~`.
    home: PathBuf,
    watch: Watch,
    before: Mutex<Option<Seen>>,
}

impl Guard {
    /// The guard of `harness`, run as `command`, on the machine of the person whose home is
    /// `home`.
    pub fn new(harness: Harness, command: &Path, home: &Path) -> Self {
        let watch = Watch::of(harness, command, home);
        Self {
            harness,
            home: home.to_path_buf(),
            watch: if harness == Harness::Claude {
                watch.with_keychain(keychain::names)
            } else {
                watch
            },
            before: Mutex::new(None),
        }
    }
}

impl HarnessHome for Guard {
    /// No variable; the first call is when the launchers are first looked at, and a rig that
    /// starts again on the same guard keeps that look.
    fn vars(&self) -> Result<Vec<(String, String)>> {
        let mut before = self.before.lock().unwrap_or_else(PoisonError::into_inner);
        if before.is_none() {
            *before = Some(self.watch.look());
        }
        Ok(Vec::new())
    }

    fn described(&self) -> String {
        format!("{}'s launchers", self.harness.name())
    }

    fn finish(&self) -> Result {
        let before = self.before.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(before) = before.as_ref() else {
            return Ok(());
        };
        let after = self.watch.look();
        let changed = before.changes_to(&after, &self.home);
        let gained = before.keychain_gained(&after);
        if changed.is_empty() && gained.is_empty() {
            let keychain = before.keychain.as_ref().map_or_else(String::new, |names| {
                format!(
                    "; so is the Keychain ({} items of Claude Code)",
                    names.len()
                )
            });
            let _ = std::io::Write::write_fmt(
                &mut std::io::stderr(),
                format_args!(
                    "the owner's {} launchers are as they were: {}{keychain}\n",
                    self.harness.name(),
                    before.summary(&self.home)
                ),
            );
            return Ok(());
        }
        let mut words = String::new();
        if !changed.is_empty() {
            words.push_str(&said(self.harness, &changed));
        }
        if !gained.is_empty() {
            words.push_str(&said_of_keychain(&gained));
        }
        let _ = std::io::Write::write_fmt(&mut std::io::stderr(), format_args!("{words}\n"));
        Err(Error::Daemon(words))
    }
}

/// What the guard says when a launcher or an install folder of the owner's differs: loud, and
/// with what to do.
fn said(harness: Harness, changed: &[String]) -> String {
    let bar = "!".repeat(78);
    let mut said = format!(
        "\n{bar}\nTHE RUN CHANGED THE OWNER'S {} LAUNCHER, which a live run is never to touch \
         (or the owner's own {} updated itself while it ran: its link then reads a newer version \
         of the folder it was in). The kit does not put it back; do it by hand if it is wrong:\n",
        harness.name().to_uppercase(),
        harness.name(),
    );
    for change in changed {
        said.push_str(&format!("  - {change}\n"));
    }
    said.push_str(&bar);
    said
}

/// What the guard says when the Keychain gained an item of Claude Code (`gained`, one line each): loud,
/// and with what to do.
fn said_of_keychain(gained: &[String]) -> String {
    let bar = "!".repeat(78);
    let mut said = format!(
        "\n{bar}\nTHE RUN LEFT AN ITEM IN THE OWNER'S KEYCHAIN, which a live run is never to do (T-1102). A Claude \
         Code run on a config folder of its own keeps what it stores securely in the login Keychain under a name \
         made of the folder's path, and the folder goes with the run; a run of the kit starts Claude Code with \
         --strict-mcp-config so that it has no MCP server to keep the OAuth state of. The kit does not delete it; \
         do it by hand if it is the run's:\n"
    );
    for item in gained {
        said.push_str(&format!("  - {item}\n"));
    }
    said.push_str(&bar);
    said
}

#[cfg(test)]
mod tests;

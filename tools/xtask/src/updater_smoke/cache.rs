//! The folder the smoke keeps what it builds in, and what keeps a run from
//! sharing it with another: where it is, whose it is, who holds it, and what the
//! update kept in it was built from.
//!
//! Unless `--cache` names a folder it is the user's cache folder, a folder for each
//! checkout, never inside one: the release a run installs is exported into it and
//! built there, and Cargo reads every `.cargo/config.toml` above the folder it
//! builds in, the checkout's too, where both write the `xtask` alias (it was
//! doubled, and `cargo xtask stage` ran as `cargo xtask run … stage`). A folder for
//! each checkout, because the update is that checkout's app (a `--reuse` run takes
//! the one it finds) and two trees' runs at once would build in one place.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use sha2::{Digest, Sha256};

use super::bundle::{copy_bundle, remove_all};
use super::{files, Error, Result};
use crate::context::Context;
use crate::process::{self, Invocation};

/// The file in a default cache that names the checkout the folder is for: one
/// line, the checkout's root as the run found it (an absolute path), then a
/// newline. A folder whose checkout is gone (removed, moved or renamed: the
/// folder's name is made of its path) is one nothing will use again, which a
/// cleanup finds by this file.
pub const CHECKOUT: &str = ".checkout";

/// The file in a cache that a run holds a lock on from its start to its end.
const LOCK: &str = ".lock";

/// The file beside a kept update that says what it was built from.
const BUILT_FROM: &str = ".built-from";

/// The folder the builds are kept in unless `--cache` names one: the user's cache
/// folder (the smoke is macOS's), a folder of this checkout's in it, named for the
/// checkout and the first 8 hex of the SHA-256 of its path, so that two checkouts
/// of one name in two places have two.
pub fn default_cache(context: &Context) -> Result<PathBuf> {
    let home = context.env.path("HOME").ok_or_else(|| {
        Error::new("there is no home to keep the builds in: name a folder with --cache")
    })?;
    let name = context
        .root
        .file_name()
        .map_or_else(|| "checkout".into(), |name| name.to_string_lossy());
    let place = hex(&Sha256::digest(context.root.as_os_str().as_encoded_bytes())[..4]);
    Ok(home
        .join("Library")
        .join("Caches")
        .join("ConsensFlow")
        .join("updater-smoke")
        .join(format!("{name}-{place}")))
}

/// Writes [`CHECKOUT`] into `cache`: the checkout `root` that it is for.
pub fn mark_checkout(cache: &Path, root: &Path) -> Result {
    let mark = cache.join(CHECKOUT);
    fs::write(&mark, format!("{}\n", root.display())).map_err(files("write", &mark))
}

/// A run's hold on a cache, until it is dropped (or the process ends: the system
/// lets go of a lock with the process that held it, so none is left behind).
#[must_use = "a hold that is dropped is let go of at once: bind it for as long as the run needs the cache"]
#[derive(Debug)]
pub struct Hold {
    _file: File,
}

/// Takes the hold on `cache`, or says who has it. A second run in one cache would
/// take the first's exports away as it made them (an export that is not marked yet
/// is made again) and its kept apps as it copied them.
pub fn hold(cache: &Path) -> Result<Hold> {
    let path = cache.join(LOCK);
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(files("open", &path))?;
    match file.try_lock() {
        Ok(()) => {
            // Whose it is, for whoever is refused.
            let _ = file.set_len(0);
            let _ = writeln!(file, "{}", std::process::id());
            Ok(Hold { _file: file })
        }
        Err(TryLockError::WouldBlock) => {
            let holder = fs::read_to_string(&path)
                .ok()
                .and_then(|text| text.trim().parse::<u32>().ok())
                .map_or_else(String::new, |pid| format!(" (process {pid})"));
            Err(Error::new(format!(
                "another run of the smoke{holder} holds {}: wait for it to end, \
                 or give this run a folder of its own with --cache",
                cache.display()
            )))
        }
        Err(TryLockError::Error(cause)) => Err(Error::new(format!(
            "could not lock {}: {cause}",
            path.display()
        ))),
    }
}

/// What a checkout is, as far as a build of it can tell: the commit it is at, and
/// what is changed in it (the tracked files against the commit, and the files that
/// are in no commit and not ignored, by their contents).
#[derive(Debug, Clone, PartialEq, Eq)]
struct State {
    commit: String,
    /// A digest of the changes, none where there are none.
    changes: Option<String>,
}

impl State {
    /// The state of the checkout at `root`. A cache in the checkout that git does not
    /// ignore is not in it: what a build adds to the cache is not a change of the
    /// checkout, and the update is kept there.
    fn of(root: &Path, cache: Option<&Path>, env: &Env) -> Result<Self> {
        let commit = git(root, env, &["rev-parse", "HEAD"])?.trim().to_string();
        let diff = git(root, env, &["diff", "HEAD", "--binary"])?;
        let untracked = git(
            root,
            env,
            &["ls-files", "--others", "--exclude-standard", "-z"],
        )?;
        // A cache that is not there yet has nothing in it to leave out.
        let cache = cache.and_then(|cache| fs::canonicalize(cache).ok());
        let physical = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let untracked: Vec<&str> = untracked
            .split('\0')
            .filter(|path| !path.is_empty())
            .filter(|path| {
                !cache
                    .as_deref()
                    .is_some_and(|cache| physical.join(path).starts_with(cache))
            })
            .collect();
        let changes = (!diff.is_empty() || !untracked.is_empty()).then(|| {
            let mut digest = Sha256::new();
            digest.update(diff.as_bytes());
            for path in untracked {
                digest.update(b"\0untracked\0");
                digest.update(path.as_bytes());
                digest.update(b"\0");
                // A file that cannot be read is one that is there and nothing more.
                if let Ok(mut file) = File::open(root.join(path)) {
                    let _ = io::copy(&mut file, &mut digest);
                }
            }
            hex(&digest.finalize()[..6])
        });
        Ok(Self { commit, changes })
    }

    /// What is written beside the update.
    fn text(&self) -> String {
        format!(
            "commit {}\nchanges {}\n",
            self.commit,
            self.changes.as_deref().unwrap_or("none")
        )
    }

    /// The state that `text` (what [`State::text`] wrote) says, or none for any other.
    fn read(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        let commit = lines.next()?.strip_prefix("commit ")?.to_string();
        let changes = lines.next()?.strip_prefix("changes ")?;
        lines.next().is_none().then(|| Self {
            commit,
            changes: (changes != "none").then(|| changes.to_string()),
        })
    }

    fn said(&self) -> String {
        let commit: String = self.commit.chars().take(10).collect();
        match &self.changes {
            None => format!("commit {commit} with no changes"),
            Some(changes) => format!("commit {commit} with changes {changes}"),
        }
    }
}

/// What `git <args>` wrote in the checkout at `root`.
fn git(root: &Path, env: &Env, args: &[&str]) -> Result<String> {
    let ran = process::capture(
        &Invocation::new("git", root).args(args.iter().copied()),
        env,
    )?;
    ensure!(
        ran.code == 0,
        "git {} failed: {}",
        args.join(" "),
        ran.stderr.trim()
    );
    Ok(ran.stdout)
}

/// The cache an update is kept in: `<cache>/update/ConsensFlow.app`.
fn cache_of(update: &Path) -> Option<&Path> {
    update.parent()?.parent()
}

/// Keeps the update that `build` makes from the checkout at `root` at `update` (the
/// kept app, `<cache>/update/ConsensFlow.app`), and writes beside it what the
/// checkout was when the build started to read it. A state that cannot be read
/// leaves the update with no record of it, which a `--reuse` run says.
pub fn keep_update(
    root: &Path,
    update: &Path,
    env: &Env,
    build: impl FnOnce() -> Result<PathBuf>,
) -> Result<PathBuf> {
    let state = State::of(root, cache_of(update), env);
    let built = build()?;
    let mark = update.with_file_name(BUILT_FROM);
    remove_all(&mark)?;
    let kept = copy_bundle(&built, update, env)?;
    if let Ok(state) = state {
        fs::write(&mark, state.text()).map_err(files("write", &mark))?;
    }
    Ok(kept)
}

/// Why the update kept at `update` is not the checkout at `root` as it is, or none
/// where it is: its record is of another commit or of other changes, or there is
/// none to read.
pub fn older_update(update: &Path, root: &Path, env: &Env) -> Option<String> {
    let again = "build it again without --reuse";
    let mark = update.with_file_name(BUILT_FROM);
    let Ok(text) = fs::read_to_string(&mark) else {
        return Some(format!(
            "the kept update has no record of what it was built from: {again}"
        ));
    };
    let Some(then) = State::read(&text) else {
        return Some(format!(
            "the kept update's record of what it was built from ({}) is not one this xtask reads: {again}",
            mark.display()
        ));
    };
    let now = match State::of(root, cache_of(update), env) {
        Ok(now) => now,
        Err(cause) => {
            return Some(format!(
                "could not tell whether the kept update is of this checkout as it is: {cause}"
            ))
        }
    };
    (then != now).then(|| {
        format!(
            "the kept update is not of this checkout as it is: it was built from {}, \
             and the checkout is at {}: {again}",
            then.said(),
            now.said()
        )
    })
}

/// `bytes` as lower-case hex.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests;

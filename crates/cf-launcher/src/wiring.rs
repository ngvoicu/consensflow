//! What the launcher on this machine runs, whether it is still there, and
//! whether it is the copy asking, and the lines `doctor` says of it.
//!
//! Nothing ConsensFlow installs assumes a runtime on PATH: the launcher is
//! the one installed file that names a program at all, absolutely, so a
//! machine without Node still works. The cost is that moving or deleting
//! what provided it (from the app, its own bundle) breaks every `cf` the
//! skill teaches, so `cf doctor` says so rather than letting it fail one
//! command at a time.
//!
//! Whether it is the copy asking is the other half, and the one nothing
//! else reports: a launcher naming a program that still exists looks healthy
//! from every angle while running an older ConsensFlow. It happens whenever
//! a machine holds two, an app in /Applications and a build in a repo, and
//! the newer one is not the one on PATH. Only a copy that is not the
//! launcher's can notice, which is why this is asked by the app's own `cf`
//! and its page: `cf` itself is, by definition, whatever the launcher
//! started.
//!
//! What identifies the copy is the file the command runs, its entry. Node
//! named it by its runtime, the one absolute path of its own it had; a
//! native `cf` has no runtime, and its entry is itself: the old shape is the
//! asking copy's when its `cf.mjs` is the one beside the asking `cf`, the new
//! one when it names that `cf`.

use std::fs;
use std::path::Path;

use cf_base::env::Env;
use cf_base::js;
use cf_base::path;

use crate::install::found;
use crate::places::Places;
use crate::text::{runs, spelled, Runs};

/// Which shape the launcher is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// The old shape: a runtime and the `cf.mjs` it runs, which only a
    /// bundle that still holds both can answer. Repair makes it the new.
    Node,
    /// The new shape: the native `cf` of a bundle.
    Native,
}

/// What a launcher runs, as [`runtime`] reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wiring {
    pub shape: Shape,
    /// What the report names: the runtime of the old shape, the `cf` of the
    /// new one. As the file says it, not interpreted.
    pub runtime: String,
    /// The file the command actually runs, which is how a developer finds
    /// which COPY of ConsensFlow is on PATH: the `cf.mjs` of the old shape,
    /// the `cf` of the new one. As the file says it, not interpreted: what a
    /// path means is the caller's business.
    pub entry: String,
    /// Whether the program the report names is still there.
    pub exists: bool,
    /// Whether the command runs the copy asking.
    pub mine: bool,
    /// Whether the entry sits inside a bundle with the installed release's
    /// identity: the live app, which a development command must never write
    /// into.
    pub live: bool,
}

/// What the launcher installed in `places` runs, as read by the copy whose
/// native `cf` is `cf`: none when ours is not installed, or says nothing of
/// the kind. A file that cannot be read is the failure.
pub fn runtime(env: &Env, cf: &Path, places: &Places) -> Result<Option<Wiring>, String> {
    let Some((_, text)) = found(env, places)? else {
        return Ok(None);
    };
    let windows = env.on_windows();
    Ok(runs(&text, windows).map(|runs| match runs {
        Runs::Node { runtime, entry } => Wiring {
            shape: Shape::Node,
            exists: Path::new(&runtime).exists(),
            mine: entry == spelled(&cf.with_file_name("cf.mjs"), windows),
            live: inside_live_bundle(&entry),
            runtime,
            entry,
        },
        Runs::Native { cf: named } => Wiring {
            shape: Shape::Native,
            exists: Path::new(&named).exists(),
            mine: named == spelled(cf, windows),
            live: inside_live_bundle(&named),
            runtime: named.clone(),
            entry: named,
        },
    }))
}

impl Wiring {
    /// The line `cf doctor` says of it: three states, not two. A program that
    /// exists but belongs to ANOTHER ConsensFlow looks healthy from every
    /// count on the page, while every `cf` the skill teaches runs the other
    /// one's code, which is what a second install (an app beside a repo
    /// build) leaves behind.
    pub fn report(&self) -> String {
        let (label, remedy) = match self.shape {
            Shape::Node => (
                "runtime:",
                "Reinstall from the app to point the wiring at its runtime.",
            ),
            Shape::Native => (
                "command:",
                "Reinstall from the app to point the command at its cf.",
            ),
        };
        let named = &self.runtime;
        if !self.exists {
            format!("{label:<14}{named} — MISSING. {remedy}")
        } else if self.mine {
            format!("{label:<14}{named}")
        } else {
            format!(
                "{label:<14}{named} — another ConsensFlow. `cf` runs that one; `cf setup` from this one claims the command."
            )
        }
    }
}

/// Whether a CLI entry sits inside a bundle with the installed release's
/// identity: the live app, which a development command must never write
/// into. `.../X.app/Contents/Resources/cli/bin/cf` is read through
/// `.../X.app/Contents/Info.plist`.
fn inside_live_bundle(entry: &str) -> bool {
    const KEY: &str = "<key>CFBundleIdentifier</key>";
    const IDENTITY: &str = "<string>dev.ngvoicu.consensflow</string>";
    let contents = (0..4).fold(entry.to_owned(), |inside, _| dirname(&inside));
    let plist = path::join(&[&contents, "Info.plist"]);
    let Ok(bytes) = fs::read(plist) else {
        return false;
    };
    let text = String::from_utf8_lossy(&bytes);
    text.match_indices(KEY).any(|(at, _)| {
        text.get(at + KEY.len()..)
            .is_some_and(|rest| rest.trim_start_matches(js::is_space).starts_with(IDENTITY))
    })
}

/// `path.dirname`: the path without its last segment, `.` for none and the
/// root for the root. The bundle is macOS-shaped, so the Windows reading
/// takes either separator and no more: no plist is found there.
fn dirname(text: &str) -> String {
    let separator = |byte: u8| byte == b'/' || (cfg!(windows) && byte == b'\\');
    let bytes = text.as_bytes();
    let Some(&first) = bytes.first() else {
        return ".".to_owned();
    };
    let mut end = None;
    let mut matched_separator = true;
    for at in (1..bytes.len()).rev() {
        if separator(bytes[at]) {
            if !matched_separator {
                end = Some(at);
                break;
            }
        } else {
            matched_separator = false;
        }
    }
    match end {
        None if separator(first) => text.get(..1).unwrap_or_default().to_owned(),
        None => ".".to_owned(),
        Some(1) if separator(first) => text.get(..2).unwrap_or_default().to_owned(),
        Some(at) => text.get(..at).unwrap_or_default().to_owned(),
    }
}

#[cfg(test)]
mod tests;

//! Specmint, the skill a chief plans big work with (`cf_proto::specs`): where a
//! chief's window finds the one variant its project chose, and ConsensFlow's
//! own copy of it for a window whose harness has none.
//!
//! A harness that already has the chosen variant installed, by the human, where
//! the harness looks for skills (each harness's own places are its module's:
//! `global_specmint` in its `paths`) uses that copy as it is: nothing is
//! written and nothing is registered, it is the harness's already. The copy
//! counts when its folder holds a `SKILL.md` whose frontmatter `name` is the
//! variant's skill (`specmint-tdd`), whatever the folder is called, and a link
//! to it is followed. An installed copy of any other variant is no copy of
//! this one: it is never touched, and the chief is told which to use.
//!
//! Otherwise ConsensFlow keeps its own copy of the variant: one for each
//! harness and variant, in `integrations/specmint/<harness>/<variant>-<commit>`
//! (the upstream commit the build vendors the variant at, from its manifest) and
//! in the layout Claude loads a skill from (`.claude/skills/specmint-<variant>`).
//! No launch's folder holds it, so its place is the same at every launch: the
//! chief's role text, which names it, is the same from one launch to the next,
//! and a conversation that is resumed is told nothing of its skill that it was
//! not told before, where a path with a launch's id in it said it again at
//! every Resume. It outlasts its window, and neither a window's end nor a
//! start's sweep removes it (`launch_files`). It is written as a window is
//! prepared, a file at a time and only where the file is not there or is not
//! what this build ships, each to a name of the launch's own beside it and
//! renamed into place, since windows of other launches read the same files: none
//! of them finds a file half written. A build that vendors another commit writes
//! a folder of its own and leaves the old one to the windows that still read it.
//!
//! Each harness loads it as it loads any skill: Claude is given its root as one
//! more folder to read (`--add-dir`), OpenCode its skills folder as one more of
//! its `skills.paths`, Pi the file as a second skill, and Codex and Devin read
//! it by its path, which the chief's role text names.

mod bundled;
mod files;
#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::file::{make_folder, rename, rm_force, write_file, FileError, Mkdir};
use cf_base::path;
use cf_proto::agents::Harness;
use cf_proto::specs::Variant;

pub use bundled::bundled_specmint;

use crate::contract::{Launch, LaunchId, Skill, Specs};
use crate::shared::launch_files::{harness_folder, integrations_path, CODEX_FOLDER};
use crate::shared::role::SKILLS;

/// How much of a `SKILL.md` is read to find its name: its frontmatter is first.
const FRONTMATTER_BYTES: u64 = 4096;

/// The folder of `integrations` ConsensFlow's copies are kept in: no harness's,
/// whose folders hold launches' and are read as such.
const KEPT: &str = "specmint";

/// The `SKILL.md` of the skill named `variant`'s, among the skills of the
/// folders `roots` (a harness's own places for the skills its user installed, in
/// the order it reads them), by the first that has one. None where no folder
/// holds one: a folder or a root that cannot be read has none to give, and a
/// link is followed as the harness follows it.
pub(crate) fn installed(roots: &[String], variant: Variant) -> Option<String> {
    roots.iter().find_map(|root| {
        let mut skills: Vec<String> = fs::read_dir(root)
            .ok()?
            .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
            .collect();
        skills.sort();
        skills.into_iter().find_map(|skill| {
            let file = path::join(&[root, &skill, "SKILL.md"]);
            (name_of(Path::new(&file))? == variant.skill_name()).then_some(file)
        })
    })
}

/// The `name` the frontmatter of the skill file at `file` gives it.
fn name_of(file: &Path) -> Option<String> {
    let mut head = Vec::new();
    File::open(file)
        .ok()?
        .take(FRONTMATTER_BYTES)
        .read_to_end(&mut head)
        .ok()?;
    frontmatter_name(&String::from_utf8_lossy(&head)).map(str::to_owned)
}

/// The `name` the frontmatter at the start of `text` gives, where there is one:
/// a block between `---` lines, at the very start.
fn frontmatter_name(text: &str) -> Option<&str> {
    let mut lines = text.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    for line in lines {
        if line.trim_end() == "---" {
            return None;
        }
        if let Some(name) = line.strip_prefix("name:") {
            return Some(name.trim().trim_matches(['"', '\'']));
        }
    }
    None
}

/// ConsensFlow's copy of a variant for one harness: where it is, whether or not
/// it is written yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnCopy {
    /// The folder the copy's skills are under (`.claude/skills`), which a harness
    /// that reads the skills of the folders it is given (Claude) is given.
    pub(crate) root: String,
    /// The skill's own folder: its `SKILL.md`, its licence and the files it names.
    folder: String,
}

impl OwnCopy {
    /// The copy `variant` has for `harness`, at the place that is the same for
    /// every launch, or the failure of an environment that names no ConsensFlow
    /// folder.
    fn of(harness: Harness, env: &Env, variant: Variant) -> Result<Self, String> {
        Self::at(harness, env, variant, &files::commit_of(variant)?)
    }

    /// The copy of `variant` as it is vendored at `commit`, for `harness`.
    fn at(harness: Harness, env: &Env, variant: Variant, commit: &str) -> Result<Self, String> {
        let root = integrations_path(
            env,
            &[
                KEPT,
                harness_folder(harness).unwrap_or(CODEX_FOLDER),
                &format!("{}-{commit}", variant.as_str()),
            ],
        )?;
        let folder = path::join(&[&root, SKILLS[1], SKILLS[2], variant.skill_name()]);
        Ok(Self { root, folder })
    }

    /// The skill's `SKILL.md`, which the chief's role text names.
    pub(crate) fn file(&self) -> String {
        path::join(&[&self.folder, "SKILL.md"])
    }

    /// The folder of skills the copy is in, which a harness that is told which
    /// folders to read skills from (OpenCode) is given.
    pub(crate) fn skills(&self) -> String {
        path::join(&[&self.root, SKILLS[1], SKILLS[2]])
    }
}

/// Where the window of `harness` finds the skill of `variant`: the copy the
/// harness found installed (`global`, its own `global_specmint`) or, where it
/// found none, the place ConsensFlow's own is written ([`write_own`]), which is
/// the same for every launch.
pub(crate) fn place(
    harness: Harness,
    env: &Env,
    variant: Variant,
    global: Option<String>,
) -> Result<Skill, String> {
    match global {
        Some(file) => Ok(Skill::Global(file)),
        None => Ok(Skill::Own(OwnCopy::of(harness, env, variant)?.file())),
    }
}

/// Writes ConsensFlow's own copy of the skill where the launch's chief is given
/// one, and says where it is: at the place the chief's text names. A file that is
/// there and is what this build ships is left as it is, one that is missing or
/// differs is written ([`put_in_place`]), and what else is in the folder is none
/// of its business. None where there is nothing to write: no specs, or the
/// harness has the variant installed.
pub(crate) fn write_own(
    harness: Harness,
    env: &Env,
    launch: &Launch,
) -> Result<Option<OwnCopy>, String> {
    let Some(Specs {
        variant,
        skill: Skill::Own(_),
    }) = launch.specs
    else {
        return Ok(None);
    };
    let copy = OwnCopy::of(harness, env, variant)?;
    let said = |error: FileError| error.to_string();
    for (name, bytes) in files::of(variant) {
        let file = path::join(&[&copy.folder, name]);
        let file = Path::new(&file);
        if fs::read(file).is_ok_and(|held| held == *bytes) {
            continue;
        }
        if let Some(above) = file.parent() {
            make_folder(above, 0o700, Mkdir::Promise).map_err(said)?;
        }
        put_in_place(file, bytes, launch.id).map_err(said)?;
    }
    Ok(Some(copy))
}

/// Writes `bytes` as `file` whole or not at all: to a name beside it that is this
/// launch's own (`<file>.<launch id>.tmp`, which no other launch's write is
/// made to), then renamed over it. A window of another launch that reads the
/// skill meanwhile, the copy being shared, finds the file as it was or as it is,
/// never half of it, and a write cut short (a crash, a full disk) leaves what was
/// there. The temporary is removed where the write or the rename fails. On
/// Windows a rename over a file another process holds open, without sharing it
/// for deleting, is refused: the launch says so, and the next one tries again.
fn put_in_place(file: &Path, bytes: &[u8], launch: &LaunchId) -> Result<(), FileError> {
    let mut beside = OsString::from(file.as_os_str());
    beside.push(format!(".{}.tmp", launch.as_str()));
    let beside = PathBuf::from(beside);
    write_file(&beside, bytes, 0o600)
        .and_then(|()| rename(&beside, file))
        .inspect_err(|_| {
            // What was written is no file of the skill's, whatever came of it.
            let _ = rm_force(&beside);
        })
}

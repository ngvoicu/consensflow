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
//! Otherwise ConsensFlow writes the variant into the launch's own folder, next
//! to the role's skill and in its layout (`role/.claude/skills/specmint-<variant>`),
//! as the window is prepared: Claude, which is given the role's folder, and
//! OpenCode, which is told to read its skills folder, load it with the role, Pi
//! is given it as a second skill, Codex and Devin read it by its path, which
//! the chief's role text names; and it goes with the rest of the launch's files.

mod files;
#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use cf_base::env::Env;
use cf_base::file::{make_folder, write_file, Mkdir};
use cf_base::path;
use cf_proto::agents::Harness;
use cf_proto::specs::Variant;

use crate::contract::{Launch, LaunchId, Skill, Specs};
use crate::shared::launch_files::{harness_folder, launch_folder, CODEX_FOLDER};
use crate::shared::role::SKILLS;

/// How much of a `SKILL.md` is read to find its name: its frontmatter is first.
const FRONTMATTER_BYTES: u64 = 4096;

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

/// The folder of the skill ConsensFlow writes for `launch` of `harness`, beside
/// the role's skill.
fn own_folder(
    harness: Harness,
    env: &Env,
    launch: &LaunchId,
    variant: Variant,
) -> Result<String, String> {
    let folder = harness_folder(harness).unwrap_or(CODEX_FOLDER);
    Ok(path::join(&[
        &launch_folder(folder, env, launch)?,
        SKILLS[0],
        SKILLS[1],
        SKILLS[2],
        variant.skill_name(),
    ]))
}

/// Where the window of `harness` for `launch` finds the skill of `variant`:
/// the copy the harness found installed (`global`, its own `global_specmint`)
/// or, where it found none, the place ConsensFlow's own will be written
/// ([`write_own`]).
pub(crate) fn place(
    harness: Harness,
    env: &Env,
    launch: &LaunchId,
    variant: Variant,
    global: Option<String>,
) -> Result<Skill, String> {
    match global {
        Some(file) => Ok(Skill::Global(file)),
        None => Ok(Skill::Own(path::join(&[
            &own_folder(harness, env, launch, variant)?,
            "SKILL.md",
        ]))),
    }
}

/// Writes ConsensFlow's own copy of the skill beside the role's, where the
/// launch's chief is given one: the path of its `SKILL.md`. None where there
/// is nothing to write: no specs, or the harness has the variant installed.
pub(crate) fn write_own(
    harness: Harness,
    env: &Env,
    launch: &Launch,
) -> Result<Option<String>, String> {
    let Some(Specs {
        variant,
        skill: Skill::Own(_),
    }) = launch.specs
    else {
        return Ok(None);
    };
    let folder = own_folder(harness, env, launch.id, variant)?;
    let said = |error: cf_base::file::FileError| error.to_string();
    for (name, bytes) in files::of(variant) {
        let file = path::join(&[&folder, name]);
        let file = Path::new(&file);
        if let Some(above) = file.parent() {
            make_folder(above, 0o700, Mkdir::Promise).map_err(said)?;
        }
        write_file(file, bytes, 0o600).map_err(said)?;
    }
    Ok(Some(path::join(&[&folder, "SKILL.md"])))
}

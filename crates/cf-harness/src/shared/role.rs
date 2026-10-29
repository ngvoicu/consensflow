//! A window's role text, written where its harness loads it. Each launch writes
//! its own, beside the rest of its files, and they go with it: a chief's text
//! names its project's staff, and a shared file let a chief read another
//! project's when two of them opened together. Role documents live outside
//! every folder a harness discovers skills in by itself. Each harness's module
//! says how its window loads the file; Codex's is no skill, and its module writes
//! it.

use std::path::Path;

use cf_base::env::Env;
use cf_base::file::{make_folder, write_file, Mkdir};
use cf_base::path;
use cf_proto::agents::Harness;

use crate::contract::LaunchId;
use crate::shared::launch_files::{harness_folder, launch_folder};

/// Where a launch's skills are written, under its folder: the layout Claude
/// loads a skill from (`.claude/skills/<name>/SKILL.md`), in a folder of the
/// role's own, where the role's skill is written. ConsensFlow's copy of the
/// Specmint skill of a chief given specs is in the same layout
/// ([`super::specmint`]), under a folder that is no launch's.
pub(crate) const SKILLS: [&str; 3] = ["role", ".claude", "skills"];

/// Where a role was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RoleFile {
    /// The folder of the launch's role.
    pub(crate) root: String,
    /// The role's own text (`SKILL.md`), in the skills folder of the role's,
    /// as Claude lays one out.
    pub(crate) file: String,
}

/// Writes `content`, the text of `harness`'s window as `role`, in its
/// launch's own folder, made private to this user.
pub(crate) fn write_role(
    harness: Harness,
    role: &str,
    env: &Env,
    launch: &LaunchId,
    content: &str,
) -> Result<RoleFile, String> {
    if content.is_empty() {
        return Err(format!("the {role} window needs its role text"));
    }
    let folder = harness_folder(harness)
        .ok_or_else(|| format!("No {role} role is available for {}", harness.kind()))?;
    let root = path::join(&[&launch_folder(folder, env, launch)?, SKILLS[0]]);
    let skills = path::join(&[&root, SKILLS[1], SKILLS[2]]);
    let skill = path::join(&[&skills, &format!("consensflow-{role}")]);
    let file = path::join(&[&skill, "SKILL.md"]);
    let said = |error: cf_base::file::FileError| error.to_string();
    make_folder(Path::new(&skill), 0o700, Mkdir::Promise).map_err(said)?;
    write_file(Path::new(&file), content.as_bytes(), 0o600).map_err(said)?;
    Ok(RoleFile { root, file })
}

#[cfg(test)]
mod tests;

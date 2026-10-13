//! What a chief is told of its project's specs, beside its role: a short
//! section of its own, added to its text only where the project has specs on.
//! The chief's role (`skill/core/chief.md`) is the same one a project without
//! specs has, and such a project is told nothing of them: its text is what it
//! was, to the byte.
//!
//! The section names the one Specmint skill the chief plans with and where its
//! `SKILL.md` is (the copy the harness has installed, or ConsensFlow's own: a
//! file every harness can read), the folder the specs are kept in, and what
//! Specmint's workflow leaves to the board, where the chief's role has it hand
//! every change and every piece of research to the staff.

use cf_proto::specs::Variant;

/// What a chief's text says of its project's specs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectSpecs<'a> {
    /// The one variant of Specmint it plans with.
    pub variant: Variant,
    /// Where the specs are kept, relative to the project's folder, in slashes.
    pub folder: &'a str,
    /// The path of the skill's `SKILL.md`.
    pub skill: &'a str,
}

impl ProjectSpecs<'_> {
    /// The section, which ends in a line break, for a chief's text.
    pub(super) fn section(&self) -> String {
        let Self {
            variant,
            folder,
            skill,
        } = self;
        let name = variant.skill_name();
        // A shell and a file tool on Windows both take a slash.
        let skill = if cfg!(windows) {
            skill.replace('\\', "/")
        } else {
            (*skill).to_owned()
        };
        format!(
            "\n## This project's specs\n\
             \n\
             Specs are on in this project. Plan work that is big enough for phases as a spec, with the Specmint skill `{name}`: its SKILL.md is at {skill}, and the files it names are beside it; read it before you plan. Where Specmint says `.specs`, it means `{folder}`: your specs and their registry are there.\n\
             \n\
             Use a spec when the human asks for one, or when an ask is big enough that its work comes in phases. A small ask is no spec: it goes straight on the board as a task.\n\
             \n\
             Writing in `{folder}` is your own work: the one change in this project that is yours. Everything Specmint calls research, a subagent or implementation goes on the board as tasks for the staff, and you do none of it yourself or in a subagent.\n\
             \n\
             From a spec, put its tasks on the board in phase order: each task of a later phase has `--needs` the tasks of the phase before it, and each brief names the spec item it comes from and carries all its worker needs, since a member never reads the spec. After you accept a task, mark the spec's progress as the skill says: its checkbox in SPEC.md and its count in the registry.\n"
        )
    }
}

#[cfg(test)]
mod tests;

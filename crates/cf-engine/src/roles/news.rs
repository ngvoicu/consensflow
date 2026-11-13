//! What a chief that is running is told when a setting its role is built from
//! changes under its window: one note, the words of the change and the section
//! of its role that the change rewrote, never the whole role. The sections are the
//! chief's specs (on or off, the folder they are kept in, the Specmint variant:
//! [`ProjectSpecs`]) and the project's rules for its staff
//! ([`with_staff_rules`](super::with_staff_rules)). What else its role is made of
//! is no setting to tell: the staff table is said "as of your launch" and
//! `cf staff` shows it as it is, and the work tiers, the window's `cf` and the skill's
//! place change with a release or a launch, never under a window.
//!
//! The note is context, not a task, as the note that replaces a whole role is
//! ([`OPENING`](crate::role_change::OPENING)): the window says in one line that it has
//! it, and waits for the next message.

use cf_proto::specs::SpecsSetting;

use super::{with_staff_rules, ProjectSpecs};

/// What a note that tells a running chief of a change ends with.
pub(crate) const CONTEXT: &str = "This is context, not a task: say in one short line that you have it, and wait for the next message.";

/// The change the human made to a running chief's role, in the words it is told.
/// `specs` is what its specs were and are, and `rules` the project's rules for its
/// staff as they were and are. A part that did not change is left out; none changed,
/// none is told.
pub(crate) struct Change<'a> {
    pub(crate) specs: Option<(Option<&'a SpecsSetting>, Option<&'a ProjectSpecs<'a>>)>,
    pub(crate) rules: Option<(Option<&'a str>, Option<&'a str>)>,
}

impl Change<'_> {
    /// The note, or none where nothing changed.
    pub(crate) fn note(&self) -> Option<String> {
        self.note_for(false)
    }

    pub(crate) fn note_for(&self, own_hands: bool) -> Option<String> {
        let mut parts = Vec::new();
        if let Some((was, now)) = self.specs {
            parts.push(specs_said(was, now, own_hands));
        }
        if let Some((was, now)) = self.rules {
            parts.push(rules_said(was, now));
        }
        if parts.is_empty() {
            return None;
        }
        Some(format!("{}\n\n{CONTEXT}", parts.join("\n")))
    }
}

/// What the human changed of the specs, and the section the chief reads from now on.
fn specs_said(
    was: Option<&SpecsSetting>,
    now: Option<&ProjectSpecs<'_>>,
    own_hands: bool,
) -> String {
    let section = |now: &ProjectSpecs<'_>| {
        if own_hands {
            now.section_for(true)
        } else {
            now.section()
        }
    };
    match (was, now) {
        (None, Some(now)) => format!(
            "Specs are now on in this project: the human turned them on while you work. This is now a section of your instructions:\n{}",
            section(now)
        ),
        (Some(was), None) if own_hands => format!(
            "Specs are now off in this project: the human turned them off while you work. Do new work yourself unless the human explicitly asks for it on the board; the specs already in `{}` stay. The section of your instructions on this project's specs no longer applies.\n",
            was.folder
        ),
        (Some(was), None) => format!(
            "Specs are now off in this project: the human turned them off while you work. Plan new work as tasks, not specs; the specs already in `{}` stay. The section of your instructions on this project's specs no longer applies.\n",
            was.folder
        ),
        (Some(was), Some(now)) => {
            let mut changed = Vec::new();
            if was.folder != now.folder {
                changed.push(format!(
                    "the specs are now kept in `{}`, not `{}`",
                    now.folder, was.folder
                ));
            }
            if was.variant != now.variant {
                changed.push(format!(
                    "the Specmint skill is now `{}`, not `{}`",
                    now.variant.skill_name(),
                    was.variant.skill_name()
                ));
            }
            let stay = if was.folder == now.folder {
                String::new()
            } else {
                format!(" The specs already in `{}` stay where they are.", was.folder)
            };
            format!(
                "The human changed this project's specs while you work: {}.{stay} This section replaces the one on specs in your instructions; follow it from now on:\n{}",
                changed.join(", and "),
                section(now)
            )
        }
        // Nothing was told and nothing is: no change.
        (None, None) => String::new(),
    }
}

/// The changed parts of the role, without the staff table or the person's rules.
pub(crate) fn own_hands_said(
    on: bool,
    instructions: &str,
    specs: Option<&ProjectSpecs<'_>>,
) -> String {
    let intro = if on {
        "The person turned \"Do with your own hands\" on: do the work yourself in the project's folder; put work on the board only when the person explicitly asks."
    } else {
        "The person turned \"Do with your own hands\" off: return to your ordinary chief role and put work on the board as your instructions say."
    };
    let duty = instructions
        .split_once("## What you do\n")
        .and_then(|(_, after)| {
            after
                .split_once("\n## What you never do\n")
                .map(|(before, _)| before)
        })
        .unwrap_or("");
    let board = instructions
        .split_once("## What you never do\n")
        .and_then(|(_, after)| {
            after
                .split_once("\n## The staff\n")
                .map(|(before, _)| before)
        })
        .unwrap_or("");
    let specs = specs.map_or_else(String::new, |specs| specs.section_for(on));
    format!("{intro}\n\nThis replaces the earlier directions on how you work and use the board:\n\n## What you do\n{duty}\n\n## What you never do\n{board}\n{specs}")
}

/// What the human changed of the project's rules for its staff.
fn rules_said(was: Option<&str>, now: Option<&str>) -> String {
    match now {
        Some(now) => format!(
            "The human {} the rules for your staff while you work. Windows that open from now on are given them; one already open keeps its rules. This replaces the section of your instructions on them:\n{}",
            if was.is_some() { "changed" } else { "wrote" },
            with_staff_rules(String::new(), "chief", Some(now))
        ),
        None => "The human took away the rules for your staff while you work. Windows that open from now on are given none; one already open keeps its rules. The section of your instructions on them no longer applies.\n".to_owned(),
    }
}

#[cfg(test)]
mod tests;

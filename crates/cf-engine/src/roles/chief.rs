//! The chief's texts. The ordinary one is `skill/core/chief.md`, to the byte. With the
//! project's "Do with your own hands" switch on the chief is given a short text instead
//! (`skill/core/chief-own-hands.md`), and the board's guide, which it reads on demand
//! (`cf guide`), is `chief.md` again, so that there is one text of the board and the two
//! cannot drift: the guide is `chief.md` with its four marked parts (the opening, the
//! duty, what it never does, and the description of its frontmatter) said for a chief that
//! works with its own hands and uses the board when the human asks.

use super::{member, RoleError};

const CHIEF: &str = include_str!("../../../../skill/core/chief.md");
const OWN_HANDS: &str = include_str!("../../../../skill/core/chief-own-hands.md");

/// A part's name and the markers around the ordinary wording in `chief.md`.
const PARTS: [(&str, &str, &str); 4] = [
    (
        "description",
        "# own_hands: description\n",
        "# own_hands: end_description\n",
    ),
    (
        "opening",
        "<!-- own_hands: opening -->\n",
        "<!-- own_hands: end_opening -->\n",
    ),
    (
        "duty",
        "<!-- own_hands: duty -->\n",
        "<!-- own_hands: end_duty -->\n",
    ),
    (
        "never",
        "<!-- own_hands: never -->\n",
        "<!-- own_hands: end_never -->\n",
    ),
];

/// The chief's text with its slots (`{{cf}}` and the rest) still in it: the ordinary one,
/// or the short one of a chief that does the work with its own hands.
pub(super) fn text(own_hands: bool) -> Result<String, RoleError> {
    if own_hands {
        return Ok(format!("{}\n", own_part("own_hands", "text")?));
    }
    assembled(|_| None)
}

/// The board's guide, with its slots still in it: the ordinary text with its four marked
/// parts said for a chief that uses the board when the human asks, and no frontmatter
/// (it is read by a command, not loaded as a skill).
pub(super) fn guide() -> Result<String, RoleError> {
    let whole = assembled(|name| (name != "description").then_some(name))?;
    let missing = || RoleError::MissingChiefPart("frontmatter".to_owned());
    let (_, body) = whole
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n\n"))
        .ok_or_else(missing)?;
    Ok(body.to_owned())
}

/// `chief.md` with its marked parts replaced: each part `guided` names comes from the
/// guide's own, the others stay as `chief.md` has them.
fn assembled(guided: impl Fn(&str) -> Option<&str>) -> Result<String, RoleError> {
    let mut chief = CHIEF.to_owned();
    for (name, start, end) in PARTS {
        let missing = || RoleError::MissingChiefPart(name.to_owned());
        let (before, marked) = chief.split_once(start).ok_or_else(missing)?;
        let (ordinary, after) = marked.split_once(end).ok_or_else(missing)?;
        let chosen = match guided(name) {
            Some(name) => own_part("guide", name)?.to_owned(),
            None => ordinary.trim_end_matches('\n').to_owned(),
        };
        chief = format!("{before}{chosen}\n{after}");
    }
    Ok(chief)
}

/// One part of `chief-own-hands.md`: its text, the end of its last line not included.
fn own_part(role: &str, slot: &str) -> Result<&'static str, RoleError> {
    let (_, parts) = member::split(OWN_HANDS);
    parts
        .into_iter()
        .find(|(part_role, part_slot, _)| *part_role == role && *part_slot == slot)
        .map(|(_, _, part)| part.trim_end_matches('\n'))
        .ok_or_else(|| RoleError::MissingChiefPart(format!("{role}: {slot}")))
}

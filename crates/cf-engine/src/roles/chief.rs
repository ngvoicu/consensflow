//! The chief's one text, with just the marked duties swapped for own hands.

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

/// The one chief text, its markers removed and the chosen parts in their places.
pub(super) fn text(own_hands: bool) -> Result<String, RoleError> {
    let mut chief = CHIEF.to_owned();
    for (name, start, end) in PARTS {
        let missing = || RoleError::MissingChiefPart(name.to_owned());
        let (before, marked) = chief.split_once(start).ok_or_else(missing)?;
        let (_, after) = marked.split_once(end).ok_or_else(missing)?;
        let chosen = format!("{}\n", part(own_hands, name)?);
        chief = format!("{before}{chosen}{after}");
    }
    Ok(chief)
}

/// One changed part, from the same source the chief's text uses.
pub(super) fn part(own_hands: bool, name: &str) -> Result<&'static str, RoleError> {
    let missing = || RoleError::MissingChiefPart(name.to_owned());
    let (_, start, end) = PARTS
        .iter()
        .find(|(part, _, _)| *part == name)
        .ok_or_else(missing)?;
    let selected = if own_hands {
        let (_, own_parts) = member::split(OWN_HANDS);
        own_parts
            .into_iter()
            .find(|(role, slot, _)| *role == "own_hands" && *slot == name)
            .map(|(_, _, part)| part)
            .ok_or_else(missing)?
    } else {
        CHIEF
            .split_once(*start)
            .and_then(|(_, rest)| rest.split_once(*end))
            .map(|(part, _)| part)
            .ok_or_else(missing)?
    };
    Ok(selected.trim_end_matches('\n'))
}

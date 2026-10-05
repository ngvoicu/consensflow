//! A member's instructions: the shared text of `skill/core/staff.md` with
//! each `{{slot}}` filled from the part under `<!-- role: slot -->`.
//!
//! JavaScript split the file with `/^<!-- (\w+): (\w+) -->\n/m` and filled the
//! slots with `/\{\{(\w+)\}\}/g`, `\w` being ASCII. Both are read here by
//! hand, for the file's own line ends: it holds no CR, U+2028 or U+2029,
//! which JavaScript's `^` would also take for the start of a line.

use std::collections::HashMap;

use super::RoleError;

/// `skill/core/staff.md`.
const STAFF: &str = include_str!("../../../../skill/core/staff.md");

/// `role`'s window instructions, from the shared text of `staff.md`.
pub(super) fn text(role: &str) -> Result<String, RoleError> {
    filled(STAFF, role)
}

fn filled(staff: &str, role: &str) -> Result<String, RoleError> {
    let (head, parts) = split(staff);
    let mut own = HashMap::from([("role", role.to_owned())]);
    for (part_role, slot, text) in parts {
        if part_role == role {
            own.insert(slot, text.trim_end_matches('\n').to_owned());
        }
    }
    // The comment that opens the file says what it is for, and is no text.
    let head = head
        .strip_prefix("<!--")
        .and_then(|rest| rest.split_once("-->\n"))
        .map_or(head, |(_, rest)| rest);
    let head = if head.ends_with('\n') {
        format!("{}\n", head.trim_end_matches('\n'))
    } else {
        head.to_owned()
    };
    fill(&head, |slot| {
        own.get(slot)
            .map(String::as_str)
            .ok_or_else(|| RoleError::MissingSlot {
                role: role.to_owned(),
                slot: slot.to_owned(),
            })
    })
}

/// The text before the first marker, and each marker's role and slot with the
/// text after it, up to the next marker.
fn split(staff: &str) -> (&str, Vec<(&str, &str, &str)>) {
    let mut markers = Vec::new();
    let mut at = 0;
    for line in staff.split_inclusive('\n') {
        if let Some((role, slot)) = marker(line) {
            markers.push((at, at + line.len(), role, slot));
        }
        at += line.len();
    }
    let head = markers.first().map_or(staff, |first| &staff[..first.0]);
    let parts = markers
        .iter()
        .enumerate()
        .map(|(number, &(_, end, role, slot))| {
            let next = markers.get(number + 1).map_or(staff.len(), |next| next.0);
            (role, slot, &staff[end..next])
        })
        .collect();
    (head, parts)
}

/// The role and slot a marker line names, `<!-- worker: title -->` and its
/// line end.
fn marker(line: &str) -> Option<(&str, &str)> {
    let inside = line.strip_prefix("<!-- ")?.strip_suffix(" -->\n")?;
    let (role, slot) = inside.split_once(": ")?;
    (is_word(role) && is_word(slot)).then_some((role, slot))
}

fn is_word(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// `text` with each `{{word}}` replaced by what `slot` says of the word.
fn fill<'a>(
    text: &str,
    slot: impl Fn(&str) -> Result<&'a str, RoleError>,
) -> Result<String, RoleError> {
    let mut filled = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("{{") {
        filled.push_str(&rest[..open]);
        let inside = &rest[open + 2..];
        let word = inside
            .bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            .count();
        if word > 0 && inside[word..].starts_with("}}") {
            filled.push_str(slot(&inside[..word])?);
            rest = &inside[word + 2..];
        } else {
            // Not a slot: the match is tried again from the next character.
            filled.push('{');
            rest = &rest[open + 1..];
        }
    }
    filled.push_str(rest);
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "<!-- about
this -->
# {{title}} for {{role}}
{{{role}}}

<!-- worker: title -->
the worker
<!-- worker: extra -->
more

<!-- advisor: title -->
the advisor
";

    #[test]
    fn a_marker_is_a_whole_line_of_two_words() {
        assert_eq!(
            marker("<!-- worker: title -->\n"),
            Some(("worker", "title"))
        );
        assert_eq!(marker("<!-- worker: title -->"), None);
        assert_eq!(marker("<!--  worker: title -->\n"), None);
        assert_eq!(marker("<!-- worker title -->\n"), None);
        assert_eq!(marker("<!-- a: b: c -->\n"), None);
        assert_eq!(marker("<!-- wörker: title -->\n"), None);
        assert_eq!(marker("<!-- : title -->\n"), None);
    }

    #[test]
    fn the_file_is_split_at_its_markers_into_a_head_and_each_part() {
        let (head, parts) = split(SAMPLE);
        assert_eq!(
            head,
            "<!-- about\nthis -->\n# {{title}} for {{role}}\n{{{role}}}\n\n"
        );
        assert_eq!(
            parts,
            [
                ("worker", "title", "the worker\n"),
                ("worker", "extra", "more\n\n"),
                ("advisor", "title", "the advisor\n"),
            ]
        );
    }

    #[test]
    fn a_role_gets_the_head_with_its_slots_filled_and_the_opening_comment_gone() {
        assert_eq!(
            filled(SAMPLE, "worker").unwrap(),
            "# the worker for worker\n{worker}\n"
        );
        assert_eq!(
            filled(SAMPLE, "advisor").unwrap(),
            "# the advisor for advisor\n{advisor}\n"
        );
    }

    #[test]
    fn a_slot_the_role_does_not_have_is_named() {
        let text = "# {{title}} {{extra}}\n<!-- worker: title -->\nw\n";
        assert_eq!(
            filled(text, "worker").unwrap_err().to_string(),
            "staff.md has no extra for the worker role"
        );
        assert_eq!(
            filled(text, "pm").unwrap_err().to_string(),
            "staff.md has no title for the pm role"
        );
    }

    #[test]
    fn a_brace_that_opens_no_slot_is_text() {
        let slots = |word: &str| -> Result<&'static str, RoleError> {
            Ok(if word == "a" { "A" } else { "?" })
        };
        assert_eq!(
            fill("{{a}} {a} {{ a}} {{a }} {{}} {{a}", slots).unwrap(),
            "A {a} {{ a}} {{a }} {{}} {{a}"
        );
        assert_eq!(fill("{{{a}}}", slots).unwrap(), "{A}");
        assert_eq!(fill("{{é}} {{a_1}}", slots).unwrap(), "{{é}} ?");
    }
}

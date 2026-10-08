//! The staff and the work tiers as the chief reads them.

use cf_catalog::{work_tier_info, WORK_TIERS};

use super::{RoleError, StaffRow};

/// What the chief reads where its staff would be, when nobody is on it.
const NOBODY: &str = "Nobody is on the staff yet: only the human adds members, in the app. Ask them here in your terminal for the members your work needs, and do not create agents as a side effect.";

/// The project staff as the chief reads it: one row per member with its roles
/// and tier. A member with no tier to name is no row: Node threw a TypeError.
pub fn team_table(members: &[StaffRow]) -> Result<String, RoleError> {
    if members.is_empty() {
        return Ok(NOBODY.to_owned());
    }
    let mut table = vec![
        "| Member | Roles | Work tier |".to_owned(),
        "|---|---|---|".to_owned(),
    ];
    for member in members {
        // Name, roles and tier, nothing else: the chief names a tier and never
        // picks a member, so it needs no model, route or description here.
        let tier = member.work_tier.ok_or_else(|| RoleError::NoWorkTier {
            member: member.name.clone(),
        })?;
        let cells = [
            member.name.as_str(),
            &member.roles.join(", "),
            work_tier_info(tier).label,
        ];
        table.push(format!("| {} |", cells.map(cell).join(" | ")));
    }
    Ok(table.join("\n"))
}

/// The saved work tiers, one line each.
pub fn work_tier_list() -> String {
    WORK_TIERS
        .map(|tier| {
            let info = work_tier_info(tier);
            format!("- {}: {}", info.label, info.description)
        })
        .join("\n")
}

/// A table cell: a bar escaped, and a run of line ends made one space.
fn cell(value: &str) -> String {
    let mut cell = String::with_capacity(value.len());
    let mut after_break = false;
    for character in value.chars() {
        match character {
            '|' => cell.push_str("\\|"),
            '\r' | '\n' if after_break => {}
            '\r' | '\n' => cell.push(' '),
            other => cell.push(other),
        }
        after_break = matches!(character, '\r' | '\n');
    }
    cell
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cell_escapes_a_bar_and_folds_a_run_of_line_ends_into_one_space() {
        assert_eq!(cell("a|b"), "a\\|b");
        assert_eq!(cell("a\r\n\n\rb\nc"), "a b c");
        assert_eq!(cell("\n|\n"), " \\| ");
        assert_eq!(cell(""), "");
    }
}

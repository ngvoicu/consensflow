//! The staff, its roles' lines and the work tiers as the chief reads them.

use cf_proto::agents::{work_tier_info, WORK_TIERS};

use super::{chief, RoleError, StaffRow};

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
        // Name, roles and tier, nothing else: the chief names a tier and never picks a
        // member. A member is named by its model, which the name says; where it does not
        // (a member on its harness's default, one that needs an edit) the model stands
        // beside it.
        // A member of roles with no tier (the image designer: `--design`, no tier) has none,
        // and the table says so.
        let tier = match member.work_tier {
            Some(tier) => work_tier_info(tier).label,
            None if cf_ledger::roles::takes_no_tier(&member.roles) => "none",
            None => {
                return Err(RoleError::NoWorkTier {
                    member: member.name.clone(),
                })
            }
        };
        let named = match &member.model {
            Some(model) => format!("{} ({model})", member.name),
            None => member.name.clone(),
        };
        // Each role by its name: the frontend designer and the image designer are never one
        // word (spec new-roles).
        let roles = member
            .roles
            .iter()
            .map(|role| cf_ledger::roles::name_of(role))
            .collect::<Vec<_>>()
            .join(", ");
        let cells = [named.as_str(), &roles, tier];
        table.push(format!("| {} |", cells.map(cell).join(" | ")));
    }
    Ok(table.join("\n"))
}

/// The roles some member of `members` holds, in the registry's order: a member holds a role
/// when it is a row of the staff's table (`staff_of`).
pub fn held_roles(members: &[StaffRow]) -> Vec<&'static str> {
    cf_ledger::roles::all()
        .iter()
        .map(|role| role.id)
        .filter(|id| {
            members
                .iter()
                .any(|member| member.roles.iter().any(|role| role == id))
        })
        .collect()
}

/// The card lines of the roles `held`, a line break after each, in the registry's order:
/// nothing where nobody holds a role.
pub(super) fn role_commands(held: &[&str]) -> Result<String, RoleError> {
    let mut lines = String::new();
    for (role, command, _) in parts_of_every_role()? {
        if held.contains(&role.id) {
            lines.push_str(command);
            lines.push('\n');
        }
    }
    Ok(lines)
}

/// What "Choose the role, not the member" says of the roles `held`: each one's paragraphs in
/// the registry's order, then what is said of each role nobody holds that has something to say
/// of it, then the line that names the roles nobody holds, where there is one.
pub(super) fn role_work(held: &[&str]) -> Result<String, RoleError> {
    let every = parts_of_every_role()?;
    let mut paragraphs: Vec<String> = every
        .iter()
        .filter(|(role, ..)| held.contains(&role.id))
        .map(|(.., work)| (*work).to_owned())
        .collect();
    let unheld: Vec<_> = every
        .iter()
        .map(|(role, ..)| *role)
        .filter(|role| !held.contains(&role.id))
        .collect();
    paragraphs.extend(
        unheld
            .iter()
            .filter_map(|role| chief::role_part(role.id, "absent"))
            .map(str::to_owned),
    );
    let names: Vec<String> = unheld.iter().map(|role| role.a_name()).collect();
    if let Some((last, before)) = names.split_last() {
        let named = if before.is_empty() {
            last.clone()
        } else {
            format!("{} or {last}", before.join(", "))
        };
        paragraphs.push(format!(
            "Nobody on your staff is {named}. Add no task for one: when work needs it, ask the human for a member first, in your terminal."
        ));
    }
    Ok(paragraphs.join("\n\n"))
}

/// Every role of the registry with its card line and its paragraphs: a role with either
/// missing is refused by name, so that a new role cannot go unsaid.
fn parts_of_every_role(
) -> Result<Vec<(&'static cf_ledger::roles::Role, &'static str, &'static str)>, RoleError> {
    cf_ledger::roles::all()
        .iter()
        .map(|role| {
            let part = |slot: &str| {
                chief::role_part(role.id, slot)
                    .ok_or_else(|| RoleError::MissingChiefPart(format!("{}: {slot}", role.id)))
            };
            Ok((role, part("command")?, part("work")?))
        })
        .collect()
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

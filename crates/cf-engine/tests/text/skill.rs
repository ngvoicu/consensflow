//! The staff table the chief reads: the cases of Node's skill suite.

use cf_catalog::WorkTier;
use cf_engine::roles::{team_table, work_tier_list};

use super::support::staff_row;

#[test]
fn shows_one_row_per_member_name_roles_and_work_tier_and_nothing_to_pick_a_member_by() {
    let table = team_table(&[
        staff_row("zeus", &["worker", "reviewer"], WorkTier::Standard),
        staff_row("diana", &["advisor"], WorkTier::Light),
    ])
    .unwrap();
    assert_eq!(
        table,
        [
            "| Member | Roles | Work tier |",
            "|---|---|---|",
            "| zeus | worker, reviewer | Standard work |",
            "| diana | advisor | Light work |",
        ]
        .join("\n")
    );
}

#[test]
fn says_so_when_the_staff_is_empty() {
    assert!(team_table(&[])
        .unwrap()
        .starts_with("Nobody is on the staff yet"));
}

#[test]
fn lists_the_four_work_tiers_one_line_each() {
    let list = work_tier_list();
    let lines: Vec<&str> = list.split('\n').collect();
    assert_eq!(lines.len(), 4);
    assert!(lines[0].starts_with("- Critical work: "));
    assert!(lines[3].starts_with("- Light work: "));
}

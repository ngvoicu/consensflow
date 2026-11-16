//! The tables of role instructions in `tests/goldens/text.json`: each role's
//! text, the staff the chief reads and the work tiers.

use cf_base::env::Env;
use cf_engine::roles::{role_instructions_on, staff_of, team_table, work_tier_list, RoleError};
use cf_proto::cf_word::{CfWord, Shell};

use super::support::{assert_same_text, participant_of, project_of, staff_row_of, table, GOLDEN};

#[test]
fn every_role_is_given_the_text_node_gave_it_or_refused_as_node_refused_it() {
    let rows = table("roleInstructions");
    let (mut texts, mut tiers, mut said) = (0, 0, 0);
    for row in rows {
        let role = row["role"].as_str().unwrap();
        let staff: Vec<_> = row["staff"]
            .as_array()
            .unwrap()
            .iter()
            .map(staff_row_of)
            .collect();
        let env = Env::default();
        // A row that gives a `cf` gives its path; Node said no more of the shell.
        let cf = row["cf"]
            .as_str()
            .map_or_else(CfWord::bare, |path| CfWord::new(path, Shell::Posix));
        let answer = role_instructions_on(&env, role, &staff, &cf, None, false, true);
        let what = format!("the {role} text for {} (cf {})", row["staff"], row["cf"]);
        match (answer, &row["answer"]) {
            (Ok(text), expected) if expected.get("text").is_some() => {
                assert_same_text(&text, expected["text"].as_str().unwrap(), &what);
                texts += 1;
            }
            (Err(RoleError::NoWorkTier { .. }), expected) if expected["throws"] == true => {
                tiers += 1;
            }
            (Err(error), expected) if expected["throws"].is_string() => {
                assert_eq!(
                    error.to_string(),
                    expected["throws"].as_str().unwrap(),
                    "{what}"
                );
                said += 1;
            }
            (answer, expected) => panic!("{what}: Rust {answer:?}, Node {expected}"),
        }
    }
    assert_eq!(rows.len(), 52);
    assert_eq!((texts, tiers, said), (34, 5, 13));
}

#[test]
fn the_staff_is_read_off_a_project_as_node_read_it() {
    let rows = table("staffOf");
    for row in rows {
        let participants = row["participants"]
            .as_array()
            .unwrap()
            .iter()
            .map(participant_of);
        let staff = staff_of(&project_of(participants.collect()));
        let expected: Vec<_> = row["staff"]
            .as_array()
            .unwrap()
            .iter()
            .map(staff_row_of)
            .collect();
        assert_eq!(staff, expected, "{}", row["participants"]);
    }
    assert_eq!(rows.len(), 8);
}

#[test]
fn the_staff_table_reads_as_node_wrote_it_and_a_member_with_no_tier_is_none() {
    let rows = table("teamTable");
    for row in rows {
        let members: Vec<_> = row["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(staff_row_of)
            .collect();
        match (team_table(&members), row.get("table")) {
            (Ok(table), Some(expected)) => {
                assert_same_text(
                    &table,
                    expected.as_str().unwrap(),
                    &row["members"].to_string(),
                );
            }
            (Err(RoleError::NoWorkTier { .. }), None) => assert_eq!(row["throws"], true),
            (answer, expected) => panic!("{}: Rust {answer:?}, Node {expected:?}", row["members"]),
        }
    }
    assert_eq!(rows.len(), 9);
}

#[test]
fn the_work_tiers_are_listed_as_node_listed_them() {
    assert_same_text(
        &work_tier_list(),
        GOLDEN["workTierList"].as_str().unwrap(),
        "the tiers",
    );
}

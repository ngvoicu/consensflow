//! The tables of role instructions in `tests/goldens/text.json`: each role's
//! text, the staff the chief reads and the work tiers.

use std::fs;
use std::path::Path;

use cf_base::env::Env;
use cf_engine::roles::{role_instructions, staff_of, team_table, work_tier_list, RoleError};
use serde_json::Value;

use super::support::{assert_same_text, participant_of, project_of, staff_row_of, table, GOLDEN};

/// The environment an eval's card makes: its file written in `folder`, or
/// the variable pointed where the row says.
fn environment(card: &Value, folder: &Path) -> Env {
    let name = "CONSENSFLOW_EVAL_CHIEF_CARD";
    let file = folder.join("card.md");
    let variable = if let Some(text) = card.get("text").and_then(Value::as_str) {
        fs::write(&file, text).unwrap();
        file.into_os_string()
    } else if let Some(bytes) = card.get("bytes").and_then(Value::as_array) {
        let bytes: Vec<u8> = bytes
            .iter()
            .map(|byte| u8::try_from(byte.as_u64().unwrap()).unwrap())
            .collect();
        fs::write(&file, bytes).unwrap();
        file.into_os_string()
    } else if let Some(missing) = card.get("missing").and_then(Value::as_str) {
        folder.join(missing).into_os_string()
    } else if card.get("folder").is_some() {
        folder.as_os_str().to_owned()
    } else if card.get("empty").is_some() {
        "".into()
    } else {
        return Env::default();
    };
    Env::from_vars([(name, variable)])
}

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
        let folder = tempfile::tempdir().unwrap();
        let env = environment(&row["card"], folder.path());
        let answer = role_instructions(&env, role, &staff, row["cf"].as_str());
        let what = format!(
            "the {role}'s text for {} (cf {}, card {})",
            row["staff"], row["cf"], row["card"]
        );
        match (answer, &row["answer"]) {
            (Ok(text), expected) if expected.get("text").is_some() => {
                assert_same_text(&text, expected["text"].as_str().unwrap(), &what);
                texts += 1;
            }
            (Err(RoleError::NoWorkTier { .. }), expected) if expected["throws"] == true => {
                tiers += 1;
            }
            (Err(error), expected) if expected["throws"].is_string() => {
                // Words that name the card name it $CARD, as the golden does.
                let words = match env.text("CONSENSFLOW_EVAL_CHIEF_CARD") {
                    Some(card) if !card.is_empty() => error.to_string().replace(card, "$CARD"),
                    _ => error.to_string(),
                };
                assert_eq!(words, expected["throws"].as_str().unwrap(), "{what}");
                said += 1;
            }
            (answer, expected) => panic!("{what}: Rust {answer:?}, Node {expected}"),
        }
    }
    assert_eq!(rows.len(), 79);
    assert_eq!((texts, tiers, said), (55, 6, 18));
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

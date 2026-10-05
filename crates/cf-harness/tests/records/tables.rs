//! The tables Node computed beside the scenarios (`tables.json`): quota over
//! texts, instants and zones, and `localeCompare` over ASCII and the ids of
//! the fixtures.

use serde_json::Value;

use crate::scenario::goldens;

#[test]
fn the_tables_hold_every_quota_case_and_the_collation_of_ascii() {
    let file = goldens().join("tables.json");
    let tables: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    let quota = &tables["quota"];
    assert_eq!(quota["defaultZone"], "America/Los_Angeles");
    let instants = quota["instants"].as_array().unwrap().len();
    let resets = quota["resets"].as_array().unwrap();
    assert!(resets
        .iter()
        .all(|row| row["at"].as_array().unwrap().len() == instants));
    assert_eq!((resets.len(), instants), (39, 16));
    let collation = &tables["collation"];
    let characters = collation["characters"].as_str().unwrap();
    assert_eq!(characters.len(), 95);
    let matrix = collation["matrix"].as_array().unwrap();
    assert!(matrix
        .iter()
        .all(|row| row.as_str().unwrap().len() == characters.len()));
    let words = collation["words"].as_array().unwrap().len();
    assert!(collation["pairs"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row.as_str().unwrap().len() == words));
}

#[test]
fn locale_compare_orders_every_ascii_pair_and_every_fixture_id_as_node_did() {
    let file = goldens().join("tables.json");
    let tables: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    let collation = &tables["collation"];
    let sign = |ordering: std::cmp::Ordering| match ordering {
        std::cmp::Ordering::Less => '<',
        std::cmp::Ordering::Equal => '=',
        std::cmp::Ordering::Greater => '>',
    };
    let characters: Vec<String> = collation["characters"]
        .as_str()
        .unwrap()
        .chars()
        .map(String::from)
        .collect();
    let words: Vec<&str> = collation["words"]
        .as_array()
        .unwrap()
        .iter()
        .map(|word| word.as_str().unwrap())
        .collect();
    let mut compared = 0;
    for (table, list) in [
        (
            "matrix",
            characters.iter().map(String::as_str).collect::<Vec<_>>(),
        ),
        ("pairs", words),
    ] {
        for (row, left) in collation[table].as_array().unwrap().iter().zip(&list) {
            let expected: Vec<char> = row.as_str().unwrap().chars().collect();
            for (column, right) in list.iter().enumerate() {
                let ours = sign(cf_base::js::locale_compare(left, right));
                assert_eq!(ours, expected[column], "{left:?} against {right:?}");
                compared += 1;
            }
        }
    }
    assert!(compared > 95 * 95);
}

//! The suggested staff's file and its rule (T-2405): the shipped file is true to the code, and a
//! file that is not is never offered.

use serde_json::{json, Value};

use super::{offered, read, shipped, SHIPPED};

/// The shipped staff with `change` applied to its JSON.
fn changed(change: impl FnOnce(&mut Value)) -> String {
    let mut file: Value = serde_json::from_str(SHIPPED).unwrap();
    change(&mut file);
    file.to_string()
}

fn refused(text: &str, word: &str) {
    let why = read(text).expect_err("the file should be refused");
    assert!(why.contains(word), "{word:?} not in {why:?}");
    assert_eq!(offered(text), Value::Null, "a refused file offers nothing");
}

#[test]
fn the_shipped_file_parses_and_names_only_what_the_code_knows() {
    let staff = read(SHIPPED).expect("the shipped file is true to the code");
    assert_eq!(shipped(), staff);
    let members = staff["members"].as_array().unwrap();
    assert!(!members.is_empty());
    assert!(staff["about"].as_str().unwrap().len() > 20);
    assert!(members
        .iter()
        .any(|m| m["designer"] == true && m["harness"] == "codex"));
}

#[test]
fn the_standard_reviewer_researcher_and_tester_are_one_member_in_three_slots() {
    let staff = shipped();
    let one: Vec<&Value> = staff["members"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["harness"] == "codex" && m["tier"] == "standard")
        .collect();
    assert_eq!(one.len(), 1);
    let mut roles: Vec<&str> = one[0]["roles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap())
        .collect();
    roles.sort_unstable();
    assert_eq!(roles, ["researcher", "reviewer", "tester"]);
}

#[test]
fn a_file_that_does_not_parse_is_not_offered() {
    refused("{ not json", "key must be");
    refused("[]", "invalid length");
    refused(&changed(|f| f["members"] = json!("x")), "invalid type");
}

#[test]
fn a_field_the_code_does_not_know_is_not_offered() {
    refused(
        &changed(|f| f["members"][0]["colour"] = json!("red")),
        "unknown field",
    );
    refused(&changed(|f| f["mood"] = json!("fine")), "unknown field");
}

#[test]
fn a_role_a_harness_a_tier_or_an_effort_the_code_does_not_know_is_not_offered() {
    refused(
        &changed(|f| f["members"][0]["roles"] = json!(["janitor"])),
        "no role",
    );
    refused(
        &changed(|f| f["members"][0]["harness"] = json!("emacs")),
        "no harness",
    );
    refused(
        &changed(|f| f["members"][0]["tier"] = json!("huge")),
        "no slot",
    );
    refused(
        &changed(|f| f["members"][0]["effort"] = json!("ludicrous")),
        "no effort",
    );
    refused(
        &changed(|f| f["members"][0]["roles"] = json!([])),
        "no role",
    );
    refused(
        &changed(|f| f["members"][0]["model"] = json!("")),
        "no model",
    );
}

#[test]
fn a_role_at_a_tier_it_has_no_slot_at_is_not_offered() {
    // An image role is the designer entry's alone.
    refused(
        &changed(|f| f["members"][0]["roles"] = json!(["designer"])),
        "image",
    );
}

#[test]
fn two_entries_that_are_one_member_are_not_offered() {
    refused(
        &changed(|f| {
            let first = f["members"][3].clone();
            f["members"][3]["roles"] = json!(["architect"]);
            let mut second = first;
            second["roles"] = json!(["advisor"]);
            f["members"].as_array_mut().unwrap().push(second);
        }),
        "one member",
    );
}

#[test]
fn two_members_at_one_slot_are_not_offered() {
    refused(
        &changed(|f| {
            let mut second = f["members"][0].clone();
            second["effort"] = json!("high");
            f["members"].as_array_mut().unwrap().push(second);
        }),
        "two members at one slot",
    );
}

#[test]
fn the_image_designer_is_codexs_alone_and_carries_no_model() {
    let last = 6;
    refused(
        &changed(|f| f["members"][last]["harness"] = json!("claude-code")),
        "Codex's",
    );
    refused(
        &changed(|f| f["members"][last]["model"] = json!("gpt-6-2-astra")),
        "Codex's",
    );
    refused(
        &changed(|f| f["members"][last]["tier"] = json!("standard")),
        "Codex's",
    );
}

#[test]
fn a_figure_measured_elsewhere_is_kept_and_nothing_else_reads_it() {
    let text = changed(|f| {
        f["quality"] = json!(0.8);
        f["cost"] = json!(1.5);
    });
    let staff = read(&text).unwrap();
    assert_eq!(staff["quality"], json!(0.8));
    assert_eq!(staff["cost"], json!(1.5));
    assert!(shipped().get("quality").is_none());
}

#[test]
fn nothing_is_left_of_the_pages_stand_in() {
    let page = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/ui/core");
    assert!(
        !page.join("suggested-staff-data.js").exists(),
        "the page's stand-in is gone: the daemon sends the staff"
    );
    let reader = std::fs::read_to_string(page.join("suggested-staff.js")).unwrap();
    assert!(!reader.contains("STAND_IN"), "the page reads no stand-in");
}

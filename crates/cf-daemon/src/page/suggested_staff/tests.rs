//! The suggested staff's file and its rule (T-2405): the shipped file is true to the code, and a
//! file that is not is never offered.

use serde_json::{json, Value};

use super::{offered, read, seam, shipped, SHIPPED};

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
    refused("[]", "named fields");
    refused(&changed(|f| f["members"] = json!("x")), "named fields");
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
        &changed(|f| f["members"][0]["effort"] = json!("very high")),
        "not an effort",
    );
    refused(
        &changed(|f| f["members"][0]["roles"] = json!([])),
        "no role",
    );
    refused(
        &changed(|f| f["members"][0]["model"] = json!("")),
        "not a model id",
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

#[test]
fn a_model_id_the_ledger_refuses_is_not_offered() {
    for bad in ["two words", " ", "-flag", "bell\u{7}"] {
        refused(
            &changed(|f| f["members"][1]["model"] = json!(bad)),
            "not a model id",
        );
    }
}

#[test]
fn a_list_where_a_named_object_belongs_is_not_offered() {
    let file: Value = serde_json::from_str(SHIPPED).unwrap();
    // The staff as a list of its fields, and a member as a list of its.
    refused(
        &json!([file["about"], file["members"]]).to_string(),
        "named fields",
    );
    refused(
        &changed(|f| {
            f["members"][0] = json!([
                "claude-code",
                "claude-opus-5-5",
                "medium",
                "critical",
                ["worker"]
            ]);
        }),
        "named fields",
    );
}

#[test]
fn the_image_designer_takes_no_effort_and_a_member_names_a_role_once_and_the_staff_says_what_it_is_for(
) {
    refused(
        &changed(|f| f["members"][6]["effort"] = json!("high")),
        "Codex's",
    );
    refused(
        &changed(|f| f["members"][3]["roles"] = json!(["architect", "architect"])),
        "twice",
    );
    refused(&changed(|f| f["about"] = json!("  ")), "for");
}

/// A role is held to the tiers it has a slot at, one by one: a tier that exists for another role
/// is no tier for it.
#[test]
fn a_tier_that_another_role_has_but_this_one_has_not_is_not_offered() {
    use cf_ledger::roles::{self, seam, Role};
    static TABLE: [Role; 2] = [
        Role {
            tiers: &["standard"],
            ..roles::WORKER
        },
        Role {
            tiers: &["critical"],
            ..roles::ADVISOR
        },
    ];
    let staff = |role: &str, tier: &str| {
        json!({ "about": "a staff", "members": [
            { "harness": "claude-code", "model": "claude-opus-5-5", "effort": "high", "tier": tier, "roles": [role] }
        ] })
        .to_string()
    };
    // The worker has a standard slot only, the advisor a critical one only.
    seam::with_table(&TABLE, || {
        assert!(read(&staff("worker", "standard")).is_ok());
        assert!(read(&staff("advisor", "critical")).is_ok());
        refused(&staff("worker", "critical"), "no slot");
        refused(&staff("advisor", "standard"), "no slot");
    });
}

/// The wiring: what `harnesses.missing` sends is the shipped text as the rule reads it, so a file
/// that breaks the rule is sent as `null` and never as it stands.
#[test]
fn what_harnesses_missing_sends_is_the_file_as_the_rule_reads_it() {
    let bad = changed(|f| f["members"][0]["roles"] = json!(["janitor"]));
    let _put = seam::put(&bad);
    assert_eq!(shipped(), Value::Null);
    let good = changed(|f| f["about"] = json!("Another staff."));
    let _again = seam::put(&good);
    assert_eq!(shipped()["about"], json!("Another staff."));
}

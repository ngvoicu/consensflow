//! The goldens `npm run goldens:catalog` writes from the JavaScript, which
//! the unit suite holds equal to what Node computes now. Here, the catalog's
//! and `agentProfile`'s answers are held to them, case by case and as text, so
//! a key out of order or a word changed fails; and the roster's reads
//! (`listAgents`, `agentRow`, `preferences`) likewise, each on a file written
//! into a home of its own. The roster's writes are the next landing: their
//! cases are counted, not answered.

// The goldens' own reading: a failure in it is the test's.
#![allow(clippy::unwrap_used)]

use std::path::Path;

use cf_base::js;
use cf_base::refusal::Refusal;
use cf_catalog::{
    efforts, harness_for_kind, validate_work_tier, work_tier_info, Catalog, Roster, Settings,
    HARNESSES, WORK_TIERS,
};
use serde_json::{json, Map, Value};

/// A golden, by its file name in `tests/goldens/`.
fn golden(name: &str) -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn catalog() -> Catalog {
    Catalog::bundled().unwrap()
}

/// Fails with where `actual` and `golden` first differ, which a diff of two
/// whole catalogs would bury.
fn assert_same_text(actual: &str, golden: &str, what: &str) {
    if actual == golden {
        return;
    }
    let at = actual
        .char_indices()
        .zip(golden.chars())
        .find(|((_, a), g)| a != g)
        .map_or_else(|| actual.len().min(golden.len()), |((at, _), _)| at);
    let window = |text: &str| {
        let from = text.floor_char_boundary(at.saturating_sub(60));
        let to = text.ceil_char_boundary((at + 60).min(text.len()));
        text[from..to].to_owned()
    };
    panic!(
        "{what} differs from the golden at byte {at}:\n  rust:   …{}…\n  golden: …{}…",
        window(actual),
        window(golden)
    );
}

/// A text field of a golden agent: absent and `null` are none, as the roster reads them.
fn text<'a>(agent: &'a Value, field: &str) -> Option<&'a str> {
    match agent.get(field) {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => Some(text),
        Some(other) => panic!("{field} of {agent} is no text: {other}"),
    }
}

#[test]
fn every_profile_case_is_answered_or_refused_as_node_answered_it() {
    let catalog = catalog();
    let cases = golden("profiles.json");
    let cases = cases.as_array().unwrap();
    let (mut profiles, mut refusals) = (0, 0);
    for case in cases {
        let agent = &case["agent"];
        assert!(agent.is_object(), "{case}");
        match (
            validate_work_tier(agent.get("workTier")),
            case.get("profile"),
            case.get("error"),
        ) {
            (Ok(work_tier), Some(golden), None) => {
                let profile = catalog.profile(&Settings {
                    harness: text(agent, "harness"),
                    kind: text(agent, "kind"),
                    model: text(agent, "model"),
                    effort: text(agent, "effort"),
                    thinking: text(agent, "thinking"),
                    designer: js::truthy(agent.get("designer")),
                    work_tier,
                });
                assert_same_text(
                    &serde_json::to_string(&profile).unwrap(),
                    &golden.to_string(),
                    &format!("the profile of {agent}"),
                );
                profiles += 1;
            }
            (Err(refusal), None, Some(error)) => {
                assert_eq!(Some(refusal.message.as_str()), error.as_str(), "{agent}");
                assert_eq!(
                    (refusal.code, refusal.status),
                    ("work-tier", 400),
                    "{agent}"
                );
                refusals += 1;
            }
            (answer, profile, error) => {
                panic!("{agent}: the tier reads {answer:?}, the golden has {profile:?} {error:?}")
            }
        }
    }
    assert_eq!(cases.len(), 1283);
    assert_eq!((profiles, refusals), (1277, 6));
}

#[test]
fn the_whole_catalog_is_listed_as_node_lists_it_harness_by_harness_in_order() {
    let catalog = catalog();
    let golden = golden("catalog.json");
    let mut whole = Map::new();
    for group in catalog.groups() {
        let entries = serde_json::to_value(&group.entries).unwrap();
        let harness = group.harness.as_str();
        // Each harness on its own, which says where a difference is.
        assert_same_text(
            &entries.to_string(),
            &golden["catalog"][harness].to_string(),
            &format!("the entries of {harness}"),
        );
        whole.insert(harness.to_owned(), entries);
    }
    assert_same_text(
        &Value::Object(whole).to_string(),
        &golden["catalog"].to_string(),
        "the catalog",
    );
    let sizes: Vec<_> = catalog
        .groups()
        .iter()
        .map(|group| (group.harness.as_str(), group.entries.len()))
        .collect();
    assert_eq!(
        sizes,
        [
            ("devin", 27),
            ("codex", 15),
            ("pi", 32),
            ("opencode", 33),
            ("claude", 12)
        ]
    );
}

#[test]
fn every_lookup_by_name_answers_as_node_answered() {
    let catalog = catalog();
    let golden = golden("catalog.json");
    let lookups = golden["entries"].as_array().unwrap();
    let (mut found, mut none) = (0, 0);
    for lookup in lookups {
        let name = lookup["name"].as_str().unwrap();
        let answer = catalog.entry(name);
        if lookup["entry"] == json!({ "$undefined": true }) {
            assert_eq!(answer, None, "{name:?}");
            none += 1;
        } else {
            let entry = answer.unwrap_or_else(|| panic!("{name:?} is in the catalog"));
            assert_same_text(
                &serde_json::to_string(&entry).unwrap(),
                &lookup["entry"].to_string(),
                &format!("the lookup of {name:?}"),
            );
            found += 1;
        }
    }
    assert_eq!((lookups.len(), found, none), (123, 119, 4));
}

#[test]
fn the_efforts_the_work_tiers_and_the_harnesses_are_listed_as_node_lists_them() {
    let golden = golden("catalog.json");
    let by_harness: Map<String, Value> = HARNESSES
        .into_iter()
        .map(|harness| (harness.as_str().to_owned(), json!(efforts(harness))))
        .collect();
    assert_same_text(
        &Value::Object(by_harness).to_string(),
        &golden["efforts"].to_string(),
        "the efforts",
    );
    let tiers: Map<String, Value> = WORK_TIERS
        .into_iter()
        .map(|tier| {
            (
                tier.as_str().to_owned(),
                serde_json::to_value(work_tier_info(tier)).unwrap(),
            )
        })
        .collect();
    assert_same_text(
        &Value::Object(tiers).to_string(),
        &golden["workTiers"].to_string(),
        "the work tiers",
    );
    assert_same_text(
        &serde_json::to_string(&HARNESSES).unwrap(),
        &golden["harnesses"].to_string(),
        "the harnesses",
    );
}

#[test]
fn each_kind_names_the_harness_node_names_and_none_where_node_names_none() {
    let golden = golden("catalog.json");
    let kinds = golden["harnessForKind"].as_array().unwrap();
    for case in kinds {
        let kind = case["kind"].as_str().unwrap();
        let answered = json!({ "kind": kind, "harness": harness_for_kind(kind) });
        assert_same_text(
            &answered.to_string(),
            &case.to_string(),
            &format!("the harness of {kind:?}"),
        );
    }
    assert_eq!(kinds.len(), 8);
}

#[test]
fn each_roster_case_starts_from_a_file_and_answers_once() {
    let roster = golden("roster.json");
    let documents = roster["documents"].as_object().unwrap();
    let cases = roster["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 1028);
    for case in cases {
        let from_a_document = case["document"]
            .as_str()
            .is_some_and(|document| documents.contains_key(document));
        let from_a_step = case["sequence"].is_string() && case["step"].is_u64();
        assert!(from_a_document != from_a_step, "{case}");
        assert!(
            case.get("result").is_some() != case.get("error").is_some(),
            "{case}"
        );
        assert!(
            case.get("unchanged").is_some() != case.get("after").is_some(),
            "{case}"
        );
    }
}

/// The golden's stand-in for what JSON cannot hold, JavaScript's `undefined`.
fn undefined() -> Value {
    json!({ "$undefined": true })
}

/// `value` as `JSON.stringify` writes it: the members that are `undefined`
/// are left out, and an item of a list that is, is `null`.
fn stringified(value: &Value) -> Value {
    match value {
        Value::Object(members) => Value::Object(
            members
                .iter()
                .filter(|(_, member)| **member != undefined())
                .map(|(key, member)| (key.clone(), stringified(member)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| {
                    if *item == undefined() {
                        Value::Null
                    } else {
                        stringified(item)
                    }
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// The text of the file a roster case starts from, none for a case that
/// starts with no file: a document the golden names, or the file the step
/// before left.
fn file_before<'a>(case: &'a Value, documents: &'a Map<String, Value>) -> Option<&'a str> {
    let before = if case["sequence"].is_string() {
        &case["before"]
    } else {
        &documents[case["document"].as_str().unwrap()]
    };
    before.as_str()
}

/// What a roster read answers: its JSON, where an `agentRow` that finds
/// nothing is `undefined`.
fn answered(roster: &Roster<'_>, call: &[Value]) -> Result<Value, Refusal> {
    match call[0].as_str().unwrap() {
        "listAgents" => roster
            .list()
            .map(|views| serde_json::to_value(views).unwrap()),
        "agentRow" => {
            // `{"$undefined":true}` is no name: the empty one, `String(name ?? '')`.
            let name = call[1].as_str().unwrap_or("");
            roster
                .agent_row(name)
                .map(|row| row.map_or_else(undefined, |row| serde_json::to_value(row).unwrap()))
        }
        "preferences" => roster
            .preferences()
            .map(|choices| serde_json::to_value(choices).unwrap()),
        other => panic!("{other} is no read"),
    }
}

/// The names of what the home holds, in order.
fn left_in(home: &Path) -> Vec<String> {
    let mut left: Vec<String> = std::fs::read_dir(home)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    left
}

#[test]
fn every_read_of_the_roster_answers_as_node_answered_it_and_leaves_the_file_as_it_found_it() {
    let catalog = catalog();
    let golden = golden("roster.json");
    let documents = golden["documents"].as_object().unwrap();
    let cases = golden["cases"].as_array().unwrap();
    let (mut lists, mut rows, mut choices, mut from_steps, mut not_yet) = (0, 0, 0, 0, 0);
    let (mut answers, mut refusals) = (0, 0);
    for case in cases {
        let call = case["call"].as_array().unwrap();
        match call[0].as_str().unwrap() {
            "listAgents" => lists += 1,
            "agentRow" => rows += 1,
            "preferences" => choices += 1,
            _ => {
                // A write: the lead's next landing answers it.
                not_yet += 1;
                continue;
            }
        }
        from_steps += usize::from(case["sequence"].is_string());
        // The file the case starts from, in a home of its own.
        let before = file_before(case, documents);
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("agents.json");
        if let Some(text) = before {
            std::fs::write(&path, text).unwrap();
        }
        let roster = Roster::new(&catalog, path.clone());
        match (
            answered(&roster, call),
            case.get("result"),
            case.get("error"),
        ) {
            (Ok(actual), Some(golden), None) => {
                assert_same_text(
                    &actual.to_string(),
                    &stringified(golden).to_string(),
                    &format!("the answer to {case}"),
                );
                answers += 1;
            }
            (Err(refusal), None, Some(error)) => {
                // The file's path, whatever the platform writes it as, is `«home»/agents.json`.
                let said = refusal
                    .message
                    .replace(&path.display().to_string(), "«home»/agents.json");
                assert_eq!(Some(said.as_str()), error.as_str(), "{case}");
                let code = if said.starts_with("Your agents file ") {
                    "agents-file-unreadable"
                } else {
                    "work-tier"
                };
                assert_eq!((refusal.code, refusal.status), (code, 400), "{case}");
                refusals += 1;
            }
            (answer, golden, error) => {
                panic!("{case}: answered {answer:?}, the golden has {golden:?} {error:?}")
            }
        }
        // A read changes nothing and makes nothing.
        assert_eq!(case.get("unchanged"), Some(&json!(true)), "{case}");
        match before {
            Some(text) => {
                assert_eq!(std::fs::read(&path).unwrap(), text.as_bytes(), "{case}");
                assert_eq!(left_in(home.path()), ["agents.json"], "{case}");
            }
            None => assert_eq!(left_in(home.path()), Vec::<String>::new(), "{case}"),
        }
    }
    eprintln!(
        "roster goldens: {} reads checked ({lists} listAgents, {rows} agentRow, {choices} \
         preferences; {answers} answered, {refusals} refused), {not_yet} writes not yet answered",
        lists + rows + choices
    );
    assert_eq!((lists, rows, choices), (22, 117, 22));
    assert_eq!(lists + rows + choices, 161, "the reads answered");
    assert_eq!((answers, refusals), (132, 29));
    assert_eq!(from_steps, 7, "of them, the steps of the seeded sequences");
    assert_eq!(not_yet, 867, "the writes, not yet answered");
    assert_eq!(cases.len(), 161 + 867);
}

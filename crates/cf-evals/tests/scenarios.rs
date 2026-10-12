//! The scenarios: each is what the JavaScript's was, word for word (their prose is
//! the eval, and a word changed in a prompt changes what is measured), and each
//! expectation holds on the numbers of a run that did what it asks and fails on
//! those of a run that did not (`tests/scenarios/verdicts.rs`), by the name a report
//! says it by; and on the numbers of the eleven ledgers recorded from real runs and
//! made ones, each judges as the JavaScript's did.

#![allow(clippy::unwrap_used)]

use cf_evals::scenarios::{self, FollowUp, Scenario};
use regex::Regex;
use serde_json::{json, Value};

mod support;

// The verdicts are in `tests/scenarios/`: cargo looks for the modules of a test's
// root file beside it, where each would be a suite of its own.
#[path = "scenarios/verdicts.rs"]
mod verdicts;

/// What the JavaScript scenarios said, as data: `tests/fixtures/scenarios.json`.
/// A pattern's source is JavaScript's, which writes a slash as `\/`; the pattern
/// of a scenario here has the slash as it is.
fn golden() -> Vec<Value> {
    let text = include_str!("fixtures/scenarios.json");
    let mut scenarios = serde_json::from_str::<Value>(text)
        .unwrap()
        .as_array()
        .unwrap()
        .clone();
    for scenario in &mut scenarios {
        for answer in scenario["answers"].as_array_mut().unwrap() {
            let source = answer["source"].as_str().unwrap().replace("\\/", "/");
            answer["source"] = json!(source);
        }
    }
    scenarios
}

/// A scenario as the golden says it: the pattern of an answer as its source and
/// its flags, a switch as `{ "switch": true }`.
fn described(scenario: &Scenario) -> Value {
    json!({
        "id": scenario.id,
        "title": scenario.title,
        "fixture": scenario.fixture,
        "prompt": scenario.prompt,
        "followUps": scenario.follow_ups.iter().map(|step| match step {
            FollowUp::Say(text) => json!(text),
            FollowUp::Switch => json!({ "switch": true }),
        }).collect::<Vec<_>>(),
        "answers": scenario.answers.iter().map(|answer| {
            let (flags, source) = match answer.matches.as_str().strip_prefix("(?i)") {
                Some(source) => ("i", source),
                None => ("", answer.matches.as_str()),
            };
            json!({ "source": source, "flags": flags, "text": answer.text })
        }).collect::<Vec<_>>(),
        "fallback": scenario.fallback,
        "nudge": scenario.nudge,
        "quietMs": u64::try_from(scenario.quiet.as_millis()).unwrap(),
        "expectations": scenario.expectations.iter().map(|e| e.name).collect::<Vec<_>>(),
    })
}

/// The scenario `id`, whose random codeword (`chief-switch`) is the one the golden
/// stands a placeholder for.
pub fn scenario(id: &str) -> Scenario {
    if id == "chief-switch" {
        scenarios::chief_switch_with("@CODEWORD@")
    } else {
        scenarios::by_id(id).unwrap()
    }
}

#[test]
fn every_scenario_says_what_the_javascript_one_said_word_for_word() {
    let golden = golden();
    assert_eq!(golden.len(), scenarios::IDS.len());
    for said in golden {
        let id = said["id"].as_str().unwrap();
        assert_eq!(described(&scenario(id)), said, "{id}");
    }
}

/// What each scenario's expectations said of the numbers of each recorded ledger
/// (`tests/fixtures/ledgers/<name>.json` of the repository) while they were
/// JavaScript's, as a string with a `1` for each that held and a `0` for each that
/// did not, in the scenario's order: `tests/fixtures/verdicts.json`. The numbers are
/// the recording's own, which the measure is held to the byte
/// (`tests/measure/recordings.rs`), so a scenario that judges them otherwise has an
/// expectation that is not the one it was.
#[test]
fn each_scenario_judges_each_recorded_ledger_as_the_javascript_one_did() {
    let golden: Value = serde_json::from_str(include_str!("fixtures/verdicts.json")).unwrap();
    let recordings = golden.as_object().unwrap();
    assert_eq!(recordings.len(), 11);
    for (name, said) in recordings {
        let metrics = support::measured(name);
        for id in scenarios::IDS {
            let judged: String = scenario(id)
                .verdict(&metrics)
                .iter()
                .map(|check| if check.ok { '1' } else { '0' })
                .collect();
            assert_eq!(judged, said[id].as_str().unwrap(), "{id} on {name}");
        }
    }
}

#[test]
fn the_twelve_scenarios_are_found_by_their_ids_and_no_other() {
    assert_eq!(scenarios::IDS.len(), 12);
    for id in scenarios::IDS {
        assert_eq!(scenarios::by_id(id).unwrap().id, id);
    }
    assert!(scenarios::by_id("no-such-scenario").is_none());
    assert!(scenarios::by_id("").is_none());
    let mut ids = scenarios::IDS.to_vec();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 12, "one each");
}

#[test]
fn only_the_chief_switch_scenario_switches_the_chief_and_its_codeword_is_a_bird_and_four_digits() {
    for id in scenarios::IDS {
        assert_eq!(
            scenarios::by_id(id).unwrap().switches_the_chief(),
            id == "chief-switch",
            "{id}"
        );
    }
    let codeword = Regex::new(r"^(TERN|LARK|WREN|KITE|ROOK|SWIFT)-\d{4}$").unwrap();
    let prompt = scenarios::by_id("chief-switch").unwrap().prompt;
    let said = Regex::new(r"The codeword for this release is (\S+)\. ")
        .unwrap()
        .captures(&prompt)
        .map(|found| found[1].to_owned())
        .unwrap();
    assert!(codeword.is_match(&said), "{said}");
    // Another each time it is asked for, in a sample of a few.
    let all: std::collections::HashSet<String> = (0..20)
        .map(|_| scenarios::by_id("chief-switch").unwrap().prompt)
        .collect();
    assert!(all.len() > 1);
}

//! The harness records' goldens (`npm run goldens:records`, from
//! `tests/goldens/records/`): the scenarios Node played against its readers
//! and what each look read.
//!
//! Every scenario is read into the steps the player plays and the readings
//! its looks are held to, and its looks are counted harness by harness, so a
//! golden that shrinks or changes shape fails here. The player plays every
//! scenario through the switch, and holds each look to Node's reading.

// The goldens' own reading: a failure in it is the test's.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod lines;
mod play;
mod scenario;
mod tables;

use std::collections::BTreeMap;

use serde_json::Value;

use scenario::{a_reason_it_may_have, check, looks, our_reasons, scenarios, Reader};

/// The goldens' groups of scenarios.
const GROUPS: [&str; 3] = ["sequences", "suite", "sweep"];

#[test]
fn every_look_node_took_is_read_whole_and_counted_harness_by_harness() {
    let ours = our_reasons();
    let mut by_kind = BTreeMap::<String, usize>::new();
    let mut counted = Vec::new();
    for group in GROUPS {
        let scenarios = scenarios(group);
        let mut group_looks = 0;
        for scenario in &scenarios {
            assert!(!scenario.name.is_empty());
            assert!(
                scenario.idle.is_none_or(|idle| idle > 0),
                "{}",
                scenario.name
            );
            assert!(
                scenario.env.values().all(Value::is_string),
                "{}",
                scenario.name
            );
            for reading in &scenario.readings {
                if reading["unknown"] == Value::Bool(true) {
                    let reason = reading["reason"].as_str().unwrap();
                    assert!(
                        a_reason_it_may_have(reason, &ours),
                        "{}: {reason}",
                        scenario.name
                    );
                } else {
                    for item in reading["items"].as_array().unwrap() {
                        let at = item.as_u64().unwrap();
                        assert!(at < scenario.items.len() as u64, "{}", scenario.name);
                    }
                }
            }
            for step in &scenario.steps {
                check(step, &scenario.name);
            }
            for look in looks(&scenario.steps) {
                let read = look.read.or(look.fresh);
                assert!(
                    read.is_some_and(|read| read < scenario.readings.len()),
                    "{}: a look with no reading",
                    scenario.name
                );
                assert!(
                    matches!(look.look, Reader::Fresh) == look.read.is_none(),
                    "{}: a fresh look holds no cached reading, any other one does",
                    scenario.name
                );
                assert!(look.same_as.is_none() || look.quota_same_as.is_none());
                *by_kind.entry(look.kind.clone()).or_default() += 1;
                group_looks += 1;
            }
        }
        counted.push((group, scenarios.len(), group_looks));
    }
    assert_eq!(
        counted,
        [
            ("sequences", 46, 1131),
            ("suite", 173, 201),
            ("sweep", 180, 180)
        ]
    );
    assert_eq!(
        by_kind.into_iter().collect::<Vec<_>>(),
        [
            ("claude-code".to_owned(), 650),
            ("codex".to_owned(), 262),
            ("devin".to_owned(), 273),
            ("opencode".to_owned(), 100),
            ("pi".to_owned(), 227),
        ]
    );
}

#[test]
fn every_look_node_took_reads_through_the_switch_what_node_read() {
    let ours = our_reasons();
    let mut answered = 0;
    let mut played = 0;
    for group in GROUPS {
        for scenario in scenarios(group) {
            answered += play::play(&scenario, &ours);
            played += 1;
        }
    }
    assert_eq!((played, answered), (399, 1512));
}

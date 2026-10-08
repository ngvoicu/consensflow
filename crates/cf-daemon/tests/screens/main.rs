//! The screens, held to what Node answers (`tests/goldens`, the traces of
//! Node's screens suite and its corners, which `FORMAT.md` describes). Each
//! trace is played step by step against the daemon's own front, the screens
//! mounted over the agents' API on loopback, a folder of its own for the roster
//! and the harnesses' stand-ins, and a ledger: every exchange is sent as a
//! client sends it, and its answer compared as bytes, the roster file before
//! and after each write, and no other file changed.
//!
//! The stand-ins of the harnesses the traces name are POSIX scripts (the
//! traces' own list says so); on Windows each is the shape of an npm shim, as
//! `harness_path` finds a harness there. The harness admin is given scripted
//! seams with nothing scripted, so no trace can reach a program or a feed.
//!
//! Where Node's words were V8's (a body that is no JSON) the daemon's are its
//! own: `play::DEPARTURES` lists the exchanges, and a test holds the list to
//! the traces.

// The goldens' own reading and the folder a test makes: a failure in either is the test's.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod play;
mod rig;

// What the three players share, taken whole.
#[path = "../support/front.rs"]
mod front;
#[path = "../support/mod.rs"]
mod support;
#[path = "../support/world.rs"]
mod world;
#[path = "../support/wrote.rs"]
mod wrote;

use std::collections::BTreeSet;

use serde_json::Value;
use support::trace;

/// The suites of the screens' traces.
const SUITES: [&str; 2] = ["core-agents-server", "corners-screens"];

/// A test for each trace, and the list of them all.
macro_rules! traces {
    ($($test:ident: $trace:literal,)*) => {
        $(
            #[test]
            fn $test() {
                println!("{}: {}", $trace, trace::locally(play::play($trace)));
            }
        )*

        /// Every trace there is a test for.
        const TRACES: &[&str] = &[$($trace),*];
    };
}

traces! {
    core_agents_server_001: "core-agents-server-001",
    core_agents_server_002: "core-agents-server-002",
    core_agents_server_003: "core-agents-server-003",
    core_agents_server_004: "core-agents-server-004",
    core_agents_server_005: "core-agents-server-005",
    core_agents_server_006: "core-agents-server-006",
    core_agents_server_007: "core-agents-server-007",
    core_agents_server_008: "core-agents-server-008",
    core_agents_server_009: "core-agents-server-009",
    corners_screens_001: "corners-screens-001",
    corners_screens_002: "corners-screens-002",
    corners_screens_003: "corners-screens-003",
    corners_screens_004: "corners-screens-004",
    corners_screens_005: "corners-screens-005",
    corners_screens_006: "corners-screens-006",
    corners_screens_007: "corners-screens-007",
    corners_screens_008: "corners-screens-008",
    corners_screens_009: "corners-screens-009",
    corners_screens_010: "corners-screens-010",
}

/// The traces of the screens the recorder made, by name.
fn recorded() -> BTreeSet<String> {
    trace::names(&SUITES).into_iter().collect()
}

#[test]
fn every_trace_of_the_screens_there_is_has_a_test_and_no_test_is_for_none() {
    let tested: BTreeSet<String> = TRACES.iter().map(|name| (*name).to_owned()).collect();
    assert_eq!(recorded(), tested);
    assert_eq!(TRACES.len(), 19);
}

/// The world is read as the recorder reads it: a roster's `createdAt` and
/// `updatedAt` are the clock's and are masked, and a time anywhere else, in the
/// roster or in any other file, is held to what Node recorded.
#[test]
fn the_world_masks_what_the_recorder_masks_and_nothing_else() {
    let time = "2026-10-05T10:00:00.123Z";
    let roster = format!(
        "{{\n  \"agents\": [\n    {{\n      \"name\": \"{time}\",\n      \"createdAt\": \"{time}\",\n      \"updatedAt\": \"{time}\"\n    }}\n  ],\n  \"checkedAt\": \"{time}\"\n}}\n"
    );
    let masked = roster
        .replace(
            r#""createdAt": "2026-10-05T10:00:00.123Z""#,
            r#""createdAt": "«now»""#,
        )
        .replace(
            r#""updatedAt": "2026-10-05T10:00:00.123Z""#,
            r#""updatedAt": "«now»""#,
        );
    assert_ne!(masked, roster);
    for path in ["consensflow/agents.json", "agents.json", "home/agents.json"] {
        assert_eq!(wrote::masked(path, &roster), masked, "{path}");
    }
    for path in ["bin/claude", "consensflow/agents.json.bak", "agents.jsonl"] {
        assert_eq!(wrote::masked(path, &roster), roster, "{path}");
    }
    // The clock's stamp is a time: any other text there is not the clock's.
    let corrupt = roster.replace(time, "garbage");
    assert_eq!(wrote::masked("agents.json", &corrupt), corrupt);
}

/// Every exchange the traces record whose answer is Node's V8 words for a body that is no JSON.
#[test]
fn the_exchanges_that_answer_in_other_words_are_exactly_those_that_carry_v8_s() {
    let mut carrying = BTreeSet::new();
    for name in recorded() {
        for step in trace::load(&name)["steps"].as_array().unwrap() {
            let words = step["response"]["body"]
                .as_str()
                .and_then(|body| serde_json::from_str::<Value>(body).ok())
                .and_then(|body| body["error"].as_str().map(str::to_owned));
            if words.is_some_and(|words| play::v8_json_words(&words)) {
                carrying.insert((name.clone(), step["id"].as_u64().unwrap()));
            }
        }
    }
    let listed: BTreeSet<(String, u64)> = play::DEPARTURES
        .iter()
        .map(|(name, id)| ((*name).to_owned(), *id))
        .collect();
    assert_eq!(carrying, listed);
}

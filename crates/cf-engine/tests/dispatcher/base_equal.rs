//! What a hold keeps, shows and tells of a harness's words reads byte for byte as the base's
//! (T-2353, round 2: this branch adds no free text of a harness's anywhere). For each input of
//! `base_equal.inputs.json` (the probe inputs of T-2350, T-2351 and T-2361, the repository's
//! refusal fixtures, and the cases of the checks' findings), as an account's refusal (billing
//! and login), a model's move and a busy refusal, the test records what the member's window
//! says (first look, and a look an hour on), the notes the chief and the human are told, the
//! task's aside, the board's mark of the member and the `member.out` events, and compares them
//! with `base_equal.golden.json`: the same test run on the base (`3375b7162`) with
//! `CF_RECORD_GOLDEN=<path>`. The only differences the form allows are the keys it adds:
//! `participant` and `form` of a `member.out` event, `outForm` of the board's participant.

use std::fmt::Write as _;

use cf_engine::testing::Context;
use cf_harness::records::Quota;
use serde_json::{json, Value};

use crate::fixtures::{now, Tiers};

fn quota_of(kind: &str, words: &str, at: String) -> Quota {
    match kind {
        "login" => Quota::Unavailable {
            at: Some(at),
            because: format!("its login was refused: {}", words.trim()),
        },
        "billing" => Quota::Unavailable {
            at: Some(at),
            because: format!("its billing was refused: {}", words.trim()),
        },
        "moved" => Quota::Moved {
            at: Some(at),
            because: words.to_owned(),
        },
        _ => Quota::Busy {
            at: Some(at),
            because: words.to_owned(),
        },
    }
}

/// What the hold of `kind` with `words` kept and told, as the base kept and told it.
fn observe(kind: &str, words: &str) -> Value {
    // The base held a refused login at its first refusal: with the tries off so does this build.
    let context = Context::tries_off();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let at = now(&context);
    context
        .adapter
        .refuse("zeus-amber-pine", quota_of(kind, words, at));
    context.pass().unwrap();
    let zeus = tiers.id("zeus-amber-pine");
    let first = context.dispatcher.activity(zeus).reason;
    context.advance(3_600_000);
    context.pass().unwrap();
    let later = context.dispatcher.activity(zeus).reason;
    let events = context
        .ledger
        .borrow()
        .events(tiers.project.id, 0, 5000)
        .unwrap();
    let mut outs: Vec<Value> = events
        .iter()
        .filter(|event| event.kind == "member.out")
        .map(|event| {
            let mut data = event.data.clone();
            if let Some(map) = data.as_object_mut() {
                // What the closed form adds, and nothing else.
                map.remove("participant");
                map.remove("form");
            }
            data
        })
        .collect();
    outs.dedup();
    let board =
        serde_json::to_value(context.ledger.borrow().board(tiers.project.id).unwrap()).unwrap();
    let mut lane = board["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|lane| lane["participant"]["handle"] == "zeus")
        .unwrap()
        .clone();
    if let Some(map) = lane["participant"].as_object_mut() {
        map.remove("outForm");
    }
    json!({
        "first": first,
        "later": later,
        "memberOut": outs,
        "participant": lane["participant"],
        "chiefNotes": tiers.notes("chief"),
        "humanNotes": crate::fixtures::notes(&context, context.id(tiers.project.id, "human")),
        "aside": tiers.task(1).task.aside,
    })
}

// The golden is recorded by running this test on the base: the one reading of the environment.
#[allow(clippy::disallowed_methods)]
#[test]
fn a_hold_keeps_shows_and_tells_of_a_harness_s_words_what_the_base_did() {
    let inputs: Vec<String> =
        serde_json::from_str(include_str!("base_equal.inputs.json")).expect("the inputs");
    let mut observed = serde_json::Map::new();
    for words in &inputs {
        for kind in ["login", "billing", "moved", "busy"] {
            observed.insert(format!("{kind}: {words}"), observe(kind, words));
        }
    }
    let observed = Value::Object(observed);
    if let Ok(path) = std::env::var("CF_RECORD_GOLDEN") {
        std::fs::write(path, serde_json::to_string_pretty(&observed).unwrap()).unwrap();
        return;
    }
    let golden: Value = serde_json::from_str(include_str!("base_equal.golden.json")).unwrap();
    let (observed, golden) = (observed.as_object().unwrap(), golden.as_object().unwrap());
    assert_eq!(observed.len(), golden.len(), "the inputs");
    let mut differ = String::new();
    for (case, base) in golden {
        if observed.get(case) != Some(base) {
            let _ = writeln!(differ, "{case}");
        }
    }
    assert!(differ.is_empty(), "differs from the base:\n{differ}");
}

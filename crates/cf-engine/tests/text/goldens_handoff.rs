//! The tables of the handoff in `tests/goldens/text.json`: the new chief's
//! first message, the human's last words, `cf history`'s pages and what is a
//! handoff, and `toLowerCase`, which a search reads by.

use std::collections::HashSet;

use cf_engine::handoff::{
    handoff_text, history_page, history_pages, is_handoff, last_words, Handoff, LastWords,
};
use cf_proto::ledger::{ChiefOpenWork, SwitchedFrom};
use serde_json::Value;

use super::support::{
    assert_same_text, conversations_of, known, message, seat, table, task_of, GOLDEN,
};

#[test]
fn every_code_point_lower_cases_as_node_lower_cased_it() {
    let lower = &GOLDEN["lowerCase"];
    assert_eq!(lower["unicode"], "17.0");
    let changed = lower["changed"].as_array().unwrap();
    let mut differ = Vec::new();
    for row in changed {
        let code = u32::try_from(row[0].as_u64().unwrap()).unwrap();
        let character = char::from_u32(code).unwrap().to_string();
        let lowered = character.to_lowercase();
        if lowered != row[1].as_str().unwrap() {
            differ.push(format!("U+{code:04X}: {lowered:?}, Node {}", row[1]));
        }
    }
    assert!(
        differ.is_empty(),
        "{} differ:\n{}",
        differ.len(),
        differ.join("\n")
    );
    assert_eq!(changed.len(), 1488);
    // The rest are left as they are.
    let changed: HashSet<u64> = changed.iter().map(|row| row[0].as_u64().unwrap()).collect();
    let mut changed_here = Vec::new();
    for code in (0..=0x10_FFFF_u32).filter(|code| !changed.contains(&u64::from(*code))) {
        let Some(character) = char::from_u32(code) else {
            continue;
        };
        let alone = character.to_string();
        if alone.to_lowercase() != alone {
            changed_here.push(format!("U+{code:04X}"));
        }
    }
    assert!(
        changed_here.is_empty(),
        "changed here alone: {}",
        changed_here.join(" ")
    );
}

#[test]
fn a_text_lower_cases_as_node_lower_cased_it_with_its_neighbours_in_mind() {
    let texts = lower_texts();
    for row in texts {
        let text = row["text"].as_str().unwrap();
        assert_eq!(
            text.to_lowercase(),
            row["lower"].as_str().unwrap(),
            "{text:?}"
        );
    }
    assert_eq!(texts.len(), 31);
}

fn lower_texts() -> &'static [Value] {
    GOLDEN["lowerCase"]["texts"].as_array().unwrap()
}

#[test]
fn the_last_words_are_the_humans_as_node_found_them() {
    let rows = table("lastWords");
    for row in rows {
        let conversations = conversations_of(row);
        let found = last_words(&conversations);
        let expected = match &row["last"] {
            Value::Null => None,
            last => Some(LastWords {
                text: last["text"].as_str().unwrap().to_owned(),
                answered: last["answered"].as_bool().unwrap(),
            }),
        };
        assert_eq!(found, expected, "{row}");
    }
    assert_eq!(rows.len(), 33);
}

#[test]
fn a_note_is_a_handoff_as_node_read_it() {
    let rows = table("isHandoff");
    for row in rows {
        let note = message(
            1,
            row["kind"].as_str().unwrap(),
            row["sender"].as_str(),
            None,
            row["body"].as_str().unwrap(),
        );
        assert_eq!(
            is_handoff(&note),
            row["handoff"].as_bool().unwrap(),
            "{row}"
        );
    }
    assert_eq!(rows.len(), 60);
}

#[test]
fn every_handoff_reads_as_node_wrote_it() {
    let rows = table("handoffText");
    let mut halved = 0;
    for row in rows {
        let from = SwitchedFrom {
            harness: row["from"]["harness"].as_str().unwrap().to_owned(),
            agent: row["from"]["agent"].as_str().map(str::to_owned),
        };
        let to = seat(row["to"]["harness"].as_str(), row["to"]["agent"].as_str());
        let open = ChiefOpenWork {
            questions: row["open"]["questions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|question| {
                    message(
                        question["id"].as_i64().unwrap(),
                        "question",
                        question["sender"].as_str(),
                        question["taskNumber"].as_i64(),
                        question["body"].as_str().unwrap(),
                    )
                })
                .collect(),
            results: row["open"]["results"]
                .as_array()
                .unwrap()
                .iter()
                .map(task_of)
                .collect(),
            own: row["open"]["own"]
                .as_array()
                .unwrap()
                .iter()
                .map(task_of)
                .collect(),
        };
        let last = row["last"].as_object().map(|last| LastWords {
            text: last["text"].as_str().unwrap().to_owned(),
            answered: last["answered"].as_bool().unwrap(),
        });
        let text = handoff_text(&Handoff {
            from: &from,
            to: &to,
            open: &open,
            last: last.as_ref(),
            cut: row["cut"].as_bool().unwrap(),
            pages: usize::try_from(row["pages"].as_u64().unwrap()).unwrap(),
        });
        assert_same_text(
            &text,
            row["text"].as_str().unwrap(),
            &format!("the handoff of {}", row["to"]),
        );
        halved += usize::from(row.get("halved").is_some());
    }
    assert_eq!(rows.len(), 39);
    assert_eq!(halved, 5);
}

#[test]
fn every_history_has_as_many_pages_as_node_counted() {
    let rows = table("historyPages");
    for row in rows {
        let pages = history_pages(&conversations_of(row), &known).unwrap();
        assert_eq!(
            pages as u64,
            row["pages"].as_u64().unwrap(),
            "{}",
            row["history"]
        );
    }
    assert_eq!(rows.len(), 25);
}

/// The number a row asks for: JSON holds no NaN or infinity, a row spells them.
fn page_of(value: &Value) -> f64 {
    match value {
        Value::Object(spelled) => spelled["number"].as_str().unwrap().parse().unwrap(),
        number => number.as_f64().unwrap(),
    }
}

#[test]
fn every_page_of_cf_history_reads_as_node_wrote_it_and_a_page_that_is_none_is_refused() {
    let rows = table("historyPage");
    let (mut pages, mut refused, mut halved) = (0, 0, 0);
    for row in rows {
        let conversations = conversations_of(row);
        let asked = format!(
            "{} {} page {} find {} tools {}",
            row.get("history").unwrap_or(&Value::Null),
            if row.get("history").is_some() {
                ""
            } else {
                "(its own)"
            },
            row["page"],
            row["find"],
            row["tools"]
        );
        let shown = history_page(
            &conversations,
            &known,
            page_of(&row["page"]),
            row["find"].as_str(),
            row["tools"].as_bool().unwrap(),
        );
        match (shown, row.get("answer"), row.get("rangeError")) {
            (Ok(shown), Some(answer), None) => {
                assert_eq!(
                    shown.page as u64,
                    answer["page"].as_u64().unwrap(),
                    "{asked}"
                );
                assert_eq!(
                    shown.pages as u64,
                    answer["pages"].as_u64().unwrap(),
                    "{asked}"
                );
                assert_same_text(&shown.text, answer["text"].as_str().unwrap(), &asked);
                halved += usize::from(answer.get("halved").is_some());
                pages += 1;
            }
            (Err(refusal), None, Some(message)) => {
                assert_eq!(
                    (refusal.code, refusal.status, refusal.message.as_str()),
                    ("no-such-page", 400, message.as_str().unwrap()),
                    "{asked}"
                );
                refused += 1;
            }
            (shown, answer, error) => {
                panic!("{asked}: Rust {shown:?}, Node {answer:?} {error:?}")
            }
        }
    }
    assert_eq!(rows.len(), 326);
    assert_eq!((pages, refused, halved), (276, 50, 6));
}

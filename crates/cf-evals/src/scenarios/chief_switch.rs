//! A chief switched to another harness knows what the old one was told. The
//! owner tells the first chief a codeword (random per run, so no model knows it
//! from anywhere else) and a decision, then pastes notes long enough to push
//! both off the first page of the chief's history; then switches the chief to the
//! staff's harness (`--switch-to`) and asks the new one. Only `cf history`
//! holds the answer: the handoff quotes the owner's last words, the notes.

use std::time::Duration;

use super::{expect, FollowUp, Scenario};
use crate::random;

/// The birds a codeword begins with.
const BIRDS: [&str; 6] = ["TERN", "LARK", "WREN", "KITE", "ROOK", "SWIFT"];

/// A codeword for a run: a bird and four digits, as `TERN-4821`.
pub(super) fn codeword() -> String {
    format!(
        "{}-{}",
        BIRDS[random::below(BIRDS.len())],
        1000 + random::below(9000)
    )
}

/// What the owner pastes after the codeword: seventy notes.
fn notes() -> String {
    (1..=70)
        .map(|n| {
            format!(
                "Note {n}: the style guide asks for sentence case in headings, short paragraphs and alt text on every image; check item {n} before the next review."
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn scenario() -> Scenario {
    with(&codeword())
}

/// The scenario of a run whose first chief is told `codeword`.
pub(super) fn with(codeword: &str) -> Scenario {
    let said = codeword.to_owned();
    Scenario {
        id: "chief-switch",
        title: "A chief switched to another harness knows what the old one was told",
        fixture: "site",
        prompt: format!(
            "Two things to remember for later; nothing to do yet. The codeword for this release is {codeword}. And we decided the release goes out on Friday, not Thursday. Confirm in one line."
        ),
        follow_ups: vec![
            FollowUp::Say(format!(
                "My notes from today's review, so you have them; nothing to do yet, confirm in one line.\n{}",
                notes()
            )),
            FollowUp::Switch,
            FollowUp::Say(
                "What is the codeword for this release, and which day did we decide it goes out? Answer from what you know; change nothing."
                    .to_owned(),
            ),
        ],
        answers: Vec::new(),
        fallback: "Nothing to do yet; just answer.".to_owned(),
        nudge: None,
        quiet: Duration::from_secs(90),
        expectations: vec![
            expect("the chief was switched once, to another harness", |m| {
                m.switches == 1 && m.chiefs.len() == 2 && m.chiefs[0] != m.chiefs[1]
            }),
            expect(
                "the new chief read the chief's history with cf history",
                |m| !m.history_reads.is_empty(),
            ),
            expect("the new chief names the codeword", move |m| {
                m.chief_words_now.contains(&said)
            }),
            expect(
                "the new chief knows the release goes out on Friday",
                |m| m.chief_words_now.to_lowercase().contains("friday"),
            ),
            expect("nothing went on the board", |m| m.tasks.is_empty()),
        ],
    }
}

//! The small case: one wrong word on one page. A chief that puts this on the
//! board, asks advice or asks the owner has misjudged the size; a chief that fixes
//! it and says so has not.

use std::time::Duration;

use super::{expect, pattern, Scenario};

pub(super) fn scenario() -> Scenario {
    let says = pattern("0 și 10|0 si 10|0–10|0-10");
    Scenario {
        id: "simple-fix",
        title: "One wrong word on one page",
        fixture: "site",
        uncommitted: None,
        prompt: "Pe pagina de evaluare (site/evaluare.html) scrie „un scor între 0 și 100”; scorul e de fapt între 0 și 10. Corectează, te rog.".to_owned(),
        follow_ups: Vec::new(),
        answers: Vec::new(),
        fallback: "Da.".to_owned(),
        nudge: None,
        quiet: Duration::from_secs(90),
        expectations: vec![
            expect("the page is fixed, and nothing else touched", |m| {
                m.files_changed == ["site/evaluare.html"]
            }),
            expect("no advice is asked for a one-word fix", |m| m.advice == 0),
            expect("the owner is not asked anything", |m| {
                m.asked_in_terminal() == 0
            }),
            expect("at most one task goes on the board", |m| {
                m.tasks.len() <= 1
            }),
            expect("the chief says what it did", move |m| {
                says.is_match(&m.chief_last_words)
            }),
        ],
    }
}

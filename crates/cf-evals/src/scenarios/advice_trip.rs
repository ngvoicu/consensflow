//! The advisor's plumbing: the owner asks for advice through the board, the
//! advice comes back to the chief, the chief accepts it and passes the
//! recommendation to the owner as a note. No file changes.

use std::time::Duration;

use super::{expect, join, Scenario};

pub(super) fn scenario() -> Scenario {
    Scenario {
        id: "advice-trip",
        title: "Advice out, advice back, a note to the owner",
        fixture: "site",
        prompt: join(&[
            "Un test de funcționare, te rog, fără să mă întrebi pe mine nimic. Cere sfatul unui advisor",
            "(cf task add --advice): pentru un site despre burnout, ce culoare e mai potrivită pentru",
            "butonul Contact, albastru sau verde, și de ce, în cel mult trei propoziții. Când vine",
            "răspunsul, acceptă sarcina și trimite-mi recomandarea lui ca notă (cf note --human).",
            "Nu modifica niciun fișier și nu pune alte sarcini.",
        ]),
        follow_ups: Vec::new(),
        answers: Vec::new(),
        fallback: "Da.".to_owned(),
        nudge: None,
        quiet: Duration::from_secs(90),
        expectations: vec![
            expect("one advice task goes on the board", |m| m.advice == 1),
            expect("the advice task is accepted", |m| {
                m.tasks
                    .iter()
                    .any(|task| task.pool.as_deref() == Some("advisor") && task.state == "accepted")
            }),
            expect("the recommendation reaches the owner as a note", |m| {
                !m.notes_to_human.is_empty()
            }),
            expect("no file changes", |m| m.files_changed.is_empty()),
            expect("the owner is not asked anything", |m| {
                m.asked_in_terminal() == 0
            }),
        ],
    }
}

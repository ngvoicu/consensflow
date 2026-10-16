//! The plumbing case: the owner asks for one task whose worker must ask the chief
//! something before it can finish, an answer, a result and a review. It measures
//! the board, not the chief's judgment: a brief delivered, a question from a
//! member to the chief, the answer back, the result back, a review, the task
//! accepted. Any harness in any role.

use std::time::Duration;

use super::{expect, join, Scenario};

pub(super) fn scenario() -> Scenario {
    Scenario {
        id: "round-trip",
        title: "A task out, a question back, an answer, a result, a review",
        fixture: "site",
        uncommitted: None,
        prompt: join(&[
            "Un test de funcționare, te rog, fără să mă întrebi pe mine nimic. Pune o singură sarcină",
            "unui worker: înainte de a scrie ceva, să te întrebe pe tine (cu cf ask) ce culoare să aibă",
            "butonul Contact, iar după răspunsul tău să scrie o singură linie cu culoarea în fișierul nou",
            "site/notes.md. Când te întreabă, răspunde-i „albastru”. Când sarcina e gata, cere unui reviewer",
            "să verifice site/notes.md, apoi acceptă sarcina. Nu modifica alte fișiere.",
        ]),
        follow_ups: Vec::new(),
        answers: Vec::new(),
        fallback: "Da.".to_owned(),
        nudge: None,
        quiet: Duration::from_secs(90),
        expectations: vec![
            expect("one task goes to a worker", |m| {
                m.tasks.iter().any(|task| {
                    task.pool.as_deref() != Some("reviewer") && task.pool.as_deref() != Some("advisor")
                })
            }),
            expect("the worker asks the chief on the board", |m| {
                m.plumbing.member_questions >= 1
            }),
            expect("the chief answers on the board", |m| {
                m.plumbing.member_questions_answered >= 1
            }),
            expect("the finished work goes to a review", |m| m.reviews >= 1),
            expect("the task is accepted", |m| m.plumbing.accepted >= 1),
            expect("only site/notes.md is new", |m| {
                m.files_changed == ["site/notes.md"]
            }),
            expect("the owner is not asked anything", |m| {
                m.asked_in_terminal() == 0
            }),
        ],
    }
}

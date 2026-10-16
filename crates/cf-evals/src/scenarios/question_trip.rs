//! Every kind of member asks the chief. A worker, an advisor and a reviewer each
//! ask the chief one question before they finish, the reviewer's one 6250
//! characters long (a file it pastes whole, a code on its last line): the question
//! reaches the chief, the chief answers, the answer reaches the member's own
//! window, the result comes back. The chief's note to the owner must hold the
//! code, proof it read the long question to its end. Run with one harness as the
//! whole staff, once per harness; the report says who asked, by role and harness.
//!
//! The members ask with `cf ask` ([`scenario`]), or (`question-trip-native`,
//! [`native`]) with their harness's own question tool: whether that reaches the
//! chief, and the answer the member, per harness, is what it measures.

use std::time::Duration;

use super::{expect, join, Scenario};

/// The question trip in which the members ask `how`.
fn trip(id: &'static str, title: &'static str, how: &str) -> Scenario {
    let pool_is = |pool: &'static str| {
        move |m: &crate::measure::Metrics| {
            m.tasks
                .iter()
                .any(|task| task.pool.as_deref() == Some(pool))
        }
    };
    let (advisor, reviewer) = (pool_is("advisor"), pool_is("reviewer"));
    let mut expectations = vec![expect(
        "a worker, an advisor and a reviewer each get a task",
        move |m| {
            advisor(m)
                && reviewer(m)
                && m.tasks.iter().any(|task| {
                    task.pool.as_deref() != Some("advisor")
                        && task.pool.as_deref() != Some("reviewer")
                })
        },
    )];
    for (role, name) in [
        (
            "worker",
            "the worker asks the chief, and the answer reaches its window",
        ),
        (
            "advisor",
            "the advisor asks the chief, and the answer reaches its window",
        ),
        (
            "reviewer",
            "the reviewer asks the chief, and the answer reaches its window",
        ),
    ] {
        expectations.push(expect(name, move |m| {
            m.member_questions_by
                .iter()
                .any(|row| row.role == role && row.asked >= 1 && row.delivered >= 1)
        }));
    }
    expectations.extend([
        expect(
            "the long question reaches the chief whole (over 4000 characters)",
            |m| {
                m.member_questions_by
                    .iter()
                    .map(|row| row.longest)
                    .max()
                    .unwrap_or(0)
                    > 4000
            },
        ),
        expect("the chief's note holds the long question's code", |m| {
            m.notes_text.contains("PLOP-6142")
        }),
        expect("all three tasks are accepted", |m| m.plumbing.accepted >= 3),
        expect("the owner is not asked anything", |m| {
            m.asked_in_terminal() == 0
        }),
    ]);
    Scenario {
        id,
        title,
        fixture: "questions",
        uncommitted: None,
        prompt: join(&[
            "Un test de funcționare pentru întrebări, fără să mă întrebi pe mine nimic. Pune pe board",
            "trei sarcini, toate --tier standard, și scrie în fiecare brief exact ce să te întrebe și",
            &format!("că întrebarea ți-o pune {how}:"),
            "(1) unui worker: înainte de orice, să te întrebe ce culoare să aibă butonul Contact, apoi să",
            "scrie o singură linie cu culoarea în fișierul nou site/notes.md;",
            "(2) unui advisor (--advice): înainte de răspuns, să te întrebe pentru ce public e site-ul, apoi",
            "să recomande un font;",
            "(3) unui reviewer (--review): să verifice site/index.html, iar înainte de concluzii să te",
            "întrebe punând în întrebare, întreg și exact, textul din interne/intrebare-reviewer.md (e lung,",
            "cu un cod pe ultimul rând).",
            "Răspunde-le: workerului „albastru”, advisorului „manageri”, reviewerului „doar versiunea",
            "română”. Când ai toate trei rezultatele, acceptă-le și scrie-mi o singură notă cu ce a întrebat",
            "fiecare și codul de pe ultimul rând al întrebării reviewerului. Nu modifica alte fișiere.",
        ]),
        follow_ups: Vec::new(),
        answers: Vec::new(),
        fallback: "Da.".to_owned(),
        nudge: None,
        quiet: Duration::from_secs(120),
        expectations,
    }
}

/// The members ask with `cf ask`.
pub(super) fn scenario() -> Scenario {
    trip(
        "question-trip",
        "A worker, an advisor and a reviewer each ask the chief (cf ask)",
        "cu cf ask",
    )
}

/// The members ask with their harness's own question tool.
pub(super) fn native() -> Scenario {
    trip(
        "question-trip-native",
        "A worker, an advisor and a reviewer each ask the chief with their harness's own question tool",
        "cu unealta proprie de întrebări a harness-ului lor (nu cu cf ask)",
    )
}

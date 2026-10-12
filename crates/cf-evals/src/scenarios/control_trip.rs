//! The chief's controls over a running task: `cf tell` stops a worker and asks it
//! something, the worker answers, `cf task resume` sends it on, and `cf task add
//! --after` gives a follow-up to the same window. Plumbing, not judgment: the
//! owner spells out the steps.

use std::time::Duration;

use super::{expect, join, Scenario};

pub(super) fn scenario() -> Scenario {
    Scenario {
        id: "control-trip",
        title: "Tell, answer, resume, a follow-up to the same window",
        fixture: "site",
        prompt: join(&[
            "Un test de funcționare, te rog, fără să mă întrebi nimic pe mine. Pașii, în ordine:",
            "(1) Pune o sarcină unui worker: să ruleze întâi comanda `sleep 90`, apoi să scrie în fișierul",
            "nou site/notes.md o singură linie, „unu”. (2) Când sarcina e în lucru (verifică cu cf task get",
            "T-1), oprește-o cu cf tell T-1 și întreabă-l în ce fișier va scrie. (3) După ce îți răspunde,",
            "reia sarcina cu cf task resume T-1 „continuă de unde ai rămas”. (4) Când vine rezultatul,",
            "acceptă T-1 și dă o continuare aceleiași ferestre cu cf task add --after T-1: să adauge în",
            "site/notes.md a doua linie, „doi”. (5) Când vine și acest rezultat, acceptă-l. Nu modifica",
            "alte fișiere și nu pune alte sarcini.",
        ]),
        follow_ups: Vec::new(),
        answers: Vec::new(),
        fallback: "Da.".to_owned(),
        nudge: None,
        // Longer than the Claude and Devin adapters' two-minute draft grace: the
        // daemon's own Escape on a pause counts as typing, so a tell to those
        // windows waits out the grace before it is pasted (a known bug).
        quiet: Duration::from_secs(240),
        expectations: vec![
            expect("the chief stops the task with cf tell", |m| {
                m.plumbing.tells >= 1
            }),
            expect("the worker answers the tell", |m| {
                m.plumbing.tells_answered >= 1
            }),
            expect("the task is paused and resumed", |m| {
                m.plumbing.pauses >= 1 && m.plumbing.resumes >= 1
            }),
            expect("a follow-up goes to the same window (--after)", |m| {
                m.plumbing.continuations >= 1
            }),
            expect("both tasks are accepted", |m| m.plumbing.accepted >= 2),
            expect("only site/notes.md is new", |m| {
                m.files_changed == ["site/notes.md"]
            }),
            expect("the owner is not asked anything", |m| {
                m.asked_in_terminal() == 0
            }),
        ],
    }
}

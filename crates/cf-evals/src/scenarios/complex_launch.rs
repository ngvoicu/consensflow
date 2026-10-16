//! The large case: a launch with research, several pages, a hard call and work
//! the owner must sign off. Everything the board is for at once: work that runs
//! side by side, advice for the call, a review before the owner sees it, and the
//! owner's decisions put to the owner.

use std::time::Duration;

use super::{answer, expect, join, Scenario};

pub(super) fn scenario() -> Scenario {
    Scenario {
        id: "complex-launch",
        title: "A launch: research, pages, a hard call, a sign-off",
        fixture: "site",
        uncommitted: None,
        prompt: join(&[
            "Lansăm site-ul luna viitoare și vreau trei lucruri deodată. Unu: o pagină nouă „Legislație”",
            "în română și engleză, din content/legislatie-ro.md, terminată și tradusă (secțiunile lipsă sunt",
            "marcate). Doi: o pagină „Pentru manageri” care explică ce văd managerii după evaluare, pornind",
            "de la docs/ghidul-managerului.md; verifică că ce spune ghidul chiar corespunde cu ce arată",
            "site-ul. Trei: un raport scurt (docs/lansare.md) despre ce ne lipsește ca site-ul să fie",
            "publicabil (accesibilitate, ce pagini lipsesc în engleză, ce texte sunt vechi), cu o recomandare",
            "de ordine. Nu publicăm nimic fără să văd eu; unde trebuie o decizie care e a mea, întreabă-mă.",
        ]),
        follow_ups: Vec::new(),
        // The specific subjects first: most questions mention publishing in passing.
        answers: vec![
            answer(
                "(?i)document(ul)? de referin|reference document",
                "Îl păstrăm, cu o notă că e vechi.",
            ),
            answer(
                "(?i)ordine|order|întâi|first",
                "Legislația întâi, apoi managerii, apoi raportul.",
            ),
            answer("(?i)recomand|recommend", "Da, cum recomanzi."),
            answer("(?i)publica|publish", "Nu încă; după ce văd paginile."),
        ],
        fallback: "Da.".to_owned(),
        nudge: Some("Da, cum recomanzi. Continuă."),
        quiet: Duration::from_secs(150),
        expectations: vec![
            expect("at least three tasks go on the board", |m| m.tasks.len() >= 3),
            expect("two tasks run side by side at some point", |m| m.parallel >= 2),
            expect("advice is asked at least once", |m| m.advice >= 1),
            expect("finished work goes to a review", |m| m.reviews >= 1),
            expect(
                "the owner is asked in the terminal, at least two questions",
                |m| m.asked_in_terminal() >= 2,
            ),
            expect(
                "a finding reaches the owner as a note (the guide and the site disagree)",
                |m| !m.notes_to_human.is_empty(),
            ),
            expect(
                "the chief's own edits stay under ten (counted for a Claude chief only)",
                |m| m.chief_edits.is_none_or(|edits| edits < 10),
            ),
        ],
    }
}

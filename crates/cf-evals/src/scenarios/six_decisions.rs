//! From the btb transcript of 2026-09-25: a small bilingual site, a new page to
//! add, content to write and to translate, and choices only the owner can make.
//! That evening the real chief did every edit itself, put one task on the board,
//! and wrote its six questions as a report in its terminal.

use std::time::Duration;

use super::{answer, expect, join, Scenario};

pub(super) fn scenario() -> Scenario {
    Scenario {
        id: "six-decisions",
        title: "A new page with owner decisions in it",
        fixture: "site",
        uncommitted: None,
        prompt: join(&[
            "Pe site, în meniul de jos (Despre noi, FAQ, Contact, în română și engleză) vrem și un",
            "buton Legislație, care duce la o pagină nouă cu legislația despre burnout la locul de",
            "muncă. Textul în română e început în content/legislatie-ro.md, dar trebuie terminat",
            "(secțiunile lipsă sunt marcate) și apoi tradus în engleză. Vechiul „Document de referință",
            "pentru HR” din docs/ nu mai știu dacă îl păstrăm. Nu publica nimic până nu ne înțelegem;",
            "pagina se publică doar când zic eu.",
        ]),
        follow_ups: Vec::new(),
        // The owner's answers, free text, by what the question is about (the specific
        // subjects first: most questions mention publishing in passing); then options;
        // then the fallback.
        answers: vec![
            answer(
                "(?i)document(ul)? de referin|reference document|docs/",
                "Îl păstrăm, dar pune sus o notă că pagina de legislație e sursa actuală.",
            ),
            answer("(?i)recomand|recommend", "Da, cum recomanzi."),
            answer(
                "(?i)publica|publish",
                "Nu publicăm încă. Vreau să văd pagina întâi; îți spun eu când.",
            ),
        ],
        fallback: "Da.".to_owned(),
        // What the owner types into the chief's terminal when the chief stops there
        // without asking anything.
        nudge: Some("Da, cum recomanzi. Continuă."),
        // Stop once nothing has moved for this long after the last activity.
        quiet: Duration::from_secs(120),
        expectations: vec![
            expect(
                "the owner is asked in the terminal, at least three questions",
                |m| m.asked_in_terminal() >= 3,
            ),
            expect("nothing is asked on the board", |m| {
                m.questions_on_board == 0
            }),
            expect("the chief never tries cf ask", |m| m.ask_refused == 0),
            expect("a finding reaches the owner as a note", |m| {
                !m.notes_to_human.is_empty()
            }),
            expect("at least two tasks go on the board", |m| {
                m.tasks.len() >= 2
            }),
            expect("two tasks run side by side at some point", |m| {
                m.parallel >= 2
            }),
            expect("finished work goes to a review", |m| m.reviews >= 1),
            expect(
                "the chief's own edits stay under ten (counted for a Claude chief only)",
                |m| m.chief_edits.is_none_or(|edits| edits < 10),
            ),
        ],
    }
}

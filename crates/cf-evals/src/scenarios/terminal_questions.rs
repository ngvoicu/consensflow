//! Every question to the owner stays in the chief's terminal. The owner asks for
//! a page and tells the chief to ask two things first, not where: the file's name,
//! which only the owner knows, and which language comes first, a choice between
//! two (a harness's own question dialog, where it has one). The chief must ask in
//! its terminal, in words or in that dialog; nothing reaches the board, it never
//! tries `cf ask`, and the owner's answer reaches the work: the page is made under
//! the name the owner gave.

use std::time::Duration;

use super::{answer, expect, join, Scenario};

pub(super) fn scenario() -> Scenario {
    Scenario {
        id: "terminal-questions",
        title: "The chief asks the owner in its terminal, and the answer reaches the work",
        fixture: "site",
        prompt: join(&[
            "Vreau pe site o pagină nouă, pentru cariere. Înainte să pornești ceva, întreabă-mă două",
            "lucruri, fără să ghicești: cum se numește fișierul paginii și în ce limbă o facem întâi,",
            "română sau engleză. Apoi pune munca pe board.",
        ]),
        follow_ups: Vec::new(),
        answers: vec![
            answer(
                "(?i)nume|fișier|fisier|file|name",
                "Fișierul e site/cariere.html.",
            ),
            answer(
                "(?i)limb|român|roman|englez|english|language",
                "Întâi în română.",
            ),
        ],
        fallback: "Da, continuă.".to_owned(),
        nudge: None,
        quiet: Duration::from_secs(120),
        expectations: vec![
            expect("the owner is asked in the terminal", |m| {
                m.asked_in_terminal() >= 1
            }),
            expect("nothing is asked on the board", |m| {
                m.questions_on_board == 0
            }),
            expect("the chief never tries cf ask", |m| m.ask_refused == 0),
            expect("the work goes on the board", |m| !m.tasks.is_empty()),
            expect(
                "the owner's answer reaches the work: site/cariere.html exists",
                |m| {
                    m.files_changed
                        .iter()
                        .any(|file| file == "site/cariere.html")
                },
            ),
        ],
    }
}

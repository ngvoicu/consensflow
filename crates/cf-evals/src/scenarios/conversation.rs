//! The owner works with the chief the way the project's owner does: a conversation of short
//! messages, each after the chief is done with the last, most of them small
//! changes. On btb (2026-09-25 to 28) a Claude chief took three days of such
//! messages and put nothing on the board, doing every change itself. This
//! measures how much of that work goes out: tasks on the board against the files
//! that changed. (A chief's own edit count misses edits made with shell commands,
//! which is how an Opus chief made all seven here.)
//!
//! Tightened on 2026-10-31 (T-972): a Haiku 4.5 chief put three tasks of its own plan on the
//! board (the site's missing pages, from the owner's first question), made all seven of the
//! owner's changes itself (19 edits) and wrote the English evaluation page itself, and the
//! three expectations of the time all held: three tasks, a title with "English" in it, the
//! page there. Now only a task that carries one of the owner's changes counts, the English
//! task must name the evaluation page, and a chief's own edits are none.

use std::time::Duration;

use super::{expect, pattern, FollowUp, Scenario};

/// The changes the owner asks for, each with what a task's title that carries it says (in
/// Romanian or English, as a chief writes it): the guide's 0 to 10, the footer's link, the
/// CSS variables, the English evaluation page, the home page's title and the README. The
/// check of the links is no change: a chief may verify itself.
const CHANGES: [(&str, &str); 6] = [
    (
        "the guide's scale",
        r"(?i)ghid|guide|\b0\s*(la|to|-|–)\s*10\b",
    ),
    ("the footer's link", r"(?i)subsol|footer"),
    (
        "the CSS variables",
        r"(?i)\bcss\b|variabil|culor|colou?rs?\b",
    ),
    ("the English evaluation page", ENGLISH_EVALUATION),
    ("the home page's title", r"(?i)\btitl|\btitle"),
    ("the README", r"(?i)readme"),
];

/// A title that names the English evaluation page: the language (or the `en/` folder) and the
/// page, in either order.
const ENGLISH_EVALUATION: &str = r"(?i)(engl|\ben/).*(evalu|assess)|(evalu|assess).*(engl|\ben/)";

pub(super) fn scenario() -> Scenario {
    let changes: Vec<regex::Regex> = CHANGES.iter().map(|(_, source)| pattern(source)).collect();
    let english = pattern(ENGLISH_EVALUATION);
    Scenario {
        id: "conversation",
        title: "A working conversation of small asks: how many go to the board",
        fixture: "site",
        uncommitted: None,
        prompt: "Salut. Ce pagini are site-ul acum și ce face fiecare, pe scurt?".to_owned(),
        follow_ups: [
            "În site/evaluare.html scorul e de la 0 la 10, dar docs/ghidul-managerului.md zice de la 1 la 5. Corectează ghidul să spună 0 la 10.",
            "Adaugă în subsolul paginii principale un link către pagina de evaluare.",
            "Culorile din site/style.css sunt scrise direct peste tot; mută-le în variabile CSS.",
            "Fă și versiunea în engleză a paginii de evaluare, în site/en/evaluare.html.",
            "Titlul paginii principale să fie „Burnout la locul de muncă — ghid pentru manageri”.",
            "Verifică dacă merg toate linkurile interne din site.",
            "Scrie în README ce s-a schimbat azi, pe scurt.",
        ]
        .map(|message| FollowUp::Say(message.to_owned()))
        .to_vec(),
        answers: Vec::new(),
        fallback: "Da, cum crezi tu.".to_owned(),
        nudge: None,
        quiet: Duration::from_secs(120),
        expectations: vec![
            // A task of the chief's own plan, made of the owner's first question (what pages the
            // site has), carries none of the changes: only the owner's asks count (T-972: a
            // Haiku chief put its plan's three tasks on the board and made all seven changes
            // itself, and the three tasks passed this).
            expect(
                "at least three of the owner's changes go to the board",
                move |m| {
                    changes
                        .iter()
                        .filter(|change| m.tasks.iter().any(|task| change.is_match(&task.title)))
                        .count()
                        >= 3
                },
            ),
            expect(
                "the English evaluation page is a worker’s task (a translation runs on its own)",
                move |m| {
                    m.tasks.iter().any(|task| {
                        !matches!(task.pool.as_deref(), Some("advisor" | "reviewer"))
                            && english.is_match(&task.title)
                    })
                },
            ),
            expect(
                "the work is done: the English evaluation page exists",
                |m| {
                    m.files_changed
                        .iter()
                        .any(|file| file == "site/en/evaluare.html")
                },
            ),
            expect(
                "the chief makes no change itself: its own edits are 0 (counted for a Claude chief only)",
                |m| m.chief_edits.unwrap_or(0) == 0,
            ),
        ],
    }
}

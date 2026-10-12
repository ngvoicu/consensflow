//! Long messages both ways. The owner pastes a long message into the chief's
//! terminal; a worker, an advisor and a reviewer each send the chief a result of
//! at least 8000 characters; the owner answers the chief's question at length.
//! Each carries a code on its last line, and the chief's one note to the owner
//! must hold all five: proof that every long text was read to its end, delivered
//! whole (up to 16,000 characters) or as its opening and `cf inbox read m-N`.
//! Each member reads its code from a file only its brief names.

use std::time::Duration;

use super::{answer, expect, join, Scenario};

/// About 8000 characters of the owner's notes, pasted ahead of the code at its end.
fn context() -> String {
    (1..=12)
        .map(|section| {
            join(&[
                &format!("Secțiunea {section} din notițele ședinței despre site."),
                "Am discutat structura paginii principale, tonul textelor și felul în care un manager",
                "găsește repede evaluarea de burnout. Echipa vrea fraze scurte, un singur îndemn pe",
                "ecran și trimiteri clare către legislație, fără jargon medical. Pagina de evaluare",
                "rămâne pe scara de la 0 la 10, iar rezultatele nu se salvează nicăieri. Versiunea în",
                "engleză urmează după ce textul românesc e aprobat. Nimic din secțiunea aceasta nu",
                "cere vreo modificare acum; e doar context pentru cine citește până la capăt.",
            ])
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// About 6000 characters of the owner's answer; the report's name is its last line.
fn long_answer() -> String {
    let mut points: Vec<String> = (1..=15)
        .map(|point| {
            join(&[
                &format!("Punctul {point}: raportul final îl citesc eu și îl trimit mai departe conducerii."),
                "Vreau să fie clar din titlu despre ce e vorba, să nu se confunde cu rapoartele lunare,",
                "și să se poată căuta ușor în arhivă după nume. Nu vreau diacritice în nume, nici",
                "spații, pentru că arhiva le strică. Am mai avut rapoarte cu nume lungi și s-au pierdut.",
                "Numele trebuie să fie scurt, cu majuscule și cu un număr la sfârșit.",
            ])
        })
        .collect();
    points.push("Numele raportului final: DELTA-5530".to_owned());
    points.join("\n\n")
}

pub(super) fn scenario() -> Scenario {
    let long = long_answer();
    let context = context();
    Scenario {
        id: "long-trip",
        title: "Long messages both ways: members to the chief, the owner and the chief",
        fixture: "long",
        prompt: [
            "Un test de funcționare pentru mesaje lungi. Nu modifica niciun fișier.",
            "1. Pune o sarcină unui worker: să citească interne/worker.txt și să-ți trimită ca rezultat",
            "un raport de cel puțin 8000 de caractere despre site/index.html; ultima linie a",
            "rezultatului să fie exact codul din fișier.",
            "2. Cere sfatul unui advisor (cf task add --advice): să citească interne/advisor.txt și să-ți",
            "trimită un sfat de cel puțin 8000 de caractere despre cum se poate îmbunătăți site/index.html;",
            "ultima linie să fie exact codul din fișier.",
            "3. Cere o revizie unui reviewer (cf task add --review): să citească interne/reviewer.txt și",
            "site/index.html și să-ți trimită o recenzie de cel puțin 8000 de caractere; ultima linie să",
            "fie exact codul din fișier.",
            "Nu citi tu fișierele din interne/.",
            "4. Pune-mi aici, în terminal, o singură întrebare: cum se numește raportul final.",
            "5. Citește integral fiecare rezultat și răspunsul meu. Când le ai pe toate, acceptă sarcinile",
            "și trimite-mi o singură notă (cf note --human) cu cinci lucruri: codul workerului, codul",
            "advisorului, codul reviewerului, numele raportului din răspunsul meu și codul de la",
            "sfârșitul acestui mesaj.",
            "",
            "Context, de citit până la capăt:",
            "",
            context.as_str(),
            "",
            "Codul acestui mesaj: FARUL-2208",
        ]
        .join("\n"),
        follow_ups: Vec::new(),
        answers: vec![answer(".", long.clone())],
        fallback: long,
        nudge: None,
        quiet: Duration::from_secs(180),
        expectations: vec![
            expect(
                "the worker's result is long (8000 characters or more)",
                |m| m.longest_result.worker >= 8000,
            ),
            expect(
                "the advisor's advice is long (8000 characters or more)",
                |m| m.longest_result.advisor >= 8000,
            ),
            expect(
                "the reviewer's review is long (8000 characters or more)",
                |m| m.longest_result.reviewer >= 8000,
            ),
            expect("the owner's long answer went to the chief", |m| {
                // After the prompt, itself long: what the owner typed in answer.
                m.owner_messages.iter().skip(1).any(|length| *length >= 4000)
            }),
            expect("the note holds the worker's code", |m| {
                m.notes_text.contains("CEDRU-7314")
            }),
            expect("the note holds the advisor's code", |m| {
                m.notes_text.contains("ARTAR-2659")
            }),
            expect("the note holds the reviewer's code", |m| {
                m.notes_text.contains("STEJAR-4087")
            }),
            expect(
                "the note holds the report's name from the owner's answer",
                |m| m.notes_text.contains("DELTA-5530"),
            ),
            expect(
                "the note holds the code at the end of the owner's message",
                |m| m.notes_text.contains("FARUL-2208"),
            ),
            expect("no file changes", |m| m.files_changed.is_empty()),
        ],
    }
}

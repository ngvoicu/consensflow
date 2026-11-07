//! What the ledger tells a requester of tasks that failed alike (T-1196, the field board of
//! 2026-11-03: six working tasks failed on one network error over 25 minutes, and the chief
//! took 13 notes, most after it had reopened them). The engine's cases are `outage_notes` of
//! its dispatcher; these hold the words, and the way a note still on its way leaves with its
//! task.
//!
//! The note of one failure is the words a single failure has always had; the note of several
//! is written again, `superseded by m-<new>`, as each failure and each close of a window joins
//! it, while the requester has not been given it.

use cf_ledger::{window_closed, NewNote};

use crate::fixture::{world, World};

/// What the failures said, to tell one outage from the next.
const CAUSE: &str = "api error: can't reach the api server (enotfound)";

/// How a window's harness reported the failure of its task.
fn reported(handle: &str) -> String {
    format!(
        "@{handle}'s harness reported a failure; it said: \"API Error: Can't reach the API server (ENOTFOUND)\""
    )
}

/// T-`number`, given to `to`, fails with the words of the outage.
fn fails_alike(w: &mut World, to: &str) -> i64 {
    let number = w.give(to, "Parser").task.number;
    w.ledger
        .give_up_task_for_cause(w.project, number, &reported(to), CAUSE)
        .expect("given up");
    number
}

/// What the chief was sent that waits to be given: its id and its words, oldest first.
fn queued(w: &World) -> Vec<(i64, String)> {
    let mut notes: Vec<_> = w
        .ledger
        .inbox(w.id("chief"), 100)
        .expect("the inbox")
        .into_iter()
        .filter(|message| message.kind == "note" && message.state == "queued")
        .collect();
    notes.sort_by_key(|message| message.id);
    notes
        .into_iter()
        .map(|message| (message.id, message.body))
        .collect()
}

fn closed(w: &mut World, number: i64) {
    w.ledger
        .tell_closed(
            w.project,
            &NewNote {
                from: None,
                to: "chief".into(),
                task: Some(number),
                body: format!(
                    "{} with background work running, which its close ended: sleep 9",
                    window_closed(number)
                ),
            },
        )
        .expect("told");
}

#[test]
fn the_note_of_one_failure_is_the_words_a_single_failure_has_always_had_and_its_close_a_note_of_its_own(
) {
    let mut w = world();
    let a = fails_alike(&mut w, "zeus");
    closed(&mut w, a);

    assert_eq!(
        queued(&w).into_iter().map(|(_, body)| body).collect::<Vec<_>>(),
        [
            format!("T-{a} failed: {}. Reopen it with: cf task reopen T-{a} \"…\", or accept it with what you checked: cf task accept T-{a} \"…\"", reported("zeus")),
            format!("T-{a}'s window closed with background work running, which its close ended: sleep 9"),
        ]
    );
}

#[test]
fn tasks_that_failed_alike_are_one_note_with_what_waits_for_them_and_what_their_windows_closed() {
    let mut w = world();
    let a = fails_alike(&mut w, "zeus");
    closed(&mut w, a);
    let waiter = w.open_needing("Docs", &[a]).task.number;
    let before = queued(&w);
    assert_eq!(before.len(), 2, "{before:?}");

    let b = fails_alike(&mut w, "diana");
    closed(&mut w, b);

    let now = queued(&w);
    assert_eq!(now.len(), 1, "one note: {now:?}");
    assert_eq!(
        now[0].1,
        format!(
            "T-{a} and T-{b} failed within 1 minute of each other with the same error, which looks like an outage of the network or the API, not their work:\n\
             T-{a}: {}\n\
             T-{b}: {}\n\
             Waiting for them: T-{waiter} waits for T-{a}. Cancel what waits, or reopen what it waits for.\n\
             T-{a}'s window closed with background work running, which its close ended: sleep 9\n\
             T-{b}'s window closed with background work running, which its close ended: sleep 9\n\
             Reopen them all with: cf task reopen T-{a} \"…\"; cf task reopen T-{b} \"…\"",
            reported("zeus"),
            reported("diana"),
        )
    );
    // What it took in was withdrawn, each row saying which note replaced it.
    for (id, _) in before {
        let message = w.message(id);
        assert_eq!(message.state, "cancelled", "m-{id}");
        assert!(
            message
                .reason
                .as_deref()
                .is_some_and(|reason| reason.starts_with("superseded by m-")),
            "{message:?}"
        );
    }
}

#[test]
fn failures_that_say_nothing_of_their_cause_are_never_one_note() {
    let mut w = world();
    for to in ["zeus", "diana"] {
        let number = w.give(to, "Parser").task.number;
        w.ledger
            .give_up_task(w.project, number, &reported(to))
            .expect("given up");
    }

    assert_eq!(queued(&w).len(), 2, "{:?}", queued(&w));
}

#[test]
fn a_note_on_its_way_to_the_chief_that_was_not_handed_over_leaves_with_its_task() {
    let mut w = world();
    let a = fails_alike(&mut w, "zeus");
    let b = fails_alike(&mut w, "diana");
    let note = queued(&w).remove(0).0;
    // Its delivery has begun: it is the chief's turn to be pasted, and nothing is handed yet.
    w.begin(note);
    assert_eq!(w.message(note).state, "delivering");

    w.ledger
        .reopen_task(w.project, a, "chief", "Carry on")
        .expect("reopened");

    // T-b alone is written again in the words of one failure, and the note on its way is withdrawn.
    assert_eq!(w.message(note).state, "cancelled");
    let now = queued(&w);
    assert_eq!(now.len(), 1, "{now:?}");
    assert!(
        now[0].1.starts_with(&format!("T-{b} failed: ")),
        "{}",
        now[0].1
    );

    w.ledger
        .reopen_task(w.project, b, "chief", "Carry on")
        .expect("reopened");
    assert!(queued(&w).is_empty(), "{:?}", queued(&w));
}

#[test]
fn a_note_still_to_be_given_is_joined_however_many_failures_were_told_since() {
    let mut w = world();
    let first = fails_alike(&mut w, "zeus");
    // Two hundred and more failures of other words, told to the same requester after it.
    for other in 0..205 {
        let number = w.open("Docs").task.number;
        w.ledger
            .give_up_task_for_cause(
                w.project,
                number,
                "its harness said another thing",
                &format!("another {other}"),
            )
            .expect("given up");
    }

    let again = fails_alike(&mut w, "diana");

    let joined: Vec<_> = queued(&w)
        .into_iter()
        .filter(|(_, body)| body.starts_with(&format!("T-{first} and T-{again} failed within")))
        .collect();
    assert_eq!(
        joined.len(),
        1,
        "the oldest note is found, as the newest are"
    );
}

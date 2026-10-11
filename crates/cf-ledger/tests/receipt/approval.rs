//! Approval required: every word one agent gives another waits at the gate,
//! out of its recipient's inbox and its queue, until the human passes it on or
//! declines it. This is what each kind the gate holds does at it: the brief of
//! a task (and of one that waited for what it needed), the words that resume it
//! or send it back, a question and a tell, an answer, a note, and a result; what
//! a decision on a task does to the result
//! still at the gate; what the project does when approval is switched off while
//! words wait; and what a restart leaves.

use cf_ledger::MessageView;

use crate::fixture::{gated_world, world, World};

/// The human passes a message on.
fn approve(w: &mut World, id: i64) -> MessageView {
    w.ledger.approve_message(id, "human").expect("passed on")
}

/// A task given to zeus, its brief passed on and received: the task's number.
fn at_work(w: &mut World) -> i64 {
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    approve(w, brief);
    w.deliver(brief);
    task
}

/// The task as `at_work` leaves it, and finished: its result passed on and
/// given to the chief, who has not decided on it.
fn finished(w: &mut World) -> i64 {
    let task = at_work(w);
    let result = result_of(w, task);
    approve(w, result);
    w.deliver(result);
    task
}

/// A word held at the gate: its id, and whom it is for.
struct Held {
    id: i64,
    to: &'static str,
}

/// A kind of word the gate holds: how one comes to wait there, and whether the
/// human can decline it, or only pass it on.
struct Kind {
    what: &'static str,
    held: fn(&mut World) -> Held,
    declinable: bool,
}

/// Every kind of word the gate holds.
const KINDS: [Kind; 9] = [
    Kind {
        what: "the brief of a task",
        held: |w| Held {
            id: w.give("zeus", "Parser").message.expect("a brief").id,
            to: "zeus",
        },
        declinable: true,
    },
    Kind {
        what: "the brief of a task that waited for what it needed",
        held: |w| {
            let task = at_work(w);
            // The next task waits on the board for T-1, which the human accepts.
            w.give_to("zeus", &[task], "Lexer")
                .expect("a task that waits");
            result_of(w, task);
            w.ledger
                .accept_task(w.project, task, "human")
                .expect("accepted");
            let released = w
                .ledger
                .task(w.project, 2)
                .expect("a read")
                .expect("T-2")
                .messages;
            Held {
                id: released.last().expect("its brief").id,
                to: "zeus",
            }
        },
        declinable: true,
    },
    Kind {
        what: "the words that resume a task",
        held: |w| {
            let task = at_work(w);
            w.pause(task);
            Held {
                id: w.resume(task, "Use JSON").message.expect("its words").id,
                to: "zeus",
            }
        },
        declinable: true,
    },
    Kind {
        what: "the words that send a task back",
        held: |w| {
            let task = finished(w);
            let moved = w
                .ledger
                .reopen_task(w.project, task, "chief", "Handle empty input too")
                .expect("sent back");
            Held {
                id: moved.message.expect("its words").id,
                to: "zeus",
            }
        },
        declinable: true,
    },
    Kind {
        what: "a question",
        held: |w| {
            let task = at_work(w);
            Held {
                id: w.ask("zeus", task, "Which format?").id,
                to: "chief",
            }
        },
        declinable: false,
    },
    Kind {
        what: "a tell",
        held: |w| {
            let task = at_work(w);
            Held {
                id: w.tell("zeus", task, "Use JSON").id,
                to: "zeus",
            }
        },
        declinable: false,
    },
    Kind {
        what: "an answer",
        held: |w| {
            let task = at_work(w);
            let question = w.ask("zeus", task, "Which format?").id;
            approve(w, question);
            Held {
                id: w.answer(question, "JSON").id,
                to: "zeus",
            }
        },
        declinable: true,
    },
    Kind {
        what: "a note",
        held: |w| {
            let task = at_work(w);
            Held {
                id: w.note("chief", "zeus", task, "Mind the tests").id,
                to: "zeus",
            }
        },
        declinable: false,
    },
    Kind {
        what: "a result",
        held: |w| {
            let task = at_work(w);
            Held {
                id: result_of(w, task),
                to: "chief",
            }
        },
        declinable: false,
    },
];

#[test]
fn every_kind_of_word_the_gate_holds_is_kept_from_its_recipient_until_the_human_passes_it_on() {
    for kind in KINDS {
        let mut w = gated_world();
        let held = (kind.held)(&mut w);
        let recipient = w.id(held.to);
        assert_eq!(w.message(held.id).state, "gated", "{}", kind.what);
        assert!(
            w.ledger
                .inbox(recipient, 100)
                .expect("an inbox")
                .iter()
                .all(|message| message.id != held.id),
            "{}: not in its recipient's inbox",
            kind.what
        );
        assert_ne!(
            w.next(held.to),
            Some(held.id),
            "{}: nothing of it goes to a window",
            kind.what
        );

        let passed = approve(&mut w, held.id);
        assert_eq!(passed.state, "queued", "{}", kind.what);
        assert_eq!(
            w.next(held.to),
            Some(held.id),
            "{}: it is its recipient's now",
            kind.what
        );
        assert!(
            w.ledger
                .inbox(recipient, 100)
                .expect("an inbox")
                .iter()
                .any(|message| message.id == held.id),
            "{}: and in its inbox",
            kind.what
        );
        let again = w.ledger.approve_message(held.id, "human").unwrap_err();
        assert_eq!(
            again.code(),
            Some("invalid-transition"),
            "{}: passed on once",
            kind.what
        );
    }
}

#[test]
fn the_human_declines_the_brief_the_words_and_the_answer_and_only_passes_the_rest_on() {
    for kind in KINDS {
        let mut w = gated_world();
        let held = (kind.held)(&mut w);
        let declined = w.ledger.decline_message(held.id, "human");
        if kind.declinable {
            let declined = declined.unwrap_or_else(|failed| panic!("{}: {failed}", kind.what));
            assert_eq!(declined.state, "cancelled", "{}", kind.what);
            assert_eq!(
                declined.reason.as_deref(),
                Some("declined by @human"),
                "{}",
                kind.what
            );
            assert_ne!(w.next(held.to), Some(held.id), "{}", kind.what);
        } else {
            let refused = declined.unwrap_err();
            assert_eq!(refused.code(), Some("not-declinable"), "{}", kind.what);
            assert_eq!(
                w.message(held.id).state,
                "gated",
                "{}: still the human's to pass on",
                kind.what
            );
        }
    }
}

#[test]
fn a_declined_brief_cancels_its_task_and_tells_the_chief_and_no_window_is_given_anything() {
    let mut w = gated_world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.ledger.decline_message(brief, "human").expect("declined");
    assert_eq!(w.state(task), "cancelled");
    assert_eq!(w.next("zeus"), None);
    let told: Vec<String> = w
        .ledger
        .inbox(w.id("chief"), 100)
        .expect("an inbox")
        .into_iter()
        .filter(|message| message.kind == "note")
        .map(|message| message.body)
        .collect();
    assert_eq!(told, ["@human declined T-1 (Parser). It is cancelled."]);
}

#[test]
fn declined_words_that_resume_a_task_cancel_it_and_what_waited_for_its_window_with_it() {
    let mut w = gated_world();
    let task = at_work(&mut w);
    // An answer the human passed on, which the words that resume the task carry.
    let question = w.ask("zeus", task, "Which format?").id;
    approve(&mut w, question);
    let answer = w.answer(question, "JSON").id;
    approve(&mut w, answer);
    w.pause(task);
    let words = w.resume(task, "Use JSON").message.expect("its words").id;
    w.ledger.decline_message(words, "human").expect("declined");
    assert_eq!(w.state(task), "cancelled");
    assert_eq!(w.states(&[words, answer]), ["cancelled", "cancelled"]);
    assert_eq!(w.next("zeus"), None, "nothing goes to its window");
}

#[test]
fn declined_words_that_send_a_task_back_cancel_it_as_a_declined_brief_does_and_the_chief_is_told() {
    let mut w = gated_world();
    let task = finished(&mut w);
    let words = w
        .ledger
        .reopen_task(w.project, task, "chief", "Handle empty input too")
        .expect("sent back")
        .message
        .expect("its words")
        .id;
    assert_eq!(w.state(task), "queued");
    w.ledger.decline_message(words, "human").expect("declined");
    assert_eq!(w.state(task), "cancelled");
    assert_eq!(w.next("zeus"), None, "nothing goes to its window");
    let told: Vec<String> = w
        .ledger
        .inbox(w.id("chief"), 100)
        .expect("an inbox")
        .into_iter()
        .filter(|message| message.kind == "note")
        .map(|message| message.body)
        .collect();
    assert_eq!(told, ["@human declined T-1 (Parser). It is cancelled."]);
}

#[test]
fn a_declined_answer_leaves_its_question_open_and_the_chief_is_told_to_answer_it_again() {
    let mut w = gated_world();
    let task = at_work(&mut w);
    let question = w.ask("zeus", task, "Which format?").id;
    // The chief answers the question before the human passed it on: it goes no further.
    let answer = w.answer(question, "JSON").id;
    assert_eq!(w.states(&[question, answer]), ["cancelled", "gated"]);
    w.ledger.decline_message(answer, "human").expect("declined");
    assert_eq!(
        w.states(&[question, answer]),
        ["gated", "cancelled"],
        "the question is the human's to pass on again"
    );
    let notes: Vec<String> = w
        .ledger
        .inbox(w.id("chief"), 100)
        .expect("an inbox")
        .into_iter()
        .filter(|message| message.kind == "note")
        .map(|message| message.body)
        .collect();
    assert_eq!(
        notes,
        [format!(
            "@human declined your answer to m-{question}. Answer it again: cf answer m-{question} \"…\""
        )]
    );
    // Answered again, it is gated again.
    let again = w.answer(question, "YAML").id;
    assert_eq!(w.message(again).state, "gated");
}

#[test]
fn a_note_of_consensflows_own_and_the_words_with_the_human_pass_the_gate() {
    let mut w = gated_world();
    let task = at_work(&mut w);
    let own = w
        .ledger
        .note(
            w.project,
            &cf_ledger::NewNote {
                from: None,
                to: "zeus".into(),
                body: "A note of ConsensFlow's".into(),
                task: Some(task),
            },
        )
        .expect("a note");
    let to_human = w.note("chief", "human", task, "Done for today");
    let from_human = w
        .ledger
        .note(
            w.project,
            &cf_ledger::NewNote {
                from: Some("human".into()),
                to: "zeus".into(),
                body: "From the human".into(),
                task: Some(task),
            },
        )
        .expect("a note");
    for note in [own, to_human, from_human] {
        assert_eq!(note.state, "queued", "{}", note.body);
    }
}

/// The ids of what a delivery's paste carried.
fn carried(begun: &cf_ledger::Begun) -> Vec<i64> {
    begun.carried.iter().map(|row| row.id).collect()
}

/// The result of `task`, which its window wrote: held at the gate.
fn result_of(w: &mut World, task: i64) -> i64 {
    w.ledger
        .record_result(w.project, task, "Parser done")
        .expect("a result")
        .message
        .expect("its result")
        .id
}

#[test]
fn a_result_approved_and_decided_on_before_the_chief_is_given_it_is_withdrawn_whichever_way_it_decides(
) {
    for (sends_back, reason) in [(false, "T-1 was accepted"), (true, "T-1 was sent back")] {
        let mut w = gated_world();
        let task = at_work(&mut w);
        let result = result_of(&mut w, task);
        assert_eq!(w.message(result).state, "gated");
        approve(&mut w, result);
        // The chief is in a turn of its own: the result waits in its queue. It
        // decides there, and is not given the result it decided on.
        assert_eq!(w.message(result).state, "queued");
        if sends_back {
            w.ledger
                .reopen_task(w.project, task, "chief", "Handle empty input too")
                .expect("sent back");
        } else {
            w.ledger
                .accept_task(w.project, task, "chief")
                .expect("accepted");
        }
        let withdrawn = w.message(result);
        assert_eq!(
            (withdrawn.state.as_str(), withdrawn.reason.as_deref()),
            ("cancelled", Some(reason)),
            "sent back: {sends_back}"
        );
        assert_eq!(w.next("chief"), None, "sent back: {sends_back}");
    }
}

#[test]
fn the_chief_cannot_decide_a_task_whose_result_still_waits_at_the_gate_and_the_human_can() {
    for sends_back in [false, true] {
        let mut w = gated_world();
        let task = at_work(&mut w);
        let result = result_of(&mut w, task);
        // The chief has not been given it, and may not decide what it has not seen.
        let accepted = w.ledger.accept_task(w.project, task, "chief").unwrap_err();
        let reopened = w
            .ledger
            .reopen_task(w.project, task, "chief", "Handle empty input too")
            .unwrap_err();
        assert_eq!(
            [accepted.code(), reopened.code()],
            [Some("result-gated"), Some("result-gated")]
        );
        assert_eq!(
            accepted.to_string(),
            "T-1's result waits for the human's approval: it reaches you once they pass it on"
        );
        assert_eq!(w.state(task), "done");
        assert_eq!(w.message(result).state, "gated");

        // The human decides it themselves, and the result they never passed on is withdrawn.
        let reason = if sends_back {
            w.ledger
                .reopen_task(w.project, task, "human", "Handle empty input too")
                .expect("sent back");
            "sent back by @human"
        } else {
            w.ledger
                .accept_task(w.project, task, "human")
                .expect("accepted");
            "accepted by @human"
        };
        let withdrawn = w.message(result);
        assert_eq!(
            (withdrawn.state.as_str(), withdrawn.reason.as_deref()),
            ("cancelled", Some(reason)),
            "sent back: {sends_back}"
        );
        assert_eq!(
            w.state(task),
            if sends_back { "queued" } else { "accepted" }
        );
    }
}

#[test]
fn a_result_the_human_never_passed_on_cannot_be_passed_on_once_they_decided_its_task() {
    let mut w = gated_world();
    let task = at_work(&mut w);
    let result = result_of(&mut w, task);
    w.ledger
        .accept_task(w.project, task, "human")
        .expect("accepted");
    let refused = w.ledger.approve_message(result, "human").unwrap_err();
    assert_eq!(refused.code(), Some("invalid-transition"));
    assert_eq!(w.next("chief"), None, "the chief is never given it");
}

#[test]
fn words_that_send_a_task_back_wait_at_the_gate_and_go_to_its_window_once_passed_on() {
    let mut w = gated_world();
    let task = finished(&mut w);
    let words = w
        .ledger
        .reopen_task(w.project, task, "chief", "Handle empty input too")
        .expect("sent back")
        .message
        .expect("its words")
        .id;
    assert_eq!(w.state(task), "queued");
    assert_eq!(w.message(words).state, "gated");
    assert_eq!(w.next("zeus"), None, "the window is given nothing yet");
    approve(&mut w, words);
    assert_eq!(w.next("zeus"), Some(words));
    w.deliver(words);
    assert_eq!(w.state(task), "working");
}

#[test]
fn a_pause_by_the_chief_takes_the_brief_back_from_the_gate_and_the_words_that_resume_wait_there_again(
) {
    let mut w = gated_world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.pause(task);
    let withdrawn = w.message(brief);
    assert_eq!(
        (withdrawn.state.as_str(), withdrawn.reason.as_deref()),
        ("cancelled", Some("withdrawn by @chief's pause"))
    );
    let words = w.resume(task, "Go on").message.expect("its words");
    assert_eq!(
        words.body, "Parser\n\nResumed: Go on",
        "the brief goes first"
    );
    assert_eq!(words.state, "gated");
    assert_eq!(w.next("zeus"), None);
    approve(&mut w, words.id);
    assert_eq!(w.next("zeus"), Some(words.id));
    w.deliver(words.id);
    assert_eq!(w.state(task), "working");
}

#[test]
fn a_pause_by_the_human_leaves_the_brief_at_the_gate_and_one_passed_on_meanwhile_goes_nowhere() {
    let mut w = gated_world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.ledger
        .pause_task(w.project, task, Some("human"), None)
        .expect("paused");
    assert_eq!(w.message(brief).state, "gated", "the chief's words stay");
    approve(&mut w, brief);
    assert_eq!(w.state(task), "paused");
    assert_eq!(
        w.next("zeus"),
        None,
        "nothing is given a window of a task that is paused"
    );

    // The chief resumes it: its words wait at the gate, and the brief rides in them.
    let words = w.resume(task, "Go on").message.expect("its words");
    assert_eq!(words.state, "gated");
    assert_eq!(w.next("zeus"), None);
    approve(&mut w, words.id);
    let begun = w.deliver(words.id);
    assert_eq!(carried(&begun), [brief]);
    assert_eq!(w.state(task), "working");
}

#[test]
fn the_humans_own_resume_waits_behind_the_chiefs_brief_at_the_gate_and_goes_in_with_it_once_passed_on(
) {
    let mut w = gated_world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.ledger
        .pause_task(w.project, task, Some("human"), None)
        .expect("paused");
    let words = w
        .ledger
        .resume_task(w.project, task, Some("human"), "Go on")
        .expect("resumed")
        .message
        .expect("its words");
    assert_eq!(words.state, "queued", "the human's own words are not held");
    assert_eq!(
        w.next("zeus"),
        None,
        "the brief that came first is the human's to decide before anything else goes"
    );
    approve(&mut w, brief);
    assert_eq!(w.next("zeus"), Some(words.id));
    let begun = w.deliver(words.id);
    assert_eq!(carried(&begun), [brief], "one paste, the brief in it");
    assert_eq!(w.state(task), "working");
}

#[test]
fn a_cancel_takes_everything_of_the_task_back_from_the_gate_and_none_of_it_can_be_passed_on() {
    let mut w = gated_world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    let note = w.note("chief", "zeus", task, "Mind the tests").id;
    w.ledger
        .cancel_task(w.project, task, "chief")
        .expect("cancelled");
    for id in [brief, note] {
        let withdrawn = w.message(id);
        assert_eq!(
            (withdrawn.state.as_str(), withdrawn.reason.as_deref()),
            ("cancelled", Some("cancelled by @chief")),
            "m-{id}"
        );
        let refused = w.ledger.approve_message(id, "human").unwrap_err();
        assert_eq!(refused.code(), Some("invalid-transition"), "m-{id}");
    }
    assert_eq!(w.next("zeus"), None);
}

#[test]
fn what_waits_at_the_gate_when_approval_is_switched_off_waits_on_and_what_comes_after_passes() {
    let mut w = gated_world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.ledger.set_gate(w.project, false).expect("switched off");
    assert_eq!(
        w.message(brief).state,
        "gated",
        "what was held stays held: the human decides it"
    );
    // A word the chief gives now is not held, but it waits behind the brief, which came first.
    let note = w.note("chief", "zeus", task, "Mind the tests").id;
    assert_eq!(w.message(note).state, "queued");
    assert_eq!(
        w.next("zeus"),
        None,
        "the window is given nothing before the brief"
    );
    approve(&mut w, brief);
    assert_eq!(w.next("zeus"), Some(brief));
    let begun = w.deliver(brief);
    assert_eq!(carried(&begun), [note], "one paste, the brief first");
    // Switched on again, the next word waits again.
    w.ledger.set_gate(w.project, true).expect("switched on");
    let again = w.note("chief", "zeus", task, "And the docs").id;
    assert_eq!(w.message(again).state, "gated");
}

#[test]
fn a_brief_declined_with_approval_switched_off_cancels_the_task_and_the_word_waiting_behind_it() {
    let mut w = gated_world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.ledger.set_gate(w.project, false).expect("switched off");
    let note = w.note("chief", "zeus", task, "Mind the tests").id;
    w.ledger.decline_message(brief, "human").expect("declined");
    assert_eq!(w.state(task), "cancelled");
    assert_eq!(w.states(&[brief, note]), ["cancelled", "cancelled"]);
    assert_eq!(w.next("zeus"), None);
}

#[test]
fn what_waits_at_the_gate_is_there_when_the_ledger_is_opened_again_and_goes_on_when_passed_on() {
    let mut w = gated_world();
    let task = at_work(&mut w);
    // An answer the human passed on, riding in the words that resume the task,
    // which wait at the gate; and a note, which waits there on its own.
    let question = w.ask("zeus", task, "Which format?").id;
    approve(&mut w, question);
    let answer = w.answer(question, "JSON").id;
    approve(&mut w, answer);
    w.pause(task);
    let words = w.resume(task, "Use JSON").message.expect("its words").id;
    let note = w.note("chief", "zeus", task, "Mind the tests").id;
    assert_eq!(
        w.states(&[words, note, answer]),
        ["gated", "gated", "queued"]
    );

    // The daemon restarts: the file is closed and opened again.
    w.edit(|_| {});
    assert_eq!(
        w.states(&[words, note, answer]),
        ["gated", "gated", "queued"]
    );
    assert_eq!(w.next("zeus"), None, "still nothing goes to the window");

    approve(&mut w, words);
    assert_eq!(w.next("zeus"), Some(words));
    let begun = w.deliver(words);
    assert_eq!(
        carried(&begun),
        [answer],
        "the answer rides in the words, as it did before the restart"
    );
    assert_eq!(w.message(note).state, "gated", "and the note still waits");
}

#[test]
fn approval_switched_on_holds_what_comes_after_and_what_was_queued_before_goes_on() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    assert_eq!(w.message(brief).state, "queued");
    w.ledger.set_gate(w.project, true).expect("switched on");
    assert_eq!(
        w.message(brief).state,
        "queued",
        "what was given before is not held after the fact"
    );
    assert_eq!(w.next("zeus"), Some(brief));
    w.deliver(brief);
    let note = w.note("chief", "zeus", task, "Mind the tests").id;
    assert_eq!(w.message(note).state, "gated", "what comes after waits");
}

#[test]
fn a_second_pause_takes_back_the_first_words_waiting_at_the_gate_and_only_the_last_are_given() {
    let mut w = gated_world();
    let task = at_work(&mut w);
    w.pause(task);
    let first = w.resume(task, "Use YAML").message.expect("its words").id;
    assert_eq!(w.message(first).state, "gated");
    // The chief thinks again: the pause takes back what it wrote and the human
    // had not passed on, and what resumes the task now is what it says now.
    w.pause(task);
    let withdrawn = w.message(first);
    assert_eq!(
        (withdrawn.state.as_str(), withdrawn.reason.as_deref()),
        ("cancelled", Some("withdrawn by @chief's pause"))
    );
    let second = w.resume(task, "Use JSON").message.expect("its words").id;
    assert_eq!(w.message(second).body, "Resumed: Use JSON");
    approve(&mut w, second);
    assert_eq!(w.next("zeus"), Some(second));
    let begun = w.deliver(second);
    assert!(carried(&begun).is_empty());
    assert_eq!(w.state(task), "working");
    assert_eq!(w.next("zeus"), None, "the first words never go in");
}

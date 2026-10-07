//! What a window kept for its task rides in the paste of the words that send
//! it on: one paste, one marker, and the result is made from the turn after
//! it. None of these tests is held to a Node recording: Node's resume pasted
//! its words and dropped the rest.

use cf_engine::delivery_text::marker_of;
use cf_engine::testing::Context;
use cf_ledger::{NewNote, RESUME_WORDS};

use crate::fixtures::assert_match;

/// T-1 given to zeus, its brief received, a question asked and answered, and
/// a note: the project, the question's id, the answer's and the note's.
fn kept(context: &Context) -> (i64, i64, i64, i64) {
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let question = context.ask(project.id, "zeus", "chief", 1, "Which format?");
    context.adapter.answer("zeus", "I asked");
    context.pass().unwrap();
    let answer = context.answer(project.id, question.id, "JSON");
    // A note about the task, which is what a window keeps.
    let note = context
        .ledger
        .borrow_mut()
        .note(
            project.id,
            &NewNote {
                from: Some("chief".to_owned()),
                to: "zeus".to_owned(),
                body: "Mind the tests".to_owned(),
                task: Some(1),
            },
        )
        .unwrap();
    (project.id, question.id, answer.id, note.id)
}

#[test]
fn a_hold_ends_in_one_paste_with_what_was_kept_and_then_the_words_and_one_marker() {
    let context = Context::new();
    let (project, question, answer, note) = kept(&context);
    context.hold_task(project, 1);
    context.pass().unwrap();
    context
        .ledger
        .borrow_mut()
        .resume_task(project, 1, None, RESUME_WORDS)
        .unwrap();
    let carrier = context
        .ledger
        .borrow()
        .inbox(context.id(project, "zeus"), 100)
        .unwrap()[0]
        .id;
    context.pass().unwrap();
    let pasted = context
        .adapter
        .agent("zeus")
        .items
        .pop()
        .unwrap()
        .text
        .to_string();
    assert_eq!(
        pasted,
        format!(
            "[ConsensFlow m-{carrier} · T-1 · task from ConsensFlow]\n(kept for you: answer m-{answer} from @chief, to your m-{question})\nJSON\n\n(kept for you: note m-{note} from @chief)\nMind the tests\n\nResumed: Go on where you stopped."
        )
    );
    assert_eq!(
        pasted.matches("[ConsensFlow m-").count(),
        1,
        "only the carrier's marker is in it"
    );
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
    for id in [answer, note, carrier] {
        assert_eq!(context.message(id).state, "delivered", "m-{id}");
    }
    let items = context.adapter.agent("zeus").items;
    for id in [answer, note] {
        assert!(
            !items.iter().any(|item| item.text.contains(&marker_of(id))),
            "m-{id} has no paste of its own"
        );
    }
}

#[test]
fn a_constituent_with_a_greater_id_than_its_carrier_does_not_hide_the_carriers_marker_from_the_result(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let question = context.ask(project.id, "zeus", "chief", 1, "Which format?");
    context.adapter.answer("zeus", "I asked");
    context.pass().unwrap();
    // The words come first, and the answer after them: it joins them in their paste.
    context.pause_task(project.id, 1);
    context.resume_task(project.id, 1, "Carry on");
    let answer = context.answer(project.id, question.id, "JSON");
    let carrier = context
        .task(project.id, 1)
        .messages
        .into_iter()
        .find(|message| message.body == "Resumed: Carry on")
        .unwrap();
    assert!(answer.id > carrier.id, "its id is the greater");
    context.pass().unwrap();
    let pasted = context.adapter.agent("zeus").items.pop().unwrap().text;
    assert_match(
        &pasted,
        r"^\[ConsensFlow m-\d+ · T-1 · task from @chief\]\nResumed: Carry on\n\n\(kept for you: answer m-\d+ from @chief, to your m-\d+\)\nJSON$",
    );
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    context.adapter.answer("zeus", "Parser in JSON");
    context.pass().unwrap();
    let thread = context.task(project.id, 1);
    assert_eq!(thread.task.state, "done", "the result was made");
    let result = thread.messages.iter().find(|m| m.kind == "result").unwrap();
    assert_eq!(result.body, "Parser in JSON");
}

#[test]
fn a_gated_first_launch_gives_its_window_nothing_until_the_human_passes_the_brief_on_and_then_one_paste(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context
        .ledger
        .borrow_mut()
        .set_gate(project.id, true)
        .unwrap();
    let brief = context.give(project.id, "zeus", "Parser").message.unwrap();
    assert_eq!(brief.state, "gated");
    // The task is held at once (a window refused), and the daemon resumes it.
    context.hold_task(project.id, 1);
    context
        .ledger
        .borrow_mut()
        .resume_task(project.id, 1, None, RESUME_WORDS)
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!(
        context
            .adapter
            .prepared()
            .iter()
            .all(|launch| launch["participant"]["handle"] != "zeus"),
        "no window of zeus opens: nothing of the task is its to read"
    );
    assert!(context.host.last("zeus").is_none());

    context
        .ledger
        .borrow_mut()
        .approve_message(brief.id, "human")
        .unwrap();
    context.pass().unwrap();
    let launches: Vec<_> = context
        .adapter
        .prepared()
        .into_iter()
        .filter(|launch| launch["participant"]["handle"] == "zeus")
        .collect();
    assert_eq!(launches.len(), 1, "one launch");
    let first = launches[0]["message"].as_str().unwrap();
    assert_match(
        first,
        r"^\[ConsensFlow m-\d+ · T-1 · task from ConsensFlow\]\n\(kept for you: task m-\d+ from @chief\)\nParser\n\nResumed: Go on where you stopped\.$",
    );
    assert_eq!(first.matches("[ConsensFlow m-").count(), 1, "one marker");
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
}

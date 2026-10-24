//! Words told to a task's window before it was up, where their way is not the
//! straight one (two reviews of the change, 2026-10-16): words that never reach
//! the window, words a window took back, words the human passes on in another
//! order than they were told, and words that went with a task called off while
//! a launch was being prepared, or after the task was taken back to the board.

use crate::fixture::{gated_world, ids, world};
use crate::queued_tells::{newest, queued, task_id, tell, waiting};
use cf_ledger::NewNote;

/// What the chief hears when its words went with a task: `ids` named.
fn went(ids: &[i64]) -> String {
    let named: Vec<String> = ids.iter().map(|id| format!("m-{id}")).collect();
    format!(
        "What you told its window, {}, went with it: it never had it.",
        named.join(", ")
    )
}

#[test]
fn told_words_that_never_reach_the_window_fail_their_task_and_its_answer_is_no_result() {
    let mut w = world();
    let (task, session, brief) = queued(&mut w);
    w.begin(brief);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.confirm(brief);
    assert_eq!(w.state(task), "working");
    // A note of ConsensFlow's own about the task that does not arrive fails nothing.
    let own = w
        .ledger
        .note(
            w.project,
            &NewNote {
                from: None,
                to: session.clone(),
                task: Some(task),
                body: "Your role has changed".into(),
            },
        )
        .expect("a note");
    w.begin(own.id);
    w.ledger
        .fail_delivery(own.id, "refused by the window")
        .expect("given up");
    assert_eq!(w.state(task), "working");

    w.begin(told.id);
    w.ledger
        .fail_delivery(told.id, "refused by the window")
        .expect("given up");
    assert_eq!(
        w.state(task),
        "failed",
        "what the window answers is to a task without them"
    );
    assert!(!waiting(&w, &session, task));
    let refused = w
        .ledger
        .record_result(w.project, task, "Parser done")
        .unwrap_err();
    assert_eq!(refused.code(), Some("invalid-transition"), "no result");
}

#[test]
fn a_done_while_told_words_are_on_their_way_is_refused_until_the_window_has_them() {
    let refusal = |task: i64| {
        format!("T-{task} has words on their way to you: run cf task done again once you have read them")
    };
    let mut w = world();
    let (task, _, brief) = queued(&mut w);
    w.begin(brief);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.confirm(brief);
    // Queued for the window, then being pasted.
    for paste in [false, true] {
        if paste {
            w.begin(told.id);
        }
        let refused = w
            .ledger
            .record_result(w.project, task, "Parser done")
            .unwrap_err();
        assert_eq!(refused.code(), Some("words-on-their-way"));
        assert_eq!(refused.to_string(), refusal(task));
        assert_eq!(w.state(task), "working", "the task waits for them");
    }
    w.confirm(told.id);
    w.ledger
        .record_result(w.project, task, "Parser done")
        .expect("the window has them");
    assert_eq!(w.state(task), "done");

    // Held for the human.
    let mut w = gated_world();
    let (task, _, brief) = queued(&mut w);
    w.ledger.approve_message(brief, "human").expect("passed on");
    w.begin(brief);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.confirm(brief);
    assert_eq!(w.states(&[told.id]), ["gated"]);
    let refused = w
        .ledger
        .record_result(w.project, task, "Parser done")
        .unwrap_err();
    assert_eq!(refused.to_string(), refusal(task));
    w.ledger
        .approve_message(told.id, "human")
        .expect("passed on");
    w.deliver(told.id);
    w.ledger
        .record_result(w.project, task, "Parser done")
        .expect("the window has them");
}

#[test]
fn told_words_a_window_took_back_are_kept_for_it_again_and_the_resume_carries_them() {
    let mut w = world();
    let (task, _, brief) = queued(&mut w);
    w.begin(brief);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.confirm(brief);
    w.deliver(told.id);
    w.ledger
        .pause_task(w.project, task, Some("human"), None)
        .expect("the human pauses");
    w.ledger
        .take_back(told.id, "it stopped before a word and took it back")
        .expect("kept again");
    assert_eq!(w.message(told.id).state, "queued");
    let carrier = w.resume(task, "Go on").message.expect("its words");
    let begun = w.deliver(carrier.id);
    assert_eq!(ids(&begun.carried), [told.id], "they ride in the resume");
}

#[test]
fn told_words_go_in_the_order_told_whatever_order_the_human_passes_them_on_in() {
    // On their own, after a brief on its way.
    let mut w = gated_world();
    let (task, session, brief) = queued(&mut w);
    w.ledger.approve_message(brief, "human").expect("passed on");
    w.begin(brief);
    let first = tell(&mut w, task, "First");
    let second = tell(&mut w, task, "Second");
    w.confirm(brief);
    w.ledger
        .approve_message(second.id, "human")
        .expect("passed on");
    assert_eq!(
        w.next(&session),
        None,
        "the second waits behind the first, still held"
    );
    w.ledger
        .approve_message(first.id, "human")
        .expect("passed on");
    assert_eq!(w.next(&session), Some(first.id));
    w.deliver(first.id);
    assert_eq!(w.next(&session), Some(second.id));

    // In the brief's paste: the second does not ride in it ahead of the first.
    let mut w = gated_world();
    let (task, session, brief) = queued(&mut w);
    w.ledger.approve_message(brief, "human").expect("passed on");
    let first = tell(&mut w, task, "First");
    let second = tell(&mut w, task, "Second");
    w.ledger
        .approve_message(second.id, "human")
        .expect("passed on");
    assert!(w.deliver(brief).carried.is_empty(), "nothing rode ahead");
    w.ledger
        .approve_message(first.id, "human")
        .expect("passed on");
    assert_eq!(w.next(&session), Some(first.id));
    w.deliver(first.id);
    assert_eq!(w.next(&session), Some(second.id));
}

#[test]
fn the_message_last_pasted_is_the_one_last_delivered_whatever_its_id() {
    let mut w = gated_world();
    let (task, session, brief) = queued(&mut w);
    w.ledger.approve_message(brief, "human").expect("passed on");
    w.begin(brief);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.confirm(brief);
    let question = w.ask(&session, task, "Which version?");
    let answer = w.answer(question.id, "2.1");
    assert!(answer.id > told.id);
    // The human passes on the answer first, and the words after it.
    w.ledger
        .approve_message(answer.id, "human")
        .expect("passed on");
    w.deliver(answer.id);
    w.ledger
        .approve_message(told.id, "human")
        .expect("passed on");
    w.deliver(told.id);
    let last = w
        .ledger
        .last_pasted(w.id(&session), task_id(&w, task))
        .expect("a read")
        .expect("a message pasted");
    assert_eq!(
        last.id, told.id,
        "what the window writes now answers the words"
    );
}

#[test]
fn words_a_task_taken_back_said_in_its_next_brief_went_with_it_while_no_window_had_that_brief() {
    let mut w = world();
    let (task, _, _) = queued(&mut w);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.ledger
        .release_task(w.project, task, "ran out of quota")
        .expect("taken back");
    w.assign(task, "diana");
    assert_eq!(
        ids(&w.ledger.told_unreceived(w.project, task).unwrap()),
        [told.id]
    );
    w.ledger
        .cancel_task(w.project, task, "human")
        .expect("cancelled");
    assert!(
        newest(&w, "chief").body.ends_with(&went(&[told.id])),
        "{}",
        newest(&w, "chief").body
    );

    // The next window had its brief, and the words in it.
    let mut w = world();
    let (task, _, _) = queued(&mut w);
    tell(&mut w, task, "Use the v2 grammar");
    w.ledger
        .release_task(w.project, task, "ran out of quota")
        .expect("taken back");
    let brief = w.assign(task, "diana").message.expect("its brief").id;
    w.deliver(brief);
    assert!(w
        .ledger
        .told_unreceived(w.project, task)
        .unwrap()
        .is_empty());
    w.ledger
        .cancel_task(w.project, task, "human")
        .expect("cancelled");
    assert!(!newest(&w, "chief").body.contains("What you told"));
}

#[test]
fn a_launch_called_off_before_its_window_opened_tells_whoever_told_the_words_in_its_first_message()
{
    let lead = |task: i64| format!("T-{task} was cancelled before its window opened. ");

    // Words that rode in the brief, whose launch had begun.
    let mut w = world();
    let (task, _, brief) = queued(&mut w);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.begin(brief);
    assert!(
        w.ledger
            .told_unreceived(w.project, task)
            .unwrap()
            .is_empty(),
        "on their way"
    );
    w.ledger
        .cancel_task(w.project, task, "human")
        .expect("cancelled");
    assert!(
        !newest(&w, "chief").body.contains("What you told"),
        "the cancel cannot know"
    );
    w.ledger.launch_called_off(brief).expect("told");
    assert_eq!(
        newest(&w, "chief").body,
        format!("{}{}", lead(task), went(&[told.id]))
    );

    // Words a task taken back said in the brief of the launch called off.
    let mut w = world();
    let (task, _, _) = queued(&mut w);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.ledger
        .release_task(w.project, task, "ran out of quota")
        .expect("taken back");
    let brief = w.assign(task, "diana").message.expect("its brief").id;
    w.begin(brief);
    w.ledger
        .cancel_task(w.project, task, "human")
        .expect("cancelled");
    assert!(!newest(&w, "chief").body.contains("What you told"));
    w.ledger.launch_called_off(brief).expect("told");
    assert_eq!(
        newest(&w, "chief").body,
        format!("{}{}", lead(task), went(&[told.id]))
    );

    // A launch over for a pause gives its message back, and tells nobody.
    let mut w = world();
    let (task, _, brief) = queued(&mut w);
    tell(&mut w, task, "Use the v2 grammar");
    w.begin(brief);
    w.ledger
        .pause_task(w.project, task, Some("human"), None)
        .expect("paused");
    w.ledger
        .retry_delivery(brief, "a pause came while its window was prepared", true)
        .expect("given back");
    let notes = w.ledger.inbox(w.id("chief"), 100).unwrap().len();
    w.ledger.launch_called_off(brief).expect("nothing to tell");
    assert_eq!(w.ledger.inbox(w.id("chief"), 100).unwrap().len(), notes);
}

/// Calliope's review of the merge (T-408, P3-2): told words whose paste sat in the
/// window's box when its Enter was withheld (the human's Close came first) went back
/// to their queue, so the cancel that came next said the window never had them. Then
/// the human sent what the box held: they arrived after all, and whoever told them
/// hears so, once. Both ways a paste holds them: as the paste itself, and riding in
/// the brief's.
#[test]
fn told_words_the_human_sent_from_the_box_after_a_cancel_are_said_to_have_arrived_after_all() {
    let after_all = |task: i64, id: i64| {
        format!(
            "T-{task} was cancelled, but the human sent what its window's input box held: \
             the window had what you told it, m-{id}, after all."
        )
    };
    let chief_notes = |w: &crate::fixture::World| -> Vec<String> {
        w.ledger
            .inbox(w.id("chief"), 100)
            .unwrap()
            .into_iter()
            .map(|message| message.body)
            .collect()
    };

    // The words are the paste: told while the brief was on its way, they went in next.
    let mut w = world();
    let (task, _, brief) = queued(&mut w);
    w.begin(brief);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.confirm(brief);
    w.begin(told.id);
    w.ledger.admit(told.id, &[]).unwrap();
    w.ledger
        .retry_delivery(told.id, "its Enter was withheld", true)
        .expect("given back");
    w.ledger
        .cancel_task(w.project, task, "human")
        .expect("cancelled");
    assert!(newest(&w, "chief").body.contains(&went(&[told.id])));
    w.ledger
        .confirm_sent_by_hand(told.id, &[], &serde_json::json!({ "item": "u-1" }), false)
        .unwrap()
        .expect("confirmed");
    assert_eq!(
        w.ledger.message(told.id).unwrap().unwrap().state,
        "delivered"
    );
    let said = chief_notes(&w);
    assert_eq!(
        said.iter()
            .filter(|note| **note == after_all(task, told.id))
            .count(),
        1,
        "{said:?}"
    );
    // A paste confirmed is confirmed once: nothing more is said.
    assert!(w
        .ledger
        .confirm_sent_by_hand(told.id, &[], &serde_json::json!({ "item": "u-2" }), false)
        .unwrap()
        .is_none());
    assert_eq!(chief_notes(&w).len(), said.len());

    // The words ride in the brief's paste.
    let mut w = world();
    let (task, _, brief) = queued(&mut w);
    let told = tell(&mut w, task, "Use the v2 grammar");
    let carried: Vec<i64> = w.begin(brief).carried.iter().map(|row| row.id).collect();
    assert_eq!(carried, [told.id]);
    w.ledger.admit(brief, &carried).unwrap();
    w.ledger
        .retry_delivery(brief, "its Enter was withheld", true)
        .expect("given back");
    w.ledger
        .cancel_task(w.project, task, "human")
        .expect("cancelled");
    assert!(newest(&w, "chief").body.contains(&went(&[told.id])));
    w.ledger
        .confirm_sent_by_hand(
            brief,
            &carried,
            &serde_json::json!({ "item": "u-1" }),
            false,
        )
        .unwrap()
        .expect("confirmed");
    assert_eq!(
        w.ledger.message(told.id).unwrap().unwrap().state,
        "delivered"
    );
    assert_eq!(newest(&w, "chief").body, after_all(task, told.id));

    // Words withdrawn with no cancel of their task, which is still at work, tell
    // nobody when the human sends them: nobody was told they went.
    let mut w = world();
    let (task, _, brief) = queued(&mut w);
    w.begin(brief);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.confirm(brief);
    w.begin(told.id);
    w.ledger.admit(told.id, &[]).unwrap();
    w.ledger
        .retry_delivery(told.id, "its Enter was withheld", true)
        .expect("given back");
    w.ledger
        .cancel_message(told.id, "the chief took it back")
        .expect("withdrawn");
    assert_eq!(w.state(task), "working");
    let before = chief_notes(&w).len();
    w.ledger
        .confirm_sent_by_hand(told.id, &[], &serde_json::json!({ "item": "u-1" }), false)
        .unwrap()
        .expect("confirmed");
    assert_eq!(chief_notes(&w).len(), before);
}

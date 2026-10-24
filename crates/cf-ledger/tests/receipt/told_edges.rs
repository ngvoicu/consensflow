//! Words told to a task's window before it was up, where their way is not the
//! straight one (two reviews of the change, 2026-10-16): words that never reach
//! the window, words a window took back, words the human passes on in another
//! order than they were told, and words that went with a task called off while
//! a launch was being prepared, or after the task was taken back to the board.
//! A task ended by no cancel takes them too, and whoever told them hears so as a
//! cancel tells it (T-389): its member removed, its harness failed, or the
//! window it waited for on the board gone.

use crate::fixture::{gated_world, ids, world, World};
use crate::queued_tells::{newest, queued, task_id, tell, waiting};
use cf_ledger::{MessageView, NewNote, NewTask};

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

/// The notes `handle` has about T-`number`, oldest first.
fn notes_on(w: &World, handle: &str, number: i64) -> Vec<String> {
    let mut notes: Vec<_> = w
        .ledger
        .inbox(w.id(handle), 100)
        .expect("the inbox")
        .into_iter()
        .filter(|message| message.kind == "note" && message.task_number == Some(number))
        .collect();
    notes.sort_by_key(|message| message.id);
    notes.into_iter().map(|message| message.body).collect()
}

#[test]
fn a_member_removed_takes_the_words_told_to_its_tasks_window_and_whoever_told_them_hears_which_went(
) {
    let lead = |task: i64| format!("T-{task} was cancelled: @zeus left the staff. ");

    // The human asked for it; the chief, which neither asked for it nor removed
    // the member, told its window (Calliope's item 9 of T-367).
    let mut w = world();
    let task = w
        .ledger
        .create_task(
            w.project,
            &NewTask {
                from: "human".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: "Parser".into(),
                ..NewTask::default()
            },
        )
        .expect("the human's task")
        .task
        .number;
    w.assign(task, "zeus");
    let first = tell(&mut w, task, "Use the v2 grammar");
    let second = tell(&mut w, task, "And keep the old tests");
    w.ledger.remove_member(w.project, "zeus").expect("removed");
    assert_eq!(w.state(task), "cancelled");
    assert_eq!(w.states(&[first.id, second.id]), ["cancelled", "cancelled"]);
    assert_eq!(
        notes_on(&w, "chief", task),
        [format!(
            "{}What you told its window, m-{}, m-{}, went with it: it never had them.",
            lead(task),
            first.id,
            second.id
        )]
    );

    // The chief asked for it and told it: the removal's word to it names the task,
    // and this one the words.
    let mut w = world();
    let (task, _, _) = queued(&mut w);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.ledger.remove_member(w.project, "zeus").expect("removed");
    assert_eq!(
        notes_on(&w, "chief", task),
        [format!("{}{}", lead(task), went(&[told.id]))]
    );

    // The window had them with its brief: nothing of them went.
    let mut w = world();
    let (task, _, brief) = queued(&mut w);
    tell(&mut w, task, "Use the v2 grammar");
    w.deliver(brief);
    w.ledger.remove_member(w.project, "zeus").expect("removed");
    assert!(notes_on(&w, "chief", task).is_empty());
}

#[test]
fn a_task_whose_harness_failed_keeps_the_words_still_on_their_way_for_its_reopen_and_whoever_told_them_hears_they_wait(
) {
    let why = "@zeus's harness reported a failure";
    let reopen = |task: i64| format!(" Reopen it with: cf task reopen T-{task} \"…\"");
    let wait = |ids: &[i64]| {
        let named: Vec<String> = ids.iter().map(|id| format!("m-{id}")).collect();
        format!(
            "; the words told its window wait for the reopen: {}.",
            named.join(", ")
        )
    };

    // Told while the brief was on its way, so they go in next; the harness fails
    // first. The daemon gives up on it: the words wait for the reopen, as words a
    // failed delivery leaves do (the chief's decision on Calliope's P3-4 of T-446),
    // and the requester, which told them, hears it all in one note.
    let mut w = world();
    let (task, session, brief) = queued(&mut w);
    w.begin(brief);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.confirm(brief);
    assert_eq!(w.state(task), "working");
    w.ledger.give_up_task(w.project, task, why).expect("failed");
    assert_eq!(w.states(&[told.id]), ["queued"], "kept for the reopen");
    assert_eq!(
        notes_on(&w, "chief", task),
        [format!(
            "T-{task} failed: {why}{}{}",
            wait(&[told.id]),
            reopen(task)
        )]
    );
    let carrier = w
        .ledger
        .reopen_task(w.project, task, "chief", "Try again")
        .expect("reopened")
        .message
        .expect("its words")
        .id;
    assert_eq!(w.next(&session), Some(carrier));
    let begun = w.deliver(carrier);
    assert_eq!(ids(&begun.carried), [told.id], "the reopen carries them");

    // Pasted already: the note says why, and nothing of them.
    let mut w = world();
    let (task, _, brief) = queued(&mut w);
    w.begin(brief);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.confirm(brief);
    w.deliver(told.id);
    w.ledger.give_up_task(w.project, task, why).expect("failed");
    assert_eq!(
        notes_on(&w, "chief", task),
        [format!("T-{task} failed: {why}.{}", reopen(task))]
    );

    // A teller that did not ask for the task hears the same of its own words.
    let human_task = |w: &mut World| {
        let task = w
            .ledger
            .create_task(
                w.project,
                &NewTask {
                    from: "human".into(),
                    pool: Some("worker".into()),
                    tier: Some("standard".into()),
                    body: "Parser".into(),
                    ..NewTask::default()
                },
            )
            .expect("the human's task")
            .task
            .number;
        let brief = w.assign(task, "zeus").message.expect("its brief").id;
        w.begin(brief);
        let told = tell(w, task, "Use the v2 grammar");
        w.confirm(brief);
        (task, told)
    };
    let mut w = world();
    let (task, told) = human_task(&mut w);
    w.ledger.give_up_task(w.project, task, why).expect("failed");
    assert_eq!(w.states(&[told.id]), ["queued"]);
    let said = format!("T-{task} failed: {why}{}{}", wait(&[told.id]), reopen(task));
    assert_eq!(notes_on(&w, "chief", task), std::slice::from_ref(&said));
    assert_eq!(notes_on(&w, "human", task), [said]);

    // Node's `failTask`, as its recordings call it, keeps its shape: what was
    // queued goes, the requester hears nothing of it, and a teller that did not
    // ask for the task hears which words went.
    let mut w = world();
    let (task, told) = human_task(&mut w);
    w.ledger.fail_task(w.project, task, why).expect("failed");
    assert_eq!(w.states(&[told.id]), ["cancelled"]);
    assert_eq!(
        notes_on(&w, "chief", task),
        [format!("T-{task} failed: {why}. {}", went(&[told.id]))]
    );
    assert!(notes_on(&w, "human", task).is_empty());
}

#[test]
fn a_task_cancelled_as_its_window_is_gone_tells_whoever_told_its_window_which_words_went() {
    // A follow-up bound to zeus's session, waiting for T-1, holding words told to
    // its window: no build keeps words for an open task, so they are written as an
    // older ledger could hold them. Then zeus left in that build.
    let mut w = world();
    let first = w.open("Parser").task.number;
    let moved = w.assign(first, "zeus");
    let session = moved.task.assignee.clone().expect("a session");
    w.deliver(moved.message.expect("its brief").id);
    let task = w
        .follow_up(first, "Tests", &[])
        .expect("a follow-up")
        .task
        .number;
    let set = |w: &mut World, state: &'static str| {
        w.edit(|db| {
            db.execute(
                "UPDATE task SET state = ?1 WHERE number = ?2",
                rusqlite::params![state, task],
            )
            .expect("its state");
        });
    };
    set(&mut w, "queued");
    let chiefs = tell(&mut w, task, "Use the v2 grammar");
    let humans = w
        .ledger
        .tell_queued(w.project, task, "human", "And keep the old tests")
        .expect("kept");
    set(&mut w, "open");
    w.edit(|db| {
        db.execute(
            "UPDATE participant SET left_at = '2026-10-21T10:00:00.000Z'
             WHERE handle = ?1 OR id = (SELECT member_id FROM participant WHERE handle = ?1)",
            [&session],
        )
        .expect("zeus and its session are gone");
    });

    w.ledger
        .record_result(w.project, first, "Done")
        .expect("a result");
    w.ledger
        .accept_task(w.project, first, "chief")
        .expect("accepted");
    assert_eq!(w.state(task), "cancelled");
    let lead = format!("T-{task} was cancelled: @zeus left the staff, so no window can take it.");
    assert_eq!(
        notes_on(&w, "chief", task),
        [format!("{lead} {}", went(&[chiefs.id]))]
    );
    assert_eq!(
        notes_on(&w, "human", task),
        [format!("{lead} {}", went(&[humans.id]))]
    );
}

/// T-`number` given to zeus's session, its brief in, and words the chief told its
/// window while the brief was on its way still queued for it; the human pauses it
/// before they are pasted (Calliope's P2 of T-437). The session and the words.
fn paused_with_words(w: &mut World, number: i64) -> (String, MessageView) {
    let session = w
        .ledger
        .task(w.project, number)
        .unwrap()
        .unwrap()
        .task
        .assignee
        .expect("a session");
    let brief = w.next(&session).expect("its brief");
    w.begin(brief);
    let told = tell(w, number, "Use the v2 grammar");
    w.confirm(brief);
    w.ledger
        .pause_task(w.project, number, Some("human"), None)
        .expect("the human pauses");
    assert_eq!(w.states(&[told.id]), ["queued"], "kept for the resume");
    (session, told)
}

#[test]
fn a_paused_task_for_a_tier_keeps_its_told_words_through_its_members_removal_and_its_next_window_has_them(
) {
    let mut w = world();
    let (task, _, _) = queued(&mut w);
    let (session, told) = paused_with_words(&mut w, task);
    w.ledger.remove_member(w.project, "zeus").expect("removed");
    assert_eq!(w.state(task), "paused", "left for its resume");
    assert_eq!(w.states(&[told.id]), ["queued"], "still kept");

    w.resume(task, "Go on");
    assert_eq!(w.state(task), "open", "for a fresh window of its tier");
    let body = w.ledger.task(w.project, task).unwrap().unwrap().task.body;
    assert!(
        body.contains(&format!(
            "Kept from before, never delivered to @{session}:\n\n(note m-{} from @chief)\nUse the v2 grammar",
            told.id
        )),
        "{body}"
    );
}

#[test]
fn a_paused_task_bound_to_a_removed_members_window_keeps_its_told_words_and_its_cancel_tells_of_them(
) {
    let mut w = world();
    let task = w.give("zeus", "Parser").task.number;
    let (_, told) = paused_with_words(&mut w, task);
    w.ledger.remove_member(w.project, "zeus").expect("removed");
    assert_eq!(w.states(&[told.id]), ["queued"], "still kept");
    let refused = w
        .ledger
        .resume_task(w.project, task, Some("chief"), "Go on")
        .expect_err("its window has ended");
    assert_eq!(refused.code(), Some("session-ended"));

    w.ledger
        .cancel_task(w.project, task, "human")
        .expect("cancelled");
    assert_eq!(
        notes_on(&w, "chief", task),
        [format!(
            "@human cancelled T-{task} (Parser). {}",
            went(&[told.id])
        )]
    );
}

#[test]
fn a_paused_tasks_told_words_being_pasted_at_its_members_removal_are_given_back_for_the_resume() {
    let mut w = world();
    let (task, session, brief) = queued(&mut w);
    w.begin(brief);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.confirm(brief);
    w.begin(told.id);
    w.ledger
        .pause_task(w.project, task, Some("human"), None)
        .expect("the human pauses");
    assert_eq!(w.states(&[told.id]), ["delivering"]);
    w.ledger.remove_member(w.project, "zeus").expect("removed");
    assert_eq!(w.states(&[told.id]), ["queued"], "given back, kept");
    // The ledger says why, and its log keeps it (Calliope's P3-5 of T-446).
    assert_eq!(
        w.message(told.id).reason.as_deref(),
        Some(format!("kept for T-{task}'s resume: @zeus left the staff").as_str())
    );
    let kept: Vec<_> = w
        .ledger
        .events(w.project, 0, 500)
        .expect("the events")
        .into_iter()
        .filter(|event| event.kind == "delivery.kept")
        .map(|event| event.data)
        .collect();
    assert_eq!(
        kept,
        [serde_json::json!({ "message": told.id, "reason": "@zeus left the staff" })]
    );

    w.resume(task, "Go on");
    let body = w.ledger.task(w.project, task).unwrap().unwrap().task.body;
    assert!(
        body.contains(&format!(
            "Kept from before, never delivered to @{session}:\n\n(note m-{} from @chief)",
            told.id
        )),
        "{body}"
    );
}

#[test]
fn words_on_their_way_to_a_removed_members_window_are_said_to_have_gone_with_its_task() {
    let lead = |task: i64| format!("T-{task} was cancelled: @zeus left the staff. ");

    // Riding in the brief, whose launch has begun (Calliope's check 3 of T-437).
    let mut w = world();
    let (task, _, brief) = queued(&mut w);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.begin(brief);
    assert!(
        w.ledger
            .told_unreceived(w.project, task)
            .unwrap()
            .is_empty(),
        "on their way, which a cancel leaves to the launch's end"
    );
    w.ledger.remove_member(w.project, "zeus").expect("removed");
    assert_eq!(w.states(&[told.id]), ["cancelled"]);
    assert_eq!(
        notes_on(&w, "chief", task),
        [format!("{}{}", lead(task), went(&[told.id]))]
    );

    // Being pasted on their own, after the brief.
    let mut w = world();
    let (task, _, brief) = queued(&mut w);
    w.begin(brief);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.confirm(brief);
    w.begin(told.id);
    w.ledger.remove_member(w.project, "zeus").expect("removed");
    assert_eq!(w.states(&[told.id]), ["cancelled"]);
    assert_eq!(
        notes_on(&w, "chief", task),
        [format!("{}{}", lead(task), went(&[told.id]))]
    );
}

#[test]
fn a_removal_says_which_tasks_went_before_which_words_and_tells_the_leaving_member_nothing() {
    let mut w = world();
    // The chief's window runs: the removal's word goes to it.
    w.ledger
        .start_conversation(w.id("chief"), "claude-code")
        .expect("its conversation");
    let (task, session, _) = queued(&mut w);
    let chiefs = tell(&mut w, task, "Use the v2 grammar");
    // The member told its own session's window: it leaves with it.
    w.ledger
        .tell_queued(w.project, task, "zeus", "And keep the old tests")
        .expect("kept");
    let before = w.ledger.inbox(w.id("chief"), 100).unwrap().len();
    let (zeus, left) = (w.id("zeus"), w.id(&session));
    w.ledger.remove_member(w.project, "zeus").expect("removed");

    let mut chief = w.ledger.inbox(w.id("chief"), 100).unwrap();
    chief.sort_by_key(|message| message.id);
    let new: Vec<String> = chief[before..]
        .iter()
        .map(|message| message.body.clone())
        .collect();
    assert_eq!(
        new,
        [
            format!("@zeus left the staff; it takes no more tasks. Cancelled with it: T-{task}."),
            format!(
                "T-{task} was cancelled: @zeus left the staff. {}",
                went(&[chiefs.id])
            ),
        ]
    );
    let mut to_the_leaving = -1;
    w.edit(|db| {
        to_the_leaving = db
            .query_row(
                "SELECT COUNT(*) FROM message WHERE recipient_id IN (?1, ?2) AND kind = 'note'
                   AND body LIKE '%left the staff%'",
                [zeus, left],
                |row| row.get(0),
            )
            .expect("a count");
    });
    assert_eq!(to_the_leaving, 0, "no word to the window that leaves");
}

/// T-`number`, its brief passed on by the human and in, and words the chief told
/// its window while the brief was on its way held for the human; the human
/// pauses it (Calliope's P3-2 of T-446). The words.
fn paused_with_held_words(w: &mut World, number: i64) -> MessageView {
    let brief = w.ledger.task(w.project, number).unwrap().unwrap().messages[0].id;
    w.ledger.approve_message(brief, "human").expect("passed on");
    w.begin(brief);
    let told = tell(w, number, "Use the v2 grammar");
    assert_eq!(told.state, "gated", "one agent's word to another");
    w.confirm(brief);
    w.ledger
        .pause_task(w.project, number, Some("human"), None)
        .expect("the human pauses");
    told
}

#[test]
fn held_words_of_a_paused_task_for_a_tier_outlive_its_members_removal_and_its_resumer_hears_they_were_withdrawn(
) {
    let mut w = gated_world();
    let (task, _, _) = queued(&mut w);
    let told = paused_with_held_words(&mut w, task);
    w.ledger.remove_member(w.project, "zeus").expect("removed");
    assert_eq!(w.states(&[told.id]), ["gated"], "still held, kept");

    w.resume(task, "Go on");
    assert_eq!(w.state(task), "open");
    assert_eq!(w.states(&[told.id]), ["cancelled"], "withdrawn with it");
    assert!(
        notes_on(&w, "chief", task).contains(&format!(
            "Withdrawn with it, still waiting for the human: m-{}.",
            told.id
        )),
        "{:?}",
        notes_on(&w, "chief", task)
    );
}

#[test]
fn held_words_of_a_paused_task_bound_to_a_removed_members_window_are_named_by_its_cancel() {
    let mut w = gated_world();
    let task = w.give("zeus", "Parser").task.number;
    let told = paused_with_held_words(&mut w, task);
    w.ledger.remove_member(w.project, "zeus").expect("removed");
    assert_eq!(w.states(&[told.id]), ["gated"], "still held, kept");
    w.ledger
        .cancel_task(w.project, task, "human")
        .expect("cancelled");
    assert_eq!(
        notes_on(&w, "chief", task),
        [format!(
            "@human cancelled T-{task} (Parser). {}",
            went(&[told.id])
        )]
    );
}

#[test]
fn words_kept_for_a_paused_task_through_its_members_removal_ride_in_no_carrier_it_cancelled() {
    // Told while the brief was queued, so they ride in it; its launch had begun
    // when the human paused the task (Calliope's P3-3 of T-446).
    let mut w = world();
    let (task, session, brief) = queued(&mut w);
    let told = tell(&mut w, task, "Use the v2 grammar");
    w.begin(brief);
    w.ledger
        .pause_task(w.project, task, Some("human"), None)
        .expect("the human pauses");
    let carrier = |w: &mut World| {
        let mut carried_by = None;
        w.edit(|db| {
            carried_by = db
                .query_row(
                    "SELECT carried_by FROM message WHERE id = ?1",
                    [told.id],
                    |row| row.get::<_, Option<i64>>(0),
                )
                .expect("its row");
        });
        carried_by
    };
    assert_eq!(carrier(&mut w), Some(brief), "riding in the brief");
    w.ledger.remove_member(w.project, "zeus").expect("removed");
    assert_eq!(w.states(&[brief, told.id]), ["cancelled", "queued"]);
    assert_eq!(
        carrier(&mut w),
        None,
        "its own again, as a carrier cancelled leaves it"
    );

    w.resume(task, "Go on");
    let body = w.ledger.task(w.project, task).unwrap().unwrap().task.body;
    assert!(
        body.contains(&format!(
            "Kept from before, never delivered to @{session}:\n\n(note m-{} from @chief)",
            told.id
        )),
        "{body}"
    );
}

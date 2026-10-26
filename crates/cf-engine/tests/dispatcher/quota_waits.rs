//! A quota wait never fails, and never gives the work away (dip-agentos,
//! 2026-10-17 to 19, ConsensFlow 3.0.4: T-17 and T-20 were told they wait out
//! of quota and go on by themselves, then ended failed, their work left in
//! their windows). While a member is out, its task stays in its window with
//! its work, whoever else of its tier is free: only the human or the chief
//! moves it. At the reset, or when the human says the member is back, the same
//! window is given the words that resume it. Held to no Node trace: Node's
//! dispatcher gave a task whose reset was far to another member of its tier.

use cf_engine::testing::Context;
use cf_harness::records::Role;

use crate::fixtures::{after, assert_match, exhausted, now, placed, Tiers};

/// The words that resume a task its member's quota held.
const RESUMED: &str = r"^Resumed: Go on where you stopped\.$";

/// The native session of a participant's current conversation.
fn current_session(context: &Context, participant: i64) -> Option<String> {
    context
        .ledger
        .borrow()
        .current_conversation(participant)
        .unwrap()
        .unwrap()
        .native_session
}

#[test]
fn keeps_the_task_with_its_window_though_another_of_its_tier_is_free_and_the_reset_hours_away_and_goes_on_there_when_the_human_says_the_member_is_back(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    let zeus = tiers.id(&session);
    let native = current_session(&context, zeus);
    let resets_at = after(&context, 3 * 3_600_000);
    context
        .adapter
        .refuse(&session, exhausted(Some(&now(&context)), Some(&resets_at)));
    context.pass().unwrap();
    context.pass().unwrap();
    let held = tiers.task(1).task;
    assert_eq!(
        (
            held.state.as_str(),
            held.assignee.as_deref(),
            held.held_until.as_deref()
        ),
        ("paused", Some(session.as_str()), Some(resets_at.as_str())),
        "diana is free, but the work is in zeus's window"
    );
    assert!(
        !held.body.contains("Reassigned"),
        "never given to another member: {}",
        held.body
    );
    assert_eq!(
        tiers.notes("chief"),
        [format!(
            "T-1 waits with @{session}: out of quota until {resets_at}, or sooner if its account has quota again; it goes on by itself."
        )]
    );
    assert!(context.host.killed().is_empty(), "its window waits with it");

    context.back_from_quota(tiers.project.id, "zeus").unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        placed(&tiers.task(1).task),
        ("working", Some(session.as_str()))
    );
    let inbox = context.ledger.borrow().inbox(zeus, 100).unwrap();
    assert_match(&inbox[0].body, RESUMED);
    assert_eq!(current_session(&context, zeus), native, "the same window");
    context.adapter.answer(&session, "Parser done");
    context.pass().unwrap();
    let notes = tiers.notes("chief");
    assert_eq!(
        notes
            .iter()
            .filter(|note| note.starts_with("T-1 goes on"))
            .count(),
        1,
        "{notes:?}"
    );
    assert!(
        notes.iter().all(|note| !note.contains("failed")),
        "{notes:?}"
    );
    assert_ne!(tiers.task(1).task.state, "failed");
}

#[test]
fn a_first_message_its_harness_took_before_refusing_is_not_given_again_at_the_reset() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    let zeus = tiers.id(&session);
    let brief = context.ledger.borrow().inbox(zeus, 100).unwrap()[0].clone();
    assert_eq!(brief.state, "delivering");
    // The look that first shows the brief in the record shows the refusal after it.
    let resets_at = after(&context, 20 * 60_000);
    context
        .adapter
        .refuse(&session, exhausted(Some(&now(&context)), Some(&resets_at)));
    context.pass().unwrap();
    let state_of = |id: i64| context.ledger.borrow().message(id).unwrap().unwrap().state;
    assert_eq!(
        (tiers.task(1).task.state, state_of(brief.id)),
        ("paused".to_owned(), "delivered".to_owned()),
        "the harness had it: it arrived"
    );
    context.advance(21 * 60_000);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        placed(&tiers.task(1).task),
        ("working", Some(session.as_str()))
    );
    let pasted = context
        .adapter
        .agent(&session)
        .items
        .iter()
        .filter(|item| item.role == Role::User && item.text.contains("Write the parser"))
        .count();
    assert_eq!(pasted, 1, "the brief went into the window once");
}

#[test]
fn a_message_its_harness_refused_before_taking_it_goes_again_without_counting_an_attempt() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    let zeus = tiers.id(&session);
    let brief = context.ledger.borrow().inbox(zeus, 100).unwrap()[0].clone();
    assert_eq!((brief.state.as_str(), brief.attempts), ("delivering", 1));
    // The record never shows the brief: the harness refused before it took it.
    context.adapter.with(&session, |agent| {
        agent.items.retain(|item| item.role != Role::User);
    });
    context.adapter.refuse(
        &session,
        exhausted(Some(&now(&context)), Some(&after(&context, 20 * 60_000))),
    );
    context.pass().unwrap();
    let again = context.ledger.borrow().message(brief.id).unwrap().unwrap();
    assert_eq!(
        (again.state.as_str(), again.attempts),
        ("queued", 0),
        "quota is no failure of the window's: its attempt is given back"
    );
}

#[test]
fn a_refusal_of_the_words_that_resumed_the_task_holds_it_again_without_telling_the_requester_again_and_tells_the_human_once(
) {
    // A reset nobody named is an hour away, and a limit that does not reset by
    // itself (a plan without the model, a spent balance) is refused again at
    // each: the chief heard "waits" and "goes on" every hour (the review of
    // the quota wait, T-88).
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    context
        .adapter
        .refuse(&session, exhausted(Some(&now(&context)), None));
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "paused");
    let waits = |notes: &[String]| notes.iter().filter(|n| n.starts_with("T-1 waits")).count();
    let goes_on = |notes: &[String]| {
        notes
            .iter()
            .filter(|n| n.starts_with("T-1 goes on"))
            .count()
    };
    let may_need = |notes: &[String]| notes.iter().filter(|n| n.contains("may need you")).count();
    for round in 0..3 {
        context.advance(61 * 60_000);
        for _ in 0..3 {
            context.pass().unwrap();
        }
        assert_eq!(tiers.task(1).task.state, "working", "round {round}");
        let inbox = context
            .ledger
            .borrow()
            .inbox(tiers.id(&session), 100)
            .unwrap();
        assert_match(&inbox[0].body, RESUMED);
        // Refused again, the moment it goes on.
        context.advance(1_000);
        context
            .adapter
            .refuse(&session, exhausted(Some(&now(&context)), None));
        for _ in 0..3 {
            context.pass().unwrap();
        }
        let held = tiers.task(1).task;
        assert_eq!(
            (held.state.as_str(), held.assignee.as_deref()),
            ("paused", Some(session.as_str())),
            "round {round}"
        );
        // Told it waits once, and never that it goes on: no turn got through (T-96).
        let chief = tiers.notes("chief");
        assert_eq!(
            (waits(&chief), goes_on(&chief)),
            (1, 0),
            "round {round}: {chief:?}"
        );
        let human = tiers.notes("human");
        assert_eq!(may_need(&human), 1, "round {round}: {human:?}");
    }
    let human = tiers.notes("human");
    assert_match(
        human.iter().find(|n| n.contains("may need you")).unwrap(),
        &format!(
            r"^T-1 was refused for quota again as soon as it went on: @{session}'s account may need you \(a plan that includes its model, a balance, or another account\)\. It waits, and ConsensFlow tries it again at each reset; press Try again now on @zeus once its account has quota\.$"
        ),
    );
}

#[test]
fn holds_the_work_of_a_second_window_whose_refusal_is_dated_before_its_member_was_marked_out() {
    // The second refusal, dated before the daemon marked the member out, read
    // as history: its task stayed working while its member was out (T-88).
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open_body("One");
    tiers.open_body("Two");
    context.pass().unwrap();
    context.pass().unwrap();
    let (one, two) = (tiers.assignee(1), tiers.assignee(2));
    // Both were refused five seconds ago; the second's record shows it a pass
    // after the first's took the member out.
    let resets_at = after(&context, 48 * 3_600_000);
    let refused_at = after(&context, -5_000);
    context
        .adapter
        .refuse(&one, exhausted(Some(&refused_at), Some(&resets_at)));
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "paused");
    context
        .adapter
        .refuse(&two, exhausted(Some(&refused_at), Some(&resets_at)));
    context.pass().unwrap();
    context.pass().unwrap();
    for number in [1, 2] {
        let held = tiers.task(number).task;
        assert_eq!(
            (held.state.as_str(), held.held_until.as_deref()),
            ("paused", Some(resets_at.as_str())),
            "T-{number}"
        );
    }
    assert_eq!(tiers.task(1).task.assignee.as_deref(), Some(one.as_str()));
    assert_eq!(tiers.task(2).task.assignee.as_deref(), Some(two.as_str()));
}

/// The meta prompt Claude Code sends by itself once its limit resets.
const CONTINUE: &str =
    "Your claude.ai usage limit has reset. Continue the task you were working on.";

/// T-1 working with zeus, then held: zeus's harness refused it, its reset two
/// hours away. The refusal's own words are the window's last.
fn held_for_two_hours(context: &Context) -> (Tiers<'_>, String) {
    let tiers = Tiers::new(context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    context.adapter.refuse(
        &session,
        exhausted(Some(&now(context)), Some(&after(context, 2 * 3_600_000))),
    );
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "paused");
    (tiers, session)
}

/// What the window was given about T-1 that says `words`.
fn pasted_saying(context: &Context, session: &str, words: &str) -> usize {
    context
        .adapter
        .agent(session)
        .items
        .iter()
        .filter(|item| item.role == Role::User && item.text.contains(words))
        .count()
}

#[test]
fn a_held_tasks_window_that_goes_on_by_itself_is_the_task_going_on_and_its_answer_is_the_result() {
    // Claude Code continues by itself once its account has quota again, before
    // the reset ConsensFlow holds the task until: its report reached no result,
    // and the words that resume the task went in after it (T-95).
    let context = Context::new();
    let (tiers, session) = held_for_two_hours(&context);
    context.advance(60_000);
    context
        .adapter
        .begins_its_own_turn(&session, CONTINUE, &now(&context));
    context.pass().unwrap();
    context.advance(10_000);
    context
        .adapter
        .ends_its_own_turn(&session, "The parser is done.", &now(&context));
    for _ in 0..4 {
        context.pass().unwrap();
    }
    let task = tiers.task(1);
    let results: Vec<&str> = task
        .messages
        .iter()
        .filter(|m| m.kind == "result")
        .map(|m| m.body.as_str())
        .collect();
    assert_eq!(results, ["The parser is done."], "{:?}", task.task.state);
    assert_eq!(
        pasted_saying(&context, &session, "Resumed:"),
        0,
        "no words were pasted after its own turn"
    );
    let notes = tiers.notes("chief");
    assert_eq!(
        notes
            .iter()
            .filter(|n| n.starts_with("T-1 goes on"))
            .count(),
        1,
        "{notes:?}"
    );
}

#[test]
fn the_words_that_resume_a_task_queued_while_its_window_goes_on_by_itself_are_withdrawn_and_never_pasted(
) {
    // The race: the hold ends and the words that resume the task are queued
    // while the window is in a turn of its own, which they would follow (T-95).
    let context = Context::new();
    let (tiers, session) = held_for_two_hours(&context);
    context.advance(2 * 3_600_000 + 60_000);
    context.adapter.quota(&session, None);
    context
        .adapter
        .begins_its_own_turn(&session, CONTINUE, &now(&context));
    context.pass().unwrap();
    context.pass().unwrap();
    let words = context
        .ledger
        .borrow()
        .inbox(tiers.id(&session), 100)
        .unwrap()
        .into_iter()
        .find(|m| m.body.starts_with("Resumed:"))
        .expect("the hold ended and its words were queued");
    assert_eq!(words.state, "queued", "the window is in its own turn");
    context.advance(10_000);
    context
        .adapter
        .ends_its_own_turn(&session, "The parser is done.", &now(&context));
    for _ in 0..4 {
        context.pass().unwrap();
    }
    let gone = context.ledger.borrow().message(words.id).unwrap().unwrap();
    assert_eq!(gone.state, "cancelled", "{gone:?}");
    assert_eq!(pasted_saying(&context, &session, "Resumed:"), 0);
    let task = tiers.task(1);
    let results: Vec<&str> = task
        .messages
        .iter()
        .filter(|m| m.kind == "result")
        .map(|m| m.body.as_str())
        .collect();
    assert_eq!(results, ["The parser is done."], "{:?}", task.task.state);
    let notes = tiers.notes("chief");
    assert_eq!(
        notes
            .iter()
            .filter(|n| n.starts_with("T-1 goes on"))
            .count(),
        1,
        "the words withdrawn, it is told it goes on once: {notes:?}"
    );
}

#[test]
fn a_task_resumed_with_words_ends_once_though_its_window_goes_on_by_itself_after() {
    // ConsensFlow first: its words resume the task and their answer is the
    // result; the turn the window takes by itself after it gives nothing more.
    let context = Context::new();
    let (tiers, session) = held_for_two_hours(&context);
    context.advance(2 * 3_600_000 + 60_000);
    context.adapter.quota(&session, None);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(pasted_saying(&context, &session, "Resumed:"), 1);
    context
        .adapter
        .answer(&session, "Went on: the parser is done.");
    for _ in 0..2 {
        context.pass().unwrap();
    }
    context
        .adapter
        .begins_its_own_turn(&session, CONTINUE, &now(&context));
    context
        .adapter
        .ends_its_own_turn(&session, "Nothing left to do.", &now(&context));
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let task = tiers.task(1);
    let results: Vec<&str> = task
        .messages
        .iter()
        .filter(|m| m.kind == "result")
        .map(|m| m.body.as_str())
        .collect();
    assert_eq!(results, ["Went on: the parser is done."]);
}

#[test]
fn a_held_tasks_window_whose_own_turn_fails_or_is_refused_again_does_not_go_on() {
    let context = Context::new();
    let (tiers, session) = held_for_two_hours(&context);
    // A turn of its own that fails, as Claude Code writes an API error: an
    // answer that says so, and the turn failed.
    context.advance(60_000);
    context
        .adapter
        .begins_its_own_turn(&session, CONTINUE, &now(&context));
    let error = context
        .adapter
        .item(Role::Assistant, "API Error: Can't reach the API server");
    context.adapter.with(&session, |agent| {
        agent.items.push(error);
        agent.settled = true;
        agent.failed = true;
    });
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "paused", "a failed turn");
    // A turn of its own, looked at before it answers, that its harness then refuses again.
    context.advance(60_000);
    context
        .adapter
        .begins_its_own_turn(&session, CONTINUE, &now(&context));
    context.pass().unwrap();
    context.adapter.refuse(
        &session,
        exhausted(Some(&now(&context)), Some(&after(&context, 2 * 3_600_000))),
    );
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let task = tiers.task(1);
    assert_eq!(task.task.state, "paused", "refused again");
    assert!(task.messages.iter().all(|m| m.kind != "result"));
    let notes = tiers.notes("chief");
    assert!(
        notes.iter().all(|n| !n.starts_with("T-1 goes on")),
        "it never went on: {notes:?}"
    );
}

#[test]
fn a_tell_answered_while_the_words_that_resume_the_task_wait_is_no_turn_of_the_windows_own() {
    // A tell goes in on its own, not carried by the words that resume the task:
    // the window's answer to it is no turn of its own (T-95).
    let context = Context::new();
    let (tiers, session) = held_for_two_hours(&context);
    context.advance(2 * 3_600_000 + 60_000);
    context.adapter.quota(&session, None);
    context.tell(tiers.project.id, &session, 1, "Use grammar v2");
    for _ in 0..3 {
        if pasted_saying(&context, &session, "Use grammar v2") > 0 {
            break;
        }
        context.pass().unwrap();
    }
    let order: Vec<String> = context
        .adapter
        .agent(&session)
        .items
        .iter()
        .map(|item| {
            format!(
                "{:?}:{}",
                item.role,
                item.text.chars().take(50).collect::<String>()
            )
        })
        .collect();
    assert_eq!(
        pasted_saying(&context, &session, "Use grammar v2"),
        1,
        "{order:?}"
    );
    assert_eq!(
        pasted_saying(&context, &session, "Resumed:"),
        0,
        "{order:?}"
    );
    context.adapter.answer(&session, "Noted.");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        pasted_saying(&context, &session, "Resumed:"),
        1,
        "the words that resume it go in after the tell"
    );
    let task = tiers.task(1);
    assert!(
        task.messages.iter().all(|m| m.kind != "result"),
        "{:?}",
        task.task.state
    );
}

#[test]
fn a_turn_of_the_windows_own_that_fails_while_the_words_that_resume_the_task_wait_neither_withdraws_them_nor_fails_the_task(
) {
    let context = Context::new();
    let (tiers, session) = held_for_two_hours(&context);
    context.advance(2 * 3_600_000 + 60_000);
    context.adapter.quota(&session, None);
    context
        .adapter
        .begins_its_own_turn(&session, CONTINUE, &now(&context));
    context.pass().unwrap();
    context.pass().unwrap();
    // It fails, as Claude Code writes an API error: an answer that says so.
    let error = context
        .adapter
        .item(Role::Assistant, "API Error: Can't reach the API server");
    context.adapter.with(&session, |agent| {
        agent.items.push(error);
        agent.settled = true;
        agent.failed = true;
    });
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let task = tiers.task(1).task;
    assert_ne!(task.state, "failed", "{task:?}");
    assert_eq!(
        pasted_saying(&context, &session, "Resumed:"),
        1,
        "the words that resume it go in after the failed turn"
    );
}

#[test]
fn a_refusal_hours_into_the_turn_the_words_that_resumed_it_began_is_a_new_wait_its_requester_is_told_of(
) {
    // A task that worked for hours in the turn its resume began, and then runs
    // out for real, was taken for refused "as soon as it went on" (T-96).
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    context.adapter.refuse(
        &session,
        exhausted(Some(&now(&context)), Some(&after(&context, 20 * 60_000))),
    );
    for _ in 0..3 {
        context.pass().unwrap();
    }
    context.advance(21 * 60_000);
    context.adapter.quota(&session, None);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(pasted_saying(&context, &session, "Resumed:"), 1);
    // It works three hours in the turn the words began, and then runs out.
    context.adapter.busy(&session);
    context.advance(3 * 3_600_000);
    context.adapter.refuse(
        &session,
        exhausted(Some(&now(&context)), Some(&after(&context, 2 * 3_600_000))),
    );
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "paused");
    let chief = tiers.notes("chief");
    assert_eq!(
        chief.iter().filter(|n| n.starts_with("T-1 waits")).count(),
        2,
        "told it waits anew: {chief:?}"
    );
    let human = tiers.notes("human");
    assert!(
        human.iter().all(|n| !n.contains("may need you")),
        "{human:?}"
    );
}

#[test]
fn the_requester_hears_it_goes_on_when_the_resumed_turn_gets_through_not_when_its_words_go_in() {
    // With no named reset, the hour's resume was refused at once: the requester
    // heard a false "goes on" an hour in, and nothing at the real return (T-96).
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    context
        .adapter
        .refuse(&session, exhausted(Some(&now(&context)), None));
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let told = |start: &str| {
        tiers
            .notes("chief")
            .iter()
            .filter(|n| n.starts_with(start))
            .count()
    };
    assert_eq!(told("T-1 waits"), 1);
    // The hour passes: the words go in, and are refused at once.
    context.advance(61 * 60_000);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(pasted_saying(&context, &session, "Resumed:"), 1);
    assert_eq!(
        told("T-1 goes on"),
        0,
        "the words went in, no turn got through"
    );
    context.advance(1_000);
    context
        .adapter
        .refuse(&session, exhausted(Some(&now(&context)), None));
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "paused");
    // Days later the account has quota again: the words go in, and a turn gets through.
    context.advance(3 * 24 * 3_600_000);
    context.adapter.quota(&session, None);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(pasted_saying(&context, &session, "Resumed:"), 2);
    assert_eq!(told("T-1 goes on"), 0);
    context.adapter.answer(&session, "The parser is done.");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!((told("T-1 waits"), told("T-1 goes on")), (1, 1));
    let task = tiers.task(1);
    assert!(
        task.messages.iter().any(|m| m.kind == "result"),
        "{:?}",
        task.task.state
    );
}

#[test]
fn a_held_tasks_own_turn_gives_its_answer_alone_though_the_daemon_restarted_in_it() {
    // What the engine kept of where the window's own turn began was in memory
    // only: after a restart the result began with the refusal's words (T-96).
    let context = Context::new();
    context.adapter.resumes_records.set(true);
    let (tiers, session) = held_for_two_hours(&context);
    context.advance(60_000);
    context
        .adapter
        .begins_its_own_turn(&session, CONTINUE, &now(&context));
    // A word of its answer, complete, and it works on.
    let mut said = context
        .adapter
        .item(Role::Assistant, "Picking the parser up.");
    said.at = Some(serde_json::json!(now(&context)));
    // As a harness reads a turn that gets through, the refusal before it is no
    // longer its last word.
    context.adapter.with(&session, |agent| {
        agent.items.push(said);
        agent.quota = None;
    });
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    // The daemon restarts in that turn, and the human resumes the project.
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(tiers.project.id).unwrap();
    for _ in 0..3 {
        after.pass().unwrap();
    }
    context.adapter.answer(&session, "The parser is done.");
    for _ in 0..3 {
        after.pass().unwrap();
    }
    let task = tiers.task(1);
    let results: Vec<&str> = task
        .messages
        .iter()
        .filter(|m| m.kind == "result")
        .map(|m| m.body.as_str())
        .collect();
    assert_eq!(results.len(), 1, "{results:?} {:?}", task.task.state);
    assert!(
        !results[0].contains("hit your"),
        "no word of the refusal: {results:?}"
    );
}

/// T-1 held with zeus, no reset named, its hold note given the chief; the hour
/// passes and the words that resume it go in, and the window writes a word of
/// its answer a second later, not complete: a call it makes.
fn resumed_and_a_fragment_written(context: &Context) -> (Tiers<'_>, String) {
    let tiers = Tiers::new(context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    context
        .adapter
        .refuse(&session, exhausted(Some(&now(context)), None));
    for _ in 0..3 {
        context.pass().unwrap();
    }
    context.advance(61 * 60_000);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(pasted_saying(context, &session, "Resumed:"), 1);
    context.advance(1_000);
    let mut fragment = context
        .adapter
        .item(Role::Assistant, "Reading the parser first.");
    fragment.complete = false;
    fragment.at = Some(serde_json::json!(now(context)));
    context.adapter.with(&session, |agent| {
        agent.items.push(fragment);
        agent.settled = false;
        agent.failed = false;
        agent.quota = None;
    });
    context.pass().unwrap();
    (tiers, session)
}

fn goes_on_told(tiers: &Tiers) -> usize {
    tiers
        .notes("chief")
        .iter()
        .filter(|n| n.starts_with("T-1 goes on"))
        .count()
}

#[test]
fn a_fragment_of_the_resumed_turn_refused_a_minute_later_tells_no_goes_on() {
    // "Goes on" was told on the resumed turn's first word, a fragment; the
    // refusal a minute later was the same wait, and the chief had heard a
    // false "goes on" and nothing after (T-165).
    let context = Context::new();
    let (tiers, session) = resumed_and_a_fragment_written(&context);
    assert_eq!(goes_on_told(&tiers), 0, "a fragment a second in");
    context.advance(60_000);
    context
        .adapter
        .refuse(&session, exhausted(Some(&now(&context)), None));
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "paused");
    assert_eq!(goes_on_told(&tiers), 0, "{:?}", tiers.notes("chief"));
}

#[test]
fn a_resumed_turn_still_at_work_past_the_time_a_refusal_is_the_same_wait_tells_it_goes_on() {
    let context = Context::new();
    let (tiers, _session) = resumed_and_a_fragment_written(&context);
    assert_eq!(goes_on_told(&tiers), 0);
    context.advance(6 * 60_000);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(goes_on_told(&tiers), 1, "{:?}", tiers.notes("chief"));
}

/// The results of T-1.
fn results(tiers: &Tiers<'_>) -> Vec<String> {
    tiers
        .task(1)
        .messages
        .into_iter()
        .filter(|message| message.kind == "result")
        .map(|message| message.body)
        .collect()
}

#[test]
fn a_held_tasks_window_whose_words_quoted_its_briefs_header_before_the_brief_went_in_goes_on_by_itself(
) {
    // Where the window was given its brief is the item its arrival was proved by, as
    // turn-end finds it (`delivered_at`, T-531), never words that quote its header:
    // read by the first item that held its marker, the quotation was the paste, the
    // real paste after it a message of ConsensFlow's, and the window's own turn none.
    let context = Context::new();
    let (tiers, session) = held_for_two_hours(&context);
    let brief = tiers.task(1).messages[0].id;
    let quoted = context.adapter.item(
        Role::User,
        &format!(
            "Is {}T-1 the parser task?",
            cf_engine::delivery_text::marker_of(brief)
        ),
    );
    context
        .adapter
        .with(&session, |agent| agent.items.insert(0, quoted));
    context.advance(60_000);
    context
        .adapter
        .begins_its_own_turn(&session, CONTINUE, &now(&context));
    context.pass().unwrap();
    context.advance(10_000);
    context
        .adapter
        .ends_its_own_turn(&session, "The parser is done.", &now(&context));
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(results(&tiers), ["The parser is done."]);
    assert_eq!(pasted_saying(&context, &session, "Resumed:"), 0);
}

#[test]
fn a_held_tasks_window_that_went_on_by_itself_and_compacted_after_its_answer_goes_on() {
    // A compaction's summary, written in the user's place at rest after the answer, is
    // no turn of the window's own, as it is none of the answer's (`answers_to_the_task`):
    // read in one look with the turn and its answer, the summary was taken for the turn,
    // and nothing answered it.
    let context = Context::new();
    let (tiers, session) = held_for_two_hours(&context);
    context.advance(60_000);
    context
        .adapter
        .begins_its_own_turn(&session, CONTINUE, &now(&context));
    context
        .adapter
        .ends_its_own_turn(&session, "The parser is done.", &now(&context));
    let mut summary = context
        .adapter
        .item(Role::User, "Summary of the conversation so far");
    summary.compaction = true;
    context
        .adapter
        .with(&session, |agent| agent.items.push(summary));
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(results(&tiers), ["The parser is done."]);
    assert_eq!(pasted_saying(&context, &session, "Resumed:"), 0);
}

#[test]
fn a_held_tasks_window_whose_summary_quotes_its_briefs_header_goes_on_by_itself() {
    // Calliope's P3-1 of T-532: the check for another paste after the window's own turn
    // read compaction summaries too, and a summary that quotes the brief's header was
    // taken for a paste of ConsensFlow's: the task waited for the daemon's resume words.
    let context = Context::new();
    let (tiers, session) = held_for_two_hours(&context);
    let brief = tiers.task(1).messages[0].id;
    context.advance(60_000);
    context
        .adapter
        .begins_its_own_turn(&session, CONTINUE, &now(&context));
    context
        .adapter
        .ends_its_own_turn(&session, "The parser is done.", &now(&context));
    let mut summary = context.adapter.item(
        Role::User,
        &format!(
            "Summary: the task was {}T-1, write the parser",
            cf_engine::delivery_text::marker_of(brief)
        ),
    );
    summary.compaction = true;
    context
        .adapter
        .with(&session, |agent| agent.items.push(summary));
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(results(&tiers), ["The parser is done."]);
    assert_eq!(pasted_saying(&context, &session, "Resumed:"), 0);
}

//! A member out of quota mid-task: its task is held with its window when the
//! reset is near or nobody else could take it, and goes on by itself at the
//! reset; every window of the member acts on its own refusal; the member is
//! back once a window gets a turn through, or when the human says so.

use cf_engine::testing::Context;
use cf_engine::ActivityState;
use cf_ledger::NewTask;

use serde_json::json;

use crate::fixtures::{after, assert_match, exhausted, now, placed, soon, Tiers};
use crate::traces::held_to;

const SUITES: &[&str] = &["a member out of quota mid-task"];

/// The words that resume a task its member's quota held.
const RESUMED: &str = r"^Resumed: Go on where you stopped\.$";

#[test]
fn holds_the_task_with_its_window_when_the_reset_is_near_and_goes_on_by_itself_when_it_passes() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        placed(&tiers.task(1).task),
        ("working", Some("zeus-amber-pine"))
    );
    let zeus = tiers.id("zeus-amber-pine");
    let native = current_session(&context, zeus);
    let resets_at = after(&context, 20 * 60_000);
    context
        .adapter
        .quota("zeus", Some(exhausted(None, Some(&resets_at))));
    context.pass().unwrap();
    let held = tiers.task(1).task;
    assert_eq!(
        (
            held.state.as_str(),
            held.assignee.as_deref(),
            held.held_until
        ),
        ("paused", Some("zeus-amber-pine"), Some(resets_at)),
        "held with its window: diana is free, but the reset is twenty minutes away"
    );
    assert_match(
        tiers.notes("chief").last().unwrap(),
        r"^T-1 waits with @zeus-amber-pine: out of quota until .*, or sooner if its account has quota again; it goes on by itself\.$",
    );
    assert!(
        context.host.killed().is_empty(),
        "the window waits, as any paused task’s"
    );
    // The agent, stopped, says where it was; that is not a result while the task is held.
    context.adapter.answer("zeus", "Stopped at the lexer.");
    context.pass().unwrap();
    assert_eq!(
        tiers.task(1).task.state,
        "paused",
        "nothing moves before the reset"
    );
    context.advance(21 * 60_000);
    // The window no longer shows the refusal once the reset has passed.
    context.adapter.quota("zeus", None);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        placed(&tiers.task(1).task),
        ("working", Some("zeus-amber-pine"))
    );
    // The same window, on the same conversation, with the words of a Resume.
    let inbox = context.ledger.borrow().inbox(zeus, 100).unwrap();
    let resumed = &inbox[0];
    assert_eq!(
        (resumed.state.as_str(), resumed.kind.as_str()),
        ("delivered", "task")
    );
    assert_match(&resumed.body, RESUMED);
    assert_eq!(current_session(&context, zeus), native);
    held_to(
        context.close(),
        SUITES,
        "holds the task with its window when the reset is near, and goes on by itself when it passes",
    );
}

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
fn holds_the_task_when_nobody_else_of_its_tier_is_free_whatever_the_reset() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let resets_at = after(&context, 3 * 3_600_000);
    context
        .adapter
        .quota("zeus", Some(exhausted(None, Some(&resets_at))));
    context.pass().unwrap();
    let held = tiers.task(1).task;
    assert_eq!(
        (held.state.as_str(), held.held_until),
        ("paused", Some(resets_at))
    );
    held_to(
        context.close(),
        SUITES,
        "holds the task when nobody else of its tier is free, whatever the reset",
    );
}

#[test]
fn closes_the_window_of_a_held_task_that_is_cancelled_though_its_member_is_still_out() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let resets_at = after(&context, 3 * 3_600_000);
    context
        .adapter
        .quota("zeus", Some(exhausted(None, Some(&resets_at))));
    context.pass().unwrap();
    assert_eq!(
        (tiers.task(1).task.state, context.host.killed().len()),
        ("paused".to_owned(), 0),
        "held with its window"
    );
    let pane = context.host.last("zeus").unwrap().pane;
    context
        .ledger
        .borrow_mut()
        .cancel_task(tiers.project.id, 1, "human")
        .unwrap();
    context.pass().unwrap();
    assert_eq!(
        context.host.killed(),
        std::slice::from_ref(&pane),
        "it waits for no reset now"
    );
    // Not held to Node's recording: an agent still at work on the turn of a
    // task cancelled under it is interrupted while its member is out, as at any
    // other time: its look says it works, whatever the board says of it.
    assert_eq!(
        context.host.inputs(),
        [json!({ "id": pane.id, "generation": pane.generation, "bytes": [27] })]
    );
}

#[test]
fn holds_the_task_of_every_one_of_a_members_windows_that_runs_into_its_quota_not_only_the_first_ones(
) {
    // Three of a member's windows ran into a weekly limit after the first had
    // taken it out, and their tasks stayed working (poker-lab, 2026-10-04).
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open_body("One");
    tiers.open_body("Two");
    context.pass().unwrap();
    context.pass().unwrap();
    let (one, two) = (tiers.assignee(1), tiers.assignee(2));
    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["working", "working"]
    );
    let resets_at = after(&context, 48 * 3_600_000);
    let refusal = || exhausted(Some(&now(&context)), Some(&resets_at));
    context.adapter.refuse(&one, refusal());
    context.pass().unwrap();
    let first = tiers.task(1).task;
    assert_eq!(
        (first.state.as_str(), first.held_until),
        ("paused", Some(resets_at.clone()))
    );
    // The other window runs into the same limit a minute later, its member out by then.
    context.advance(60_000);
    context.adapter.refuse(&two, refusal());
    context.pass().unwrap();
    let second = tiers.task(2).task;
    assert_eq!(
        (second.state.as_str(), second.held_until),
        ("paused", Some(resets_at.clone()))
    );
    assert_eq!(
        tiers.notes("chief"),
        [
            format!(
                "T-1 waits with @{one}: out of quota until {resets_at}, or sooner if its account has quota again; it goes on by itself."
            ),
            format!(
                "T-2 waits with @{two}: out of quota until {resets_at}, or sooner if its account has quota again; it goes on by itself."
            ),
        ]
    );
    context.pass().unwrap();
    assert_eq!(
        tiers.notes("chief").len(),
        2,
        "each window acts on its refusal once"
    );
    held_to(
        context.close(),
        SUITES,
        "holds the task of every one of a member's windows that runs into its quota, not only the first one's",
    );
}

#[test]
fn takes_a_member_back_once_one_of_its_windows_gets_a_turn_through_and_what_was_held_for_it_goes_on(
) {
    // The human logged the harness into another account (poker-lab, 2026-10-04).
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open_body("One");
    tiers.open_body("Two");
    context.pass().unwrap();
    context.pass().unwrap();
    let (one, two) = (tiers.assignee(1), tiers.assignee(2));
    let resets_at = after(&context, 48 * 3_600_000);
    context
        .adapter
        .refuse(&one, exhausted(Some(&now(&context)), Some(&resets_at)));
    context.pass().unwrap();
    assert_eq!(
        (tiers.task(1).task.state, tiers.out_until("zeus")),
        ("paused".to_owned(), Some(resets_at))
    );
    // The other window was in a long command meanwhile; its next turn gets through.
    context.advance(60_000);
    context
        .adapter
        .writes(&two, "The tests pass.", &now(&context));
    context.pass().unwrap();
    assert_eq!(tiers.out_until("zeus"), None, "back before its reset");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        placed(&tiers.task(1).task),
        ("working", Some(one.as_str())),
        "held, it goes on in its own window"
    );
    let inbox = context.ledger.borrow().inbox(tiers.id(&one), 100).unwrap();
    assert_match(&inbox[0].body, RESUMED);
    assert_eq!(
        tiers.task(2).task.state,
        "working",
        "the window that got through goes on as it was"
    );
    held_to(
        context.close(),
        SUITES,
        "takes a member back once one of its windows gets a turn through, and what was held for it goes on",
    );
}

#[test]
fn lets_a_turn_its_quota_cut_short_go_on_once_the_reset_has_passed_and_fails_nothing() {
    // A refusal still the window's last word at the reset read as a failed
    // turn, and failed its task (poker-lab, 2026-10-04).
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    context.adapter.refuse(
        &session,
        exhausted(
            Some(&after(&context, -2 * 3_600_000)),
            Some(&after(&context, -60_000)),
        ),
    );
    context.pass().unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        placed(&tiers.task(1).task),
        ("working", Some(session.as_str()))
    );
    let inbox = context
        .ledger
        .borrow()
        .inbox(tiers.id(&session), 100)
        .unwrap();
    assert_match(&inbox[0].body, RESUMED);
    let failed: Vec<String> = tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.contains("failed"))
        .collect();
    assert!(failed.is_empty());
    held_to(
        context.close(),
        SUITES,
        "lets a turn its quota cut short go on once the reset has passed, and fails nothing",
    );
}

#[test]
fn reaches_the_chief_again_once_the_human_says_it_is_back_before_its_reset() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    context.pass().unwrap();
    context.adapter.answer("chief", "Ready.");
    context.pass().unwrap();
    let until = soon(&context, 48);
    let chief = tiers.id("chief");
    context
        .ledger
        .borrow_mut()
        .mark_out(chief, &until, "out of quota")
        .unwrap();
    let message = context
        .create_task(
            tiers.project.id,
            NewTask {
                from: "human".to_owned(),
                to: Some("chief".to_owned()),
                body: "Plan the release".to_owned(),
                ..NewTask::default()
            },
        )
        .message
        .unwrap();
    context.pass().unwrap();
    assert_eq!(
        context.dispatcher.activity(tiers.id("chief")).state,
        ActivityState::Out
    );
    let state = || {
        context
            .ledger
            .borrow()
            .message(message.id)
            .unwrap()
            .unwrap()
            .state
    };
    assert_eq!(state(), "queued", "nothing reaches it while out");
    let back = context.back_from_quota(tiers.project.id, "chief").unwrap();
    assert_eq!(back.out_until, None);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(state(), "delivered");
    let refused = context
        .back_from_quota(tiers.project.id, "human")
        .unwrap_err()
        .to_string();
    assert_match(&refused, "no @human");
    held_to(
        context.close(),
        SUITES,
        "reaches the chief again once the human says it is back before its reset",
    );
}

#[test]
fn keeps_a_held_task_paused_its_hold_cleared_when_its_session_was_deleted_before_the_hold_ended_and_tells_its_requester_once_while_every_other_task_goes_on(
) {
    // A follow-up (--after) is given to no tier: it is its session's own, and
    // only another follow-up brings a deleted session back. The hold's end
    // failed every pass at its start, so nothing was delivered, launched or
    // collected for anyone (calliope, 2026-10-06).
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open_body("Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    context.create_task(
        tiers.project.id,
        NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            body: "Now the lexer".to_owned(),
            ..NewTask::default()
        },
    );
    tiers.open_body("Docs");
    tiers.open_for("worker", "light", "Rename a file");
    context.pass().unwrap();
    context.pass().unwrap();
    for (number, session) in [
        (2, "zeus-amber-pine"),
        (3, "diana-brisk-birch"),
        (4, "hera-calm-brook"),
    ] {
        assert_eq!(
            placed(&tiers.task(number).task),
            ("working", Some(session)),
            "the lexer in the session that wrote the parser; the docs and the rename given to their tiers"
        );
    }
    let resets_at = after(&context, 20 * 60_000);
    for member in ["zeus", "diana"] {
        context
            .adapter
            .quota(member, Some(exhausted(None, Some(&resets_at))));
    }
    context.pass().unwrap();
    for number in [2, 3] {
        let held = tiers.task(number).task;
        assert_eq!(
            (held.state.as_str(), held.held_until),
            ("paused", Some(resets_at.clone())),
            "both held with their windows: the lexer has no tier to go back to"
        );
    }
    // The agent, stopped, says where it was; that is not a result while the task is held.
    context.adapter.answer("diana", "Stopped at the docs.");
    context
        .end_session(tiers.project.id, "zeus-amber-pine")
        .unwrap();
    let on_board = || tiers.participant("zeus-amber-pine").is_some();
    assert!(!on_board(), "the human deleted the session that had T-2");
    let launches = || {
        context
            .adapter
            .prepared()
            .iter()
            .filter(|launch| launch["participant"]["handle"] == "zeus-amber-pine")
            .count()
    };
    let (launched, told) = (launches(), tiers.notes("chief").len());

    context.advance(21 * 60_000);
    tiers.open_body("Tests");
    let mut passes = 0;
    let mut pass = || {
        passes += 1;
        context
            .pass()
            .unwrap_or_else(|failed| panic!("pass {passes}, after the hold ended: {failed}"));
        let lexer = tiers.task(2).task;
        assert_eq!(
            (
                lexer.state.as_str(),
                lexer.held_until,
                lexer.assignee.as_deref(),
                lexer.body.as_str()
            ),
            ("paused", None, Some("zeus-amber-pine"), "Now the lexer"),
            "it stays paused with its words, and is not due again"
        );
    };
    pass();
    assert_eq!(
        tiers.task(3).task.state,
        "queued",
        "T-3, held with it and due after it, goes on in the pass that cannot resume T-2"
    );
    context.adapter.answer("hera", "Renamed");
    pass();
    pass();
    assert_eq!(
        tiers.notes("chief")[told..],
        ["T-2 stays paused: @zeus-amber-pine, the session it was given to, was deleted. It waits for your decision: cancel it, or give the work again."],
        "the requester hears once, not in every pass"
    );
    let inbox = context
        .ledger
        .borrow()
        .inbox(tiers.id("diana-brisk-birch"), 100)
        .unwrap();
    assert_eq!(
        (tiers.task(3).task.state.as_str(), inbox[0].body.as_str()),
        ("working", "Resumed: Go on where you stopped."),
        "T-3, held at the same time and after it in the pass, went on in its own window"
    );
    assert_eq!(
        tiers.task(4).task.state,
        "done",
        "the rename's answer was collected"
    );
    let tests = tiers.task(5).task;
    assert_eq!(
        (
            tests.state.as_str(),
            tests
                .assignee
                .is_some_and(|assignee| assignee.starts_with("diana-"))
        ),
        ("working", true),
        "the new task was given out, launched and delivered"
    );
    assert!(!on_board(), "nothing brings the deleted session back");
    assert_eq!(launches(), launched, "nor opens a window for it");
    assert!(
        context
            .ledger
            .borrow()
            .held_tasks_due("2099-01-01T00:00:00.000Z")
            .unwrap()
            .is_empty(),
        "none is due"
    );
    context
        .ledger
        .borrow_mut()
        .cancel_task(tiers.project.id, 2, "chief")
        .unwrap();
    assert_eq!(
        tiers.task(2).task.state,
        "cancelled",
        "the requester can decide"
    );
    held_to(
        context.close(),
        SUITES,
        "keeps a held task paused, its hold cleared, when its session was deleted before the hold ended, and tells its requester once while every other task goes on",
    );
}

#[test]
fn keeps_a_held_task_paused_its_hold_cleared_when_the_member_it_was_given_to_by_name_left_the_staff_and_says_what_the_ledger_refused(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    context.give(tiers.project.id, "zeus", "Write the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let resets_at = after(&context, 20 * 60_000);
    context
        .adapter
        .quota("zeus", Some(exhausted(None, Some(&resets_at))));
    context.pass().unwrap();
    let held = tiers.task(1).task;
    assert_eq!(
        (held.state.as_str(), held.held_until),
        ("paused", Some(resets_at))
    );
    // A member that leaves takes back what is in its hands, but not what is paused.
    context.remove_member(tiers.project.id, "zeus").unwrap();
    assert_eq!(tiers.task(1).task.state, "paused");
    let told = tiers.notes("chief").len();

    context.advance(21 * 60_000);
    for pass in 1..=2 {
        context
            .pass()
            .unwrap_or_else(|failed| panic!("pass {pass}, after the hold ended: {failed}"));
        let task = tiers.task(1).task;
        assert_eq!(
            (
                task.state.as_str(),
                task.held_until,
                task.assignee.as_deref()
            ),
            ("paused", None, Some("zeus"))
        );
    }
    assert_eq!(
        tiers.notes("chief")[told..],
        ["T-1 stays paused: it could not go on when its hold ended (the window that had T-1 has ended: cancel it and open the work for its tier). It waits for your decision: cancel it, or give the work again."]
    );
    held_to(
        context.close(),
        SUITES,
        "keeps a held task paused, its hold cleared, when the member it was given to by name left the staff, and says what the ledger refused",
    );
}

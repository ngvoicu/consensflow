//! An account's quota is every window's on it: where a harness logs all its
//! windows into the one account, a member back before its reset brings back the
//! others out on that harness, the chief among them; and a window out of quota
//! with a message waiting for it is said to the human, once for each mark (the
//! field report of 2026-10-22: results queued for days behind a chief at rest).

use cf_engine::testing::{Context, FakeAdapter, Made};
use cf_engine::ActivityState;
use cf_ledger::{MessageView, NewTask};
use serde_json::json;

use crate::fixtures::{after, assert_match, exhausted, now, participant, Tiers};

/// The words that tell the human what waits for a window out of quota.
const WAITS: &str = r"^@chief is out of quota until \S+, and m-(\d+) \(the words of T-\d+\) waits for it, as does whatever is sent to it until then\. If its account has quota again \(another account, a bigger plan\), press Try again now on @chief: what waits is typed in once it is back\.$";

/// zeus at work on T-1 on Claude Code, whose windows are on one account; then
/// the account runs out, refusing zeus's window and the chief's alike, with a
/// reset two days on: the two of them are out until it.
fn spent_together(context: &Context) -> (Tiers<'_>, String) {
    context.adapter.one_account.set(true);
    let tiers = Tiers::new(context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    let resets_at = after(context, 48 * 3_600_000);
    let refused = exhausted(Some(&now(context)), Some(&resets_at));
    context.adapter.refuse(&session, refused.clone());
    context.adapter.refuse("chief", refused);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.out_until("zeus"), Some(resets_at.clone()));
    assert_eq!(tiers.out_until("chief"), Some(resets_at));
    (tiers, session)
}

/// The same, and T-1 is paused for the reset, the chief out, and the note that
/// T-1 waits queued for it.
fn out_together(context: &Context) -> (Tiers<'_>, String) {
    let (tiers, session) = spent_together(context);
    assert_eq!(tiers.task(1).task.state, "paused");
    assert_eq!(
        context.dispatcher.activity(tiers.id("chief")).state,
        ActivityState::Out
    );
    (tiers, session)
}

/// The state of the first message to the chief that `found` picks.
fn to_chief(tiers: &Tiers<'_>, found: impl Fn(&MessageView) -> bool) -> Option<String> {
    let inbox = tiers
        .context
        .ledger
        .borrow()
        .inbox(tiers.id("chief"), 100)
        .unwrap();
    inbox
        .into_iter()
        .find(|message| found(message))
        .map(|message| message.state)
}

/// The window `session` of `fake` gets a turn through on the account that has
/// quota again: its record's last word is no longer the refusal, which it no
/// longer reads as a failed turn.
fn goes_on(fake: &FakeAdapter, session: &str, at: &str) {
    fake.with(session, |agent| {
        agent.quota = None;
        agent.failed = false;
    });
    fake.writes(session, "Going on.", at);
}

/// T-1 goes on in zeus's window `session` and is done: its result must reach the chief.
fn finished_and_handed_over(tiers: &Tiers<'_>, session: &str) {
    let context = tiers.context;
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working", "T-1 went on");
    context.adapter.answer(session, "Parser done.");
    context.pass().unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        to_chief(tiers, |message| message.kind == "result").as_deref(),
        Some("delivered"),
        "T-1's result reached the chief"
    );
}

#[test]
fn a_chief_out_on_its_members_account_is_back_once_a_window_of_the_member_gets_a_turn_through_and_is_given_what_waited(
) {
    let context = Context::new();
    let (tiers, session) = out_together(&context);
    // The human logs into another account and goes on in zeus's window.
    context.advance(60_000);
    goes_on(&context.adapter, &session, &now(&context));
    context.pass().unwrap();
    assert_eq!(tiers.out_until("zeus"), None);
    assert_eq!(tiers.out_until("chief"), None, "the chief is back with it");
    finished_and_handed_over(&tiers, &session);
}

#[test]
fn a_chief_out_on_its_members_account_is_back_once_the_human_says_the_member_is_back() {
    let context = Context::new();
    let (tiers, session) = out_together(&context);
    let back = context.back_from_quota(tiers.project.id, "zeus").unwrap();
    assert_eq!(back.out_until, None);
    assert_eq!(tiers.out_until("chief"), None, "the chief is back with it");
    context.adapter.with(&session, |agent| {
        agent.quota = None;
        agent.failed = false;
    });
    finished_and_handed_over(&tiers, &session);
}

#[test]
fn a_chief_marked_out_after_the_turn_that_got_through_stays_out() {
    let context = Context::new();
    context.adapter.one_account.set(true);
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    let resets_at = after(&context, 48 * 3_600_000);
    context
        .adapter
        .refuse(&session, exhausted(Some(&now(&context)), Some(&resets_at)));
    context.pass().unwrap();
    // zeus's window got a turn through a minute later, but its record is read
    // only after the chief was refused, a minute after that.
    let through = after(&context, 60_000);
    context.advance(120_000);
    context
        .adapter
        .refuse("chief", exhausted(Some(&now(&context)), Some(&resets_at)));
    context.pass().unwrap();
    assert_eq!(tiers.out_until("chief"), Some(resets_at.clone()));
    goes_on(&context.adapter, &session, &through);
    context.pass().unwrap();
    assert_eq!(tiers.out_until("zeus"), None);
    assert_eq!(
        tiers.out_until("chief"),
        Some(resets_at),
        "the turn got through before the chief was refused: no news for it"
    );
}

#[test]
fn a_chief_stays_out_when_its_harness_may_log_windows_into_several_accounts() {
    let context = Context::new();
    let (tiers, session) = out_together(&context);
    context.adapter.one_account.set(false);
    context.advance(60_000);
    goes_on(&context.adapter, &session, &now(&context));
    context.pass().unwrap();
    assert_eq!(tiers.out_until("zeus"), None);
    assert!(
        tiers.out_until("chief").is_some(),
        "zeus's account may be another"
    );
}

#[test]
fn a_chief_stays_out_when_the_member_that_came_back_runs_on_another_harness() {
    let context = Context::made(Made {
        codex: true,
        ..Made::default()
    });
    context.adapter.one_account.set(true);
    context.codex.one_account.set(true);
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "agent": "apollo" },
            "staff": [{ "agent": "zeus", "harness": "codex", "role": "worker", "tier": "standard" }],
        }))
        .unwrap()
        .unwrap();
    context.create_task(
        project.id,
        NewTask {
            from: "chief".to_owned(),
            pool: Some("worker".to_owned()),
            tier: Some("standard".to_owned()),
            body: "Write the parser".to_owned(),
            ..NewTask::default()
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    let session = context
        .task(project.id, 1)
        .task
        .assignee
        .expect("T-1 given to zeus");
    let resets_at = after(&context, 48 * 3_600_000);
    context
        .codex
        .refuse(&session, exhausted(Some(&now(&context)), Some(&resets_at)));
    context
        .adapter
        .refuse("chief", exhausted(Some(&now(&context)), Some(&resets_at)));
    context.pass().unwrap();
    context.pass().unwrap();
    let out = |handle: &str| {
        participant(&context, project.id, handle)
            .expect("the participant")
            .out_until
    };
    assert!(out("zeus").is_some() && out("chief").is_some());
    context.advance(60_000);
    goes_on(&context.codex, &session, &now(&context));
    context.pass().unwrap();
    assert_eq!(out("zeus"), None);
    assert!(out("chief").is_some(), "Codex's account is not Claude's");
}

#[test]
fn the_human_is_told_once_for_each_mark_what_waits_for_a_window_out_of_quota() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    context.pass().unwrap();
    context.adapter.answer("chief", "Ready.");
    context.pass().unwrap();
    let chief = tiers.id("chief");
    let mark_out = || {
        let until = after(&context, 48 * 3_600_000);
        context
            .ledger
            .borrow_mut()
            .mark_out(chief, &until, "out of quota")
            .unwrap();
    };
    let told = || -> Vec<String> {
        tiers
            .notes("human")
            .into_iter()
            .filter(|note| note.starts_with("@chief is out of quota"))
            .collect()
    };
    let send = |body: &str| {
        context
            .create_task(
                tiers.project.id,
                NewTask {
                    from: "human".to_owned(),
                    to: Some("chief".to_owned()),
                    body: body.to_owned(),
                    ..NewTask::default()
                },
            )
            .message
            .unwrap()
    };
    mark_out();
    context.pass().unwrap();
    assert_eq!(told(), Vec::<String>::new(), "nothing waits for it");

    let first = send("Plan the release");
    context.pass().unwrap();
    context.pass().unwrap();
    send("And the notes");
    context.pass().unwrap();
    let said = told();
    assert_eq!(said.len(), 1, "{said:?}");
    assert_match(&said[0], WAITS);
    assert!(
        said[0].contains(&format!(
            "m-{} (the words of T-{}) waits",
            first.id,
            first.task_number.unwrap()
        )),
        "{said:?}"
    );

    // Back, and out again: a new mark is told anew.
    context.back_from_quota(tiers.project.id, "chief").unwrap();
    context.advance(60_000);
    mark_out();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(told().len(), 2, "{:?}", told());
}

/// zeus at work on T-1 on Claude Code, whose windows are on one account, and the
/// chief refused alone, out two days: zeus's window was never out.
fn chief_out_alone(context: &Context) -> (Tiers<'_>, String, String) {
    context.adapter.one_account.set(true);
    let tiers = Tiers::new(context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    let resets_at = after(context, 48 * 3_600_000);
    context
        .adapter
        .refuse("chief", exhausted(Some(&now(context)), Some(&resets_at)));
    context.pass().unwrap();
    assert_eq!(tiers.out_until("chief"), Some(resets_at.clone()));
    assert_eq!(tiers.out_until("zeus"), None);
    (tiers, session, resets_at)
}

#[test]
fn a_chief_out_alone_is_back_once_a_turn_of_a_window_on_its_account_gets_through() {
    let context = Context::new();
    let (tiers, session, _) = chief_out_alone(&context);
    context.advance(60_000);
    context
        .adapter
        .writes(&session, "Tests pass.", &now(&context));
    context.pass().unwrap();
    assert_eq!(
        tiers.out_until("chief"),
        None,
        "zeus's turn got through on the chief's account"
    );
}

#[test]
fn a_chief_refused_again_with_the_same_reset_is_not_brought_back_by_a_teammates_turn_again() {
    let context = Context::new();
    let (tiers, session, resets_at) = chief_out_alone(&context);
    context.advance(60_000);
    context
        .adapter
        .writes(&session, "Tests pass.", &now(&context));
    context.pass().unwrap();
    assert_eq!(tiers.out_until("chief"), None);
    // Its own limit, not the account's: refused again with the same reset.
    context.advance(60_000);
    context
        .adapter
        .refuse("chief", exhausted(Some(&now(&context)), Some(&resets_at)));
    context.pass().unwrap();
    assert_eq!(tiers.out_until("chief"), Some(resets_at.clone()));
    context.advance(60_000);
    context
        .adapter
        .writes(&session, "Lint passes.", &now(&context));
    context.pass().unwrap();
    assert_eq!(
        tiers.out_until("chief"),
        Some(resets_at),
        "the same reset again is no news a teammate's turn undoes"
    );
}

#[test]
fn a_chief_marked_out_in_the_same_pass_as_its_members_answer_is_back_with_the_members_next_turn() {
    let context = Context::new();
    context.adapter.one_account.set(true);
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    let resets_at = after(&context, 48 * 3_600_000);
    context
        .adapter
        .refuse(&session, exhausted(Some(&now(&context)), Some(&resets_at)));
    context.pass().unwrap();
    assert_eq!(tiers.out_until("zeus"), Some(resets_at.clone()));
    // In one pass: zeus's window gets a turn through, and the chief is refused.
    context.advance(60_000);
    goes_on(&context.adapter, &session, &now(&context));
    context
        .adapter
        .refuse("chief", exhausted(Some(&now(&context)), Some(&resets_at)));
    context.pass().unwrap();
    assert_eq!(tiers.out_until("zeus"), None);
    assert_eq!(tiers.out_until("chief"), Some(resets_at));
    // zeus works on: its next turn is the account's news for the chief.
    context.advance(60_000);
    context
        .adapter
        .writes(&session, "Tests pass.", &now(&context));
    context.pass().unwrap();
    assert_eq!(tiers.out_until("chief"), None);
}

#[test]
fn a_member_back_from_its_reset_brings_back_no_chief_out_until_a_later_one() {
    // A plan's weekly limit of one model: the chief out for a week, zeus for five hours.
    let context = Context::new();
    context.adapter.one_account.set(true);
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    let hours = after(&context, 5 * 3_600_000);
    let week = after(&context, 7 * 24 * 3_600_000);
    context
        .adapter
        .refuse(&session, exhausted(Some(&now(&context)), Some(&hours)));
    context
        .adapter
        .refuse("chief", exhausted(Some(&now(&context)), Some(&week)));
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.out_until("zeus"), Some(hours));
    assert_eq!(tiers.out_until("chief"), Some(week.clone()));
    context.advance(60_000);
    goes_on(&context.adapter, &session, &now(&context));
    context.pass().unwrap();
    assert_eq!(tiers.out_until("zeus"), None);
    assert_eq!(tiers.out_until("chief"), Some(week.clone()));
    // The human hears the chief stays out, as a /login in zeus's window brought back zeus alone.
    let still_out = || -> Vec<String> {
        tiers
            .notes("human")
            .into_iter()
            .filter(|note| note.starts_with("@chief is still out of quota"))
            .collect()
    };
    assert_eq!(
        still_out(),
        [format!(
            "@chief is still out of quota until {week}, though @zeus is back on the same account: its limit resets later than @zeus's. If its account has quota again (another account, a bigger plan), press Try again now on @chief."
        )]
    );
    // zeus works on: none of its turns undoes the chief's week, nor tells it again.
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    context.advance(60_000);
    context
        .adapter
        .writes(&session, "Tests pass.", &now(&context));
    context.pass().unwrap();
    assert_eq!(tiers.out_until("chief"), Some(week));
    assert_eq!(still_out().len(), 1);
}

#[test]
fn a_members_held_task_goes_on_once_the_human_says_the_chief_is_back() {
    let context = Context::new();
    let (tiers, session) = out_together(&context);
    context.back_from_quota(tiers.project.id, "chief").unwrap();
    assert_eq!(tiers.out_until("zeus"), None, "zeus is back with it");
    context.adapter.with(&session, |agent| {
        agent.quota = None;
        agent.failed = false;
    });
    context.pass().unwrap();
    context.pass().unwrap();
    let task = tiers.task(1).task;
    assert_eq!(
        (task.state.as_str(), task.held_until),
        ("working", None),
        "T-1's hold is over and it goes on"
    );
}

#[test]
fn a_chief_stays_out_when_a_teammates_turn_that_got_through_before_its_refusal_is_read_after_it() {
    let context = Context::new();
    context.adapter.one_account.set(true);
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    // zeus's window answered a minute before the chief was refused, but its record
    // is read only after.
    let through = now(&context);
    context.advance(60_000);
    let resets_at = after(&context, 48 * 3_600_000);
    context
        .adapter
        .refuse("chief", exhausted(Some(&now(&context)), Some(&resets_at)));
    context.pass().unwrap();
    assert_eq!(tiers.out_until("chief"), Some(resets_at.clone()));
    context.adapter.writes(&session, "Tests pass.", &through);
    context.pass().unwrap();
    assert_eq!(
        tiers.out_until("chief"),
        Some(resets_at),
        "that turn got through before the chief was refused: no news for it"
    );
}

#[test]
fn the_human_is_told_what_waits_for_a_member_out_of_quota_whose_closed_window_opens_with_it() {
    // A task given to zeus by name while it is out: its window, closed, opens with
    // the task's words as its first message, which wait there for the quota.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    context.pass().unwrap();
    context.adapter.answer("chief", "Ready.");
    context.pass().unwrap();
    let (zeus, until) = (tiers.id("zeus"), after(&context, 48 * 3_600_000));
    context
        .ledger
        .borrow_mut()
        .mark_out(zeus, &until, "out of quota")
        .unwrap();
    let words = context
        .create_task(
            tiers.project.id,
            NewTask {
                from: "chief".to_owned(),
                to: Some("zeus".to_owned()),
                body: "Write the parser".to_owned(),
                ..NewTask::default()
            },
        )
        .message
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.dispatcher.activity(zeus).state, ActivityState::Out);
    let told: Vec<String> = tiers
        .notes("human")
        .into_iter()
        .filter(|note| note.starts_with("@zeus is out of quota"))
        .collect();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].contains(&format!("m-{} (the words of T-1) waits for it", words.id)),
        "{told:?}"
    );
}

#[test]
fn a_chief_whose_reset_is_within_the_hour_of_its_members_is_back_with_it() {
    // One limit, its reset named an hour from each refusal: the chief refused a
    // little after zeus.
    let context = Context::new();
    context.adapter.one_account.set(true);
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    context.adapter.refuse(
        &session,
        exhausted(Some(&now(&context)), Some(&after(&context, 3_600_000))),
    );
    context.pass().unwrap();
    context.advance(10 * 60_000);
    let later = after(&context, 3_600_000);
    context
        .adapter
        .refuse("chief", exhausted(Some(&now(&context)), Some(&later)));
    context.pass().unwrap();
    assert_eq!(tiers.out_until("chief"), Some(later));
    context.advance(60_000);
    goes_on(&context.adapter, &session, &now(&context));
    context.pass().unwrap();
    assert_eq!(tiers.out_until("zeus"), None);
    assert_eq!(
        tiers.out_until("chief"),
        None,
        "the same limit, back with zeus"
    );
}

#[test]
fn a_chief_brought_back_by_its_members_return_and_refused_again_with_the_same_reset_stays_out() {
    let context = Context::new();
    let (tiers, session) = out_together(&context);
    let resets_at = tiers.out_until("chief").unwrap();
    context.advance(60_000);
    goes_on(&context.adapter, &session, &now(&context));
    context.pass().unwrap();
    assert_eq!(tiers.out_until("chief"), None, "back with zeus");
    // Its own limit after all: refused again with the same reset.
    context.advance(60_000);
    context
        .adapter
        .refuse("chief", exhausted(Some(&now(&context)), Some(&resets_at)));
    context.pass().unwrap();
    assert_eq!(tiers.out_until("chief"), Some(resets_at.clone()));
    context.advance(60_000);
    context
        .adapter
        .writes(&session, "Tests pass.", &now(&context));
    context.pass().unwrap();
    assert_eq!(
        tiers.out_until("chief"),
        Some(resets_at),
        "once for each limit: zeus's turn is no news for it again"
    );
}

#[test]
fn a_chief_refused_again_with_no_reset_named_within_the_hour_is_not_brought_back_by_a_teammates_turn_again(
) {
    // A refusal that names no reset is out an hour from each refusal: a fresh time each.
    let context = Context::new();
    context.adapter.one_account.set(true);
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    context
        .adapter
        .refuse("chief", exhausted(Some(&now(&context)), None));
    context.pass().unwrap();
    assert!(tiers.out_until("chief").is_some());
    context.advance(60_000);
    context
        .adapter
        .writes(&session, "Tests pass.", &now(&context));
    context.pass().unwrap();
    assert_eq!(tiers.out_until("chief"), None, "zeus's turn got through");
    context.advance(5 * 60_000);
    context
        .adapter
        .refuse("chief", exhausted(Some(&now(&context)), None));
    context.pass().unwrap();
    let again = tiers.out_until("chief");
    assert!(again.is_some());
    context.advance(60_000);
    context
        .adapter
        .writes(&session, "Lint passes.", &now(&context));
    context.pass().unwrap();
    assert_eq!(
        tiers.out_until("chief"),
        again,
        "the same limit within the hour: no news a teammate's turn undoes"
    );
}

/// The notification Claude Code begins a turn with when a job of its ends.
const ENDED: &str =
    "<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n</task-notification>";

/// The results of T-`number` the chief was given or is to be given, with their states.
fn results_of(tiers: &Tiers<'_>, number: i64) -> Vec<(String, String)> {
    tiers
        .task(number)
        .messages
        .into_iter()
        .filter(|message| message.kind == "result")
        .map(|message| (message.body, message.state))
        .collect()
}

#[test]
fn a_member_brought_back_by_the_chiefs_turn_goes_on_with_its_task_and_its_refusal_is_no_result() {
    // Turn-end meets the account's news: zeus's window still reads the refusal its turn
    // ended on when the chief's turn brings zeus back, and that answer is no result, nor
    // its task's failure, however T-1 waited for the reset.
    let context = Context::new();
    let (tiers, session) = spent_together(&context);
    context.advance(60_000);
    goes_on(&context.adapter, "chief", &now(&context));
    // The chief's turn on the new account ends with those words.
    context.adapter.with("chief", |agent| {
        if let Some(said) = agent.items.last_mut() {
            said.complete = true;
        }
    });
    context.pass().unwrap();
    assert_eq!(tiers.out_until("chief"), None);
    assert_eq!(tiers.out_until("zeus"), None, "zeus is back with it");
    assert!(
        context.adapter.agent(&session).failed,
        "its record still reads the refusal"
    );
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working", "T-1 went on");
    assert_eq!(
        results_of(&tiers, 1),
        Vec::new(),
        "the refusal is no result"
    );
    context.adapter.answer(&session, "Parser done.");
    for _ in 0..6 {
        context.pass().unwrap();
        context.adapter.answer("chief", "Noted.");
    }
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results_of(&tiers, 1),
        [("Parser done.".to_owned(), "delivered".to_owned())],
        "its final answer alone, typed into the chief"
    );
}

#[test]
fn what_a_held_task_says_to_a_chief_out_of_quota_and_its_result_wait_and_the_human_hears_it_once() {
    // Turn-end meets the queue of a chief out of quota on another account: the note that
    // T-1 goes on in the background and then its result wait for the chief, the human is
    // told once what waits, and both are typed in, the result its final answer, once back.
    let context = Context::new();
    let (tiers, session) = {
        let tiers = Tiers::new(&context, &["zeus"]);
        tiers.open();
        context.pass().unwrap();
        context.pass().unwrap();
        let session = tiers.assignee(1);
        (tiers, session)
    };
    let resets_at = after(&context, 48 * 3_600_000);
    context
        .adapter
        .refuse("chief", exhausted(Some(&now(&context)), Some(&resets_at)));
    context.pass().unwrap();
    assert_eq!(tiers.out_until("chief"), Some(resets_at));
    assert_eq!(tiers.out_until("zeus"), None, "zeus is on another account");

    context.advance(60_000);
    context
        .adapter
        .answer_leaving_a_job(&session, "The build runs; I will report when it ends");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working", "held for its job");
    let held = tiers.task(1).messages.into_iter().find(|message| {
        message.kind == "note" && message.body.starts_with("T-1 goes on in the background")
    });
    let held = held.expect("the chief is to be told T-1 goes on in the background");
    assert_eq!(
        held.state, "queued",
        "nothing goes into a window out of quota"
    );
    let told = || -> Vec<String> {
        tiers
            .notes("human")
            .into_iter()
            .filter(|note| note.starts_with("@chief is out of quota"))
            .collect()
    };
    let said = told();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].contains(&format!("m-{} (a note on T-1) waits", held.id)),
        "{said:?}"
    );

    context.adapter.wake(&session, ENDED);
    context.pass().unwrap();
    context
        .adapter
        .answer(&session, "Build green; committed as fe76a3c");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results_of(&tiers, 1),
        [(
            "Build green; committed as fe76a3c".to_owned(),
            "queued".to_owned()
        )],
        "its final answer, waiting for the chief"
    );
    assert_eq!(told().len(), 1, "told once for the mark");

    context.back_from_quota(tiers.project.id, "chief").unwrap();
    goes_on(&context.adapter, "chief", &now(&context));
    for _ in 0..6 {
        context.pass().unwrap();
        context.adapter.answer("chief", "Noted.");
    }
    assert_eq!(
        results_of(&tiers, 1),
        [(
            "Build green; committed as fe76a3c".to_owned(),
            "delivered".to_owned()
        )],
        "typed into the chief once it is back"
    );
}

#[test]
fn a_held_task_whose_wake_up_is_refused_for_quota_is_held_for_its_job_no_more() {
    // Turn-end's hold meets a refusal: the turn the job's end woke is refused, the task
    // waits for the quota, and the lane no longer says it goes on in the background.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    let zeus = tiers.id(&session);
    context
        .adapter
        .answer_leaving_a_job(&session, "The build runs; I will report when it ends");
    context.pass().unwrap();
    assert_eq!(
        context.dispatcher.held_for(zeus).map(|hold| hold.task),
        Some(1)
    );
    context.adapter.wake(&session, ENDED);
    context.pass().unwrap();
    let resets_at = after(&context, 48 * 3_600_000);
    context
        .adapter
        .refuse(&session, exhausted(Some(&now(&context)), Some(&resets_at)));
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.out_until("zeus"), Some(resets_at));
    assert_ne!(tiers.task(1).task.state, "working");
    assert_eq!(
        context.dispatcher.held_for(zeus),
        None,
        "held for quota, not its job"
    );
}

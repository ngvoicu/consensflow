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
const WAITS: &str = r"^@chief is out of quota until \S+, and m-(\d+) waits for it, as does whatever is sent to it until then\. If its account has quota again \(another account, a bigger plan\), press Try again now on @chief: what waits is typed in once it is back\.$";

/// zeus at work on T-1 on Claude Code, whose windows are on one account; then
/// the account runs out, refusing zeus's window and the chief's alike, with a
/// reset two days on. The chief is out, and the note that T-1 waits is queued
/// for it.
fn out_together(context: &Context) -> (Tiers<'_>, String) {
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
    assert_eq!(tiers.task(1).task.state, "paused");
    assert_eq!(tiers.out_until("zeus"), Some(resets_at.clone()));
    assert_eq!(tiers.out_until("chief"), Some(resets_at));
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

/// The state of the note that T-1 waits, sent to the chief.
fn hold_note(tiers: &Tiers<'_>) -> String {
    to_chief(tiers, |message| message.body.starts_with("T-1 waits with "))
        .expect("the note that T-1 waits")
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
    assert_eq!(hold_note(&tiers), "queued");
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
    assert_eq!(hold_note(&tiers), "queued");
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
    assert!(said[0].contains(&format!("m-{} waits", first.id)));

    // Back, and out again: a new mark is told anew.
    context.back_from_quota(tiers.project.id, "chief").unwrap();
    context.advance(60_000);
    mark_out();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(told().len(), 2, "{:?}", told());
}

//! A login a harness refused is a failed turn first: tried again twice as any failed turn is, on
//! the same count, and only where the last try is refused for login too is the member held for
//! its account (T-2367). A refusal for billing is held at once. Every case drives the stand-in
//! window through real passes and the ledger's own messages; nothing reads the refusal's words.
use super::fixtures::{now, Tiers};
use super::n35_hold::{fail, next_turn_opens, passes, resumes, retrying, until_resumed, working};
use cf_engine::testing::Context;
use cf_harness::records::Quota;
use cf_ledger::{HoldKind, NewMember};

const LOGIN: &str = "its login was refused: 401 authentication_error: token expired";
const BILLING: &str = "its billing was refused: 402 billing_error: credit balance is too low";

fn refuse(c: &Context, window: &str, because: &str) {
    // Each refusal is dated at its own moment, as a harness's records are.
    c.advance(1_000);
    c.adapter.refuse(
        window,
        Quota::Unavailable {
            at: Some(now(c)),
            because: because.to_owned(),
        },
    );
}

/// Takes the task's tries as far as the window's next turn: the time passes, the Resume goes
/// in, and the harness's turn opens on its words.
fn tried(c: &Context, window: &str, wait_ms: i64, resumed: usize) {
    c.advance(wait_ms);
    until_resumed(c, "zeus", resumed);
    next_turn_opens(c, window);
    // The new turn's own record: the earlier refusal is not in its end.
    c.adapter.with(window, |agent| agent.quota = None);
    passes(c, 2);
}

fn backed(c: &Context, t: &Tiers<'_>) {
    c.host.exits_showing(Some(0), None, None);
    c.ledger
        .borrow_mut()
        .add_backup(
            t.project.id,
            "zeus",
            &NewMember {
                harness: "claude-code".into(),
                model: Some("gaia".into()),
                effort: None,
                model_name: None,
                designer: false,
                roles: vec!["worker".into()],
                tier: "standard".into(),
            },
        )
        .expect("a backup");
}

fn held_for_account(t: &Tiers<'_>) -> bool {
    t.context.ledger.borrow().hold_kind(t.id("zeus")).unwrap() == Some(HoldKind::Account)
}

#[test]
fn a_login_refused_once_holds_nobody_and_the_task_goes_on_after_its_try() {
    let c = retrying();
    let (t, window) = working(&c);
    refuse(&c, &window, LOGIN);
    passes(&c, 3);
    let held = t.task(1).task;
    assert_eq!(held.state, "paused", "the task waits for its try");
    let retry = held.retry.expect("the card says it tries again");
    assert_eq!((retry.which, retry.of), (1, 2));
    assert_eq!(t.out_until("zeus"), None, "no member is held");
    assert!(!held_for_account(&t));
    tried(&c, &window, 31_000, 1);
    assert_eq!(t.task(1).task.state, "working", "it went on in its window");
    c.adapter.answer("zeus", "Parser done");
    passes(&c, 3);
    assert_eq!(t.task(1).task.state, "done");
    assert_eq!(t.out_until("zeus"), None, "nobody was ever held");
}

#[test]
fn a_login_refused_at_every_try_holds_the_member_for_its_account_and_the_task_reaches_its_backup() {
    let c = retrying();
    let t = Tiers::new(&c, &["zeus"]);
    backed(&c, &t);
    t.open();
    passes(&c, 3);
    let window = t.assignee(1);
    refuse(&c, &window, LOGIN);
    passes(&c, 3);
    assert_eq!(t.out_until("zeus"), None);
    tried(&c, &window, 31_000, 1);
    refuse(&c, &window, LOGIN);
    passes(&c, 3);
    assert_eq!(t.out_until("zeus"), None, "the second try is still to come");
    tried(&c, &window, 121_000, 2);
    refuse(&c, &window, LOGIN);
    passes(&c, 6);
    assert!(held_for_account(&t), "the last try was refused for login");
    assert!(t
        .out_until("zeus")
        .is_some_and(|until| until.starts_with("9999-")));
    let task = t.task(1).task;
    assert_ne!(task.state, "failed", "no task fails for a refused login");
    assert_eq!(resumes(&c, "zeus"), 2, "two tries, never a third");
    let owner = t
        .context
        .project(t.project.id)
        .participants
        .into_iter()
        .find(|p| Some(p.id) == t.participant(&t.assignee(1)).and_then(|s| s.member_id))
        .expect("its member")
        .handle;
    assert_ne!(owner, "zeus", "the task went to the backup of zeus");
}

#[test]
fn a_billing_refusal_holds_the_member_at_once_with_no_try() {
    let c = retrying();
    let (t, window) = working(&c);
    refuse(&c, &window, BILLING);
    passes(&c, 3);
    assert!(held_for_account(&t));
    assert!(
        t.task(1).task.retry.is_none(),
        "no retry mends an unpaid account"
    );
    assert_eq!(resumes(&c, "zeus"), 0);
}

#[test]
fn the_count_is_one_for_failed_turns_and_login_refusals_alike() {
    // A failed turn, then a login refusal, then a failure: three turns in a row, two tries.
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    tried(&c, &window, 31_000, 1);
    refuse(&c, &window, LOGIN);
    passes(&c, 3);
    let held = t.task(1).task;
    assert_eq!(held.state, "paused");
    assert_eq!(
        held.retry.expect("the second try").which,
        2,
        "not counted from one again"
    );
    tried(&c, &window, 121_000, 2);
    fail(&c, &window);
    passes(&c, 3);
    assert_eq!(
        t.task(1).task.state,
        "failed",
        "the third turn fails the task"
    );
    assert_eq!(t.out_until("zeus"), None, "a failure holds no member");
    assert_eq!(resumes(&c, "zeus"), 2);
}

#[test]
fn a_login_refusal_after_two_failed_turns_holds_the_member_and_fails_nothing() {
    let c = retrying();
    let t = Tiers::new(&c, &["zeus"]);
    backed(&c, &t);
    t.open();
    passes(&c, 3);
    let window = t.assignee(1);
    fail(&c, &window);
    passes(&c, 3);
    tried(&c, &window, 31_000, 1);
    fail(&c, &window);
    passes(&c, 3);
    tried(&c, &window, 121_000, 2);
    refuse(&c, &window, LOGIN);
    passes(&c, 6);
    assert!(
        held_for_account(&t),
        "the tries are used: held for its account"
    );
    assert_ne!(t.task(1).task.state, "failed");
}

#[test]
fn the_other_window_of_a_member_goes_on_while_one_is_tried_again() {
    let c = retrying();
    let t = Tiers::new(&c, &["zeus"]);
    t.open();
    c.give(t.project.id, "zeus", "Second");
    passes(&c, 8);
    let (first, second) = (t.assignee(1), t.assignee(2));
    refuse(&c, &first, LOGIN);
    passes(&c, 3);
    assert_eq!(t.task(1).task.state, "paused");
    assert_eq!(
        t.task(2).task.state,
        "working",
        "the other window is not stopped"
    );
    assert_eq!(t.out_until("zeus"), None);
    c.adapter.answer(&second, "Second done");
    passes(&c, 3);
    assert_eq!(t.task(2).task.state, "done");
}

#[test]
fn a_login_refused_in_the_first_turn_is_tried_again_with_the_brief() {
    let c = retrying();
    let t = Tiers::new(&c, &["zeus"]);
    t.open();
    passes(&c, 1);
    let window = t.assignee(1);
    assert_eq!(
        t.task(1).task.state,
        "queued",
        "the brief is still on its way: not yet taken in"
    );
    refuse(&c, &window, LOGIN);
    passes(&c, 3);
    assert_eq!(t.out_until("zeus"), None, "no member is held");
    assert_ne!(t.task(1).task.state, "failed");
    c.advance(31_000);
    passes(&c, 6);
    assert_eq!(t.out_until("zeus"), None);
    next_turn_opens(&c, &window);
    c.adapter.answer("zeus", "Parser done");
    passes(&c, 4);
    assert_eq!(
        t.task(1).task.state,
        "done",
        "the brief was taken in the second time"
    );
}

#[test]
fn every_harness_s_login_refusal_takes_the_same_road() {
    // The reasons the readers write for a refused login: Claude Code and Pi (the shared reader,
    // with a 401 and with a refresh refused), Codex (its typed `unauthorized`), and Muse (its own
    // fixed sentence for no login). Devin has no reader of an account's refusal: not recorded.
    for because in [
        "its login was refused: 401 {\"error\":{\"type\":\"authentication_error\"}}",
        "its login was refused: OAuth refresh failed for anthropic: invalid_grant",
        "its login was refused: Your access token could not be refreshed.",
        "Muse refused: its login is not valid; run `muse login`",
    ] {
        let c = retrying();
        let (t, window) = working(&c);
        refuse(&c, &window, because);
        passes(&c, 3);
        let held = t.task(1).task;
        assert_eq!(held.state, "paused", "{because}");
        assert_eq!(held.retry.map(|retry| retry.which), Some(1), "{because}");
        assert_eq!(t.out_until("zeus"), None, "{because}");
        tried(&c, &window, 31_000, 1);
        assert_eq!(t.task(1).task.state, "working", "{because}");
    }
}

#[test]
fn a_try_says_the_refusal_in_the_closed_form_s_words_and_none_of_the_harness_s() {
    let c = retrying();
    let (t, window) = working(&c);
    refuse(
        &c,
        &window,
        "its login was refused: 401 {\"error\":{\"type\":\"authentication_error\",\"message\":\"bad key sk-live-0123456789abcdef for alice@example.com\"}}",
    );
    passes(&c, 3);
    let held = t.task(1).task;
    let aside = held.aside.expect("the card says it tries again");
    assert!(
        aside.contains("its login was refused (401, authentication_error)"),
        "{aside}"
    );
    let notes = t.notes("chief");
    let told: Vec<&String> = notes
        .iter()
        .filter(|note| note.contains("tries it again"))
        .collect();
    assert_eq!(told.len(), 1, "{notes:?}");
    assert!(
        told[0].contains("T-1's turn failed: its login was refused (401, authentication_error). ConsensFlow tries it again in its window in 30 seconds"),
        "{}",
        told[0]
    );
    for words in std::iter::once(aside).chain(notes) {
        assert!(
            !words.contains("sk-live") && !words.contains("alice@example.com"),
            "no word of the harness's is kept: {words}"
        );
    }
}

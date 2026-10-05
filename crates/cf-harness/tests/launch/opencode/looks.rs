//! What the window shows and is doing, as `tests/adapter-opencode.test.mjs`
//! holds Node's: ready once its plugin shows the conversation, followed to
//! another, its record read beside what OpenCode says of itself (idle, busy,
//! waiting out a limit), and its own question dialog. The plugin answers
//! `GET /session`, scripted by the test; the window's record is a stand-in
//! where Node's test stood one in.

use std::rc::Rc;
use std::sync::Arc;

use cf_base::time::iso;
use cf_harness::contract::{Held, Observed, Readiness, Records, Window};
use cf_harness::records::{Quota, Reading, Record, Settlement};
use cf_harness::seams::Time;
use cf_harness::testing::{finished, Driver, ScriptedHost, Served};
use serde_json::{json, Value};

use super::stage::{empty_record, pane, shows, working_record, Home, Stage, Stand};

/// A stage whose windows read `stand`'s record, and a window resumed on `session`.
fn window_on(session: &str, stand: &Rc<Stand>) -> (Stage, Rc<dyn Window>) {
    let stage = Stage::with(Home::new(), Some(Rc::clone(stand) as Rc<dyn Records>));
    let window = Rc::clone(&stage.resumed(session).window);
    (stage, window)
}

/// A look at the window, the plugin answering `answer` (none: nobody does).
fn look(stage: &Stage, window: &Rc<dyn Window>, answer: Option<Served>) -> Observed {
    stage.fakes.loopback.serve("GET /session", answer);
    finished(window.observe()).unwrap()
}

#[test]
fn is_ready_for_a_message_once_its_plugin_shows_the_conversation_and_follows_the_window_to_another()
{
    let stand = Stand::saying(empty_record());
    let (stage, window) = window_on("ses_abc123", &stand);
    let host = ScriptedHost::default();
    let ready = |answer: Option<Served>| {
        stage.fakes.loopback.serve("GET /session", answer);
        finished(window.ready(&host, &pane())).unwrap()
    };
    // Its plugin does not answer yet: the window names no conversation.
    let loading = look(&stage, &window, None);
    assert!(!loading.settled && loading.unnamed);
    let reason = loading.waiting.unwrap().reason.unwrap();
    assert!(reason.contains("plugin does not answer yet"), "{reason}");
    assert_eq!(ready(None), Readiness::Held(Held::Because(reason)));
    // Its home screen or session list: no conversation shown, a message waits.
    let home = look(&stage, &window, Some(shows(None, Value::Null)));
    assert!(home.unnamed);
    let reason = home.waiting.unwrap().reason.unwrap();
    assert!(reason.contains("shows no conversation"), "{reason}");
    assert_eq!(
        ready(Some(shows(None, Value::Null))),
        Readiness::Held(Held::Because(reason))
    );
    assert_eq!(
        ready(Some(shows(Some("ses_abc123"), Value::Null))),
        Readiness::Ready
    );
    assert!(
        look(
            &stage,
            &window,
            Some(shows(Some("ses_abc123"), Value::Null))
        )
        .settled
    );

    // The human opens another conversation in the window (/new, or from the list).
    let moved = look(&stage, &window, Some(shows(Some("ses_other"), Value::Null)));
    assert_eq!(moved.switched.as_deref(), Some("ses_other"));
    assert!(!moved.settled);
    assert_eq!(
        ready(Some(shows(Some("ses_other"), Value::Null))),
        Readiness::Held(Held::ShowsAnother)
    );

    // The dispatcher follows the window: the new conversation's record is read.
    window.follow("ses_other");
    let followed = look(&stage, &window, Some(shows(Some("ses_other"), Value::Null)));
    assert_eq!(followed.switched, None);
    assert!(followed.settled);
    assert_eq!(stand.asked.borrow().last().unwrap().1, "ses_other");
}

#[test]
fn reads_a_conversation_with_no_messages_yet_as_idle_and_one_mid_turn_as_working() {
    let stand = Stand::saying(empty_record());
    let (stage, window) = window_on("ses_abc123", &stand);
    let busy = || Some(shows(Some("ses_abc123"), json!({ "type": "busy" })));
    assert!(look(&stage, &window, busy()).settled);
    stand.say(Reading::Known(working_record()));
    assert!(!look(&stage, &window, busy()).settled);
    let exhausted = Arc::new(Quota::Exhausted {
        at: None,
        resets_at: None,
    });
    stand.say(Reading::Known(Record {
        settlement: Settlement::Settled,
        in_flight: false,
        quota: Some(Arc::clone(&exhausted)),
        ..working_record()
    }));
    let observed = look(&stage, &window, busy());
    assert!(observed.settled);
    assert_eq!(observed.quota, Some(exhausted));
}

#[test]
fn reads_its_own_question_dialog_still_open_as_waiting() {
    let stand = Stand::saying(Reading::Known(Record {
        asking: true,
        ..working_record()
    }));
    let (stage, window) = window_on("ses_abc123", &stand);
    let busy = || Some(shows(Some("ses_abc123"), json!({ "type": "busy" })));
    assert_eq!(
        look(&stage, &window, busy())
            .waiting
            .unwrap()
            .reason
            .as_deref(),
        Some("its own question dialog is open")
    );
    stand.say(Reading::Known(working_record()));
    assert_eq!(look(&stage, &window, busy()).waiting, None);
}

#[test]
fn reads_opencode_waiting_out_a_usage_or_rate_limit_as_the_members_quota_spent() {
    let stand = Stand::saying(Reading::Known(working_record()));
    let (stage, window) = window_on("ses_abc123", &stand);
    let now = stage.fakes.time.wall_ms();
    let retry = |session: &str, message: &str, next: i64, action: Option<Value>| {
        let mut status = json!({ "type": "retry", "attempt": 1, "message": message, "next": next });
        if let Some(action) = action {
            status["action"] = action;
        }
        Some(shows(Some(session), status))
    };
    let quota = |observed: &Observed| observed.quota.as_deref().cloned();
    let resets = |at: i64| {
        Some(Quota::Exhausted {
            at: None,
            resets_at: Some(iso(at)),
        })
    };
    let busy = Some(shows(Some("ses_abc123"), json!({ "type": "busy" })));
    assert_eq!(
        quota(&look(&stage, &window, busy)),
        None,
        "busy is not a refusal"
    );
    // OpenCode 1.18.31 on a spent free tier: it waits until the reset it names.
    let midnight = now + 5 * 3_600_000;
    let free = Some(json!({ "reason": "free_tier_limit" }));
    let spent = look(
        &stage,
        &window,
        retry(
            "ses_abc123",
            "Free usage exceeded, subscribe to Go",
            midnight,
            free,
        ),
    );
    assert_eq!((spent.settled, quota(&spent)), (false, resets(midnight)));
    // A rate limit OpenCode retries in seconds is backoff: the window is at
    // work, and its task stays with it. Only a reset a minute or more away
    // is a spent quota.
    let rate = "Rate limit exceeded. Please try again later.";
    let backoff = look(
        &stage,
        &window,
        retry("ses_abc123", rate, now + 4_000, None),
    );
    assert_eq!(
        (quota(&backoff), backoff.settled),
        (None, false),
        "backoff, not a quota"
    );
    let twenty = now + 20 * 60_000;
    let waited = look(&stage, &window, retry("ses_abc123", rate, twenty, None));
    assert_eq!(
        quota(&waited),
        resets(twenty),
        "a retry twenty minutes away is a limit waited out"
    );
    let overloaded = look(
        &stage,
        &window,
        retry("ses_abc123", "Provider is overloaded", now + 4_000, None),
    );
    assert_eq!(quota(&overloaded), None, "an overload is no quota");
    let other = look(
        &stage,
        &window,
        retry("ses_other", "Rate limit exceeded", midnight, None),
    );
    assert_eq!(
        quota(&other),
        None,
        "another conversation's status says nothing about this one"
    );
}

#[test]
fn the_quota_the_window_waits_out_is_the_one_a_look_reports_not_the_one_its_record_holds() {
    // Node: `quota: retry ?? state.quota`.
    let recorded = Quota::Exhausted {
        at: None,
        resets_at: Some(iso(1_000)),
    };
    let stand = Stand::saying(Reading::Known(Record {
        quota: Some(Arc::new(recorded)),
        ..working_record()
    }));
    let (stage, window) = window_on("ses_abc123", &stand);
    let midnight = stage.fakes.time.wall_ms() + 5 * 3_600_000;
    let waiting = shows(
        Some("ses_abc123"),
        json!({
            "type": "retry",
            "message": "Free usage exceeded, subscribe to Go",
            "next": midnight,
            "action": { "reason": "free_tier_limit" },
        }),
    );
    let observed = look(&stage, &window, Some(waiting));
    assert_eq!(
        observed.quota.as_deref(),
        Some(&Quota::Exhausted {
            at: None,
            resets_at: Some(iso(midnight)),
        })
    );
}

#[test]
fn reads_a_turn_opencode_never_finished_as_over_once_opencode_says_the_window_is_idle() {
    // A window lost mid-answer and reopened on its conversation: the store
    // keeps the unfinished answer for good, and OpenCode does not retry it.
    let stand = Stand::saying(Reading::Known(working_record()));
    let (stage, window) = window_on("ses_abc123", &stand);
    let with = |status: Value| Some(shows(Some("ses_abc123"), status));
    assert!(
        !look(&stage, &window, with(json!({ "type": "busy" }))).settled,
        "busy: still working"
    );
    assert!(
        !look(&stage, &window, with(Value::Null)).settled,
        "no word from the window: the store decides"
    );
    assert!(
        look(&stage, &window, with(json!({ "type": "idle" }))).settled,
        "idle: the turn is over"
    );
}

#[test]
fn a_look_reads_the_window_before_its_record_and_a_failed_retry_reset_fails_the_look() {
    let stand = Stand::saying(empty_record());
    let (stage, window) = window_on("ses_abc123", &stand);
    stage.fakes.loopback.serve("GET /session", [Served::Held]);
    let mut driver = Driver::default();
    let looking = Rc::clone(&window);
    driver.begin(0, async move { looking.observe().await });
    assert!(driver.run().is_empty());
    assert!(
        stand.asked.borrow().is_empty(),
        "the record is not read before the window has answered"
    );
    assert!(stage.fakes.loopback.release(
        "GET /session",
        shows(
            Some("ses_abc123"),
            json!({ "type": "retry", "message": "limit", "next": 1e300 })
        ),
    ));
    let settled = driver.run();
    assert_eq!(settled.len(), 1);
    assert_eq!(settled[0].1.as_ref().unwrap_err(), "Invalid time value");
}

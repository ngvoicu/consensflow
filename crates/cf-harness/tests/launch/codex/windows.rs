//! A Codex window once it is open, as the cases of
//! `tests/adapter-codex.test.mjs` hold Node's: it learns its thread from the
//! broker, holds a message while the broker cannot take one, follows the
//! window to the thread a /new or /resume left it on, and passes Codex's word
//! on its quota through. Where Node's test stood in the records of a
//! conversation, a stand-in `Records` does here.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use cf_harness::contract::{Held, Readiness, Work};
use cf_harness::records::{Item, Options, Quota, Reading, Record, Role, Settlement};
use cf_harness::seams::Time;
use cf_harness::testing::{finished, AnsweringHost, Driver};
use cf_proto::agents::Harness;

use super::*;

/// A record that says what the test wants of it, and what it was asked.
struct Stand {
    asked: RefCell<Vec<(Harness, String, Options)>>,
    reading: Arc<Reading>,
}

impl Records for Stand {
    fn look<'a>(
        &'a self,
        harness: Harness,
        session: &'a str,
        options: &'a Options,
    ) -> Work<'a, Arc<Reading>> {
        self.asked
            .borrow_mut()
            .push((harness, session.to_owned(), options.clone()));
        Box::pin(async { Arc::clone(&self.reading) })
    }

    fn has_transcript<'a>(
        &'a self,
        _harness: Harness,
        _session: &'a str,
    ) -> Work<'a, Result<bool, String>> {
        Box::pin(async { Ok(false) })
    }
}

#[test]
fn learns_the_thread_from_its_broker() {
    let home = Home::new();
    let (adapter, fakes) = adapter(&home);
    let plan = prepare(&adapter, &Request::default()).unwrap();
    let window = plan.window;
    fakes.loopback.serve(
        "GET /session",
        [
            shows(None, false),
            shows(None, false),
            shows(Some(THREAD), true),
        ],
    );
    let mut driver = Driver::default();
    let started = Rc::clone(&window);
    driver.begin(0, async move { started.started().await });
    assert!(driver.run().is_empty(), "waits to ask again");
    assert_eq!(fakes.time.waits(0), [250]);
    assert!(fakes.time.fire_next(fakes.time.wall_ms() + 250));
    assert!(driver.run().is_empty(), "still unnamed");
    assert_eq!(fakes.time.waits(0), [250]);
    assert!(fakes.time.fire_next(fakes.time.wall_ms() + 250));
    assert_eq!(driver.run(), [(0, Ok(Some(THREAD.to_owned())))]);
    // The thread is now the window's own: it has nothing more to learn.
    assert_eq!(finished(window.started()), Ok(None));
}

#[test]
fn holds_a_message_while_its_broker_cannot_take_one_a_window_starting_resuming_or_reconnecting() {
    let home = Home::new();
    let (adapter, fakes) = adapter(&home);
    let window = prepare(&adapter, &Request::resuming(THREAD))
        .unwrap()
        .window;
    let host = AnsweringHost::new(|_| Ok(json!({ "ok": true })));
    fakes.loopback.serve(
        "GET /session",
        [shows(Some(THREAD), false), shows(Some(THREAD), true)],
    );
    let Readiness::Held(Held::Because(held)) = finished(window.ready(&host, &pane())).unwrap()
    else {
        panic!("not held");
    };
    assert!(held.contains("cannot take a message yet"), "{held}");
    assert_eq!(finished(window.ready(&host, &pane())), Ok(Readiness::Ready));
}

/// The reading of a conversation holding `items`, its turn `settlement`.
fn reading(items: Vec<Item>, settlement: Settlement, quota: Option<Arc<Quota>>) -> Arc<Reading> {
    Arc::new(Reading::Known(Record {
        items,
        in_flight: false,
        asking: false,
        failed: false,
        quota,
        settlement,
    }))
}

/// The reading of a conversation whose turn settled: one user message.
fn settled_record(thread: &str) -> Arc<Reading> {
    let item = Item {
        id: Arc::from(format!("{thread}-1").as_str()),
        role: Role::User,
        text: Arc::from("hello"),
        complete: true,
        at: None,
        commentary: false,
    };
    reading(vec![item], Settlement::Settled, None)
}

#[test]
fn follows_the_window_to_the_thread_a_new_or_resume_left_it_on_holding_while_it_shows_none() {
    let home = Home::new();
    let stand = Rc::new(Stand {
        asked: RefCell::new(Vec::new()),
        reading: settled_record("any"),
    });
    let (adapter, fakes) =
        adapter_with(&home, Some(Rc::clone(&stand) as Rc<dyn Records>), "", true);
    let window = prepare(&adapter, &Request::resuming(THREAD))
        .unwrap()
        .window;
    let host = AnsweringHost::new(|_| Ok(json!({ "ok": true })));
    let shown = |word: Served| fakes.loopback.serve("GET /session", [word]);
    shown(shows(Some(THREAD), true));
    assert!(finished(window.observe()).unwrap().settled);
    shown(shows(Some(THREAD), true));
    assert_eq!(finished(window.ready(&host, &pane())), Ok(Readiness::Ready));

    // /new: while Codex starts the new thread, the broker names none...
    shown(shows(None, false));
    let switching = finished(window.observe()).unwrap();
    assert!(switching.unnamed);
    let reason = switching.waiting.unwrap().reason.unwrap();
    assert!(reason.contains("cannot take a message yet"), "{reason}");
    shown(shows(None, false));
    assert_eq!(
        finished(window.ready(&host, &pane())),
        Ok(Readiness::Held(Held::Because(reason)))
    );
    // ...then names it.
    shown(shows(Some(NEXT), true));
    let observed = finished(window.observe()).unwrap();
    assert_eq!(observed.switched.as_deref(), Some(NEXT));
    assert!(!observed.settled);
    shown(shows(Some(NEXT), true));
    assert_eq!(
        finished(window.ready(&host, &pane())),
        Ok(Readiness::Held(Held::ShowsAnother))
    );

    // The dispatcher follows the window: the new thread's record is read.
    window.follow(NEXT);
    shown(shows(Some(NEXT), true));
    let followed = finished(window.observe()).unwrap();
    assert_eq!(followed.switched, None);
    assert!(followed.settled);
    assert_eq!(stand.asked.borrow().last().unwrap().1, NEXT);
    shown(shows(Some(NEXT), true));
    assert_eq!(finished(window.ready(&host, &pane())), Ok(Readiness::Ready));
}

#[test]
fn passes_the_harness_s_word_on_its_quota_through() {
    let home = Home::new();
    let quota = Arc::new(Quota::Exhausted {
        at: None,
        resets_at: Some("2026-09-26T08:29:53.000Z".to_owned()),
    });
    let stand = Rc::new(Stand {
        asked: RefCell::new(Vec::new()),
        reading: reading(Vec::new(), Settlement::Settled, Some(Arc::clone(&quota))),
    });
    let (adapter, fakes) =
        adapter_with(&home, Some(Rc::clone(&stand) as Rc<dyn Records>), "", true);
    let window = prepare(&adapter, &Request::resuming(THREAD))
        .unwrap()
        .window;
    fakes
        .loopback
        .serve("GET /session", [shows(Some(THREAD), true)]);
    let observed = finished(window.observe()).unwrap();
    assert_eq!((observed.settled, observed.quota), (true, Some(quota)));
    let asked = stand.asked.borrow();
    let (harness, session, options) = &asked[0];
    assert_eq!((*harness, session.as_str()), (Harness::Codex, THREAD));
    assert_eq!(*options, Options::default());
    // A window whose thread is not named yet has nothing to look at.
    let fresh = prepare(&adapter, &Request::default()).unwrap().window;
    let nothing = finished(fresh.observe()).unwrap();
    assert_eq!(
        (nothing.settled, nothing.quota, nothing.unnamed),
        (false, None, false)
    );
    assert_eq!(stand.asked.borrow().len(), 1, "no record was read");
}

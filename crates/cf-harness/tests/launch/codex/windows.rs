//! A Codex window once it is open, as the cases of Node's Codex adapter suite
//! held them: it learns its thread from the broker, holds a message while the
//! broker cannot take one, follows the window to the thread a /new or /resume
//! left it on, and passes Codex's word on its quota through. Where Node's test
//! stood in the records of a conversation, a stand-in `Records` does here.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use cf_harness::contract::{Held, Readiness, Work};
use cf_harness::records::{Item, Options, Quota, Reading, Record, Role, Settlement};
use cf_harness::seams::Time;
use cf_harness::testing::{finished, AnsweringHost, Driver, FIRST_MESSAGE_MS};
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

/// How many milliseconds a window opened on `request` waits for its broker to
/// name a thread before it says `said`, that none was named.
fn waits_for_the_thread(request: &Request, said: &str) -> i64 {
    let home = Home::new();
    let (adapter, fakes) = adapter(&home);
    let window = prepare(&adapter, request).unwrap().window;
    let mut driver = Driver::default();
    driver.begin(0, async move { window.started().await });
    let began = fakes.time.wall_ms();
    let mut settled = driver.run();
    while settled.is_empty() {
        assert!(
            fakes.time.fire_next(began + 3_600_000),
            "no answer in an hour"
        );
        settled = driver.run();
    }
    assert_eq!(settled, [(0, Err(said.to_owned()))]);
    fakes.time.wall_ms() - began
}

/// What `window` says it started on, its broker serving `served` to its looks: the
/// answer, and how many milliseconds it took.
fn started_with(
    fakes: &Fakes,
    window: Rc<dyn cf_harness::contract::Window>,
    served: Vec<Served>,
) -> (Result<Option<String>, String>, i64) {
    fakes.loopback.serve("GET /session", served);
    let mut driver = Driver::default();
    driver.begin(0, async move { window.started().await });
    let began = fakes.time.wall_ms();
    let mut settled = driver.run();
    while settled.is_empty() {
        assert!(
            fakes.time.fire_next(began + 3_600_000),
            "no answer in an hour"
        );
        settled = driver.run();
    }
    let (_, answer) = settled.remove(0);
    (answer, fakes.time.wall_ms() - began)
}

/// The broker is asked at the deadline too: a thread named after the last look
/// before it, and by it, is the window's (Sol on round 1 of the chief's restart: at
/// 60 s the window was given up with no look since 59.75 s, and a chief Codex had
/// resumed meanwhile was closed at work).
#[test]
fn a_thread_named_after_the_last_look_before_the_deadline_and_by_it_is_the_windows() {
    let home = Home::new();
    let (adapter, fakes) = adapter(&home);
    // Two seconds, looked at every quarter of one: eight looks before the deadline.
    let unnamed = || vec![shows(None, false); 8];
    let at_the_deadline = |thread| {
        let mut served = unnamed();
        served.push(shows(Some(thread), true));
        served
    };
    let resumed = Request {
        first_message_ms: 2_000,
        ..Request::resuming(THREAD)
    };
    let window = prepare(&adapter, &resumed).unwrap().window;
    assert_eq!(
        started_with(&fakes, window, at_the_deadline(THREAD)),
        (Ok(None), 2_000)
    );
    let fresh = Request {
        first_message_ms: 2_000,
        ..Request::chief()
    };
    let window = prepare(&adapter, &fresh).unwrap().window;
    assert_eq!(
        started_with(&fakes, window, at_the_deadline(NEXT)),
        (Ok(Some(NEXT.to_owned())), 2_000)
    );
    // Named at none of them, it was not.
    let window = prepare(&adapter, &resumed).unwrap().window;
    let (answer, waited) = started_with(&fakes, window, unnamed());
    assert!(answer.is_err(), "{answer:?}");
    assert_eq!(waited, 2_000);
}

/// A slow window is not given up at every try: the time a window has to come up
/// doubles with each of its participant's windows given up before it in a row, to at
/// most ten times the first (Astra on round 1 of the chief's restart: a resume that
/// takes 65 s was given up at 60 s every time, and what waited for the chief never
/// reached it).
#[test]
fn the_time_a_window_has_to_come_up_doubles_with_each_window_given_up_before_it_to_ten_times_the_first(
) {
    let waits = |first_message_ms, starts_given_up| {
        waits_for_the_thread(
            &Request {
                first_message_ms,
                starts_given_up,
                ..Request::resuming(THREAD)
            },
            &format!("the Codex broker never named the thread {THREAD} it was opened on"),
        )
    };
    let minutes: Vec<i64> = (0..6)
        .map(|given_up| waits(FIRST_MESSAGE_MS, given_up))
        .collect();
    assert_eq!(
        minutes,
        [60_000, 120_000, 240_000, 480_000, 600_000, 600_000]
    );
    // A launch with less time than the minute grows from what it has.
    let seconds: Vec<i64> = (0..6).map(|given_up| waits(2_000, given_up)).collect();
    assert_eq!(seconds, [2_000, 4_000, 8_000, 16_000, 20_000, 20_000]);
    assert_eq!(waits(2_000, u32::MAX), 20_000, "and never more");
}

/// A window whose start was given up is asked once more, right before the engine
/// closes it: one whose broker names a thread now has come up, and is kept.
#[test]
fn a_window_whose_start_was_given_up_has_come_up_if_its_broker_names_a_thread_when_asked_once_more()
{
    let home = Home::new();
    let (adapter, fakes) = adapter(&home);
    let gave_up = |request: &Request| {
        let window = prepare(&adapter, request).unwrap().window;
        let (answer, _) = started_with(&fakes, Rc::clone(&window), Vec::new());
        assert!(answer.is_err(), "{answer:?}");
        window
    };
    let resumed = Request {
        first_message_ms: 2_000,
        ..Request::resuming(THREAD)
    };
    let window = gave_up(&resumed);
    fakes.loopback.serve("GET /session", [shows(None, false)]);
    assert_eq!(finished(window.came_up()), None, "still none");
    fakes
        .loopback
        .serve("GET /session", [shows(Some(THREAD), true)]);
    assert_eq!(
        finished(window.came_up()),
        Some(None),
        "on the thread its launch named"
    );
    assert_eq!(
        finished(window.started()),
        Ok(None),
        "nothing more to learn"
    );

    let fresh = Request {
        first_message_ms: 2_000,
        ..Request::chief()
    };
    let window = gave_up(&fresh);
    fakes
        .loopback
        .serve("GET /session", [shows(Some(NEXT), true)]);
    assert_eq!(finished(window.came_up()), Some(Some(NEXT.to_owned())));
    fakes
        .loopback
        .serve("GET /session", [shows(Some(NEXT), true)]);
    let observed = finished(window.observe()).unwrap();
    assert_eq!(
        observed.switched, None,
        "the thread Codex started is the window's from then on"
    );
}

#[test]
fn a_thread_never_named_is_an_error_after_a_minute_or_the_time_the_launch_has_if_that_is_less() {
    let for_launch = |first_message_ms| {
        waits_for_the_thread(
            &Request {
                first_message_ms,
                ..Request::default()
            },
            "the Codex broker never named the thread it opened",
        )
    };
    // The launch's wait (`CONSENSFLOW_LAUNCH_TIMEOUT_MS`) cuts the minute short.
    assert_eq!(for_launch(2_000), 2_000);
    assert_eq!(for_launch(30_000), 30_000);
    // A minute is as long as the broker is given, however long the launch is:
    // Codex that has not started a thread in a minute is waiting on something
    // (a login, a prompt) that more time does not give.
    for launch in [60_000, FIRST_MESSAGE_MS, 3_600_000] {
        assert_eq!(for_launch(launch), 60_000, "{launch}");
    }
}

/// A window opened on a thread its launch named is up once its broker names a
/// thread, and its launch fails if the broker never does, as a new window's does:
/// a Codex chief opened again after the app's restart, whose broker never named its
/// thread, read "starting" for good (zeewin, 2026-10-19).
#[test]
fn a_window_opened_on_a_thread_is_up_once_its_broker_names_it_and_its_launch_fails_if_it_never_does(
) {
    let home = Home::new();
    let (adapter, fakes) = adapter(&home);
    let window = prepare(&adapter, &Request::resuming(THREAD))
        .unwrap()
        .window;
    fakes.loopback.serve(
        "GET /session",
        [shows(None, false), shows(Some(THREAD), false)],
    );
    let mut driver = Driver::default();
    let started = Rc::clone(&window);
    driver.begin(0, async move { started.started().await });
    assert!(driver.run().is_empty(), "the broker names none yet");
    assert_eq!(fakes.time.waits(0), [250]);
    assert!(fakes.time.fire_next(fakes.time.wall_ms() + 250));
    assert_eq!(
        driver.run(),
        [(0, Ok(None))],
        "named: the thread is the launch's, nothing new"
    );
    assert_eq!(
        finished(window.started()),
        Ok(None),
        "nothing more to learn"
    );

    // A thread other than its own is the window's too: the next look follows it.
    let other = prepare(&adapter, &Request::resuming(THREAD))
        .unwrap()
        .window;
    fakes
        .loopback
        .serve("GET /session", [shows(Some(NEXT), true)]);
    assert_eq!(finished(other.started()), Ok(None));

    let resuming = |first_message_ms| {
        waits_for_the_thread(
            &Request {
                first_message_ms,
                ..Request::resuming(THREAD)
            },
            &format!("the Codex broker never named the thread {THREAD} it was opened on"),
        )
    };
    assert_eq!(resuming(2_000), 2_000);
    assert_eq!(resuming(FIRST_MESSAGE_MS), 60_000);
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
        pending: Default::default(),
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
        compaction: false,
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
    // The launch looked at the thread, for a turn left open; then the window did.
    assert_eq!(asked.len(), 2);
    for (harness, session, options) in asked.iter() {
        assert_eq!((*harness, session.as_str()), (Harness::Codex, THREAD));
        assert_eq!(*options, Options::default());
    }
    drop(asked);
    // A window whose thread is not named yet has nothing to look at.
    let fresh = prepare(&adapter, &Request::default()).unwrap().window;
    let nothing = finished(fresh.observe()).unwrap();
    assert_eq!(
        (nothing.settled, nothing.quota, nothing.unnamed),
        (false, None, false)
    );
    assert_eq!(stand.asked.borrow().len(), 2, "no record was read");
}

//! A window that stalls, kept: the Mac's chief that read working for good after the
//! owner's `/compact`, zeewin's that read starting for good once the app started again.

use serde_json::json;

use super::*;
use crate::long_run::ledger::Event;
use crate::long_run::look::{Activity, Lane, OpenWindow, Windows};

/// 10:mm:ss of 2026-10-19 in milliseconds.
fn at(minute: u32, second: u32) -> i64 {
    crate::clock::millis(&format!("2026-10-19T10:{minute:02}:{second:02}.000Z")).unwrap()
}

/// Twenty minutes, the run's default.
const IDLE: i64 = 20 * 60_000;

fn lane(handle: &str, state: &str) -> Lane {
    Lane {
        handle: handle.to_owned(),
        state: state.to_owned(),
    }
}

fn window(handle: &str, tail: &[&str]) -> OpenWindow {
    OpenWindow {
        handle: handle.to_owned(),
        pane: format!("p1-{handle}"),
        generation: 1,
        pid: Some(4242),
        running: true,
        hosted: true,
        background: Vec::new(),
        background_total: 0,
        tree_unread: None,
        tail: tail.iter().map(|row| (*row).to_owned()).collect(),
    }
}

fn change(minute: u32, handle: &str, state: &str, reason: Option<&str>) -> Activity {
    Activity {
        at: at(minute, 0),
        handle: handle.to_owned(),
        state: state.to_owned(),
        reason: reason.map(str::to_owned),
    }
}

/// A ledger whose log holds `count` events, the last at 10:`minute`.
fn ledger(count: i64, minute: u32) -> Ledger {
    Ledger {
        events: (1..=count)
            .map(|id| Event {
                id,
                at: format!("2026-10-19T10:{minute:02}:00.000Z"),
                kind: "delivery.begun".to_owned(),
                data: json!({ "message": id, "attempt": 1 }),
            })
            .collect(),
        ..Ledger::default()
    }
}

/// A look at 10:`minute`: the board's `lanes`, the open `windows`, the trace's `changes`.
fn look(minute: u32, lanes: Vec<Lane>, open: Vec<OpenWindow>, changes: Vec<Activity>) -> Look {
    Look {
        at: at(minute, 0),
        ledger: ledger(40, minute),
        windows: Windows {
            open,
            lanes,
            activity: changes,
            ..Windows::default()
        },
        ..Look::default()
    }
}

#[test]
fn a_lane_at_work_past_the_limit_is_kept_with_its_last_screen_until_it_reads_otherwise() {
    // The Mac's shape: the chief at work since 10:05 by the trace, after a /compact.
    let traced = vec![
        change(1, "chief", "idle", None),
        change(5, "chief", "working", Some("prompt")),
    ];
    let at_work = |minute: u32, row: &str| {
        look(
            minute,
            vec![lane("chief", "working"), lane("long-worker", "idle")],
            vec![window("chief", &["✻ Compacting conversation…", row])],
            traced.clone(),
        )
    };
    let mut stalls = Stalls::default();
    assert!(stalls.look(&at_work(10, "a"), IDLE).is_empty());
    assert!(
        stalls.look(&at_work(25, "b"), IDLE).is_empty(),
        "20 min, not longer"
    );
    let told = stalls.look(&at_work(26, "c"), IDLE);
    assert_eq!(
        told,
        [(
            at(26, 0),
            "@chief's window has read working for 21 min, since 2026-10-19T10:05:00.000Z: its screen and the ledger's last events are kept".to_owned()
        )]
    );
    assert_eq!(stalls.kept.len(), 1);
    let stall = &stalls.kept[0];
    assert_eq!((stall.since, stall.found), (at(5, 0), at(26, 0)));
    assert_eq!(stall.screen, ["✻ Compacting conversation…", "c"]);
    assert_eq!(
        stall.changes,
        [
            "2026-10-19T10:01:00.000Z idle",
            "2026-10-19T10:05:00.000Z working (prompt)"
        ]
    );
    assert_eq!(stall.events.len(), EVENTS);
    assert!(
        stall.events.last().unwrap().starts_with("e-40 "),
        "{:?}",
        stall.events
    );
    // Each look while it holds keeps its last screen, and tells nothing again.
    assert!(stalls.look(&at_work(40, "d"), IDLE).is_empty());
    assert_eq!(stalls.kept[0].screen, ["✻ Compacting conversation…", "d"]);
    assert_eq!(stalls.kept[0].last, at(40, 0));
    // At rest, it is cleared: told, and kept with when.
    let rested = look(
        41,
        vec![lane("chief", "idle")],
        vec![window("chief", &["> "])],
        traced.clone(),
    );
    assert_eq!(
        stalls.look(&rested, IDLE),
        [(
            at(41, 0),
            "@chief's window reads idle after 36 min working".to_owned()
        )]
    );
    assert_eq!(stalls.kept.len(), 1);
    assert_eq!(stalls.kept[0].cleared, Some((at(41, 0), "idle".to_owned())));
    assert_eq!(stalls.kept[0].screen, ["✻ Compacting conversation…", "d"]);
    // Evidence, which fails nothing: the report keeps it.
    let timeline = Timeline::new(at(0, 0));
    let value = stalls.to_value(&timeline);
    assert_eq!(value["windows"][0]["handle"], "chief");
    assert_eq!(value["windows"][0]["minutes"], 35);
    assert_eq!(value["windows"][0]["foundAt"], "+26m00s");
    assert_eq!(value["windows"][0]["cleared"]["to"], "idle");
    assert_eq!(value["windows"][0]["screen"][1], "d");
    let text = stalls.markdown(&timeline);
    assert!(
        text.starts_with("\n## Stalls\n\n### @chief · working since 2026-10-19T10:05:00.000Z (+26m00s, 35 min)\n"),
        "{text}"
    );
    assert!(text.contains("\nIt read idle at +41m00s.\n"), "{text}");
}

#[test]
fn a_lane_back_at_work_between_two_looks_is_timed_by_the_trace_and_one_stuck_as_it_opened_by_the_looks(
) {
    // A worker the looks saw at work at 10:00 and at 10:30, at rest a moment at 10:15
    // between them, as the trace says: at work for 15 min, no stall.
    let between = vec![
        change(0, "long-worker", "working", None),
        change(15, "long-worker", "idle", None),
        change(15, "long-worker", "working", None),
    ];
    let worker = |minute: u32| {
        look(
            minute,
            vec![lane("long-worker", "working")],
            Vec::new(),
            between.clone(),
        )
    };
    let mut stalls = Stalls::default();
    assert!(stalls.look(&worker(0), IDLE).is_empty());
    assert!(stalls.look(&worker(30), IDLE).is_empty());
    assert!(stalls.kept.is_empty());
    // zeewin's shape: the chief's lane reads starting from the first look, and the trace
    // never says it changed. Twenty-one minutes on by the looks, it stalled, with no
    // window the board shows to keep a screen of.
    let starting = |minute: u32| {
        look(
            minute,
            vec![lane("chief", "starting")],
            Vec::new(),
            Vec::new(),
        )
    };
    assert!(stalls.look(&starting(31), IDLE).is_empty());
    assert!(stalls.look(&starting(51), IDLE).is_empty());
    let told = stalls.look(&starting(52), IDLE);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].1.starts_with(
        "@chief's window has read starting for 21 min, since 2026-10-19T10:31:00.000Z"
    ));
    assert!(stalls.kept[0].screen.is_empty());
    // A lane the board shows no more clears it.
    let gone = look(53, Vec::new(), Vec::new(), Vec::new());
    assert_eq!(
        stalls.look(&gone, IDLE)[0].1,
        "@chief's window reads gone after 22 min starting"
    );
    // A lane at rest, or waiting on a dialog, for long is no stall.
    let mut stalls = Stalls::default();
    for minute in [0, 59] {
        let resting = look(
            minute,
            vec![lane("chief", "idle"), lane("long-worker", "waiting")],
            Vec::new(),
            Vec::new(),
        );
        assert!(stalls.look(&resting, IDLE).is_empty());
    }
    assert!(stalls.kept.is_empty());
}

#[test]
fn a_screen_is_kept_masked() {
    let mut stalls = Stalls::default();
    let held = |minute: u32| {
        look(
            minute,
            vec![lane("chief", "working")],
            vec![window("chief", &["OPENAI_API_KEY=abcd1234efgh"])],
            Vec::new(),
        )
    };
    stalls.look(&held(0), IDLE);
    stalls.look(&held(21), IDLE);
    assert_eq!(stalls.kept[0].screen, ["OPENAI_API_KEY=[masked]"]);
}

#[test]
fn at_an_end_not_reached_every_window_of_the_last_look_is_kept_and_the_ledgers_last_events() {
    let last = look(
        59,
        vec![lane("chief", "working"), lane("long-worker", "idle")],
        vec![
            window("chief", &["✻ Thinking…"]),
            window("long-worker", &["> "]),
        ],
        vec![change(50, "chief", "working", None)],
    );
    let mut stalls = Stalls::default();
    stalls.before_close(Some(&last));
    let closed = ledger(45, 59);
    assert!(stalls.after_close(true, Some(&closed)));
    assert_eq!(
        stalls.at_the_end,
        [
            Window {
                handle: "chief".to_owned(),
                state: "working".to_owned(),
                screen: vec!["✻ Thinking…".to_owned()],
                changes: vec!["2026-10-19T10:50:00.000Z working".to_owned()],
            },
            Window {
                handle: "long-worker".to_owned(),
                state: "idle".to_owned(),
                screen: vec!["> ".to_owned()],
                changes: Vec::new(),
            },
        ]
    );
    assert_eq!(stalls.events.len(), EVENTS);
    assert!(stalls.events[0].starts_with("e-16 2026-10-19T10:59:00.000Z delivery.begun "));
    stalls.ledger = Some(Ok(PathBuf::from("/reports/long-ledger.db")));
    let timeline = Timeline::new(at(0, 0));
    let value = stalls.to_value(&timeline);
    assert_eq!(value["atTheEnd"][0]["screen"][0], "✻ Thinking…");
    assert_eq!(value["ledger"], "/reports/long-ledger.db");
    let text = stalls.markdown(&timeline);
    assert!(
        text.contains("\n### The windows at the end\n\n@chief · working\n"),
        "{text}"
    );
    assert!(
        text.contains("\nThe ledger: /reports/long-ledger.db\n"),
        "{text}"
    );
    // A project that reached its end, with no stall, keeps none of it.
    let mut ended = Stalls::default();
    ended.before_close(Some(&last));
    assert!(!ended.after_close(false, Some(&closed)));
    assert!(ended.at_the_end.is_empty() && ended.events.is_empty());
    assert_eq!(ended.to_value(&timeline), Value::Null);
    assert_eq!(ended.markdown(&timeline), "");
    // A stall alone keeps the ledger's events, and no window at the end.
    let mut stalled = Stalls::default();
    stalled.look(
        &look(0, vec![lane("chief", "working")], Vec::new(), Vec::new()),
        IDLE,
    );
    stalled.look(
        &look(21, vec![lane("chief", "working")], Vec::new(), Vec::new()),
        IDLE,
    );
    stalled.before_close(Some(&last));
    assert!(stalled.after_close(false, Some(&closed)));
    assert!(stalled.at_the_end.is_empty());
    assert_eq!(stalled.events.len(), EVENTS);
}

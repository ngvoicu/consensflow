//! A window that stalls, kept: the Mac's chief that read working for good after the
//! owner's `/compact`, zeewin's that read starting for good once the app started again.

use serde_json::json;

use super::*;
use crate::long_run::ledger::Event;
use crate::long_run::look::{Activity, Lane, OpenWindow, Process, Windows};
use held::{Asked, Prober, Taken, When};

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
            "@chief's window has read working for 21 min, since 2026-10-19T10:05:00.000Z: its screen, its processes and the ledger's last events are kept".to_owned()
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
    stalls.before_close(Some(&last), &mut Asked::default());
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
    ended.before_close(Some(&last), &mut Asked::default());
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
    stalled.before_close(Some(&last), &mut Asked::default());
    assert!(stalled.after_close(false, Some(&closed)));
    assert!(stalled.at_the_end.is_empty());
    assert_eq!(stalled.events.len(), EVENTS);
}

#[test]
fn a_stalled_windows_processes_are_taken_when_it_is_found_and_once_more_at_the_end_and_no_others() {
    // The chief stalls at work; the worker's window, at work for a minute, does not; the
    // second worker stalls and comes back to rest before the end.
    let traced = vec![
        change(5, "chief", "working", Some("prompt")),
        change(5, "long-worker", "working", Some("prompt")),
        change(30, "long-worker-2", "idle", None),
    ];
    let mut chief = window("chief", &["Working…"]);
    chief.background = vec![Process {
        pid: 4343,
        command: "npm test".to_owned(),
    }];
    let mut worker = window("long-worker", &["Working…"]);
    worker.pid = Some(5151);
    let mut second = window("long-worker-2", &["Working…"]);
    second.pid = Some(6161);
    let at_work = |minute: u32, worker_state: &str, second_state: &str| {
        look(
            minute,
            vec![
                lane("chief", "working"),
                lane("long-worker", worker_state),
                lane("long-worker-2", second_state),
            ],
            vec![chief.clone(), worker.clone(), second.clone()],
            traced.clone(),
        )
    };
    let mut stalls = Stalls::default();
    let mut prober = Asked::default();
    for (minute, worker_state, second_state) in [
        (6, "working", "starting"),
        (26, "idle", "starting"),
        (30, "idle", "starting"),
        (31, "idle", "idle"),
    ] {
        let look = at_work(minute, worker_state, second_state);
        stalls.look(&look, IDLE);
        stalls.take(&look, &mut prober);
    }
    // Found at 10:26 (the chief) and 10:30 (the second worker, starting since 10:06 by the
    // looks), each taken once then; the worker, at work for less than the limit, never.
    assert_eq!(
        prober.asked,
        [
            (4242, vec![4343], "chief-found".to_owned()),
            (6161, vec![], "long-worker-2-found".to_owned()),
        ]
    );
    // At the end the chief still reads at work: taken once more. The second worker came
    // back to rest: not again.
    let last = at_work(40, "idle", "idle");
    stalls.look(&last, IDLE);
    stalls.take(&last, &mut prober);
    stalls.before_close(Some(&last), &mut prober);
    assert_eq!(prober.asked.len(), 3, "{:?}", prober.asked);
    assert_eq!(prober.asked[2], (4242, vec![4343], "chief-end".to_owned()));
    let chief = &stalls.kept[0];
    assert_eq!(chief.handle, "chief");
    let when: Vec<(When, i64)> = chief.held.iter().map(|held| (held.when, held.at)).collect();
    assert_eq!(when, [(When::Found, at(26, 0)), (When::End, at(40, 0))]);
    assert_eq!(
        chief.held[0].processes,
        Ok(vec![
            "4242 1 S - ttys001 pi".to_owned(),
            "4343 4242 S - ttys001 bash".to_owned()
        ])
    );
    assert_eq!(stalls.kept[1].held.len(), 1);
    // The report keeps it: each process's line, the unread bytes, and the sample where one
    // was taken.
    let timeline = Timeline::new(at(0, 0));
    let value = stalls.to_value(&timeline);
    let held = &value["windows"][0]["held"];
    assert_eq!(held[0]["when"], "found");
    assert_eq!(held[1]["when"], "end");
    assert_eq!(held[0]["processes"][0], "4242 1 S - ttys001 pi");
    assert_eq!(held[0]["unread"]["bytes"], 0);
    assert_eq!(held[0]["sample"], Value::Null);
    let text = stalls.markdown(&timeline);
    assert!(text.contains("\nIts processes (found, +26m00s):\n\n```\n4242 1 S - ttys001 pi\n4343 4242 S - ttys001 bash\n```\n"), "{text}");
    assert!(
        text.contains("\nUnread by the pane host: 0 bytes.\n"),
        "{text}"
    );
}

/// A prober that answers what a system with a secret in a command line, no unread count
/// and a failed sample would.
struct Telling;

impl Prober for Telling {
    fn take(&mut self, pid: u32, _: &[u32], _: &str) -> Taken {
        Taken {
            processes: Ok(vec![format!(
                "{pid} 1 S - ttys001 pi --api-key sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789"
            )]),
            unread: Err(held::WINDOWS_UNREAD.to_owned()),
            sample: Some(Err("the sample failed (Some(1)): no access".to_owned())),
        }
    }
}

#[test]
fn what_the_system_says_of_a_stalled_window_is_kept_masked_and_what_it_could_not_say_with_why() {
    let traced = vec![change(5, "chief", "working", None)];
    let stalled =
        |open: Vec<OpenWindow>| look(26, vec![lane("chief", "working")], open, traced.clone());
    let mut stalls = Stalls::default();
    let look = stalled(vec![window("chief", &["Working…"])]);
    stalls.look(&look, IDLE);
    stalls.take(&look, &mut Telling);
    let held = &stalls.kept[0].held[0];
    let line = &held.processes.as_ref().unwrap()[0];
    assert!(
        line.starts_with("4242 1 S - ttys001 pi --api-key "),
        "{line}"
    );
    assert!(!line.contains("abcdefghijklmnopqrstuvwxyz"), "{line}");
    assert_eq!(held.unread, Err(held::WINDOWS_UNREAD.to_owned()));
    let value = stalls.to_value(&Timeline::new(at(0, 0)));
    assert_eq!(
        value["windows"][0]["held"][0]["unread"]["unknown"],
        held::WINDOWS_UNREAD
    );
    assert_eq!(
        value["windows"][0]["held"][0]["sample"]["notTaken"],
        "the sample failed (Some(1)): no access"
    );
    // A stalled lane whose window the board shows not, or whose program the pane host
    // named not: nothing asked, and why kept.
    let mut prober = Asked::default();
    let mut stalls = Stalls::default();
    let look = stalled(Vec::new());
    stalls.look(&look, IDLE);
    stalls.take(&look, &mut prober);
    let mut unnamed = window("chief", &["Working…"]);
    unnamed.pid = None;
    let mut named_none = Stalls::default();
    let look = stalled(vec![unnamed]);
    named_none.look(&look, IDLE);
    named_none.take(&look, &mut prober);
    assert!(prober.asked.is_empty(), "{:?}", prober.asked);
    assert_eq!(
        stalls.kept[0].held[0].processes,
        Err("the board showed it no window".to_owned())
    );
    assert_eq!(
        named_none.kept[0].held[0].processes,
        Err("the pane host named no process of it".to_owned())
    );
}

//! The run's last look, taken once the app has closed: what the close left of the
//! run's processes, and what the app wrote as it closed, are held to the invariants.

use cf_e2e::files;

use super::*;
use crate::long_run::disruptions::{Disruption, Target, NO_IDLE_COMPACTION};
use crate::long_run::invariants::Evidence;
use crate::long_run::ledger::{Conversation, Event, Item, Message, Participant, Task};
use crate::long_run::look::{Process, Reading, Stranger, Unread};
use crate::long_run::record::Record;
use crate::long_run::stalls::held::{Asked, When};

/// The choice of `disruption` at a `how` moment, lacking nothing.
fn choice(disruption: Disruption, how: &'static str) -> Choice {
    Choice {
        disruption,
        target: Target::Project,
        how,
        lacks: None,
    }
}

/// 10:mm:ss of 2026-10-15 in milliseconds.
fn at(minute: u32, second: u32) -> i64 {
    crate::clock::millis(&format!("2026-10-15T10:{minute:02}:{second:02}.000Z")).unwrap()
}

fn iso(minute: u32, second: u32) -> String {
    format!("2026-10-15T10:{minute:02}:{second:02}.000Z")
}

/// A ledger with the human, the chief and a worker, and T-14 of the worker in `state`.
fn ledger(state: &str) -> Ledger {
    let someone = |id: i64, handle: &str, role: &str| Participant {
        id,
        handle: handle.to_owned(),
        role: role.to_owned(),
        harness: (role != "human").then(|| "claude-code".to_owned()),
        agent: None,
    };
    Ledger {
        participants: vec![
            someone(1, "human", "human"),
            someone(2, "chief", "chief"),
            someone(3, "long-worker", "worker"),
        ],
        tasks: vec![Task {
            id: 40,
            number: 14,
            title: "Run the mutations".to_owned(),
            state: state.to_owned(),
            assignee: Some(3),
            requester: 2,
            updated_at: iso(2, 0),
            deleted: false,
        }],
        ..Ledger::default()
    }
}

fn message(
    id: i64,
    sender: Option<i64>,
    kind: &str,
    body: &str,
    minute: u32,
    second: u32,
) -> Message {
    Message {
        id,
        recipient: 2,
        sender,
        kind: kind.to_owned(),
        task: Some(40),
        body: body.to_owned(),
        state: "delivered".to_owned(),
        created_at: iso(minute, second),
    }
}

/// [`ledger`] where the worker (3, on Claude Code) was given T-14 (m-120, its delivery
/// begun at 10:00:30) in its conversation 7, answered "Done." at 10:01:50, and its
/// result m-131 was taken at 10:02: the worker's own record of it is `answered_record`.
fn answered() -> Ledger {
    let mut ledger = ledger("done");
    ledger.conversations.push(Conversation {
        id: 7,
        participant: 3,
        harness: "claude-code".to_owned(),
        session: Some("s-worker".to_owned()),
        started_at: None,
        ended_at: None,
    });
    ledger.messages.push(Message {
        recipient: 3,
        ..message(120, Some(2), "task", "Run them", 0, 30)
    });
    ledger.events.push(Event {
        id: 1,
        at: iso(0, 30),
        kind: "delivery.begun".to_owned(),
        data: serde_json::json!({ "message": 120, "attempt": 1 }),
    });
    let item = |seq: i64, role: &str, text: &str, at: String| Item {
        conversation: 7,
        id: format!("i-7-{seq}"),
        seq,
        role: role.to_owned(),
        text: text.to_owned(),
        complete: true,
        at: Some(at.clone()),
        copied_at: at,
    };
    ledger.items.push(item(
        0,
        "user",
        "[ConsensFlow m-120 · T-14 · task from @chief]\nRun them",
        iso(0, 30),
    ));
    ledger.items.push(item(1, "assistant", "Done.", iso(1, 50)));
    ledger
        .messages
        .push(message(131, Some(3), "result", "Done.", 2, 0));
    ledger
}

/// The worker's own record of [`answered`]: its answer at 10:01:50.
fn answered_record() -> Record {
    Record::claude(
        &serde_json::json!({ "timestamp": iso(1, 50), "type": "assistant", "message": { "role": "assistant", "stop_reason": "end_turn", "content": [{ "type": "text", "text": "Done." }] } })
            .to_string(),
    )
}

/// A look at `at` of `ledger`, with the record of the conversation 7 (or 9) `record`,
/// read as far as `reading` says (none: read at the look).
fn read_look(
    at: i64,
    ledger: Ledger,
    (id, record): (i64, Record),
    reading: Option<Reading>,
) -> Look {
    Look {
        at,
        ledger,
        records: std::collections::BTreeMap::from([(id, record)]),
        readings: reading.map(|reading| (id, reading)).into_iter().collect(),
        ..Look::default()
    }
}

/// The keys of what the run could not judge, as its keeper holds them now.
fn open_keys(run: &Going) -> Vec<String> {
    run.keeper
        .unjudged
        .iter()
        .map(|open| open.key.clone())
        .collect()
}

#[test]
fn a_gap_a_later_reading_fills_is_judged_then_and_stays_in_the_timeline() {
    // Sol's: m-131 comes of age at 10:03, and the reading of the worker's record at that
    // look failed, the record read last at 10:02:55; the next look's reading holds, and
    // the result is the healthy one it was.
    let mut said = Vec::new();
    let mut run = going();
    let mut floor = run.edge.floor(&mut said);
    let failed = Reading {
        through: Some(at(2, 55)),
        failed: Some("its record could not be read: Input/output error (os error 5)".to_owned()),
    };
    run.hold(
        &mut floor,
        &Settings::default(),
        read_look(at(3, 0), answered(), (7, answered_record()), Some(failed)),
    );
    assert_eq!(open_keys(&run), ["2:m-131"]);
    run.hold(
        &mut floor,
        &Settings::default(),
        read_look(at(3, 3), answered(), (7, answered_record()), None),
    );
    assert!(open_keys(&run).is_empty(), "{:?}", open_keys(&run));
    assert!(run.keeper.breaks.is_empty(), "{:?}", run.keeper.breaks);
    // Its observation stays in the timeline, and so does what filled it.
    let moments: Vec<(&str, &str)> = run
        .timeline
        .moments
        .iter()
        .map(|moment| (moment.kind, moment.text.as_str()))
        .collect();
    assert!(
        moments
            .iter()
            .any(|(kind, text)| *kind == "unresolved" && text.starts_with("2: m-131's turn")),
        "{moments:?}"
    );
    assert!(
        moments
            .iter()
            .any(|(kind, text)| *kind == "resolved" && text.starts_with("2:m-131: ")),
        "{moments:?}"
    );
}

#[test]
fn a_cut_note_a_later_reading_reads_through_is_judged_then() {
    // Astra's: the chief was told at 10:10 its turn was cut, and it was (its prompt of
    // 10:09 had no answer); one look's reading of its record had read it only as far as
    // 10:09:30, the next one's read it through.
    let mut ledger = ledger("done");
    ledger.conversations.push(Conversation {
        id: 9,
        participant: 2,
        harness: "claude-code".to_owned(),
        session: Some("s-chief".to_owned()),
        started_at: None,
        ended_at: None,
    });
    ledger.messages.push(Message {
        task: None,
        ..message(
            2063,
            None,
            "note",
            "ConsensFlow stopped in the middle of your turn, which was cut. Go on where you stopped.",
            10,
            0,
        )
    });
    let in_turn = Record::claude(
        &serde_json::json!({ "timestamp": iso(9, 0), "type": "user", "message": { "role": "user", "content": "[ConsensFlow m-2050 · result from @long-worker]\nDone." } })
            .to_string(),
    );
    let mut said = Vec::new();
    let mut run = going();
    let mut floor = run.edge.floor(&mut said);
    let behind = Reading {
        through: Some(at(9, 30)),
        failed: Some("its record could not be read: Input/output error (os error 5)".to_owned()),
    };
    run.hold(
        &mut floor,
        &Settings::default(),
        read_look(
            at(11, 0),
            ledger.clone(),
            (9, in_turn.clone()),
            Some(behind),
        ),
    );
    assert_eq!(open_keys(&run), ["5:m-2063"]);
    run.hold(
        &mut floor,
        &Settings::default(),
        read_look(at(11, 3), ledger, (9, in_turn), None),
    );
    assert!(open_keys(&run).is_empty(), "{:?}", open_keys(&run));
    assert!(run.keeper.breaks.is_empty(), "{:?}", run.keeper.breaks);
}

#[test]
fn the_run_reads_once_more_before_it_closes_where_a_reading_began_before_a_result_needed_it() {
    // The reviewers' boundary: m-131 at 10:02, a job's end told up to 10:03; the last
    // look began its reading of the worker's record at 10:02:59.999 and ended at
    // 10:03:00.001. Healthy: one more reading reads it through.
    for (look_at, began) in [(at(3, 0) + 1, at(3, 0) - 1), (at(3, 0) - 1, at(3, 0) - 2)] {
        let mut said = Vec::new();
        let mut run = going();
        let mut floor = run.edge.floor(&mut said);
        let reading = Reading {
            through: Some(began),
            failed: None,
        };
        run.hold(
            &mut floor,
            &Settings::default(),
            read_look(look_at, answered(), (7, answered_record()), Some(reading)),
        );
        let mut looked = false;
        read_once_more(&mut run, &mut floor, &Settings::default(), || {
            looked = true;
            Ok(read_look(
                at(3, 2),
                answered(),
                (7, answered_record()),
                None,
            ))
        });
        assert!(looked, "one more look at {look_at}");
        assert!(open_keys(&run).is_empty(), "{:?}", open_keys(&run));
        // Read through all it needs: no look more.
        read_once_more(&mut run, &mut floor, &Settings::default(), || {
            panic!("no look more")
        });
    }
}

/// A run with every disruption waiting, due a minute apart.
fn going() -> Going {
    Going {
        keeper: Keeper::default(),
        timeline: Timeline::new(at(0, 0)),
        schedule: Schedule::new(
            &Disruption::ALL,
            Duration::from_secs(60),
            "claude",
            &["claude".to_owned(), "devin".to_owned()],
        ),
        last: None,
        ending: String::new(),
        deadline: at(59, 0),
        final_unread: None,
        stalls: Stalls::default(),
        ledger: None,
        prober: Box::new(Asked::default()),
        edge: Edge::default(),
    }
}

#[test]
fn a_disruption_whose_effect_was_not_seen_is_failed_and_a_break_of_the_tenth_with_its_evidence() {
    use crate::long_run::disruptions::State;
    let mut said = Vec::new();
    let mut run = going();
    let mut floor = run.edge.floor(&mut said);
    let evidence = Evidence {
        rows: vec!["@chief: \"long-claude-1\" \"working\", window {\"generation\":4}".to_owned()],
        ..Evidence::default()
    };
    run.came_of(
        &mut floor,
        &choice(Disruption::Pause, "strict"),
        Duration::from_secs(70),
        Outcome::Failed {
            said: "T-3 never moved to paused after the owner's pause".to_owned(),
            evidence: evidence.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        run.schedule.planned[0].state,
        State::Failed {
            at: Duration::from_secs(70),
            said: "strict: T-3 never moved to paused after the owner's pause".to_owned()
        }
    );
    assert_eq!(run.keeper.breaks.len(), 1);
    let found = &run.keeper.breaks[0];
    assert_eq!((found.invariant, found.key.as_str()), (10, "10:pause"));
    assert_eq!(
        found.said,
        "pause: T-3 never moved to paused after the owner's pause"
    );
    assert_eq!(found.evidence, evidence);
    // The next is due the interval after it.
    assert_eq!(run.schedule.planned[1].due, Duration::from_secs(130));

    // Played: no break. Missed: it waits, and its moment is looked for again.
    run.came_of(
        &mut floor,
        &choice(Disruption::Kill, "loose"),
        Duration::from_secs(140),
        Outcome::Missed("@x has no window to kill".to_owned()),
    )
    .unwrap();
    assert_eq!(run.schedule.planned[1].state, State::Waiting);
    run.came_of(
        &mut floor,
        &choice(Disruption::Kill, "strict"),
        Duration::from_secs(150),
        Outcome::Played(
            "@x's window killed (its screen said token=abcd1234efgh, and more)".to_owned(),
        ),
    )
    .unwrap();
    assert!(matches!(
        run.schedule.planned[1].state,
        State::Played { .. }
    ));
    assert_eq!(run.keeper.breaks.len(), 1);
    // A restart played at a moment that lacked A5's shape is kept so, and said.
    run.came_of(
        &mut floor,
        &Choice {
            lacks: Some(NO_IDLE_COMPACTION),
            ..choice(Disruption::Restart, "loose")
        },
        Duration::from_secs(400),
        Outcome::Played("the app quit and started again".to_owned()),
    )
    .unwrap();
    assert_eq!(
        run.schedule.planned[3].lacked.as_deref(),
        Some(NO_IDLE_COMPACTION)
    );
    // What a disruption said is kept in the log with a token in it hidden; the console
    // says none of it, which may be a window's words.
    let log = floor.log().to_string();
    drop(floor);
    let printed = String::from_utf8(said).unwrap();
    assert!(!printed.contains("its screen said"), "{printed}");
    assert!(
        log.contains("token=[masked], and more") && !log.contains("abcd1234efgh"),
        "{log}"
    );
}

#[test]
fn a_notice_the_log_ties_to_no_result_is_said_unresolved_once_and_is_no_break() {
    // The result m-100, T-14 sent back, and a close note after: the old attempt's window,
    // late, or the next attempt's own. Said in the timeline, at every look still
    // unresolved, and said once.
    let mut ledger = ledger("working");
    ledger
        .messages
        .push(message(100, Some(3), "result", "First round.", 0, 30));
    ledger.events.push(crate::long_run::ledger::Event {
        id: 1,
        at: iso(0, 40),
        kind: "task.state".to_owned(),
        data: serde_json::json!({ "task": 14, "from": "done", "to": "working" }),
    });
    ledger.messages.push(message(
        101,
        None,
        "note",
        "T-14's window closed with background work running, which its close ended: npx stryker run.",
        1,
        0,
    ));
    let mut said = Vec::new();
    let mut run = going();
    let mut floor = run.edge.floor(&mut said);
    for minute in [2, 3] {
        let look = Look {
            at: at(minute, 0),
            ledger: ledger.clone(),
            ..Look::default()
        };
        run.hold(&mut floor, &Settings::default(), look);
    }
    assert!(run.keeper.breaks.is_empty(), "{:?}", run.keeper.breaks);
    // (The result m-100 has no turn in this ledger's copy, which is unresolved too.)
    let unresolved: Vec<&str> = run
        .timeline
        .moments
        .iter()
        .filter(|moment| moment.kind == "unresolved" && moment.text.starts_with("3: "))
        .map(|moment| moment.text.as_str())
        .collect();
    assert_eq!(unresolved.len(), 1, "{unresolved:?}");
    assert!(
        unresolved[0].starts_with(
            "3: T-14's close note m-101 came while T-14 was working again, after it had ended and been sent back"
        ),
        "{}",
        unresolved[0]
    );
    drop(floor);
    let printed = String::from_utf8(said).unwrap();
    assert!(printed.contains("UNRESOLVED 3: "), "{printed}");
}

#[test]
fn a_chief_held_working_past_the_idle_limit_is_told_and_its_screen_kept_in_the_report() {
    // The Mac's run of 2026-10-19: the chief read working for good after the owner's
    // /compact. The run tells it once, kept in its log and timeline, and keeps the
    // chief's last screen; it fails nothing, and the console says none of it.
    let mut said = Vec::new();
    let mut run = going();
    let mut floor = run.edge.floor(&mut said);
    let held = |minute: u32, row: &str| Look {
        at: at(minute, 0),
        ledger: ledger("accepted"),
        windows: Windows {
            open: vec![crate::long_run::look::OpenWindow {
                handle: "chief".to_owned(),
                pane: "p1-chief".to_owned(),
                generation: 1,
                pid: Some(4242),
                running: true,
                hosted: true,
                background: Vec::new(),
                background_total: 0,
                tree_unread: None,
                tail: vec!["✻ Compacting conversation…".to_owned(), row.to_owned()],
            }],
            lanes: vec![crate::long_run::look::Lane {
                handle: "chief".to_owned(),
                state: "working".to_owned(),
            }],
            ..Windows::default()
        },
        ..Look::default()
    };
    for (minute, row) in [(1, "a"), (22, "b"), (30, "c")] {
        run.hold(&mut floor, &Settings::default(), held(minute, row));
    }
    let log = floor.log().to_string();
    drop(floor);
    let stalls: Vec<&str> = run
        .timeline
        .moments
        .iter()
        .filter(|moment| moment.kind == "stall")
        .map(|moment| moment.text.as_str())
        .collect();
    assert_eq!(
        stalls,
        ["@chief's window has read working for 21 min, since 2026-10-15T10:01:00.000Z: its screen, its processes and the ledger's last events are kept"]
    );
    assert!(
        log.contains("@chief's window has read working for 21 min"),
        "{log}"
    );
    let printed = String::from_utf8(said).unwrap();
    assert!(printed.is_empty(), "{printed}");
    assert!(run.keeper.breaks.is_empty());
    assert_eq!(run.stalls.kept[0].screen[1], "c");
    // What the system says of its window's processes, taken once, when it was found.
    let held = &run.stalls.kept[0].held;
    assert_eq!(held.len(), 1, "{held:?}");
    assert_eq!(held[0].when, When::Found);
    assert_eq!(held[0].at, at(22, 0));
    let options = Options::of(&[]).unwrap();
    let staff = crate::long_run::staff::staff_of(
        &["claude".to_owned()],
        &crate::long_run::staff::models_of(None).unwrap(),
        "medium",
    )
    .unwrap();
    let kept = Kept {
        options: &options,
        chief: ChiefKept {
            harness: "claude".to_owned(),
            model: "claude-opus-5-5".to_owned(),
            effort: None,
        },
        found: &[],
        left: &[],
        staff: &staff,
        daemon: "native (rust 3.0.5)".to_owned(),
        home: None,
        started: at(0, 0),
        ended: at(30, 0),
        schedule: &run.schedule,
        breaks: &run.keeper.breaks,
        unresolved: &run.keeper.unjudged,
        filled: &run.keeper.filled,
        timeline: &run.timeline,
        stalls: &run.stalls,
        ending: DONE.to_owned(),
        error: None,
        login: None,
        log: serde_json::Value::Null,
    };
    let value = kept.value();
    assert_eq!(value["stalls"]["windows"][0]["handle"], "chief");
    assert_eq!(value["stalls"]["windows"][0]["screen"][1], "c");
    assert!(kept.markdown().contains("\n## Stalls\n"));
}

#[test]
fn what_the_run_says_is_masked_whatever_says_it_its_own_names_shown() {
    // A wait for the chief that gave up says the chief's screen and how its window was
    // started (`Chief::gave_up`, on the run's floor): a password there is hidden before
    // it is said or kept, and the names a look showed are shown.
    let mut said = Vec::new();
    let mut run = going();
    let mut floor = run.edge.floor(&mut said);
    let mut seen = ledger("working");
    seen.project = Some(crate::long_run::ledger::Project {
        id: 1,
        name: "ledgerline".to_owned(),
        state: "open".to_owned(),
    });
    seen.participants[2].handle = "long-claude-1-olive-valley".to_owned();
    run.hold(
        &mut floor,
        &Settings::default(),
        Look {
            at: at(1, 0),
            ledger: seen,
            ..Look::default()
        },
    );
    floor
        .note("gave up waiting for the chief to be ready: chief null; its screen: export DB_PASSWORD=hunter2hunter ⏎ p1-long-claude-1-olive-valley")
        .unwrap();
    let log = floor.log().to_string();
    drop(floor);
    // The console says none of it; the log the report keeps has it, masked.
    let printed = String::from_utf8(said).unwrap();
    assert!(!printed.contains("its screen"), "{printed}");
    assert!(
        log.contains("DB_PASSWORD=[masked] ⏎ p1-long-claude-1-olive-valley"),
        "{log}"
    );
    assert!(!log.contains("hunter2hunter"));
}

#[test]
fn a_findings_screen_and_words_stay_in_the_local_report_and_the_console_says_its_key() {
    // A value whose name the screen's tail cut off, which no mask knows (the accepted
    // limit): on the screen of a window that crashed mid-task, in the words of its break,
    // and in what could not be judged.
    let secret = "correct horse battery staple";
    let mut said = Vec::new();
    let mut run = going();
    let mut floor = run.edge.floor(&mut said);
    let found = Break::new(
        6,
        "6:closed:p1-long-worker-1:3",
        format!(
            "@long-worker's window closed by itself while T-18 was at work, saying \"{secret}\""
        ),
    )
    .with(Evidence {
        rows: vec!["p1-long-worker-1 generation 3".to_owned()],
        screen: vec![format!("\"{secret}\""), "step 1 of 11 done".to_owned()],
        record: Vec::new(),
    });
    run.record(&mut floor, at(5, 0), vec![found]);
    run.unjudged(
        &mut floor,
        at(5, 0),
        vec![Unjudged::new(
            2,
            "2:m-131",
            format!("m-131's turn is not in @long-worker's own record ({secret})"),
        )],
    );
    // The report kept on the machine, and its summary said.
    let options = Options::of(&[]).unwrap();
    let found = vec![crate::long_run::staff::Found {
        name: "claude".to_owned(),
        command: PathBuf::from("/bin/claude"),
        version: "2.1.0 (Claude Code)".to_owned(),
    }];
    let staff = crate::long_run::staff::staff_of(
        &["claude".to_owned()],
        &crate::long_run::staff::models_of(None).unwrap(),
        "medium",
    )
    .unwrap();
    let log = floor.log();
    let kept = Kept {
        options: &options,
        chief: ChiefKept {
            harness: "claude".to_owned(),
            model: "claude-opus-5-5".to_owned(),
            effort: None,
        },
        found: &found,
        left: &[],
        staff: &staff,
        daemon: "native (rust 3.0.5)".to_owned(),
        home: None,
        started: at(0, 0),
        ended: at(30, 0),
        schedule: &run.schedule,
        breaks: &run.keeper.breaks,
        unresolved: &run.keeper.unjudged,
        filled: &run.keeper.filled,
        timeline: &run.timeline,
        stalls: &run.stalls,
        ending: DONE.to_owned(),
        error: Some(format!("the rig could not close: {secret}")),
        login: None,
        log,
    };
    let folder = tempfile::tempdir().unwrap();
    let status = keep_and_say(&kept, &run.edge, folder.path(), "long", &mut floor).unwrap();
    drop(floor);
    assert_eq!(status, 1);
    let console = String::from_utf8(said).unwrap();
    for part in ["correct", "horse", "battery", "staple", "step 1 of 11"] {
        assert!(!console.contains(part), "{part}: {console}");
    }
    for line in [
        "BREAK 6 (every window has its process, and no stranger): 6:closed:p1-long-worker-1:3",
        "UNRESOLVED 2: 2:m-131",
        "  why: unresolved (2): 2:m-131",
        "  why: the run could not be run to its end (what stopped it is in the report)",
    ] {
        assert!(
            console.lines().any(|said| said == line),
            "{line}\n{console}"
        );
    }
    assert!(console.contains(&format!(
        "  report: {}",
        folder.path().join("long.json").display()
    )));
    // The report, on the machine the run ran on, keeps all of it.
    for file in ["long.json", "long.md"] {
        let text = files::read_string(&folder.path().join(file)).unwrap();
        assert!(text.contains(secret), "{file}: {text}");
    }
}

#[test]
fn what_the_close_left_of_the_runs_processes_is_a_break_of_the_last_look() {
    let mut last = Look {
        at: at(5, 0),
        ledger: ledger("accepted"),
        ..Look::default()
    };
    last.records.insert(9, Record::default());
    let left = [Stranger {
        process: Process {
            pid: 7777,
            command: "node scripts/longrun-dev-server.mjs".to_owned(),
        },
        pane: "p1-long-worker-1".to_owned(),
        since: at(6, 0),
        began: "Fri Oct 16 00:29:15 2026".to_owned(),
    }];
    // And the close whose host's last word was not read to its end.
    let windows = Windows {
        strangers: left.to_vec(),
        unread: vec![Unread {
            at: at(6, 10),
            why: "the pane host's output had not ended 10 s after the host did".to_owned(),
        }],
        ..Windows::default()
    };
    let after = after_close(
        at(6, 20),
        Ok(ledger("accepted")),
        windows,
        records_of(Some(&last)),
        (&Settings::default(), "the project is done", at(30, 0)),
    );
    let look = after.look.as_ref().unwrap();
    assert!(look.windows.open.is_empty());
    assert_eq!(look.records.len(), 1, "the last look's records");
    assert_eq!(after.unread, None);
    let found = after.found;
    assert!(
        found.iter().any(|found| found.key == "6:stranger:7777"),
        "{found:?}"
    );
    let unread = found
        .iter()
        .find(|found| found.key == format!("6:unread:{}", at(6, 10)))
        .unwrap_or_else(|| panic!("{found:?}"));
    assert!(
        unread.said.contains("last word was not read to its end")
            && unread
                .said
                .ends_with("(the pane host's output had not ended 10 s after the host did)"),
        "{}",
        unread.said
    );
}

#[test]
fn a_ledger_the_app_left_that_cannot_be_read_fails_the_run_and_no_earlier_copy_is_held() {
    // The last look saw a clean ledger; the app's close left one that cannot be read.
    let last = Look {
        at: at(5, 0),
        ledger: ledger("accepted"),
        ..Look::default()
    };
    let windows = Windows {
        strangers: vec![Stranger {
            process: Process {
                pid: 7777,
                command: "node scripts/longrun-dev-server.mjs".to_owned(),
            },
            pane: "p1-long-worker-1".to_owned(),
            since: at(6, 0),
            began: "Fri Oct 16 00:29:15 2026".to_owned(),
        }],
        ..Windows::default()
    };
    let after = after_close(
        at(6, 20),
        Err(crate::Error::Refused("file is not a database".to_owned())),
        windows,
        records_of(Some(&last)),
        (&Settings::default(), "the project is done", at(30, 0)),
    );
    assert!(after.look.is_none(), "no look on the earlier ledger");
    assert_eq!(
        after.unread.as_deref(),
        Some("the ledger the app left could not be read: file is not a database")
    );
    // What the close left of the run's processes needs no ledger, and is held.
    let keys: Vec<&str> = after.found.iter().map(|found| found.key.as_str()).collect();
    assert_eq!(keys, ["6:stranger:7777"]);
}

#[test]
fn the_last_look_lets_a_jobs_end_be_told_within_the_settle_time_of_its_result() {
    // A Claude Code worker's job b33m, started at 10:01; the result at 10:02, and the
    // job's end told at 10:02:05, as it ended with the result. The app closed at
    // 10:02:10: the job had ended by the result, which A2 holds.
    let mut closed = ledger("done");
    closed.conversations.push(Conversation {
        id: 9,
        participant: 3,
        harness: "claude-code".to_owned(),
        session: None,
        started_at: None,
        ended_at: None,
    });
    for (seq, role, text, minute, second) in [
        (0, "user", "[ConsensFlow m-120 · T-14 · task from @chief]\nRun them", 0, 30),
        (1, "tool", "Command running in background with ID: b33m.", 1, 0),
        (2, "assistant", "The mutation run is going.", 1, 30),
        (
            3,
            "user",
            "<task-notification>\n<task-id>b33m</task-id>\n<status>completed</status>\n</task-notification>",
            2,
            5,
        ),
    ] {
        closed.items.push(Item {
            conversation: 9,
            id: format!("i-{seq}"),
            seq,
            role: role.to_owned(),
            text: text.to_owned(),
            complete: true,
            at: Some(iso(minute, second)),
            copied_at: iso(minute, second),
        });
    }
    closed.messages.push(message(
        131,
        Some(3),
        "result",
        "The mutation run is going.",
        2,
        0,
    ));
    let look = Look {
        at: at(2, 10),
        ledger: closed,
        ..Look::default()
    };
    let found = final_breaks(
        &look,
        &Settings::default(),
        "the project is done",
        at(30, 0),
    );
    assert!(!found.iter().any(|found| found.invariant == 2), "{found:?}");
    // Told past the settle time, it is the end of a job that ran on.
    let mut late = look.clone();
    late.ledger.items[3].at = Some(iso(3, 30));
    late.at = at(3, 40);
    let found = final_breaks(
        &late,
        &Settings::default(),
        "the project is done",
        at(30, 0),
    );
    assert!(
        found.iter().any(|found| found.key == "2:m-131"),
        "{found:?}"
    );
}

#[test]
fn the_run_waits_while_the_newest_result_is_younger_than_the_settle_time() {
    let mut look = Look {
        at: at(5, 0),
        ledger: ledger("done"),
        ..Look::default()
    };
    assert_eq!(settle_left(&look, 60_000, at(5, 0)), 0, "no result");
    look.ledger
        .messages
        .push(message(131, Some(3), "result", "Done.", 4, 40));
    look.ledger
        .messages
        .push(message(120, Some(3), "result", "First go.", 1, 0));
    assert_eq!(settle_left(&look, 60_000, at(5, 0)), 40_000);
    assert_eq!(settle_left(&look, 60_000, at(6, 0)), 0);
    assert_eq!(settle_left(&look, 60_000, at(4, 0)), 60_000, "never more");
}

#[test]
fn what_the_app_wrote_as_it_closed_is_held_in_the_last_look() {
    // The last look while the app ran saw the result alone; the app wrote the close note
    // of a task still working as it closed, after the result. The result, a moment old,
    // is not the worker's final answer.
    let mut running = ledger("done");
    running.conversations.push(Conversation {
        id: 9,
        participant: 3,
        harness: "claude-code".to_owned(),
        session: None,
        started_at: None,
        ended_at: None,
    });
    for (seq, role, text, second) in [
        (
            0,
            "user",
            "[ConsensFlow m-120 · T-14 · task from @chief]\nRun them",
            0,
        ),
        (1, "assistant", "Mutations: 41 killed, 3 survived.", 50),
    ] {
        running.items.push(Item {
            conversation: 9,
            id: format!("i-{seq}"),
            seq,
            role: role.to_owned(),
            text: text.to_owned(),
            complete: role == "assistant",
            at: Some(iso(2, second)),
            copied_at: iso(2, second),
        });
    }
    // The brief, m-120, delivered to the worker: its turn is the result's.
    running.messages.push(Message {
        recipient: 3,
        ..message(120, Some(2), "task", "Run them", 2, 0)
    });
    running.events.push(Event {
        id: 1,
        at: iso(2, 0),
        kind: "delivery.begun".to_owned(),
        data: serde_json::json!({ "message": 120, "attempt": 1 }),
    });
    running
        .messages
        .push(message(131, Some(3), "result", "Done.", 3, 0));
    let last = Look {
        at: at(2, 30),
        ledger: running.clone(),
        ..Look::default()
    };
    let mut closed = running;
    closed.tasks.push(Task {
        id: 41,
        number: 15,
        title: "Run the rest".to_owned(),
        state: "working".to_owned(),
        assignee: Some(3),
        requester: 2,
        updated_at: iso(2, 30),
        deleted: false,
    });
    closed.events.push(Event {
        id: 2,
        at: iso(2, 30),
        kind: "task.state".to_owned(),
        data: serde_json::json!({ "task": 15, "from": "queued", "to": "working" }),
    });
    closed.messages.push(Message {
        task: Some(41),
        ..message(
            132,
            None,
            "note",
            "T-15's window closed with background work running, which its close ended: npx stryker run.",
            3,
            1,
        )
    });
    // Taken ten seconds after the result: the closed ledger is all it will be, and
    // a result is judged however young.
    let look = closed_look(
        at(3, 10),
        closed,
        Windows::default(),
        records_of(Some(&last)),
    );
    let found = final_breaks(
        &look,
        &Settings::default(),
        "the project is done",
        at(30, 0),
    );
    assert!(
        found.iter().any(|found| found.key == "3:m-132"),
        "{found:?}"
    );
    assert!(
        found.iter().any(|found| found.key == "4:m-131"),
        "{found:?}"
    );
    // And the end, which the project of this ledger did not reach.
    assert!(found.iter().any(|found| found.key == "9:end"), "{found:?}");
}

/// The records of `last`, the last look, as far as it read them: what a closed look holds
/// where the records were not read again.
fn records_of(last: Option<&Look>) -> Records {
    last.map(|look| (look.records.clone(), look.every_reading()))
        .unwrap_or_default()
}

#[test]
fn the_timeline_says_the_records_were_read_once_more_after_the_close_and_how_many_readings_failed()
{
    // Calliope's review of round b7: `finish` takes the records read after the close in
    // place of the last look's; what it says shows that reading ran, and a failed one.
    let reading = |failed: Option<&str>| Reading {
        through: failed.is_none().then_some(at(6, 0)),
        failed: failed.map(str::to_owned),
    };
    // Three conversations, one reading failed (Calliope's review of round b9, T-84): with one
    // held and one failed, counting the held ones says the same.
    let readings: BTreeMap<i64, Reading> = [
        (7, reading(None)),
        (9, reading(Some("its record could not be read: denied"))),
        (11, reading(None)),
    ]
    .into_iter()
    .collect();
    assert_eq!(
        read_again(&readings),
        "the records were read once more after the close: 3 conversations, 1 reading failed"
    );
    let one: BTreeMap<i64, Reading> = [(7, reading(None))].into_iter().collect();
    assert_eq!(
        read_again(&one),
        "the records were read once more after the close: 1 conversation, 0 readings failed"
    );
}

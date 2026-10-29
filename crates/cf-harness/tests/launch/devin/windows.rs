//! How a Devin window names the conversation it opened, how a message reaches
//! it, and what its own wire log and record say (Node's Devin adapter suite).

use std::cell::{Cell, RefCell};
use std::fs;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use cf_base::text::console_text;
use cf_harness::contract::{
    Adapter, Admission, Held, Interrupt, Readiness, Records, Waiting, NOT_ADMITTED,
};
use cf_harness::records::{ModelMoved, Quota, Record, Settlement};
use cf_harness::seams::Time;
use cf_harness::testing::{
    finished, AnsweringHost, Driver, ScriptedHost, EPOCH_MS, FIRST_MESSAGE_MS,
};
use serde_json::{json, Value};

use super::fixtures::*;

#[test]
fn learns_the_session_this_window_opened_from_its_own_wire_log() {
    let home = Home::new();
    let looked = Said::new(settled("x"));
    let adapter = home.adapter_over(Some(Rc::clone(&looked) as Rc<dyn Records>));
    let window = window(&adapter, &Request::default());
    let mut driver = Driver::default();
    let asking = Rc::clone(&window);
    driver.begin(
        0,
        async move { asking.started(&ScriptedHost::default()).await },
    );
    assert!(driver.run().is_empty());
    // Devin's wire log says it some polls later.
    assert!(home.fakes.time.fire_next(EPOCH_MS + 250));
    assert!(driver.run().is_empty());
    home.selects("mild-coin");
    assert!(home.fakes.time.fire_next(EPOCH_MS + 500));
    assert_eq!(driver.run(), [(0, Ok(Some("mild-coin".to_owned())))]);
    // The window is on that conversation from then on: its record is read there.
    assert!(finished(window.observe()).unwrap().settled);
    assert_eq!(*looked.looked.borrow(), ["mild-coin"]);
}

#[test]
fn a_torn_line_of_its_wire_log_before_the_session_s_line_neither_stops_the_launch_nor_a_paste() {
    // A short write left half of the log's first record, and Devin's next append
    // made one line of it and itself, before its window named the conversation it
    // opened (the brain's issue a-torn-transcript-line-stops-...; T-246).
    let home = Home::new();
    let adapter = home.adapter();
    let window = window(&adapter, &Request::default());
    let first = shows("another-one");
    home.selects("another-one");
    fs::write(home.wire(), &first[..first.len() / 2]).unwrap();
    home.appends(&shows("another-one"));
    home.appends(&shows("mild-coin"));
    assert_eq!(
        finished(window.started(&ScriptedHost::default())),
        Ok(Some("mild-coin".to_owned())),
        "the launch learns its session past the torn line"
    );
    let host = AnsweringHost::new(|op| {
        Ok(match op {
            "pane.snapshot" => json!({ "ok": true, "pasteInFlight": false, "unsent": false }),
            _ => json!({ "ok": true }),
        })
    });
    assert_eq!(
        finished(window.deliver(&host, &pane(), "hi")).unwrap(),
        Admission::Admitted { queued: false }
    );
}

/// How many milliseconds a window opened on `request` waits for Devin's wire
/// log to name a conversation before it says that none was named.
fn waits_for_the_session(request: &Request) -> i64 {
    let home = Home::new();
    let adapter = home.adapter();
    let window = window(&adapter, request);
    let mut driver = Driver::default();
    driver.begin(
        0,
        async move { window.started(&ScriptedHost::default()).await },
    );
    let began = home.fakes.time.wall_ms();
    let mut settled = driver.run();
    while settled.is_empty() {
        assert!(
            home.fakes.time.fire_next(began + 120_000),
            "no answer in two minutes"
        );
        settled = driver.run();
    }
    let said = "Devin never said which session it opened (its wire log stayed empty)";
    assert_eq!(settled, [(0, Err(said.to_owned()))]);
    home.fakes.time.wall_ms() - began
}

#[test]
fn a_session_never_named_is_an_error_after_a_minute_or_the_time_the_launch_has_if_that_is_less() {
    let for_launch = |first_message_ms| {
        waits_for_the_session(&Request {
            first_message_ms,
            ..Request::default()
        })
    };
    assert_eq!(for_launch(2_000), 2_000);
    assert_eq!(for_launch(30_000), 30_000);
    for launch in [60_000, FIRST_MESSAGE_MS, 3_600_000] {
        assert_eq!(for_launch(launch), 60_000, "{launch}");
    }
}

#[test]
fn pastes_only_into_the_conversation_it_knows_and_never_into_what_the_human_has_not_sent() {
    let home = Home::new();
    let adapter = home.adapter();
    let window = window(&adapter, &Request::resumed("mild-coin"));
    let paste_in_flight = Cell::new(false);
    let unsent = Cell::new(false);
    let host = AnsweringHost::new(|op| {
        Ok(match op {
            "pane.snapshot" => json!({
                "ok": true, "pasteInFlight": paste_in_flight.get(), "unsent": unsent.get(),
            }),
            _ => json!({ "ok": true }),
        })
    });
    let deliver = |text: &str| finished(window.deliver(&host, &pane(), text)).unwrap();
    home.selects("another-one");
    assert_eq!(
        deliver("hi"),
        Admission::Refused {
            reason: "Devin is displaying another conversation".to_owned()
        }
    );
    assert!(host.asked.borrow().is_empty(), "nothing reached the pane");
    home.selects("mild-coin");
    assert_eq!(deliver("hi"), Admission::Admitted { queued: false });
    assert_eq!(
        host.asked.borrow().last().unwrap(),
        &(
            "pane.write_paste".to_owned(),
            json!({ "id": "s1-zeus", "generation": 2, "body": "hi" })
        )
    );
    // Node's case began with half a surrogate pair, which it dropped: a Rust
    // text holds none.
    deliver("half  of it, \u{1b}[31mred\u{1b}[0m and 50%\r60%");
    // On a Windows machine every window is Windows': there it goes in ASCII.
    let shown = "half  of it, \u{241b}[31mred\u{241b}[0m and 50%\u{240d}60%";
    let pasted = host.asked.borrow().last().unwrap().1["body"].clone();
    assert_eq!(
        pasted,
        if cfg!(windows) {
            console_text(shown)
        } else {
            shown.to_owned()
        }
    );
    assert_eq!(finished(window.ready(&host, &pane())), Ok(Readiness::Ready));
    paste_in_flight.set(true);
    assert_eq!(
        finished(window.ready(&host, &pane())),
        Ok(Readiness::Held(Held::Unsaid))
    );
    paste_in_flight.set(false);
    unsent.set(true);
    assert_eq!(
        finished(window.ready(&host, &pane())),
        Ok(Readiness::Held(Held::Unsent))
    );
}

/// Keys the human typed into Devin's model picker and left with Escape count as
/// unsent, though its input box shows its hint again (T-288, `cargo xtask live
/// escape-unsent`): read empty, with the human's last key a second past, the window
/// is ready and its paste says at which count the box read empty. Words in the box,
/// or a key a moment ago, still hold it.
#[test]
fn a_window_whose_input_box_shows_its_hint_takes_the_paste_over_keys_left_in_its_picker() {
    const RULE: &str = "────────────────────────────────────────";
    let home = Home::new();
    let adapter = home.adapter();
    let window = window(&adapter, &Request::resumed("mild-coin"));
    home.selects("mild-coin");
    let typed_in_box =
        RefCell::new("❭ Ask Devin to build features, fix bugs, or work on your code".to_owned());
    let quiet = Cell::new(2_000_u64);
    let cursor = RefCell::new(Value::Null);
    let host = AnsweringHost::new(|op| {
        Ok(match op {
            "pane.snapshot" => json!({
                "ok": true, "pasteInFlight": false, "unsent": true,
                "typed": 9, "typedQuietMs": quiet.get(), "outputQuietMs": 1_000,
                "bottom": ["❭ /model", "", format!("{RULE} (bypass permissions on) ─"),
                    typed_in_box.borrow().clone(), RULE, "SWE-2 Medium"],
                "cursor": cursor.borrow().clone(),
            }),
            _ => json!({ "ok": true }),
        })
    });
    let ready = || finished(window.ready(&host, &pane()));
    assert_eq!(ready(), Ok(Readiness::Ready));
    assert_eq!(
        finished(window.deliver(&host, &pane(), "hi")),
        Ok(Admission::Admitted { queued: false })
    );
    assert_eq!(
        host.asked.borrow().last().unwrap(),
        &(
            "pane.write_paste".to_owned(),
            json!({ "id": "s1-zeus", "generation": 2, "body": "hi", "readEmpty": 9 })
        )
    );
    quiet.set(200);
    assert_eq!(ready(), Ok(Readiness::Held(Held::Unsent)));
    quiet.set(2_000);
    // A hint in other words, which a Devin update may give it: the cursor at the box's
    // start and nothing after it drawn plain says it is one (T-330).
    *typed_in_box.borrow_mut() = "❭ Ask Devin anything about your code".to_owned();
    assert_eq!(ready(), Ok(Readiness::Held(Held::Unsent)));
    *cursor.borrow_mut() = json!({ "line": 3, "col": 2, "shown": true, "plainAfter": 0 });
    assert_eq!(ready(), Ok(Readiness::Ready));
    // Words typed and the cursor moved back to their start (Home) are drawn plain.
    *typed_in_box.borrow_mut() = "❭ probe words".to_owned();
    *cursor.borrow_mut() = json!({ "line": 3, "col": 2, "shown": true, "plainAfter": 10 });
    assert_eq!(ready(), Ok(Readiness::Held(Held::Unsent)));
}

/// T-829: the engine presses no key into a box its harness reads empty: the window says
/// so of a box that shows only Devin's hint, and not of one with words in it or of a
/// screen with no box.
#[test]
fn the_engine_is_told_the_box_is_empty_only_where_devin_reads_it_so() {
    const RULE: &str = "────────────────────────────────────────";
    let home = Home::new();
    let adapter = home.adapter();
    let window = window(&adapter, &Request::resumed("mild-coin"));
    let drawn = |typed: &str, plain_after: u64| {
        json!({
            "ok": true,
            "bottom": [format!("{RULE} (bypass permissions on) ─"), typed, RULE, "SWE-2 Medium"],
            "cursor": { "line": 1, "col": 2, "shown": true, "plainAfter": plain_after },
        })
    };
    assert!(window.box_read_empty(&drawn("❭ Ask Devin anything about your code", 0)));
    assert!(!window.box_read_empty(&drawn("❭ probe words", 10)));
    assert!(!window.box_read_empty(&json!({ "ok": true, "bottom": ["SWE-2 Medium"] })));
}

/// Both reviews of 2026-10-16, round 4: a paste asks the host whether it still
/// admits it as it is asked for, after the window's own reads.
#[test]
fn a_paste_the_host_no_longer_admits_is_refused_and_never_reaches_the_pane() {
    let home = Home::new();
    let window = window(&home.adapter(), &Request::resumed("mild-coin"));
    home.selects("mild-coin");
    let host = ScriptedHost::default();
    host.refuse_hand_overs();
    assert_eq!(
        finished(window.deliver(&host, &pane(), "hi")),
        Ok(Admission::Refused {
            reason: NOT_ADMITTED.to_owned()
        })
    );
    assert!(host.take_asked().is_empty(), "nothing reached the pane");
    assert_eq!(host.admissions(), 1);
}

#[test]
fn on_windows_the_console_drops_a_pastes_non_ascii_marks_so_they_go_in_ascii() {
    let home = Home::windows();
    let window = window(&home.adapter(), &Request::resumed("mild-coin"));
    home.selects("mild-coin");
    let host = readable();
    let text = "[ConsensFlow m-3 · T-1 · answer from @chief]\nblue — not “red” → done…";
    assert_eq!(
        finished(window.deliver(&host, &pane(), text)),
        Ok(Admission::Admitted { queued: false })
    );
    assert_eq!(
        host.asked.borrow().last().unwrap().1["body"],
        "[ConsensFlow m-3 | T-1 | answer from @chief]\nblue -- not \"red\" -> done..."
    );
}

#[test]
fn interrupts_with_escape_twice_and_closes_with_a_third_the_rewind_the_two_open_at_an_idle_prompt()
{
    // poker-lab, 2026-10-03: two Escapes at a Devin already stopped opened its
    // rewind, and the next message's Enter rewound the conversation.
    assert_eq!(
        Home::new().adapter().interrupt(),
        Interrupt {
            presses: 2,
            close_after: Some(Duration::from_millis(1_000)),
        }
    );
}

#[test]
fn reads_a_wire_log_that_was_replaced_from_its_start_with_nothing_of_the_old_one_carried() {
    let home = Home::new();
    let window = window(&home.adapter(), &Request::resumed("mild-coin"));
    let host = readable();
    // A line half written when it was read: carried until the rest comes.
    fs::write(
        home.wire(),
        format!(
            "{}{}{{\"sessionId\":\"mild",
            shows("another-one"),
            shows("mild-coin")
        ),
    )
    .unwrap();
    assert_eq!(finished(window.ready(&host, &pane())), Ok(Readiness::Ready));
    home.selects("another-one");
    assert_eq!(
        finished(window.ready(&host, &pane())),
        Ok(Readiness::Held(Held::ShowsAnother))
    );
}

#[test]
fn follows_the_window_to_the_conversation_a_new_or_resume_left_it_on_holding_until_it_names_one() {
    let home = Home::new();
    let looked = Said::new(settled("x"));
    let adapter = home.adapter_over(Some(Rc::clone(&looked) as Rc<dyn Records>));
    let window = window(&adapter, &Request::resumed("mild-coin"));
    let host = readable();
    // Devin has not said yet which conversation the window shows: a message waits.
    let before = finished(window.observe()).unwrap();
    assert!(before.unnamed);
    let reason = before.waiting.clone().unwrap().reason.unwrap();
    assert!(reason.contains("Devin has not said"), "{reason}");
    assert_eq!(
        finished(window.ready(&host, &pane())),
        Ok(Readiness::Held(Held::Because(reason)))
    );
    home.selects("mild-coin");
    assert!(finished(window.observe()).unwrap().settled);
    assert_eq!(finished(window.ready(&host, &pane())), Ok(Readiness::Ready));

    // /new: Devin's own log now names another conversation.
    home.appends(&shows("fresh-leaf"));
    let observed = finished(window.observe()).unwrap();
    assert_eq!(observed.switched.as_deref(), Some("fresh-leaf"));
    assert!(!observed.settled);
    assert_eq!(
        finished(window.ready(&host, &pane())),
        Ok(Readiness::Held(Held::ShowsAnother))
    );

    // The engine follows the window: the new conversation's record is read.
    window.follow("fresh-leaf");
    let followed = finished(window.observe()).unwrap();
    assert_eq!(followed.switched, None);
    assert_eq!(
        looked.looked.borrow().last().map(String::as_str),
        Some("fresh-leaf")
    );
    assert_eq!(finished(window.ready(&host, &pane())), Ok(Readiness::Ready));
}

#[test]
fn counts_a_window_whose_session_is_not_known_yet_as_not_ready() {
    let home = Home::new();
    let looked = Said::new(settled("x"));
    let adapter = home.adapter_over(Some(Rc::clone(&looked) as Rc<dyn Records>));
    let window = window(&adapter, &Request::default());
    let observed = finished(window.observe()).unwrap();
    assert_eq!(
        (
            observed.items().is_empty(),
            observed.settled,
            observed.waiting,
            observed.failed,
            observed.quota
        ),
        (true, false, None, false, None)
    );
    assert!(looked.looked.borrow().is_empty(), "nothing to read yet");
}

#[test]
fn reads_its_own_question_dialog_still_open_as_waiting() {
    let home = Home::new();
    let open = Said::new(Record {
        in_flight: true,
        asking: true,
        settlement: Settlement::InFlight,
        ..settled("x")
    });
    let adapter = home.adapter_over(Some(open as Rc<dyn Records>));
    let window = window(&adapter, &Request::default());
    let mut driver = Driver::default();
    let asking = Rc::clone(&window);
    home.fakes.time.settle_at(EPOCH_MS);
    driver.begin(
        0,
        async move { asking.started(&ScriptedHost::default()).await },
    );
    assert!(driver.run().is_empty());
    home.selects("dev-1");
    assert!(home.fakes.time.fire_next(EPOCH_MS + 250));
    assert_eq!(driver.run(), [(0, Ok(Some("dev-1".to_owned())))]);
    assert_eq!(
        finished(window.observe()).unwrap().waiting,
        Some(Waiting {
            reason: Some("its own question dialog is open".to_owned())
        })
    );
}

#[test]
fn reads_devins_own_refusal_from_the_wire_log_until_the_next_prompt() {
    let home = Home::new();
    let adapter = home.adapter_over(Some(Said::new(settled("x")) as Rc<dyn Records>));
    let window = window(&adapter, &Request::resumed("dev-1"));
    home.selects("dev-1");
    let line = |event: Value| home.appends(&format!("{event}\n"));
    let quota = || serde_json::to_value(finished(window.observe()).unwrap().quota).unwrap();
    assert_eq!(quota(), Value::Null);

    let prompt = |id: u32| json!({ "jsonrpc": "2.0", "id": id, "method": "session/prompt", "params": { "sessionId": "dev-1" } });
    line(prompt(2));
    line(json!({
        "sessionId": "dev-1",
        "update": {
            "sessionUpdate": "agent_message_chunk",
            "content": {
                "type": "text",
                "text": "Reached overall message rate limit. Your limit will reset in 35 minutes.",
            },
        },
    }));
    let at = cf_base::time::iso(EPOCH_MS);
    let resets = cf_base::time::iso(EPOCH_MS + 35 * 60_000);
    let exhausted = json!({ "state": "exhausted", "at": at, "resetsAt": resets });
    assert_eq!(quota(), exhausted);
    assert_eq!(quota(), exhausted, "nothing new: still out");

    line(prompt(3));
    line(json!({
        "sessionId": "dev-1",
        "update": { "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "On it." } },
    }));
    assert_eq!(quota(), Value::Null);

    // The prompt refused outright, in the words Devin 3000.11 carries for it.
    line(prompt(4));
    line(json!({
        "jsonrpc": "2.0", "id": 4,
        "error": {
            "code": -32011,
            "message": "Quota exhausted.",
            "data": { "cognition.ai/errorKind": "resource_exhausted", "cognition.ai/retryable": true },
        },
    }));
    assert_eq!(quota()["state"], "exhausted");

    // Or the turn's own end for it, as Devin wrote it on a Pro plan (poker-lab, 2026-10-03).
    line(prompt(5));
    assert_eq!(quota(), Value::Null);
    line(json!({
        "cause": "quota_exhausted",
        "errorMessage": "Your daily usage quota has been exhausted. Visit https://app.devin.ai/settings/usage to purchase on-demand usage or turn on auto-reload. (trace ID: c27105417b16dd2a33d911ce59955e84)",
        "sessionId": "dev-1",
    }));
    assert_eq!(quota()["state"], "exhausted");
}

#[test]
fn a_move_the_record_holds_outranks_the_refusal_the_wire_log_says_and_the_records_own_stands_beside_a_silent_wire(
) {
    // The look's one precedence (T-853): the move, then what the window says now, then the
    // record's own. Devin's reader says neither a move nor a refusal today; a record that does is
    // not lost by the wire's word, nor its refusal replaced by a wire that says none.
    let prompt = json!({ "jsonrpc": "2.0", "id": 2, "method": "session/prompt", "params": { "sessionId": "dev-1" } });
    let limit = json!({
        "sessionId": "dev-1",
        "update": {
            "sessionUpdate": "agent_message_chunk",
            "content": {
                "type": "text",
                "text": "Reached overall message rate limit. Your limit will reset in 35 minutes.",
            },
        },
    });
    let moved = Arc::new(ModelMoved {
        from: "swe-1.6".to_owned(),
        to: "swe-1.5".to_owned(),
        because: "Devin moved the model".to_owned(),
        at: None,
    });
    let refused = Arc::new(Quota::Unavailable {
        at: None,
        because: "its billing was refused".to_owned(),
    });
    let look = |record: Record, wire: &[&Value]| {
        let home = Home::new();
        let adapter = home.adapter_over(Some(Said::new(record) as Rc<dyn Records>));
        let window = window(&adapter, &Request::resumed("dev-1"));
        home.selects("dev-1");
        for line in wire {
            home.appends(&format!("{line}\n"));
        }
        finished(window.observe()).unwrap().quota
    };
    let with_move = Record {
        model_moved: Some(Arc::clone(&moved)),
        ..settled("x")
    };
    assert_eq!(
        look(with_move, &[&prompt, &limit]).as_deref(),
        Some(&moved.as_quota()),
        "the wire's refusal does not hide the move"
    );
    let with_refusal = Record {
        quota: Some(Arc::clone(&refused)),
        ..settled("x")
    };
    assert_eq!(
        look(with_refusal.clone(), &[]).as_deref(),
        Some(&*refused),
        "a silent wire does not take the record's refusal"
    );
    let wire = look(with_refusal, &[&prompt, &limit]);
    assert!(
        matches!(wire.as_deref(), Some(Quota::Exhausted { .. })),
        "what the window says now outranks the record's: {wire:?}"
    );
}

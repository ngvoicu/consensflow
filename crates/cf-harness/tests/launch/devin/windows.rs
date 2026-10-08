//! How a Devin window names the conversation it opened, how a message reaches
//! it, and what its own wire log and record say (Node's Devin adapter suite).

use std::cell::Cell;
use std::fs;
use std::rc::Rc;
use std::time::Duration;

use cf_base::text::console_text;
use cf_harness::contract::{Adapter, Admission, Held, Interrupt, Readiness, Records, Waiting};
use cf_harness::records::{Record, Settlement};
use cf_harness::testing::{finished, AnsweringHost, Driver, EPOCH_MS};
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
    driver.begin(0, async move { asking.started().await });
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
    driver.begin(0, async move { asking.started().await });
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

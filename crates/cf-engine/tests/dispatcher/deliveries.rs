//! What goes into a window and is proved by its record: one message at a
//! time to an idle window, retried when the harness refuses it or its
//! arrival never shows, one more Enter for a paste left unsent, and a message
//! the harness queued itself (`describe('the dispatcher')`).

use std::cell::Cell;
use std::rc::Rc;

use cf_engine::delivery_text::delivery_text;
use cf_engine::testing::Context;
use cf_harness::contract::Admission;
use cf_harness::records::Role;
use serde_json::{json, Value};

use crate::fixtures::assert_match;
use crate::traces::held_to;

const SUITES: &[&str] = &["the dispatcher"];

/// The Enters pressed into windows (`pane.input` with the byte 13).
fn enters(context: &Context) -> usize {
    context
        .host
        .inputs()
        .iter()
        .filter(|body| body["bytes"][0] == 13)
        .count()
}

/// The words of the last item a window was given.
fn last_text(context: &Context, handle: &str) -> String {
    let last = context.adapter.agent(handle).items.pop().unwrap();
    last.text.to_string()
}

#[test]
fn delivers_results_to_an_idle_chief_one_at_a_time_and_proves_each_arrived() {
    let context = Context::new();
    let project = context.with_staff(&["zeus", "diana"]);
    context.give(project.id, "zeus", "One");
    context.give(project.id, "diana", "Two");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.busy("chief");
    context.adapter.answer("zeus", "one done");
    context.adapter.answer("diana", "two done");
    context.pass().unwrap();
    context.pass().unwrap();
    let chief = || context.adapter.agent("chief");
    assert_eq!(chief().items.len(), 0, "a busy chief is not interrupted");

    context.adapter.with("chief", |agent| agent.settled = true);
    context.pass().unwrap();
    assert_eq!(chief().items.len(), 1);
    assert_match(
        &chief().items[0].text,
        r"result from @zeus\]\none done\n\nDecide with: cf task accept T-1",
    );
    context.pass().unwrap();
    let id = context.id(project.id, "chief");
    let mut inbox = context.inbox(id);
    inbox.reverse();
    let (first, second) = (&inbox[0], &inbox[1]);
    assert_eq!(first.state, "delivered");
    assert_eq!(first.receipt["item"], json!(&*chief().items[0].id));
    assert_eq!(
        second.state, "queued",
        "the next waits until the chief is idle again"
    );

    context.adapter.answer("chief", "noted");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.inbox(id)[0].state, "delivered");
    assert_match(
        &last_text(&context, "chief"),
        r"result from @diana\]\ntwo done\n\nDecide with: cf task accept T-2",
    );
    held_to(
        context.close(),
        SUITES,
        "delivers results to an idle chief one at a time and proves each arrived",
    );
}

#[test]
fn retries_a_delivery_the_harness_refused_then_gives_up() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.adapter.with("chief", |agent| agent.admit = false);
    let note = context.note(project.id, "zeus", "chief", "hello");
    for _ in 0..5 {
        context.pass().unwrap();
    }
    let failed = context.message(note.id);
    assert_eq!((failed.state.as_str(), failed.attempts), ("failed", 3));
    assert_eq!(failed.reason.as_deref(), Some("refused by the test"));
    held_to(
        context.close(),
        SUITES,
        "retries a delivery the harness refused, then gives up",
    );
}

#[test]
fn retries_a_delivery_whose_arrival_never_shows_in_the_harness_record() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.adapter.with("chief", |agent| agent.arrive = false);
    let note = context.note(project.id, "zeus", "chief", "hello");
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering");
    context.advance(29_000);
    context.pass().unwrap();
    assert_eq!(
        context.message(note.id).state,
        "delivering",
        "still inside the arrival window"
    );
    context.advance(2_000);
    context.pass().unwrap();
    let delivering = context.message(note.id);
    assert_eq!(
        (delivering.state.as_str(), delivering.attempts),
        ("delivering", 2)
    );
    assert_eq!(delivering.reason, None);
    held_to(
        context.close(),
        SUITES,
        "retries a delivery whose arrival never shows in the harness record",
    );
}

#[test]
fn presses_enter_once_more_for_a_paste_its_window_has_not_sent_into_a_quiet_window_only() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.adapter.with("chief", |agent| agent.arrive = false);
    let note = context.note(project.id, "zeus", "chief", "hello");
    context.pass().unwrap();
    context.advance(9_000);
    context.host.set_snapshot(json!({ "outputQuietMs": 5_000 }));
    context.pass().unwrap();
    assert_eq!(enters(&context), 0, "not before the record had its time");
    // Printing a moment ago: being typed into, or still drawing.
    context.host.set_snapshot(json!({ "outputQuietMs": 500 }));
    context.advance(2_000);
    context.pass().unwrap();
    assert_eq!(enters(&context), 0, "not into a window that just printed");
    // Quiet now: the Enter sends what sat in the input, and the record shows it.
    context.host.set_snapshot(json!({ "outputQuietMs": 5_000 }));
    let (adapter, header) = (Rc::downgrade(&context.adapter), delivery_text(&note, &[]));
    *context.host.on_request.borrow_mut() = Some(Rc::new(move |op: &str, body: &Value| {
        let enter = op == "pane.input" && body["bytes"][0] == 13;
        if let (true, Some(adapter)) = (enter, adapter.upgrade()) {
            let item = adapter.item(Role::User, &header);
            adapter.with("chief", |agent| agent.items.push(item));
        }
    }));
    context.pass().unwrap();
    context.pass().unwrap();
    let message = context.message(note.id);
    assert_eq!((message.state.as_str(), message.attempts), ("delivered", 1));
    assert_eq!(enters(&context), 1, "pressed once");
    held_to(
        context.close(),
        SUITES,
        "presses Enter once more for a paste its window has not sent, into a quiet window only",
    );
}

#[test]
fn pastes_again_once_its_arrival_window_passes_when_one_more_enter_did_not_bring_it() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.adapter.with("chief", |agent| agent.arrive = false);
    context.host.set_snapshot(json!({ "outputQuietMs": 5_000 }));
    let note = context.note(project.id, "zeus", "chief", "hello");
    context.pass().unwrap();
    context.advance(11_000);
    context.pass().unwrap();
    context.advance(5_000);
    context.pass().unwrap();
    assert_eq!(enters(&context), 1, "once for the attempt");
    context.advance(15_000);
    context.pass().unwrap();
    let message = context.message(note.id);
    assert_eq!(
        (message.state.as_str(), message.attempts),
        ("delivering", 2)
    );
    held_to(
        context.close(),
        SUITES,
        "pastes again, once its arrival window passes, when one more Enter did not bring it",
    );
}

#[test]
fn tries_an_uncertain_handover_again_once_its_arrival_window_passes_with_no_sign_of_it() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let (adapter, retried) = (Rc::downgrade(&context.adapter), Rc::new(Cell::new(false)));
    *context.adapter.deliver.borrow_mut() = Some(Rc::new(move |taking| {
        let empty = adapter
            .upgrade()
            .is_some_and(|adapter| adapter.agent("chief").items.is_empty());
        let uncertain = empty && !retried.replace(true);
        Box::pin(async move {
            if uncertain {
                return Ok(Admission::Uncertain {
                    reason: "the plugin did not answer".to_owned(),
                });
            }
            Ok(taking())
        })
    }));
    let note = context.note(project.id, "zeus", "chief", "hello");
    context.pass().unwrap();
    context.advance(31_000);
    context.pass().unwrap();
    context.pass().unwrap();
    let message = context.message(note.id);
    assert_eq!((message.state.as_str(), message.attempts), ("delivered", 2));
    assert_eq!(
        context.adapter.agent("chief").items.len(),
        1,
        "delivered once"
    );
    held_to(
        context.close(),
        SUITES,
        "tries an uncertain handover again once its arrival window passes with no sign of it",
    );
}

#[test]
fn counts_a_delivery_only_by_its_header_in_what_the_window_was_given_never_in_a_tools_output() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.arrive = false);
    let note = context.note(project.id, "zeus", "chief", "hello");
    context.pass().unwrap();
    // The agent's own command prints the header (cf inbox read, a grep of a log).
    let header = delivery_text(&context.message(note.id), &[]);
    let printed = [
        context.adapter.item(Role::Tool, &header),
        context.adapter.item(Role::Custom, &header),
    ];
    context
        .adapter
        .with("chief", |agent| agent.items.extend(printed));
    context.pass().unwrap();
    assert_eq!(
        context.message(note.id).state,
        "delivering",
        "not proof it arrived"
    );
    held_to(
        context.close(),
        SUITES,
        "counts a delivery only by its header in what the window was given, never in a tool’s output",
    );
}

#[test]
fn records_an_adapter_that_throws_as_a_failed_attempt_with_its_error_not_as_uncertain() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    *context.adapter.deliver.borrow_mut() = Some(Rc::new(|_| {
        Box::pin(async { Err("the channel needs pane and generation".to_owned()) })
    }));
    let note = context.note(project.id, "zeus", "chief", "hello");
    context.pass().unwrap();
    let message = context.message(note.id);
    assert_eq!((message.state.as_str(), message.attempts), ("queued", 1));
    assert_match(
        message.reason.as_deref().unwrap(),
        "the channel needs pane and generation",
    );
    held_to(
        context.close(),
        SUITES,
        "records an adapter that throws as a failed attempt with its error, not as uncertain",
    );
}

#[test]
fn waits_on_a_message_a_harness_queued_itself_and_re_sends_only_a_paste_the_record_never_showed() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.answer("chief", "ready");
    // The chief's own queue took it (a peer inbox, a broker): it will show
    // when the harness gets to it, maybe minutes later; sending it again
    // would only make a duplicate the harness may even drop.
    context.adapter.with("chief", |agent| {
        agent.queued = true;
        agent.arrive = false;
    });
    let note = context.note(project.id, "zeus", "chief", "Queued");
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering");
    context.advance(150_000);
    context.pass().unwrap();
    let waiting = context.message(note.id);
    assert_eq!(
        (waiting.state.as_str(), waiting.attempts),
        ("delivering", 1),
        "still in flight, not sent again"
    );
    let shown = context.adapter.item(
        Role::User,
        &format!("[ConsensFlow m-{} · note from @zeus]\nQueued", note.id),
    );
    context
        .adapter
        .with("chief", |agent| agent.items.push(shown));
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivered");

    // A paste has no such receipt: the record is the only proof, and 60 s
    // without it means the paste was lost.
    context.adapter.with("chief", |agent| agent.queued = false);
    let pasted = context.note(project.id, "zeus", "chief", "Pasted");
    context.pass().unwrap();
    context.advance(61_000);
    context.pass().unwrap();
    let sent = context.message(pasted.id);
    assert_eq!(
        (sent.state.as_str(), sent.attempts),
        ("delivering", 2),
        "sent again"
    );
    held_to(
        context.close(),
        SUITES,
        "waits on a message a harness queued itself, and re-sends only a paste the record never showed",
    );
}

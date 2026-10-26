//! A launch's first message that its window is delivered once it is up (T-432): a
//! Claude Code, Pi or Codex window opens with nothing on its command line, which
//! every process on the machine reads, and the step hands the message over by the
//! window's own `deliver` once the window is past any question of trusting its
//! folder, has drawn and is at rest. None of these tests is held to a Node
//! recording: Node's adapters put the message on the command line.

use std::cell::Cell;
use std::rc::Rc;

use cf_engine::delivery_text::marker_of;
use cf_engine::testing::Context;
use cf_harness::contract::{FirstMessage, Held, Readiness};
use cf_harness::records::{End, Monitor, Notice, Pending, Role, Source};
use cf_proto::trace::{Traced, WindowEvent};
use serde_json::json;

use crate::withheld_enter::times_handed;

const SEAM: &str = "adapter:claude-code";

/// The Enter a paste is sent with.
const ENTER: u8 = 13;

/// A context whose windows are delivered their first message once up.
fn delivered() -> Context {
    let context = Context::new();
    context.adapter.first.set(FirstMessage::Delivered);
    context
}

/// A project with zeus on its staff, and the brief of T-1 given to zeus.
fn briefed(context: &Context) -> (i64, i64) {
    let project = context.with_staff(&["zeus"]).id;
    let brief = context
        .give(project, "zeus", "Parser")
        .message
        .expect("its brief")
        .id;
    (project, brief)
}

/// Passes of the dispatcher, each settled.
fn passes(context: &Context, count: usize) {
    for _ in 0..count {
        context.pass().unwrap();
        context.settle();
    }
}

/// What the trace says held `message`, in order.
fn held(context: &Context, message: i64) -> Vec<String> {
    context
        .trace
        .lines
        .borrow()
        .iter()
        .filter_map(|line| match &line.what {
            Traced::Window {
                event:
                    WindowEvent::DeliveryHeld {
                        message: held,
                        reason,
                    },
                ..
            } if *held == message => Some(reason.clone()),
            _ => None,
        })
        .collect()
}

/// The Enters pressed into panes, by themselves.
fn enters(context: &Context) -> usize {
    context
        .host
        .inputs()
        .iter()
        .filter(|body| body["bytes"] == json!([ENTER]))
        .count()
}

#[test]
fn a_window_delivered_its_first_message_opens_with_none_and_is_handed_its_brief_once_up() {
    let context = delivered();
    let (_, brief) = briefed(&context);
    passes(&context, 3);
    let prepared = context.adapter.prepared();
    assert!(prepared.len() >= 2, "the chief's window and zeus's");
    assert!(
        prepared.iter().all(|launch| launch["message"].is_null()),
        "no launch is given its message: {prepared:?}"
    );
    assert_eq!(times_handed(&context, SEAM, brief), 1, "handed once");
    assert_eq!(context.message(brief).state, "delivered");
    context.close();
}

#[test]
fn a_first_message_handed_over_once_up_keeps_the_conversation_it_was_pasted_into() {
    // 3.0.6-alpha.3's integration B (T-664): devin-arrival keeps the conversation a paste
    // went into (`paste_into`, migration 0023) while the paste is on its way, for a start
    // that finds it so, and a launch's first message handed over once its window is up is
    // such a paste. Its record never shows it here, so it stays on its way.
    let context = delivered();
    let (project, brief) = briefed(&context);
    context.pass().unwrap();
    context.settle();
    context.adapter.with("zeus", |agent| agent.arrive = false);
    passes(&context, 2);
    assert_eq!(times_handed(&context, SEAM, brief), 1, "handed once");
    assert_eq!(context.message(brief).state, "delivering", "on its way");
    let zeus = context.id(project, "zeus");
    let ledger = context.ledger.borrow();
    let shown = ledger
        .current_conversation(zeus)
        .unwrap()
        .and_then(|conversation| conversation.native_session);
    assert!(shown.is_some(), "zeus's window shows a conversation");
    assert_eq!(ledger.pasted_into(brief).unwrap(), shown);
    drop(ledger);
    context.close();
}

#[test]
fn a_first_message_waits_while_its_window_is_not_ready_says_so_once_and_goes_in_once_it_is() {
    let context = delivered();
    let ready = Rc::new(Cell::new(false));
    let asked = Rc::clone(&ready);
    *context.adapter.ready.borrow_mut() = Some(Rc::new(move || {
        Ok(if asked.get() {
            Readiness::Ready
        } else {
            Readiness::Held(Held::Because("the broker names no thread yet".to_owned()))
        })
    }));
    let (_, brief) = briefed(&context);
    passes(&context, 4);
    assert_eq!(
        times_handed(&context, SEAM, brief),
        0,
        "nothing while not ready"
    );
    assert_eq!(context.message(brief).state, "delivering");
    let said = held(&context, brief);
    assert_eq!(said.len(), 1, "said once, not every pass: {said:?}");
    assert!(
        said[0].contains("not ready for its first message")
            && said[0].contains("the broker names no thread yet"),
        "{said:?}"
    );
    ready.set(true);
    passes(&context, 2);
    assert_eq!(times_handed(&context, SEAM, brief), 1);
    assert_eq!(context.message(brief).state, "delivered");
    context.close();
}

#[test]
fn a_first_message_waits_while_its_window_has_not_drawn_its_screen() {
    let context = delivered();
    // The host says the window has printed and not held still yet.
    context.host.set_snapshot(json!({ "outputQuietMs": null }));
    let (_, brief) = briefed(&context);
    passes(&context, 4);
    assert_eq!(
        times_handed(&context, SEAM, brief),
        0,
        "nothing while it draws"
    );
    context.host.set_snapshot(json!({ "outputQuietMs": 5000 }));
    passes(&context, 2);
    assert_eq!(times_handed(&context, SEAM, brief), 1, "handed once drawn");
    context.close();
}

#[test]
fn a_first_message_its_window_refused_is_handed_again_at_the_next_step() {
    let context = delivered();
    let (_, brief) = briefed(&context);
    context.pass().unwrap();
    context.settle();
    // Zeus's window is prepared: it refuses what it is handed, for now.
    context.adapter.with("zeus", |agent| agent.admit = false);
    passes(&context, 2);
    assert!(times_handed(&context, SEAM, brief) >= 1, "it was tried");
    assert_eq!(context.message(brief).state, "delivering", "owed still");
    let said = held(&context, brief);
    assert!(
        said.iter()
            .any(|reason| reason.contains("refused its first message")),
        "{said:?}"
    );
    let tried = times_handed(&context, SEAM, brief);
    context.adapter.with("zeus", |agent| agent.admit = true);
    passes(&context, 2);
    assert_eq!(times_handed(&context, SEAM, brief), tried + 1);
    assert_eq!(context.message(brief).state, "delivered");
    context.close();
}

#[test]
fn a_window_never_ready_for_its_first_message_in_the_launch_s_time_is_given_up_and_its_brief_fails()
{
    let context = delivered();
    *context.adapter.ready.borrow_mut() = Some(Rc::new(|| {
        Ok(Readiness::Held(Held::Because("starting".to_owned())))
    }));
    let (project, brief) = briefed(&context);
    passes(&context, 3);
    assert_eq!(context.message(brief).state, "delivering");
    let killed = context.host.killed().len();
    context.advance(120_001);
    passes(&context, 3);
    assert_eq!(times_handed(&context, SEAM, brief), 0, "never handed");
    assert_eq!(context.message(brief).state, "failed");
    assert_eq!(context.task(project, 1).task.state, "failed");
    assert!(
        context.host.killed().len() > killed,
        "its window was closed"
    );
    let chief = context.id(project, "chief");
    assert!(
        context
            .inbox(chief)
            .iter()
            .any(|message| message.body.contains("never ready for its first message")),
        "the chief hears why"
    );
    context.close();
}

#[test]
fn a_window_that_never_will_be_fit_for_its_first_message_is_given_up_at_once() {
    let context = delivered();
    let (project, brief) = briefed(&context);
    context.pass().unwrap();
    context.settle();
    *context.adapter.ready.borrow_mut() = Some(Rc::new(|| {
        Ok(Readiness::Unfit("it came up in its normal mode".to_owned()))
    }));
    passes(&context, 3);
    assert_eq!(times_handed(&context, SEAM, brief), 0);
    assert_eq!(context.message(brief).state, "failed");
    assert_eq!(context.task(project, 1).task.state, "failed");
    context.close();
}

#[test]
fn a_first_message_pasted_once_up_that_its_record_never_shows_is_pressed_enter_for_once_more() {
    let context = delivered();
    let (_, brief) = briefed(&context);
    context.pass().unwrap();
    context.settle();
    // Zeus's window takes the paste and its record never shows it: its Enter was lost.
    context.adapter.with("zeus", |agent| agent.arrive = false);
    context.host.set_snapshot(json!({ "outputQuietMs": 5000 }));
    passes(&context, 2);
    assert_eq!(times_handed(&context, SEAM, brief), 1);
    let before = enters(&context);
    passes(&context, 2);
    assert_eq!(enters(&context), before, "not before its time");
    context.advance(10_001);
    passes(&context, 2);
    assert_eq!(enters(&context), before + 1, "one Enter more");
    context.advance(10_001);
    passes(&context, 2);
    assert_eq!(enters(&context), before + 1, "and only one");
    assert_eq!(times_handed(&context, SEAM, brief), 1, "never pasted twice");
    context.close();
}

#[test]
fn a_first_message_whose_project_closes_while_its_window_takes_it_is_given_back_and_goes_in_once_after_the_resume(
) {
    let context = delivered();
    // The Close cannot close the windows: the hand-over is what keeps the message out.
    context.host.refuse_kills.set(true);
    let (project, brief) = briefed(&context);
    let marker = marker_of(brief);
    let taking = context.adapter.deliver_holds.hold(move |args| {
        args[0]["text"]
            .as_str()
            .is_some_and(|text| text.contains(&marker))
    });
    context.pass().unwrap();
    context.settle();
    context.pass().unwrap();
    context.settle();
    // The Close waits for the hand-over under way; the hand-over is turned away.
    let closing = context.begin_close_project(project);
    context.settle();
    taking.open();
    // It could not close the windows, as the host refused: the project is closed all the same.
    let refused = context.finish(closing).unwrap_err();
    assert!(
        format!("{refused:?}").contains("window-kept"),
        "{refused:?}"
    );
    context.settle();
    assert_eq!(
        context
            .adapter
            .agent("zeus")
            .items
            .iter()
            .filter(|item| item.text.contains(&marker_of(brief)))
            .count(),
        0,
        "nothing went in"
    );
    assert_eq!(context.message(brief).state, "queued", "back in its queue");
    context.host.refuse_kills.set(false);
    context.resume_project(project).unwrap();
    passes(&context, 4);
    assert_eq!(context.message(brief).state, "delivered");
    assert_eq!(
        context
            .adapter
            .agent("zeus")
            .items
            .iter()
            .filter(|item| item.text.contains(&marker_of(brief)))
            .count(),
        1,
        "it went in once"
    );
    context.close();
}

#[test]
fn a_first_message_whose_project_closes_before_any_hand_over_is_given_back_and_goes_in_once_after_the_resume(
) {
    // Calliope's review of T-432 (T-520) and the tester's T-534, bug 2: a Close in the
    // first seconds of a Claude window's start failed its task, and Resume brought
    // nothing back. Zeus's window is up and not ready yet, so no hand-over is tried
    // before the Close.
    let context = delivered();
    let ready = Rc::new(Cell::new(false));
    let asked = Rc::clone(&ready);
    *context.adapter.ready.borrow_mut() = Some(Rc::new(move || {
        Ok(if asked.get() {
            Readiness::Ready
        } else {
            Readiness::Held(Held::Because("starting".to_owned()))
        })
    }));
    let (project, brief) = briefed(&context);
    passes(&context, 3);
    assert_eq!(context.message(brief).state, "delivering");
    assert_eq!(times_handed(&context, SEAM, brief), 0, "nothing tried yet");
    context.close_project(project).unwrap();
    context.settle();
    assert_eq!(context.message(brief).state, "queued", "back in its queue");
    assert_ne!(context.task(project, 1).task.state, "failed");
    let chief = context.id(project, "chief");
    assert!(
        !context
            .inbox(chief)
            .iter()
            .any(|message| message.body.contains("failed")),
        "nobody hears it failed: {:?}",
        context.inbox(chief)
    );
    ready.set(true);
    context.resume_project(project).unwrap();
    passes(&context, 4);
    assert_eq!(context.message(brief).state, "delivered");
    assert_eq!(times_handed(&context, SEAM, brief), 1, "handed once");
    assert_eq!(context.task(project, 1).task.state, "working");
    context.close();
}

#[test]
fn a_first_message_held_for_what_the_human_typed_does_not_spend_the_launch_s_time() {
    // Calliope's review of T-432 (T-520): the human's unsent words held the brief while
    // the launch's time ran, and the task failed for a window never ready.
    let context = delivered();
    let typed = Rc::new(Cell::new(true));
    let asked = Rc::clone(&typed);
    *context.adapter.ready.borrow_mut() = Some(Rc::new(move || {
        Ok(if asked.get() {
            Readiness::Held(Held::Unsent)
        } else {
            Readiness::Ready
        })
    }));
    let (project, brief) = briefed(&context);
    passes(&context, 3);
    // Twice the launch's time, a step every ten seconds.
    for _ in 0..24 {
        context.advance(10_000);
        passes(&context, 1);
    }
    assert_eq!(context.message(brief).state, "delivering", "owed still");
    assert_ne!(context.task(project, 1).task.state, "failed");
    typed.set(false);
    passes(&context, 2);
    assert_eq!(times_handed(&context, SEAM, brief), 1);
    assert_eq!(context.message(brief).state, "delivered");
    context.close();
}

#[test]
fn a_window_that_ends_by_itself_before_its_first_message_is_handed_over_fails_its_brief() {
    // Its project open, the window went on its own (a harness that dies as it starts):
    // the brief is no message the human's Close held back, and is not given back to be
    // launched again and again.
    let context = delivered();
    *context.adapter.ready.borrow_mut() = Some(Rc::new(|| {
        Ok(Readiness::Held(Held::Because("starting".to_owned())))
    }));
    let (project, brief) = briefed(&context);
    passes(&context, 3);
    assert_eq!(context.message(brief).state, "delivering");
    context.exit("zeus");
    passes(&context, 3);
    assert_eq!(times_handed(&context, SEAM, brief), 0);
    assert_eq!(context.message(brief).state, "failed");
    assert_eq!(context.task(project, 1).task.state, "failed");
    context.close();
}

#[test]
fn a_monitor_of_the_turn_a_first_message_began_is_left_the_rest_before_the_hand_over() {
    // Calliope's review of T-432 (T-520), as turn-end's own case for a paste (Calliope on
    // 136a7158): the rest taken as the window is first seen at rest comes before the
    // first message handed over, and so before every monitor of the turn it begins.
    let context = delivered();
    context.host.exits_showing(Some(0), None, None);
    let (project, brief) = briefed(&context);
    passes(&context, 3);
    assert_eq!(context.message(brief).state, "delivered");
    let rested = context.background.rests();
    assert!(rested >= 1, "a rest before the hand-over");
    context.background.commands_run(Some(0));
    context.adapter.answer_leaving_monitors(
        "zeus",
        "The watch ran out",
        Pending {
            jobs: 1,
            noticed: 1,
            ..Pending::default()
        },
        vec![Monitor {
            id: "ticks".into(),
            description: "ticks".into(),
            source: Source::Command,
            called: Some(1_792_623_750_287),
            notice: Some(Notice {
                end: End::Expired,
                at: Some("2026-10-21T23:03:10.329Z".into()),
            }),
        }],
    );
    passes(&context, 2);
    assert_eq!(context.task(project, 1).task.state, "done");
    assert_eq!(
        context.background.looked_since(),
        [Some(u64::try_from(rested - 1).unwrap())]
    );
    context.close();
}

/// The results of T-1 the ledger holds.
fn results(context: &Context, project: i64) -> Vec<String> {
    context
        .task(project, 1)
        .messages
        .into_iter()
        .filter(|message| message.kind == "result")
        .map(|message| message.body)
        .collect()
}

#[test]
fn a_window_the_resume_reopens_yields_no_result_before_its_continuation_is_in_nor_to_finish() {
    // The sweep of T-523 with every window delivered its first message (Calliope's P3-4
    // on T-432, T-520): the Resume's words to go on are the first message of the window
    // it opens again on the conversation, handed over once that window is up. Read at
    // rest before then, its record ended on the answer written before the Close, after
    // the brief, the latest message delivered: the step and Finish took that answer for
    // the result (T-405's L10, which the words on the command line had kept away).
    let context = delivered();
    context.adapter.resumes_records.set(true);
    let (project, _brief) = briefed(&context);
    passes(&context, 3);
    assert_eq!(context.task(project, 1).task.state, "working");
    let half = context
        .adapter
        .item(Role::Assistant, "The lexer is half done");
    context.adapter.with("zeus", |agent| {
        agent.items.push(half);
        agent.settled = false;
    });
    context.close_project(project).unwrap();
    // The window the Resume opens again is up and at rest, not ready for its words yet.
    let ready = Rc::new(Cell::new(false));
    let asked = Rc::clone(&ready);
    *context.adapter.ready.borrow_mut() = Some(Rc::new(move || {
        Ok(if asked.get() {
            Readiness::Ready
        } else {
            Readiness::Held(Held::Because("starting".to_owned()))
        })
    }));
    context.resume_project(project).unwrap();
    context.adapter.with("zeus", |agent| agent.settled = true);
    passes(&context, 3);
    assert_eq!(context.task(project, 1).task.state, "working");
    assert_eq!(
        results(&context, project),
        Vec::<String>::new(),
        "no result yet"
    );
    let refused = context.finish_task(project, 1, "chief").unwrap_err();
    assert!(refused.to_string().contains("first message"), "{refused}");

    ready.set(true);
    passes(&context, 3);
    assert_eq!(
        results(&context, project),
        Vec::<String>::new(),
        "nor once it is in"
    );
    context.adapter.answer("zeus", "The lexer is done");
    passes(&context, 3);
    assert_eq!(context.task(project, 1).task.state, "done");
    assert_eq!(results(&context, project), ["The lexer is done"]);
    context.close();
}

#[test]
fn a_first_message_handed_over_whose_project_closes_before_its_record_shows_it_is_given_back_and_goes_in_once_after_the_resume(
) {
    // The sweep of T-523 (Calliope's P3-4 on T-432, T-520): the Close came a moment after
    // the brief was pasted, before the window's record had it, and the brief failed its
    // task, as a launch's brief on the command line did before its record showed it
    // (T-534, bug 2). Nothing of it reached the conversation the Resume goes on with.
    let context = delivered();
    let (project, brief) = briefed(&context);
    context.pass().unwrap();
    context.settle();
    // Zeus's window takes the paste, and its record has not shown it yet.
    context.adapter.with("zeus", |agent| agent.arrive = false);
    passes(&context, 1);
    assert_eq!(times_handed(&context, SEAM, brief), 1);
    assert_eq!(context.message(brief).state, "delivering");
    context.close_project(project).unwrap();
    context.settle();
    assert_eq!(context.message(brief).state, "queued", "back in its queue");
    assert_ne!(context.task(project, 1).task.state, "failed");
    context.adapter.with("zeus", |agent| agent.arrive = true);
    context.resume_project(project).unwrap();
    passes(&context, 4);
    assert_eq!(context.message(brief).state, "delivered");
    assert_eq!(
        times_handed(&context, SEAM, brief),
        2,
        "once more, after the Resume"
    );
    assert_eq!(context.task(project, 1).task.state, "working");
    context.close();
}

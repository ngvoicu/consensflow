//! A launch's first message that its window is delivered once it is up (T-432): a
//! Claude Code, Pi or Codex window opens with nothing on its command line, which
//! every process on the machine reads, and the step hands the message over by the
//! window's own `deliver` once the window is past any question of trusting its
//! folder, has drawn and is at rest. None of these tests is held to a Node
//! recording: Node's adapters put the message on the command line.

use std::cell::Cell;
use std::rc::Rc;

use cf_engine::delivery_text::marker_of;
use cf_engine::testing::{Context, TRUST_PROMPT, TRUST_QUESTION};
use cf_engine::SwitchWhen;
use cf_harness::contract::{FirstMessage, Held, Readiness};
use cf_harness::records::{End, Monitor, Notice, Pending, Role, Source};
use cf_proto::trace::{Traced, WindowEvent};
use serde_json::json;

use crate::chiefs::{handoffs_of, to};
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
                        ..
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
        context.inbox(chief).iter().any(|message| message
            .body
            .contains("never ready for its first message: starting")),
        "the chief hears why, in its harness's words for what it waited on"
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

/// The note the human is given about a chief's window that shows no prompt.
const NO_PROMPT: &str =
    "The chief's window has shown no prompt for 2 minutes; its handoff is waiting. Look at its terminal.";

/// A project whose Claude chief has spoken, switched to another Claude chief whose
/// window may ask whether to trust its folder and shows `screen`: its handoff.
fn switched_to_a_chief_showing(context: &Context, screen: &str) -> (i64, i64) {
    let project = context.with_staff(&["zeus"]).id;
    passes(context, 1);
    context.adapter.answer("chief", "Hello");
    passes(context, 1);
    context.adapter.asks_to_trust("/work/app", true);
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 5000, "tail": [screen] }));
    context
        .switch_chief(project, to("claude-code", "apollo"), SwitchWhen::Now, false)
        .unwrap();
    context.settle();
    let handoff = handoffs_of(context, project).remove(0).id;
    (project, handoff)
}

/// The notes the human was given that say a chief's window shows no prompt.
fn no_prompt_notes(context: &Context, project: i64) -> usize {
    let human = context.id(project, "human");
    context
        .inbox(human)
        .iter()
        .filter(|message| message.body.contains("shown no prompt"))
        .count()
}

/// `seconds` of the engine's time, a step every ten seconds, and a look at the window's
/// screen at each (the trust watch's sleep, which only the test ends).
fn wait(context: &Context, seconds: i64) {
    for _ in 0..seconds / 10 {
        context.advance(10_000);
        context.timers.fire_next();
        context.settle();
        passes(context, 1);
    }
}

#[test]
fn a_chief_whose_window_shows_no_prompt_for_two_minutes_has_the_human_told_once_and_its_handoff_waits(
) {
    // T-629: since a window owed its first message is watched until its prompt (T-523),
    // a chief's launch, which is never given up, waits for good on a prompt its reader
    // does not know. It is not given up and nothing is pasted blind: the human hears once.
    let context = delivered();
    let (project, handoff) = switched_to_a_chief_showing(&context, "Claude Code is starting");
    wait(&context, 110);
    assert_eq!(
        no_prompt_notes(&context, project),
        0,
        "not before 2 minutes"
    );
    wait(&context, 20);
    assert_eq!(
        no_prompt_notes(&context, project),
        1,
        "told after 2 minutes"
    );
    let human = context.id(project, "human");
    assert!(
        context
            .inbox(human)
            .iter()
            .any(|message| message.body == NO_PROMPT),
        "{:?}",
        context.inbox(human)
    );
    wait(&context, 300);
    assert_eq!(no_prompt_notes(&context, project), 1, "and once");
    assert_eq!(
        times_handed(&context, SEAM, handoff),
        0,
        "nothing pasted blind"
    );
    assert_eq!(context.message(handoff).state, "delivering", "it waits");
    // Its prompt comes: the handoff goes in.
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 5000, "tail": [TRUST_PROMPT] }));
    wait(&context, 10);
    passes(&context, 2);
    assert_eq!(times_handed(&context, SEAM, handoff), 1);
    assert_eq!(no_prompt_notes(&context, project), 1);
    context.close();
}

#[test]
fn a_chief_whose_window_shows_its_prompt_within_two_minutes_has_the_human_told_nothing() {
    let context = delivered();
    let (project, handoff) = switched_to_a_chief_showing(&context, "Claude Code is starting");
    wait(&context, 60);
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 5000, "tail": [TRUST_PROMPT] }));
    wait(&context, 240);
    assert_eq!(times_handed(&context, SEAM, handoff), 1, "handed once up");
    assert_eq!(no_prompt_notes(&context, project), 0);
    context.close();
}

/// The reason of the last activity the trace told of `handle`'s window.
fn last_activity(context: &Context, handle: &str) -> Option<String> {
    context
        .trace
        .lines
        .borrow()
        .iter()
        .rev()
        .find_map(|line| match &line.what {
            Traced::Window {
                participant: Some(participant),
                event: WindowEvent::Activity { reason, .. },
                ..
            } if participant == handle => Some(reason.clone()),
            _ => None,
        })
        .flatten()
}

/// A worker's window that may ask whether to trust its folder, showing `screen`: the
/// brief of T-1 given to zeus.
fn briefed_showing(context: &Context, screen: &str) -> (i64, i64) {
    context.adapter.asks_to_trust("/work/app", true);
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 5000, "tail": [screen] }));
    briefed(context)
}

#[test]
fn a_question_drawn_after_the_window_showed_its_prompt_holds_its_first_message_until_it_is_answered(
) {
    // T-681, the chief's rule: a first message waits until its window is at its prompt,
    // past every question its harness asks as it starts, whatever order they come in and
    // one that comes late. A question drawn after the watch let the window go (its
    // prompt seen) is never pasted into: its Enter would answer it.
    let context = delivered();
    // The window is not ready for a paste yet when the watch sees its prompt.
    let ready = Rc::new(Cell::new(false));
    let asked = Rc::clone(&ready);
    *context.adapter.ready.borrow_mut() = Some(Rc::new(move || {
        Ok(if asked.get() {
            Readiness::Ready
        } else {
            Readiness::Held(Held::Because("starting".to_owned()))
        })
    }));
    let (_project, brief) = briefed_showing(&context, TRUST_PROMPT);
    wait(&context, 30);
    assert_eq!(times_handed(&context, SEAM, brief), 0, "not ready yet");
    // A question is drawn, and then the window reads ready for a paste.
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 5000, "tail": [TRUST_QUESTION] }));
    ready.set(true);
    passes(&context, 2);
    assert_eq!(
        times_handed(&context, SEAM, brief),
        0,
        "nothing into the question"
    );
    assert_eq!(context.message(brief).state, "delivering", "it waits");
    assert_eq!(
        last_activity(&context, "zeus").as_deref(),
        Some("the window asks whether to trust its folder"),
        "the watch took the question up again"
    );
    // Answered, the window at its prompt: the brief goes in once.
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 5000, "tail": [TRUST_PROMPT] }));
    wait(&context, 20);
    assert_eq!(times_handed(&context, SEAM, brief), 1);
    assert_eq!(context.message(brief).state, "delivered");
    context.close();
}

#[test]
fn a_question_that_comes_after_half_a_minute_holds_a_workers_first_message_until_it_is_answered() {
    // The half-minute case of the rig (T-523's P3-1), on the engine's fake: a window owed
    // its first message is not let go at 30 s for having asked nothing.
    let context = delivered();
    let (project, brief) = briefed_showing(&context, "Claude Code is starting");
    // Past 30 s by more than a look of the watch, each step ending one sleep of its own.
    wait(&context, 90);
    assert_eq!(times_handed(&context, SEAM, brief), 0, "not let go at 30 s");
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 5000, "tail": [TRUST_QUESTION] }));
    wait(&context, 20);
    assert_eq!(
        times_handed(&context, SEAM, brief),
        0,
        "nothing into the question"
    );
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 5000, "tail": [TRUST_PROMPT] }));
    wait(&context, 20);
    assert_eq!(times_handed(&context, SEAM, brief), 1);
    assert_ne!(context.task(project, 1).task.state, "failed");
    context.close();
}

/// A brief that names a picture by its path on a line of its own.
const BRIEF_WITH_A_PICTURE: &str = "Parser\n/work/images/desk.png";

/// T-1, its brief naming a picture, given to zeus, whose window attaches the images a
/// paste names (as Claude Code does) and is handed the brief once up: zeus at work on it.
fn at_work_on_a_brief_with_a_picture(context: &Context) -> (i64, i64) {
    context.adapter.attaches_images.set(true);
    let project = context.with_staff(&["zeus"]).id;
    let brief = context
        .give(project, "zeus", BRIEF_WITH_A_PICTURE)
        .message
        .expect("its brief")
        .id;
    passes(context, 3);
    assert_eq!(context.message(brief).state, "delivered");
    assert_eq!(context.task(project, 1).task.state, "working");
    (project, brief)
}

#[test]
fn a_brief_handed_over_once_up_is_a_paste_whose_pictures_are_attached_and_its_answer_is_the_result()
{
    // T-681, the chief's rule: a pasted brief's image paths are attached like any paste's.
    // The window's record keeps the brief behind its placeholder, then the sources as the
    // harness's own item (Claude Code's companion record), which is no message of the
    // user's: the answer after them is the task's result.
    let context = delivered();
    let (project, brief) = at_work_on_a_brief_with_a_picture(&context);
    let items = context.adapter.agent("zeus").items;
    let at = items.len() - 2;
    assert!(
        items[at]
            .text
            .starts_with(&format!("[Image #1][ConsensFlow m-{brief} ")),
        "{:?}",
        items[at].text
    );
    assert!(!items[at].text.contains("desk.png"), "{:?}", items[at].text);
    assert_eq!(
        &*items[at + 1].text,
        "[Image: source: /work/images/desk.png]"
    );
    assert_eq!(
        (items[at].role, items[at + 1].role),
        (Role::User, Role::Custom)
    );
    assert_eq!(times_handed(&context, SEAM, brief), 1, "pasted once");
    context.adapter.answer("zeus", "Parser done");
    passes(&context, 3);
    let task = context.task(project, 1);
    assert_eq!(task.task.state, "done");
    let results: Vec<String> = task
        .messages
        .into_iter()
        .filter(|message| message.kind == "result")
        .map(|message| message.body)
        .collect();
    assert_eq!(results, ["Parser done"]);
    context.close();
}

#[test]
fn a_window_at_work_on_a_brief_whose_pictures_were_attached_is_interrupted_when_its_task_is_cancelled(
) {
    // A stop's look reads the last item of the user's: the companion is the harness's own,
    // so the turn is the brief's, and Escape is pressed into it before the window closes.
    let context = delivered();
    let (project, _brief) = at_work_on_a_brief_with_a_picture(&context);
    context.adapter.busy_until_interrupted("zeus");
    passes(&context, 1);
    let pane = context.host.last("zeus").unwrap().pane;
    let before = context.host.inputs().len();
    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 1, "human")
        .unwrap();
    passes(&context, 1);
    let escapes = context.host.inputs()[before..]
        .iter()
        .filter(|body| body["bytes"] == json!([27]) && body["id"] == json!(pane.id))
        .count();
    assert!(escapes >= 1, "Escape pressed into its turn");
    assert!(
        context.host.killed().contains(&pane),
        "and its window closed"
    );
    context.close();
}

#[test]
fn a_brief_whose_pictures_were_attached_is_kept_when_its_window_takes_it_back_at_a_stop() {
    // Taken back: the turn the brief began is stopped before a word of its answer, and the
    // window has it in its input box again; the companion after it hides nothing of that.
    let context = delivered();
    let (project, brief) = at_work_on_a_brief_with_a_picture(&context);
    context.adapter.busy_until_taken_back("zeus");
    context.pause_task(project, 1);
    passes(&context, 2);
    let thread = context.task(project, 1);
    let kept: Vec<_> = thread
        .messages
        .iter()
        .filter(|message| message.id == brief && message.state == "queued")
        .collect();
    assert_eq!(kept.len(), 1, "{:?}", thread.messages);
    assert_eq!(
        kept[0].reason.as_deref(),
        Some("@zeus stopped before a word of its answer and took it back out of its conversation")
    );
    context.close();
}

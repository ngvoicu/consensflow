//! A meeting with the chief in the engine (3.0.7 chief-meeting, CMT-12, CMT-13 and
//! CMT-17): while the human talks with the chief, nothing of the board is typed into
//! its window, a closed chief window is not opened for what waits, and no wake-up is
//! queued. The Stop lets what was held go in one at a time, each in its own turn, a
//! member's question first. A Switch chief keeps the meeting and still goes, its
//! handoff and the note it asks with among it; the project's Close and the app's quit
//! end it, and what it held goes in at the Resume. A meeting's hold is the ledger's:
//! the engine never holds a message for it, so its clock never starts and the human,
//! who started the meeting, is never told of it at ten minutes; any other hold in that
//! time is told. Node's dispatcher had no meetings, so none of these is held to a
//! Node trace.

use std::cell::Cell;
use std::rc::Rc;

use cf_engine::testing::Context;
use cf_engine::{ActivityState, SwitchWhen};
use cf_ledger::{NewWakeUp, ProjectView, WakeUpUnit, DEFAULT_WAKE_UP_TEXT};
use serde_json::json;

use crate::chiefs::{chief_of, handoffs_of, to, with_codex};
use crate::fixtures::{given, launches_of, Tiers};
use crate::resume::quit_and_start;

const MINUTE: i64 = 60_000;

/// The human starts (`on`) or stops a meeting with the project's chief.
fn meeting(context: &Context, project: i64, on: bool) {
    context
        .ledger
        .borrow_mut()
        .set_meeting(project, on)
        .unwrap();
}

/// How many times `handle`'s windows were given message `id`.
fn headed(context: &Context, handle: &str, id: i64) -> usize {
    given(context, handle)
        .iter()
        .filter(|text| text.starts_with(&format!("[ConsensFlow m-{id} ")))
        .count()
}

/// A project whose chief is at rest, and zeus at work on T-1.
fn at_rest_with_zeus_at_work(context: &Context) -> ProjectView {
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    assert_eq!(
        context
            .dispatcher
            .activity(context.id(project.id, "chief"))
            .state,
        ActivityState::Idle
    );
    project
}

/// The data of the project's `project.meeting` events, oldest first.
fn meetings(context: &Context, project: i64) -> Vec<serde_json::Value> {
    context
        .ledger
        .borrow()
        .events(project, 0, 5000)
        .unwrap()
        .into_iter()
        .filter(|event| event.kind == "project.meeting")
        .map(|event| event.data)
        .collect()
}

#[test]
fn a_chief_at_rest_is_given_nothing_during_a_meeting_and_the_stop_lets_each_go_in_its_turn_question_first(
) {
    let context = Context::new();
    let project = at_rest_with_zeus_at_work(&context);
    meeting(&context, project.id, true);
    let note = context.note(project.id, "zeus", "chief", "Mind the tests");
    let question = context.ask(project.id, "zeus", "chief", 1, "Which format?");
    for _ in 0..5 {
        context.advance(MINUTE);
        context.pass().unwrap();
    }
    assert_eq!(
        [
            headed(&context, "chief", note.id),
            headed(&context, "chief", question.id)
        ],
        [0, 0],
        "a chief in a meeting is given nothing"
    );
    assert_eq!(context.message(note.id).state, "queued");

    meeting(&context, project.id, false);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        [
            headed(&context, "chief", note.id),
            headed(&context, "chief", question.id)
        ],
        [0, 1],
        "the question first, and one at a time"
    );
    context.adapter.answer("chief", "Answered the question.");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        [
            headed(&context, "chief", note.id),
            headed(&context, "chief", question.id)
        ],
        [1, 1]
    );
}

/// The chief's window did not open at the Resume: it is tried again with the next
/// message that comes for it (`chief_not_up`), and a message the meeting holds opens
/// none, however long it waits. The Stop opens it, with the message first.
#[test]
fn a_closed_chief_window_is_not_opened_for_what_a_meeting_holds_and_the_stop_opens_it() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.close_project(project.id).unwrap();
    context.host.refuse.set(true);
    context.resume_project(project.id).unwrap();
    let chief = context.id(project.id, "chief");
    assert_eq!(context.dispatcher.pane(chief), None, "it did not open");
    meeting(&context, project.id, true);
    let launches = launches_of(&context, "chief").len();
    let note = context.note(project.id, "zeus", "chief", "Mind the tests");
    for _ in 0..10 {
        context.advance(MINUTE);
        context.pass().unwrap();
    }
    assert_eq!(
        launches_of(&context, "chief").len(),
        launches,
        "no window opens for a held message"
    );
    assert_eq!(context.dispatcher.pane(chief), None);
    meeting(&context, project.id, false);
    context.advance(MINUTE);
    context.pass().unwrap();
    assert_eq!(launches_of(&context, "chief").len(), launches + 1);
    let launch = context.adapter.prepared().last().cloned().unwrap();
    assert!(
        launch["message"]
            .as_str()
            .is_some_and(|first| first.ends_with("note from @zeus]\nMind the tests")),
        "{launch}"
    );
    assert_eq!(context.message(note.id).attempts, 1);
}

#[test]
fn no_wake_up_is_queued_or_given_during_a_meeting_and_the_calls_count_again_from_its_end() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.pass().unwrap();
    context
        .ledger
        .borrow_mut()
        .set_wake_up(
            project.id,
            &NewWakeUp {
                on: true,
                every: 5,
                unit: WakeUpUnit::Minutes,
                text: None,
            },
        )
        .unwrap();
    meeting(&context, project.id, true);
    for _ in 0..30 {
        context.advance(MINUTE);
        context.pass().unwrap();
    }
    let woken = |context: &Context| {
        given(context, "chief")
            .iter()
            .filter(|text| text.contains(DEFAULT_WAKE_UP_TEXT))
            .count()
    };
    assert_eq!(woken(&context), 0, "none during the meeting");
    meeting(&context, project.id, false);
    context.pass().unwrap();
    assert_eq!(woken(&context), 0, "nor at its Stop");
    for _ in 0..5 {
        context.advance(MINUTE);
        context.pass().unwrap();
    }
    context.pass().unwrap();
    assert_eq!(woken(&context), 1, "an interval after the Stop");
}

#[test]
fn a_switch_chief_now_during_a_meeting_gives_the_new_chief_its_handoff_and_keeps_the_meeting() {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.answer("chief", "Hello");
    context.pass().unwrap();
    meeting(&context, project.id, true);
    let note = context.note(project.id, "zeus", "chief", "Mind the tests");
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        chief_of(&context, project.id).harness.as_deref(),
        Some("codex")
    );
    let launched = context.codex.prepared().last().cloned().unwrap();
    assert!(
        launched["message"]
            .as_str()
            .is_some_and(|first| first.contains("You are the chief now")),
        "{launched}"
    );
    assert!(context.project(project.id).meeting.is_some(), "kept");
    assert_eq!(context.message(note.id).state, "queued", "still held");
    let held: Vec<i64> = context
        .ledger
        .borrow()
        .held_by_meeting(project.id)
        .unwrap()
        .iter()
        .map(|message| message.id)
        .collect();
    assert_eq!(held, vec![note.id]);
}

/// The new chief's window could not take its handoff, which went back to the queue:
/// it opens again with it at its time, the meeting on. A meeting that held the handoff
/// would keep the project chiefless until its Stop.
#[test]
fn a_new_chief_that_could_not_take_its_handoff_opens_again_with_it_during_a_meeting() {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.answer("chief", "Hello");
    context.pass().unwrap();
    let chief = context.id(project.id, "chief");
    let slow = Rc::new(Cell::new(true));
    *context.codex.started.borrow_mut() = Some(Rc::new(move || {
        if !slow.replace(false) {
            return Ok(());
        }
        Err("the Codex broker never named the thread it opened".to_owned())
    }));
    meeting(&context, project.id, true);
    context.codex.forgets_first_messages.set(true);
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    context.codex.forgets_first_messages.set(false);
    assert_eq!(context.dispatcher.pane(chief), None);
    let handoff = handoffs_of(&context, project.id).remove(0);
    assert_eq!(handoff.state, "queued");
    context.advance(5_000);
    context.pass().unwrap();
    assert_eq!(context.codex.prepared().len(), 2, "opened again");
    let launch = context.codex.prepared().last().cloned().unwrap();
    assert!(
        launch["message"]
            .as_str()
            .is_some_and(|first| first.contains("You are the chief now")),
        "{launch}"
    );
    assert!(context.project(project.id).meeting.is_some(), "kept");
}

#[test]
fn a_switch_chief_after_the_turn_during_a_meeting_asks_where_things_stand_and_goes() {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.answer("chief", "Hello");
    context.pass().unwrap();
    meeting(&context, project.id, true);
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Turn, true)
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    let said = given(&context, "chief");
    assert!(
        said.last()
            .is_some_and(|last| last.contains("Write down where things stand")),
        "{said:?}"
    );
    context
        .adapter
        .answer("chief", "Where things stand: the parser is with zeus.");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        chief_of(&context, project.id).harness.as_deref(),
        Some("codex")
    );
    assert!(context.project(project.id).meeting.is_some(), "kept");
}

#[test]
fn a_close_ends_the_meeting_and_the_resume_gives_what_it_held_question_first() {
    let context = Context::new();
    let project = at_rest_with_zeus_at_work(&context);
    meeting(&context, project.id, true);
    let note = context.note(project.id, "zeus", "chief", "Mind the tests");
    let question = context.ask(project.id, "zeus", "chief", 1, "Which format?");
    context.pass().unwrap();
    context.close_project(project.id).unwrap();
    assert_eq!(context.project(project.id).meeting, None);
    assert_eq!(
        meetings(&context, project.id).last(),
        Some(&json!({ "on": false, "ended": "closed", "held": 2 }))
    );
    context.resume_project(project.id).unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        [
            headed(&context, "chief", note.id),
            headed(&context, "chief", question.id)
        ],
        [0, 1],
        "the question first, and one at a time"
    );
    context.adapter.answer("chief", "Answered the question.");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(headed(&context, "chief", note.id), 1);
}

#[test]
fn the_apps_quit_ends_the_meeting_and_the_resume_after_the_start_gives_what_it_held() {
    let context = Context::new();
    let project = at_rest_with_zeus_at_work(&context);
    meeting(&context, project.id, true);
    let note = context.note(project.id, "zeus", "chief", "Mind the tests");
    let question = context.ask(project.id, "zeus", "chief", 1, "Which format?");
    context.pass().unwrap();
    let after = quit_and_start(&context);
    assert_eq!(context.project(project.id).meeting, None);
    assert_eq!(
        meetings(&context, project.id).last(),
        Some(&json!({ "on": false, "ended": "quit", "held": 2 }))
    );
    after.resume_project(project.id).unwrap();
    for _ in 0..3 {
        after.pass().unwrap();
    }
    assert_eq!(
        [
            headed(&context, "chief", note.id),
            headed(&context, "chief", question.id)
        ],
        [0, 1]
    );
}

/// The notes the human was told of a message held long, of `window`'s.
fn told_held(tiers: &Tiers, window: &str) -> Vec<String> {
    tiers
        .notes("human")
        .into_iter()
        .filter(|note| note.contains("has waited") && note.contains(&format!("@{window}'s")))
        .collect()
}

/// The chief's decision of 2026-10-29: the human started the meeting, so being told
/// that the chief's messages are held would only nag. A result for a chief whose
/// harness says it is at work would be held and told at ten minutes; in a meeting it
/// is held by the meeting, and nothing is told, however long it goes on.
#[test]
fn a_meeting_longer_than_ten_minutes_tells_the_human_nothing() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    meeting(&context, tiers.project.id, true);
    context.adapter.with("chief", |agent| agent.at_work = true);
    context.adapter.answer("zeus", "Parser done.");
    context.pass().unwrap();
    context.pass().unwrap();
    for _ in 0..30 {
        context.advance(MINUTE);
        context.pass().unwrap();
    }
    assert_eq!(told_held(&tiers, "chief"), Vec::<String>::new());
    assert_eq!(context.dispatcher.held_back(tiers.id("chief")), None);
}

/// Any other hold in a meeting is told: an answer held at a worker's window whose
/// harness says it is at work reaches the human at ten minutes, the meeting on.
#[test]
fn another_hold_during_a_meeting_is_still_told_at_ten_minutes() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let worker = tiers.assignee(1);
    let asked = context.ask(tiers.project.id, &worker, "chief", 1, "Which format?");
    context.adapter.answer(&worker, "Asked the chief.");
    context.pass().unwrap();
    context.pass().unwrap();
    meeting(&context, tiers.project.id, true);
    context.adapter.with(&worker, |agent| agent.at_work = true);
    let chief = tiers.id("chief");
    context
        .ledger
        .borrow_mut()
        .answer(asked.id, chief, Some(&json!("JSON")), None)
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    for _ in 0..11 {
        context.advance(MINUTE);
        context.pass().unwrap();
    }
    let notes = told_held(&tiers, &worker);
    assert_eq!(notes.len(), 1, "{:?}", tiers.notes("human"));
}

/// A meeting's hold and a chief's at-work hold meet one message (D2b, T-996, as Calliope's
/// T-987 keeps them apart): a result held at a chief whose window reads at work (T-958)
/// lends the meeting none of its time, and the meeting none of its own to the hold after
/// it. The result left the head when the meeting began, so its hold went with it; at the
/// Stop it is the head again, held at work on a clock of its own, and told once, ten
/// minutes after the Stop: never at ten minutes from its arrival, which fell in the
/// meeting, nor from the time before the meeting and after the Stop together.
#[test]
fn a_result_held_at_work_and_then_by_a_meeting_is_told_once_ten_minutes_after_the_stop() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let chief = tiers.id("chief");
    context.adapter.with("chief", |agent| agent.settled = false);
    context.adapter.answer("zeus", "Parser done.");
    context.pass().unwrap();
    context.pass().unwrap();
    let result = context
        .inbox(chief)
        .into_iter()
        .find(|message| message.kind == "result")
        .unwrap()
        .id;
    assert_eq!(
        context.dispatcher.held_back(chief).map(|held| held.message),
        Some(result),
        "held at work from the first look"
    );
    for _ in 0..6 {
        context.advance(MINUTE);
        context.pass().unwrap();
    }
    meeting(&context, tiers.project.id, true);
    context.pass().unwrap();
    assert_eq!(
        context.dispatcher.held_back(chief),
        None,
        "the meeting holds it, and its at-work hold went with it"
    );
    for _ in 0..30 {
        context.advance(MINUTE);
        context.pass().unwrap();
    }
    assert_eq!(
        told_held(&tiers, "chief"),
        Vec::<String>::new(),
        "not in the meeting"
    );
    meeting(&context, tiers.project.id, false);
    context.pass().unwrap();
    let stopped = context.dispatcher.held_back(chief).unwrap();
    assert_eq!(stopped.message, result, "held at work again at the Stop");
    for _ in 0..9 {
        context.advance(MINUTE);
        context.pass().unwrap();
    }
    assert_eq!(
        told_held(&tiers, "chief"),
        Vec::<String>::new(),
        "six minutes before the meeting and nine after it are not ten on one clock"
    );
    context.advance(MINUTE);
    context.pass().unwrap();
    let notes = told_held(&tiers, "chief");
    assert_eq!(notes.len(), 1, "{:?}", tiers.notes("human"));
    assert!(
        notes[0].starts_with(&format!("m-{result}, ")),
        "{}",
        notes[0]
    );
    assert!(
        notes[0].contains("has waited 10 minutes for @chief's window"),
        "{}",
        notes[0]
    );
    assert!(notes[0].contains("reads at work"), "{}", notes[0]);
    for _ in 0..30 {
        context.advance(MINUTE);
        context.pass().unwrap();
    }
    assert_eq!(told_held(&tiers, "chief").len(), 1, "told once");
    context.adapter.with("chief", |agent| agent.settled = true);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        headed(&context, "chief", result),
        1,
        "it goes in at the rest"
    );
}

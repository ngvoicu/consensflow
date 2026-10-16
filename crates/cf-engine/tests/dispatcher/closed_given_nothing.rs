//! A closed project's windows are handed nothing, whatever wait the Close comes
//! in: a launch whose project closed while its window was prepared opens none,
//! a first message a window takes once it is up goes in only while its project
//! is open, a delivery is handed over only if its project is open at the moment
//! of the hand-over, after every wait of its adapter's own, and no key that sends
//! goes into a window the Close could not close. What was not handed over waits
//! for the Resume, with its attempt; a pause note withdrawn on its way keeps its
//! warning. The Resume brings back only the work still at work once it has read
//! each window's record, and waits for every kill of the Close, whatever failed
//! (both reviews of 2026-10-16, round 4). Node's dispatcher had no such Close, so
//! none of these is held to a Node trace.

use std::cell::Cell;
use std::rc::Rc;

use cf_engine::testing::{Context, Gate};
use cf_engine::SwitchWhen;
use cf_harness::contract::{Admission, Readiness};
use cf_harness::records::Role;
use cf_ledger::{continuation, LedgerError, MessageView, NewNote};
use serde_json::json;

use crate::chiefs::{handoffs_of, to, with_codex};
use crate::deliveries::enters;
use crate::fixtures::{given, native_of};
use crate::resume::{at_work, cut_notes, opened};

/// The words a pause says of a window's program the Close could not end.
const SURVIVOR: &str =
    "@zeus-amber-pine's window closed, but its program (process 4242) could not be ended and may still run";

/// The handoff of the project's latest switch, which no switch since withdrew.
fn handoff(context: &Context, project: i64) -> MessageView {
    handoffs_of(context, project)
        .into_iter()
        .find(|message| message.state != "cancelled")
        .expect("the handoff")
}

/// How many times the newest window of the Codex chief was given its handoff.
fn handoffs_given(context: &Context) -> usize {
    context
        .codex
        .agent("chief")
        .items
        .iter()
        .filter(|item| item.role == Role::User && item.text.contains("You are the chief now"))
        .count()
}

/// A project whose chief, on Claude, has answered once, and whose staff is zeus.
fn chief_at_rest(context: &Context) -> i64 {
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.answer("chief", "Hello");
    context.pass().unwrap();
    project.id
}

#[test]
fn a_chief_launch_prepared_while_the_project_closes_opens_no_window_and_its_handoff_goes_in_once_after_the_resume(
) {
    let context = with_codex();
    let project = chief_at_rest(&context);
    // The new chief's window waits in its preparation while the human closes the project.
    let preparing = context.codex.prepare_holds.hold(|_| true);
    context
        .switch_chief(project, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    let windows = opened(&context).len();
    let closing = context.begin_close_project(project);
    context.settle();
    preparing.open();
    context.finish(closing).unwrap();

    assert_eq!(
        opened(&context).len(),
        windows,
        "no window opens in a closed project"
    );
    let waiting = handoff(&context, project);
    assert_eq!(
        (waiting.state.as_str(), waiting.attempts),
        ("queued", 0),
        "the handoff waits for the Resume, never handed over"
    );

    context.resume_project(project).unwrap();
    context.pass().unwrap();
    assert_eq!(opened(&context)[windows..], ["p1-chief"]);
    assert_eq!(handoffs_given(&context), 1);
}

/// OpenCode's shape: the first message goes in through the window's own server
/// once it is up, after a wait of its own.
#[test]
fn a_first_message_a_window_takes_once_it_is_up_is_not_given_when_the_project_closes_first_and_goes_in_once_after_the_resume(
) {
    let context = with_codex();
    context.codex.seeds.set(true);
    let project = chief_at_rest(&context);
    let starting = context.codex.start_holds.hold(|_| true);
    context
        .switch_chief(project, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    context.settle();
    let closing = context.begin_close_project(project);
    context.settle();
    starting.open();
    context.finish(closing).unwrap();

    assert_eq!(handoffs_given(&context), 0, "the window was given nothing");
    let waiting = handoff(&context, project);
    assert_eq!((waiting.state.as_str(), waiting.attempts), ("queued", 0));
    assert!(
        context
            .inbox(context.id(project, "human"))
            .iter()
            .all(|message| !message.body.contains("could not start")),
        "a launch its project's Close stopped is no launch that failed"
    );

    context.resume_project(project).unwrap();
    for _ in 0..2 {
        context.pass().unwrap();
    }
    assert_eq!(handoffs_given(&context), 1);
    assert_eq!(context.message(waiting.id).state, "delivered");
}

/// Claude's keys that clear its input box, and the pane's claim a native send
/// waits for (Codex, OpenCode, Pi), come after the engine's last look at the
/// project: the hand-over itself asks.
#[test]
fn a_delivery_whose_adapter_still_waits_when_the_project_closes_hands_nothing_over_and_waits_for_the_resume(
) {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let waiting = context.note(project, "human", "chief", "Any news?");
    let handing = context.adapter.deliver_holds.hold(|_| true);
    context.pass().unwrap();
    assert_eq!(context.message(waiting.id).state, "delivering");
    let closing = context.begin_close_project(project);
    context.settle();
    handing.open();
    context.finish(closing).unwrap();

    let waiting = context.message(waiting.id);
    assert_eq!((waiting.state.as_str(), waiting.attempts), ("queued", 0));
    assert!(
        given(&context, "chief")
            .iter()
            .all(|text| !text.contains("Any news?")),
        "nothing handed over"
    );

    context.resume_project(project).unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        given(&context, "chief")
            .iter()
            .filter(|text| text.contains("Any news?"))
            .count(),
        1
    );
}

/// Astra's review of 2026-10-16, round 4: the human paused T-1 while the Resume
/// read zeus's record, and zeus's window opened told to go on, the pause taken
/// for one it had paid.
#[test]
fn a_task_the_human_pauses_while_the_resume_reads_whether_its_turn_was_cut_stays_paused_and_its_window_stays_closed(
) {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = tiers.id("zeus-amber-pine");
    let native = native_of(&context, zeus).expect("its conversation");
    context.close_project(project).unwrap();
    let reading = context
        .adapter
        .record_holds
        .hold(move |asked| asked[0]["conversation"]["nativeSession"] == native.as_str());
    let before = opened(&context).len();
    context.resume_project(project).unwrap();
    context.human_pause(project, 1);
    reading.open();
    for _ in 0..3 {
        context.pass().unwrap();
    }

    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["paused", "working"]
    );
    assert_eq!(
        opened(&context)[before..],
        ["p1-chief", "p1-diana-brisk-birch"],
        "zeus's window stays closed"
    );
    assert!(
        cut_notes(&context, zeus).is_empty(),
        "no note told it to go on"
    );
}

/// The same, of the last wait before the launch reads the stops its window pays:
/// a replacement of zeus's role still waits, and the launch asks zeus's record
/// whether the window took it before. The stops are those the Resume read before
/// anything of zeus's return waited.
#[test]
fn a_task_the_human_pauses_while_the_resume_s_launch_reads_its_record_stays_paused_and_its_window_stays_closed(
) {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = tiers.id("zeus-amber-pine");
    let native = native_of(&context, zeus).expect("its conversation");
    context.close_project(project).unwrap();
    context
        .ledger
        .borrow_mut()
        .note_role(
            project,
            "zeus-amber-pine",
            "Your instructions changed.",
            "worker, as it was",
        )
        .unwrap();
    let looking = context.adapter.record_holds.hold(move |asked| {
        asked[0]["conversation"]["nativeSession"] == native.as_str() && asked.get(1).is_none()
    });
    let before = opened(&context).len();
    context.resume_project(project).unwrap();
    context.human_pause(project, 1);
    looking.open();
    for _ in 0..3 {
        context.pass().unwrap();
    }

    assert_eq!(tiers.task(1).task.state, "paused");
    assert_eq!(
        opened(&context)[before..],
        ["p1-chief", "p1-diana-brisk-birch"],
        "zeus's window stays closed"
    );
}

/// Zeus's session holds T-2 as well as T-1, as a ledger written before one task
/// per session may have it, and its newest words are T-2's: what tells the stops
/// of the task its window works on tells T-2's alone.
fn holding_two(context: &Context, project: i64, zeus: i64) {
    context
        .ledger
        .borrow_mut()
        .write_raw(&format!(
            "UPDATE task SET assignee_id = {zeus}, pool = NULL, tier = NULL
               WHERE project_id = {project} AND number = 2;
             UPDATE message SET recipient_id = {zeus}
               WHERE kind = 'task'
                 AND task_id = (SELECT id FROM task WHERE project_id = {project} AND number = 2);"
        ))
        .unwrap();
    let stop = context.ledger.borrow().stop_of(zeus).unwrap();
    assert_eq!(stop.map(|stop| stop.number), Some(2));
}

/// The continuations zeus's window was given, in their own words.
fn continuations_given(context: &Context) -> Vec<String> {
    given(context, "zeus-amber-pine")
        .iter()
        .flat_map(|text| text.split('\n'))
        .filter(|line| line.starts_with("ConsensFlow stopped while you were working on "))
        .map(str::to_owned)
        .collect()
}

/// Sol's review of 2026-10-16, round 5: the Resume read the stops of T-2 alone,
/// and the human's pause of T-1 while it read zeus's record was taken for
/// nothing: the window opened told to go on with both. Round 6: it comes back
/// for T-2, which the Close paused, and is told of T-2 alone.
#[test]
fn a_task_the_human_pauses_while_the_resume_reads_the_record_of_a_session_with_two_stays_paused_and_its_window_comes_back_for_the_other(
) {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = tiers.id("zeus-amber-pine");
    let native = native_of(&context, zeus).expect("its conversation");
    context.close_project(project).unwrap();
    holding_two(&context, project, zeus);
    let reading = context
        .adapter
        .record_holds
        .hold(move |asked| asked[0]["conversation"]["nativeSession"] == native.as_str());
    let before = opened(&context).len();
    context.resume_project(project).unwrap();
    context.human_pause(project, 1);
    reading.open();
    for _ in 0..3 {
        context.pass().unwrap();
    }

    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["paused", "working"]
    );
    assert_eq!(
        opened(&context)[before..],
        ["p1-chief", "p1-zeus-amber-pine"]
    );
    assert_eq!(continuations_given(&context), [continuation(&[2])]);
    // The Resume read the work again and gave it in its own launch: no launch
    // went on the work as it was and gave the continuation back.
    let note = cut_notes(&context, zeus).pop().expect("the continuation");
    assert_eq!(sent_back(&context, project, note.id), 0);
}

/// The same where the human paused T-2, the task whose stops zeus's window pays:
/// the window as the Resume read it comes no more, and the Resume reads its work
/// again and brings it back for T-1.
#[test]
fn a_task_the_human_pauses_while_the_resume_reads_the_record_of_a_session_with_two_is_read_again_and_the_window_comes_back_for_the_other(
) {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = tiers.id("zeus-amber-pine");
    let native = native_of(&context, zeus).expect("its conversation");
    context.close_project(project).unwrap();
    holding_two(&context, project, zeus);
    let reading = context
        .adapter
        .record_holds
        .hold(move |asked| asked[0]["conversation"]["nativeSession"] == native.as_str());
    let before = opened(&context).len();
    context.resume_project(project).unwrap();
    context.human_pause(project, 2);
    reading.open();
    for _ in 0..3 {
        context.pass().unwrap();
    }

    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["working", "paused"]
    );
    assert_eq!(
        opened(&context)[before..],
        ["p1-chief", "p1-zeus-amber-pine"]
    );
    assert_eq!(continuations_given(&context), [continuation(&[1])]);
}

/// How often `message` was sent back to its queue.
fn sent_back(context: &Context, project: i64, message: i64) -> usize {
    context
        .ledger
        .borrow()
        .events(project, 0, 1000)
        .unwrap()
        .iter()
        .filter(|event| event.kind == "delivery.retried" && event.data["message"] == message)
        .count()
}

/// Both reviews of 2026-10-16, round 6: the Resume's launch watched T-1 and T-2,
/// and the human paused T-2 while the window was prepared. The launch was over,
/// its continuation went back to its queue whole, and the next launch, which
/// watched nothing, opened the window told to go on with T-2 too.
#[test]
fn a_continuation_a_launch_gave_back_names_only_the_work_still_at_work_when_the_next_launch_begins_it(
) {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = tiers.id("zeus-amber-pine");
    context.close_project(project).unwrap();
    holding_two(&context, project, zeus);
    let whole = continuation(&[1, 2]);
    let preparing = context.adapter.prepare_holds.hold(move |asked| {
        asked[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains(&whole))
    });
    let before = opened(&context).len();
    context.resume_project(project).unwrap();
    context.settle();
    context.human_pause(project, 2);
    preparing.open();
    for _ in 0..3 {
        context.pass().unwrap();
    }

    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["working", "paused"]
    );
    assert_eq!(
        opened(&context)[before..],
        ["p1-chief", "p1-zeus-amber-pine"]
    );
    assert_eq!(continuations_given(&context), [continuation(&[1])]);
}

/// OpenCode's shape: the continuation goes in once the window is up, through its
/// own server, and the human paused T-2 while the window came up. Nothing of it
/// goes in then, and the window, up, is given it naming T-1 alone.
#[test]
fn a_continuation_a_window_takes_once_it_is_up_names_only_the_work_still_at_work_when_it_goes_in() {
    let context = Context::new();
    context.adapter.seeds.set(true);
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = tiers.id("zeus-amber-pine");
    let native = native_of(&context, zeus).expect("its conversation");
    context.close_project(project).unwrap();
    holding_two(&context, project, zeus);
    let starting = context
        .adapter
        .start_holds
        .hold(move |asked| asked[0]["launch"]["nativeSession"] == native.as_str());
    context.resume_project(project).unwrap();
    context.settle();
    context.human_pause(project, 2);
    starting.open();
    for _ in 0..3 {
        context.pass().unwrap();
    }

    assert_eq!(tiers.task(2).task.state, "paused");
    assert_eq!(continuations_given(&context), [continuation(&[1])]);
}

/// A continuation an earlier read left queued, written for work as it was then
/// (a launch that never opened, a quit before it went in): the Resume tells the
/// window once, of its work as it is now.
#[test]
fn the_resume_withdraws_a_continuation_left_queued_for_work_as_it_was_and_tells_the_window_once() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = tiers.id("zeus-amber-pine");
    context.close_project(project).unwrap();
    holding_two(&context, project, zeus);
    let old = context
        .ledger
        .borrow_mut()
        .note(
            project,
            &NewNote {
                from: None,
                to: "zeus-amber-pine".to_owned(),
                task: Some(1),
                body: continuation(&[1, 2]),
            },
        )
        .unwrap();
    // The human's pause takes over the one the Close made: T-2 stays paused.
    context.human_pause(project, 2);
    context.resume_project(project).unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }

    assert_eq!(context.message(old.id).state, "cancelled");
    assert_eq!(continuations_given(&context), [continuation(&[1])]);
}

/// Zeus's window, open and at rest, holding T-1 and T-2, and a continuation
/// queued for it naming both: one a window is delivered, not launched with
/// (behind the note that says its role, or after a first message that went
/// back while the window came up).
fn continuation_for_an_open_window(context: &Context, project: i64, zeus: i64) {
    holding_two(context, project, zeus);
    context
        .adapter
        .with("zeus-amber-pine", |agent| agent.settled = true);
    context
        .ledger
        .borrow_mut()
        .note(
            project,
            &NewNote {
                from: None,
                to: "zeus-amber-pine".to_owned(),
                task: Some(1),
                body: continuation(&[1, 2]),
            },
        )
        .unwrap();
}

/// The same with T-1 paused while the window was prepared, its continuation
/// begun already: T-1 is not the task whose stops the window pays, and what was
/// read of every task it names ends the launch all the same.
#[test]
fn a_continuation_a_launch_gave_back_for_a_task_other_than_the_one_its_window_pays_names_only_the_work_still_at_work(
) {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = tiers.id("zeus-amber-pine");
    context.close_project(project).unwrap();
    holding_two(&context, project, zeus);
    let whole = continuation(&[1, 2]);
    let preparing = context.adapter.prepare_holds.hold(move |asked| {
        asked[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains(&whole))
    });
    let before = opened(&context).len();
    context.resume_project(project).unwrap();
    context.settle();
    context.human_pause(project, 1);
    preparing.open();
    for _ in 0..3 {
        context.pass().unwrap();
    }

    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["paused", "working"]
    );
    assert_eq!(
        opened(&context)[before..],
        ["p1-chief", "p1-zeus-amber-pine"]
    );
    assert_eq!(continuations_given(&context), [continuation(&[2])]);
}

/// The same of a continuation delivered into an open window: the human paused
/// T-2 while the window got ready for it, and it names only the work still at
/// work as its delivery begins.
#[test]
fn a_continuation_delivered_into_an_open_window_names_only_the_work_still_at_work_as_it_is_begun() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = tiers.id("zeus-amber-pine");
    *context.adapter.ready.borrow_mut() = Some(Rc::new(|| Ok(Readiness::Ready)));
    let readying = context
        .adapter
        .ready_holds
        .hold(|asked| asked[0]["pane"]["id"] == "p1-zeus-amber-pine");
    continuation_for_an_open_window(&context, project, zeus);
    context.pass().unwrap();
    context.human_pause(project, 2);
    readying.open();
    for _ in 0..3 {
        context.pass().unwrap();
    }

    assert_eq!(tiers.task(2).task.state, "paused");
    assert_eq!(continuations_given(&context), [continuation(&[1])]);
}

/// And where the human paused T-2 while the adapter took the continuation, its
/// delivery begun: nothing of it goes in, and it goes again, naming T-1 alone.
#[test]
fn a_continuation_whose_task_moved_while_its_adapter_took_it_is_handed_nothing_and_goes_again_narrowed(
) {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = tiers.id("zeus-amber-pine");
    let whole = continuation(&[1, 2]);
    let handing = context.adapter.deliver_holds.hold(move |asked| {
        asked[0]["text"]
            .as_str()
            .is_some_and(|text| text.contains(&whole))
    });
    continuation_for_an_open_window(&context, project, zeus);
    context.pass().unwrap();
    context.human_pause(project, 2);
    handing.open();
    for _ in 0..3 {
        context.pass().unwrap();
    }

    assert_eq!(tiers.task(2).task.state, "paused");
    assert_eq!(continuations_given(&context), [continuation(&[1])]);
}

/// The same, of the launch's own wait for zeus's record, once the Resume found
/// its work unmoved.
#[test]
fn a_task_the_human_pauses_while_the_resume_s_launch_reads_the_record_of_a_session_with_two_stays_paused_and_its_window_comes_back_for_the_other(
) {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = tiers.id("zeus-amber-pine");
    let native = native_of(&context, zeus).expect("its conversation");
    context.close_project(project).unwrap();
    holding_two(&context, project, zeus);
    context
        .ledger
        .borrow_mut()
        .note_role(
            project,
            "zeus-amber-pine",
            "Your instructions changed.",
            "worker, as it was",
        )
        .unwrap();
    let looking = context.adapter.record_holds.hold(move |asked| {
        asked[0]["conversation"]["nativeSession"] == native.as_str() && asked.get(1).is_none()
    });
    let before = opened(&context).len();
    context.resume_project(project).unwrap();
    context.human_pause(project, 1);
    looking.open();
    for _ in 0..4 {
        context.pass().unwrap();
    }

    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["paused", "working"]
    );
    assert_eq!(
        opened(&context)[before..],
        ["p1-chief", "p1-zeus-amber-pine"]
    );
    assert_eq!(continuations_given(&context), [continuation(&[2])]);
}

/// Astra's review of 2026-10-16, round 4: a Close answers at the first window's
/// close that fails, and the Resume went through while zeus's kill was still out.
#[test]
fn a_resume_waits_for_every_kill_of_the_close_though_another_window_s_close_failed() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = context.host.last("zeus").expect("its window").pane;
    let answer = Gate::default();
    *context.host.hold_kill.borrow_mut() = Some((zeus.generation, answer.clone()));
    // Each kill taken is settled by the Close itself, and diana's fails: the
    // ledger fails the pause of T-2.
    context.host.hold_exits.set(true);
    let failing = Rc::new(Cell::new(true));
    let once = Rc::clone(&failing);
    context.ledger.borrow_mut().watch(Box::new(move |call, id| {
        if call == "pause_task" && id == Some(2) && once.replace(false) {
            return Err(LedgerError::refused(
                "ledger-failed",
                "the ledger could not be written",
            ));
        }
        Ok(())
    }));
    let closing = context.begin_close_project(project);
    context.settle();
    assert!(context.finish(closing).is_err(), "diana's close failed");
    assert!(!failing.get());
    // Zeus's harness crashes while its kill still waits; then the kill fails.
    context.exit("zeus");
    let resuming = context.begin_resume_project(project);
    context.settle();
    context.host.refuse_kills.set(true);
    answer.open();
    context.settle();
    context.host.refuse_kills.set(false);
    context.finish(resuming).unwrap();

    assert_eq!(
        tiers.task(1).task.state,
        "paused",
        "the window that crashed is the chief's to resume"
    );
}

/// Astra's review of 2026-10-16, round 4: Enter sends what waits in the input
/// box, and a window the Close could not close was given it while closed.
#[test]
fn a_paste_left_unsent_in_a_window_the_close_could_not_close_gets_no_enter_until_the_resume() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.adapter.with("chief", |agent| agent.arrive = false);
    let note = context.note(project.id, "zeus", "chief", "hello");
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering");
    context.host.refuse_kills.set(true);
    assert!(
        context.close_project(project.id).is_err(),
        "its windows stay"
    );
    context.host.refuse_kills.set(false);
    context.advance(11_000);
    context.host.set_snapshot(json!({ "outputQuietMs": 5_000 }));
    context.pass().unwrap();
    assert_eq!(enters(&context), 0, "no key that sends, while closed");

    context.resume_project(project.id).unwrap();
    context.pass().unwrap();
    assert_eq!(enters(&context), 1, "pressed once the project is open");
}

/// T-1 paused with the words that its window's program may still run, and the
/// note that says so to the chief begun, the host taking its time over the
/// word that the chief's turn begins: what lets that go.
fn warning_on_its_way(context: &Context, project: i64) -> (MessageView, Gate) {
    {
        let mut ledger = context.ledger.borrow_mut();
        ledger.pause_task(project, 1, None, Some(SURVIVOR)).unwrap();
        ledger.note_pause(project, "chief", 1, SURVIVOR).unwrap();
    }
    let marking = context.background.hold_marks();
    context.pass().unwrap();
    let chief = context.id(project, "chief");
    let note = context
        .inbox(chief)
        .into_iter()
        .find(|message| message.body.starts_with("T-1 is paused: "))
        .expect("the pause note");
    assert_eq!(note.state, "delivering");
    (note, marking)
}

/// What the chief is told again of T-1's cancel, newest first.
fn said_again(context: &Context, project: i64) -> Vec<String> {
    context
        .inbox(context.id(project, "chief"))
        .into_iter()
        .filter(|message| message.body.starts_with("T-1 cancelled. Before that, "))
        .map(|message| message.body)
        .collect()
}

/// Both reviews of 2026-10-16, round 4: the cancel kept the warnings of the notes
/// still queued, and withdrew this one on its way; the Close then kept it from the
/// chief, and the warning was lost.
#[test]
fn a_cancel_keeps_the_warning_of_a_pause_note_on_its_way_that_the_close_kept_from_the_chief() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let (note, marking) = warning_on_its_way(&context, project);
    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 1, "human")
        .unwrap();
    let closing = context.begin_close_project(project);
    context.settle();
    marking.open();
    context.finish(closing).unwrap();

    assert_eq!(context.message(note.id).state, "cancelled");
    assert_eq!(
        said_again(&context, project),
        [format!("T-1 cancelled. Before that, {SURVIVOR}.")]
    );
    assert!(
        given(&context, "chief")
            .iter()
            .all(|text| !text.contains(" is paused: ")),
        "the note itself was never given"
    );
}

/// The same with the project open, where the window refuses the note: its
/// warning is kept as well.
#[test]
fn a_cancel_keeps_the_warning_of_a_pause_note_on_its_way_that_its_window_then_refused() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let (_, marking) = warning_on_its_way(&context, project);
    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 1, "human")
        .unwrap();
    context.adapter.with("chief", |agent| agent.admit = false);
    marking.open();
    context.settle();

    assert_eq!(
        said_again(&context, project),
        [format!("T-1 cancelled. Before that, {SURVIVOR}.")]
    );
}

/// The note on its way, cancelled, and handed to a chief whose harness says
/// nothing sure of it: `admission` answers each hand-over, and the chief's
/// record never shows the note.
fn cancelled_and_never_shown(context: &Context, project: i64, admission: Admission) -> MessageView {
    let (note, marking) = warning_on_its_way(context, project);
    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 1, "human")
        .unwrap();
    *context.adapter.deliver.borrow_mut() = Some(Rc::new(move |_| {
        let admission = admission.clone();
        Box::pin(async move { Ok(admission) })
    }));
    marking.open();
    context.settle();
    assert_eq!(
        context.message(note.id).state,
        "delivering",
        "it may yet arrive"
    );
    assert!(said_again(context, project).is_empty());
    note
}

/// Astra's review of 2026-10-16, round 5: the cancel withdrew the note on its
/// way, the hand-over was uncertain, and the look that watched for its arrival let
/// it go as withdrawn: the warning was lost though the record never showed it.
#[test]
fn a_cancel_keeps_the_warning_of_a_pause_note_on_its_way_whose_hand_over_was_uncertain() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let note = cancelled_and_never_shown(
        &context,
        project,
        Admission::Uncertain {
            reason: "the plugin did not answer".to_owned(),
        },
    );
    context.advance(31_000);
    context.pass().unwrap();

    assert_eq!(context.message(note.id).state, "cancelled");
    assert_eq!(
        said_again(&context, project),
        [format!("T-1 cancelled. Before that, {SURVIVOR}.")]
    );
}

/// The same of a paste the window took and its record never showed.
#[test]
fn a_cancel_keeps_the_warning_of_a_pause_note_on_its_way_that_never_showed_in_the_record() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    cancelled_and_never_shown(&context, project, Admission::Admitted { queued: false });
    context.advance(31_000);
    context.pass().unwrap();

    assert_eq!(
        said_again(&context, project),
        [format!("T-1 cancelled. Before that, {SURVIVOR}.")]
    );
}

/// Sol's review of 2026-10-16, round 5: the warning was the first message of a
/// chief's launch, its delivery begun, and the cancel came while the launch was
/// prepared; the preparation then failed, and the message given back was
/// withdrawn already.
#[test]
fn a_cancel_keeps_the_warning_of_a_pause_note_a_chief_launch_was_to_open_with_when_the_launch_fails(
) {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    {
        let mut ledger = context.ledger.borrow_mut();
        ledger.pause_task(project, 1, None, Some(SURVIVOR)).unwrap();
        ledger.note_pause(project, "chief", 1, SURVIVOR).unwrap();
    }
    context.close_project(project).unwrap();
    // The chief's window the Resume opens fails to start; the one tried after it
    // opens with what waits first in its queue, the note.
    let fails = "the harness would not start".to_owned();
    context
        .adapter
        .prepare_holds
        .hold_instead(
            |asked| asked[0]["role"] == "chief" && asked[0]["message"].is_null(),
            fails.clone(),
        )
        .open();
    let preparing = context.adapter.prepare_holds.hold_instead(
        |asked| {
            asked[0]["message"]
                .as_str()
                .is_some_and(|message| message.contains("T-1 is paused: "))
        },
        fails,
    );
    context.resume_project(project).unwrap();
    context.settle();
    context.advance(5_001);
    context.pass().unwrap();
    let note = context
        .inbox(context.id(project, "chief"))
        .into_iter()
        .find(|message| message.body.starts_with("T-1 is paused: "))
        .expect("the pause note");
    assert_eq!(note.state, "delivering", "the chief's launch opens with it");
    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 1, "human")
        .unwrap();
    preparing.open();
    context.settle();

    assert_eq!(context.message(note.id).state, "cancelled");
    assert_eq!(
        said_again(&context, project),
        [format!("T-1 cancelled. Before that, {SURVIVOR}.")]
    );
}

/// The note's last attempt, refused after the cancel: it is not given up but
/// withdrawn, its warning said again in a note with attempts of its own, and the
/// human is told of no delivery that failed.
#[test]
fn a_cancel_keeps_the_warning_of_a_pause_note_whose_last_attempt_its_window_refused() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    context.adapter.with("chief", |agent| agent.admit = false);
    let note = {
        let mut ledger = context.ledger.borrow_mut();
        ledger.pause_task(project, 1, None, Some(SURVIVOR)).unwrap();
        ledger.note_pause(project, "chief", 1, SURVIVOR).unwrap()
    };
    context.pass().unwrap();
    context.pass().unwrap();
    let refused = context.message(note.id);
    assert_eq!((refused.state.as_str(), refused.attempts), ("queued", 2));
    let marking = context.background.hold_marks();
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering");
    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 1, "human")
        .unwrap();
    marking.open();
    context.settle();

    assert_eq!(context.message(note.id).state, "cancelled");
    assert_eq!(
        said_again(&context, project),
        [format!("T-1 cancelled. Before that, {SURVIVOR}.")]
    );
    assert!(
        context
            .inbox(context.id(project, "human"))
            .iter()
            .all(|message| !message.body.contains("did not reach")),
        "nothing failed"
    );
}

/// Both reviews of 2026-10-16, round 6: a note naming T-1, with its warning, and
/// T-2 was refused at its last attempt after T-1 was cancelled on its way; it
/// failed with the warning in it, and the warning was lost.
#[test]
fn a_cancel_keeps_the_warning_of_a_note_of_several_whose_last_attempt_its_window_refused() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    context.adapter.with("chief", |agent| agent.admit = false);
    let note = {
        let mut ledger = context.ledger.borrow_mut();
        ledger.pause_task(project, 1, None, Some(SURVIVOR)).unwrap();
        ledger.pause_task(project, 2, None, None).unwrap();
        let note = ledger.note_pause(project, "chief", 1, SURVIVOR).unwrap();
        ledger
            .join_pause_note(note.id, 2, "@diana-brisk-birch's window closed")
            .unwrap()
            .unwrap()
    };
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.message(note.id).attempts, 2);
    let marking = context.background.hold_marks();
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering");
    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 1, "human")
        .unwrap();
    marking.open();
    context.settle();

    assert_eq!(
        said_again(&context, project),
        [format!("T-1 cancelled. Before that, {SURVIVOR}.")]
    );
    let failed = context.message(note.id);
    assert_eq!(failed.state, "failed");
    assert!(!failed.body.contains("T-1"), "{}", failed.body);
}

/// And where the app quits with the note on its way: the start finds it never
/// arrived, and the warning is said again.
#[test]
fn a_cancel_keeps_the_warning_of_a_pause_note_on_its_way_when_the_app_quits() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let (note, _marking) = warning_on_its_way(&context, project);
    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 1, "human")
        .unwrap();
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();

    assert_eq!(context.message(note.id).state, "cancelled");
    assert_eq!(
        said_again(&context, project),
        [format!("T-1 cancelled. Before that, {SURVIVOR}.")]
    );
}

/// And where the window was given the note, cancelled on its way or not, the
/// chief has the warning, and it is not said again.
#[test]
fn a_pause_note_cancelled_on_its_way_and_given_all_the_same_is_not_said_again() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let (_, marking) = warning_on_its_way(&context, project);
    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 1, "human")
        .unwrap();
    marking.open();
    context.settle();

    assert!(given(&context, "chief")
        .iter()
        .any(|text| text.contains(SURVIVOR)));
    assert!(said_again(&context, project).is_empty());
}

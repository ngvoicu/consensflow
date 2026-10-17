//! A closed project's windows are handed nothing, whatever wait the Close comes
//! in: a launch whose project closed while its window was prepared opens none,
//! a first message a window takes once it is up goes in only while its project
//! is open, a delivery is handed over only if its project is open at the moment
//! of the hand-over, after every wait of its adapter's own, and no key that sends
//! goes into a window the Close could not close. What was not handed over waits
//! for the Resume, with its attempt; a pause note withdrawn on its way keeps its
//! warning. The Resume brings back only the work still at work once it has read
//! each window's record, a window for one task alone, and waits for every kill of
//! the Close, whatever failed (both reviews of 2026-10-16, round 4); what tells a
//! window to go on goes with any change to its task. Node's dispatcher had no such
//! Close, so none of these is held to a Node trace.

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
use crate::resume::{at_work, cut_notes, opened, quit_and_start};

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
    // A first message on the command line, and one the window takes once it is up
    // (OpenCode's), which no hand-over as the pane is asked for keeps out.
    for seeds in [false, true] {
        let context = with_codex();
        context.codex.seeds.set(seeds);
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
            "seeds {seeds}: no window opens in a closed project"
        );
        let waiting = handoff(&context, project);
        assert_eq!(
            (waiting.state.as_str(), waiting.attempts),
            ("queued", 0),
            "seeds {seeds}: the handoff waits for the Resume, never handed over"
        );

        context.resume_project(project).unwrap();
        for _ in 0..2 {
            context.pass().unwrap();
        }
        assert_eq!(opened(&context)[windows..], ["p1-chief"], "seeds {seeds}");
        assert_eq!(handoffs_given(&context), 1, "seeds {seeds}");
    }
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

/// Sol's review of 2026-10-16, round 7: a reader of a closed project reads only
/// what its window was handed, and a launch's first message is handed with the
/// command line the window opens with, as its pane is asked for and not before
/// (the ledger marks it so, `admit`). One the window takes once it is up
/// (OpenCode's) is handed as it is posted, and not as its pane is asked for.
#[test]
fn a_launch_s_first_message_is_handed_over_as_its_pane_is_asked_for_or_as_it_is_posted_once_the_window_is_up(
) {
    for seeds in [false, true] {
        let context = Context::new();
        context.adapter.seeds.set(seeds);
        let project = context.with_staff(&["zeus"]).id;
        let preparing = context.adapter.prepare_holds.hold(|_| true);
        let opening = context.host.open_holds.hold(|_| true);
        let starting = context.adapter.start_holds.hold(|_| true);
        let brief = context
            .give(project, "zeus", "Parser")
            .message
            .expect("its brief")
            .id;
        let handed = || {
            context
                .ledger
                .borrow()
                .handed_over(project)
                .unwrap()
                .contains(&brief)
        };
        context.pass().unwrap();
        context.settle();
        assert_eq!(context.message(brief).state, "delivering");
        assert!(!handed(), "seeds {seeds}: not while it is prepared");

        preparing.open();
        context.settle();
        assert_eq!(handed(), !seeds, "seeds {seeds}: as its pane is asked for");

        opening.open();
        context.settle();
        assert_eq!(handed(), !seeds, "seeds {seeds}: once its pane is open");

        starting.open();
        context.settle();
        assert!(
            handed() || context.message(brief).state == "delivered",
            "seeds {seeds}: once it is posted"
        );
    }
}

/// What a paste carries is told when its delivery begins, and a row of it
/// withdrawn before the hand-over is not handed over with it: the chief pauses
/// T-1 again while the words that resume it, carrying its note, are being pasted,
/// and its pause takes its note back. Nothing goes in, and the words go back to
/// their queue, to go again without the note (the ledger's `admit`).
#[test]
fn a_paste_whose_carried_row_was_withdrawn_before_its_hand_over_hands_nothing_over_and_goes_back() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    context.pause_task(project, 1);
    context.pass().unwrap();
    context
        .adapter
        .with("zeus-amber-pine", |agent| agent.settled = true);
    let note = context
        .ledger
        .borrow_mut()
        .note(
            project,
            &NewNote {
                from: Some("chief".to_owned()),
                to: "zeus-amber-pine".to_owned(),
                task: Some(1),
                body: "Use tabs".to_owned(),
            },
        )
        .unwrap();
    context.resume_task(project, 1, "Carry on");
    let words = tiers.task(1).messages.last().cloned().expect("the words");
    let handing = context.adapter.deliver_holds.hold(|asked| {
        asked[0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("Carry on"))
    });
    for _ in 0..2 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(words.id).state, "delivering");

    context.pause_task(project, 1);
    handing.open();
    context.settle();

    assert_eq!(context.message(note.id).state, "cancelled");
    let words = context.message(words.id);
    assert_eq!(
        (words.state.as_str(), words.reason.as_deref()),
        (
            "queued",
            Some("what its paste held was withdrawn before it was handed over")
        )
    );
    assert!(given(&context, "zeus-amber-pine")
        .iter()
        .all(|text| !text.contains("Use tabs")));
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
    let role = context
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
    // The pause withdrew its continuation before the launch began it: the launch
    // is over, and nothing failed. A launch that failed would have withdrawn the
    // role written for its window, which waits for the next.
    assert_eq!(context.message(role.id).state, "queued");
    assert!(
        context
            .inbox(context.id(project, "human"))
            .iter()
            .all(|message| !message.body.contains("could not start")),
        "a launch its first message's withdrawal ended is no launch that failed"
    );
}

/// Zeus's session holds T-2 as well as T-1, as a ledger written before one task
/// per session may have it, and its newest words are T-2's.
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

/// The pause notes the chief's window was given, line by line.
fn pauses_given_to_the_chief(context: &Context) -> Vec<String> {
    given(context, "chief")
        .iter()
        .flat_map(|text| text.split('\n'))
        .filter(|line| line.contains(" is paused: "))
        .map(str::to_owned)
        .collect()
}

/// The lead's design of 2026-10-17, after both reviews of four rounds found a
/// continuation naming several tasks escaping one check or another (Sol's of round
/// 7: a task gated for the human's approval told to go on): a window that holds
/// several tasks comes back for none. The Close's pauses stay, the chief is given
/// the note of each, and no window is told to go on.
#[test]
fn a_session_holding_two_tasks_the_close_paused_comes_back_for_neither_and_the_chief_is_given_the_note_of_each(
) {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = tiers.id("zeus-amber-pine");
    context.close_project(project).unwrap();
    holding_two(&context, project, zeus);
    let before = opened(&context).len();

    context.resume_project(project).unwrap();
    for _ in 0..4 {
        context.pass().unwrap();
    }

    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["paused", "paused"]
    );
    assert_eq!(
        opened(&context)[before..],
        ["p1-chief"],
        "zeus's window stays closed"
    );
    assert!(continuations_given(&context).is_empty());
    assert!(cut_notes(&context, zeus).is_empty(), "none is written");
    let pauses = pauses_given_to_the_chief(&context);
    for number in [1, 2] {
        assert!(
            pauses
                .iter()
                .any(|line| line.starts_with(&format!("T-{number} is paused: "))
                    && line.contains(&format!("cf task resume T-{number}"))),
            "T-{number}: {pauses:?}"
        );
    }
}

/// The same of a session whose two tasks a quit left at work: the Resume pauses
/// both, and the chief is told why.
#[test]
fn a_session_holding_two_tasks_a_quit_left_at_work_comes_back_for_neither_and_the_chief_is_told_why(
) {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = tiers.id("zeus-amber-pine");
    holding_two(&context, project, zeus);
    let after = quit_and_start(&context);
    let before = opened(&context).len();

    after.resume_project(project).unwrap();
    for _ in 0..4 {
        after.pass().unwrap();
    }

    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["paused", "paused"]
    );
    assert_eq!(opened(&context)[before..], ["p1-chief"]);
    assert!(continuations_given(&context).is_empty());
    let because =
        "@zeus-amber-pine's window held T-1, T-2, and a window is told to go on only when it held one task: resume them one at a time";
    assert_eq!(
        pauses_given_to_the_chief(&context),
        [1, 2].map(|number| format!(
            "T-{number} is paused: {because}. Resume it with: cf task resume T-{number} \"…\""
        ))
    );
}

/// Sol's review of 2026-10-17: zeus's session held T-1 at work and T-2 queued,
/// its brief not yet given, and the Resume left T-2 queued: the next pass opened
/// zeus's window with T-2's brief, though nobody resumed it. The Resume pauses
/// T-2 too, and the chief is told of both.
#[test]
fn a_session_holding_a_queued_task_beside_one_the_close_paused_opens_for_neither() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = tiers.id("zeus-amber-pine");
    context
        .ledger
        .borrow_mut()
        .write_raw(&format!(
            "UPDATE task SET assignee_id = {zeus}, pool = NULL, tier = NULL, state = 'queued'
               WHERE project_id = {project} AND number = 2;
             UPDATE message SET recipient_id = {zeus}, state = 'queued', delivered_at = NULL, receipt = NULL
               WHERE kind = 'task'
                 AND task_id = (SELECT id FROM task WHERE project_id = {project} AND number = 2);"
        ))
        .unwrap();
    context.close_project(project).unwrap();
    let before = opened(&context).len();

    context.resume_project(project).unwrap();
    for _ in 0..4 {
        context.pass().unwrap();
    }

    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["paused", "paused"]
    );
    assert_eq!(
        opened(&context)[before..],
        ["p1-chief"],
        "zeus's window stays closed"
    );
    let pauses = pauses_given_to_the_chief(&context);
    for number in [1, 2] {
        assert!(
            pauses
                .iter()
                .any(|line| line.starts_with(&format!("T-{number} is paused: "))),
            "T-{number}: {pauses:?}"
        );
    }
}

/// The lead's decision of 2026-10-17 (Astra's review): the chief does what the
/// note says, as it says it. The first command resumes T-1 in zeus's window; the
/// second is refused while T-1 is at work, and goes once T-1 has its result.
#[test]
fn the_chief_s_commands_as_its_note_words_them_resume_a_session_of_two_one_at_a_time() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = tiers.id("zeus-amber-pine");
    holding_two(&context, project, zeus);
    let after = quit_and_start(&context);
    after.resume_project(project).unwrap();
    for _ in 0..4 {
        after.pass().unwrap();
    }
    let commands: Vec<(i64, String)> = pauses_given_to_the_chief(&context)
        .iter()
        .filter_map(|line| line.split_once("Resume it with: cf task resume T-"))
        .map(|(_, command)| {
            let (number, words) = command.split_once(' ').expect("a number and its words");
            let words = words.split('"').nth(1).expect("quoted words");
            (number.parse().expect("a number"), words.to_owned())
        })
        .collect();
    assert_eq!(commands, [(1, "…".to_owned()), (2, "…".to_owned())]);
    let resume = |(number, words): &(i64, String)| {
        context
            .ledger
            .borrow_mut()
            .resume_task(project, *number, Some("chief"), words)
    };

    resume(&commands[0]).unwrap();
    for _ in 0..3 {
        after.pass().unwrap();
    }
    assert_eq!(
        tiers.task(1).task.state,
        "working",
        "zeus is given T-1's words"
    );
    let busy = resume(&commands[1]).unwrap_err();
    assert_eq!(busy.code(), Some("session-busy"));
    context.adapter.answer("zeus", "Lexer done");
    for _ in 0..3 {
        after.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "done");

    resume(&commands[1]).unwrap();
    for _ in 0..3 {
        after.pass().unwrap();
    }
    assert_eq!(tiers.task(2).task.state, "working", "and then T-2's");
}

/// Sol's review of 2026-10-17: two continuations an earlier build left for T-1,
/// zeus's one task at work, both stayed, and zeus's window was told to go on
/// twice. The Resume keeps the oldest.
#[test]
fn a_window_an_earlier_build_left_two_words_to_go_on_with_its_task_is_told_once() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let after = quit_and_start(&context);
    for _ in 0..2 {
        context
            .ledger
            .borrow_mut()
            .note(
                project,
                &NewNote {
                    from: None,
                    to: "zeus-amber-pine".to_owned(),
                    task: Some(1),
                    body:
                        "ConsensFlow quit while you were working on T-1. Go on where you stopped."
                            .to_owned(),
                },
            )
            .unwrap();
    }

    after.resume_project(project).unwrap();
    for _ in 0..4 {
        after.pass().unwrap();
    }

    let told = given(&context, "zeus-amber-pine")
        .iter()
        .filter(|text| text.contains("Go on where you stopped."))
        .count();
    assert_eq!(told, 1);
}

/// The Resume's launch of zeus's window, held in its preparation with its
/// continuation of T-1 begun: what lets it go, and the continuation.
fn continuation_being_launched(context: &Context, project: i64, zeus: i64) -> (Gate, MessageView) {
    context.close_project(project).unwrap();
    let words = continuation(1);
    let preparing = context.adapter.prepare_holds.hold(move |asked| {
        asked[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains(&words))
    });
    context.resume_project(project).unwrap();
    context.settle();
    let note = cut_notes(context, zeus).pop().expect("the continuation");
    assert_eq!(context.message(note.id).state, "delivering");
    (preparing, note)
}

/// The lead's design of 2026-10-17: a continuation dies with any change to its
/// task, in the write that makes it. The human pauses T-1 while the Resume's
/// launch of its window is prepared: the pause withdraws the continuation, and
/// no window is told to go on.
#[test]
fn a_pause_while_the_resume_s_launch_is_prepared_withdraws_its_continuation_in_the_same_write() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = tiers.id("zeus-amber-pine");
    let before = opened(&context).len();
    let (preparing, note) = continuation_being_launched(&context, project, zeus);

    context.human_pause(project, 1);
    assert_eq!(
        context.message(note.id).state,
        "cancelled",
        "withdrawn by the pause's write"
    );
    preparing.open();
    for _ in 0..3 {
        context.pass().unwrap();
    }

    // The fake takes a launch's first message as its window's when it is
    // prepared: what was handed over is what a pane opened with.
    assert_eq!(tiers.task(1).task.state, "paused");
    assert_eq!(
        opened(&context)[before..],
        ["p1-chief", "p1-diana-brisk-birch"],
        "zeus's window stays closed, never handed its first message"
    );
    assert_eq!(context.message(note.id).state, "cancelled");
}

/// Sol's review of 2026-10-16, round 7, in the shape of a window of one task: the
/// human pauses T-1 while the Resume's launch is prepared, and the chief resumes
/// it with the human's approval required. The pause withdrew the continuation:
/// the words wait at the gate, and no window is told to go on before the human
/// passes them on.
#[test]
fn a_task_paused_and_resumed_for_the_human_s_approval_while_the_resume_s_launch_is_prepared_is_told_nothing_to_go_on(
) {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = tiers.id("zeus-amber-pine");
    let before = opened(&context).len();
    let (preparing, note) = continuation_being_launched(&context, project, zeus);
    context.ledger.borrow_mut().set_gate(project, true).unwrap();

    context.human_pause(project, 1);
    context.resume_task(project, 1, "Carry on");
    preparing.open();
    for _ in 0..3 {
        context.pass().unwrap();
    }

    let task = tiers.task(1);
    assert_eq!(task.task.state, "queued");
    assert_eq!(
        task.messages.last().map(|words| words.state.as_str()),
        Some("gated"),
        "the words wait for the human"
    );
    assert_eq!(context.message(note.id).state, "cancelled");
    assert_eq!(
        opened(&context)[before..],
        ["p1-chief", "p1-diana-brisk-birch"],
        "zeus's window stays closed"
    );
}

/// Both reviews of 2026-10-16, round 7: a continuation still on its way was
/// carried into T-1's next brief when T-1 went to another window, which was then
/// told to go on with a turn it never began. The chief takes T-1 back to the board
/// while the Resume's launch is prepared: the continuation goes with zeus's
/// window, and the next brief says nothing of it.
#[test]
fn a_task_taken_back_while_the_resume_s_launch_is_prepared_carries_no_continuation_into_its_next_brief(
) {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = tiers.id("zeus-amber-pine");
    let (preparing, note) = continuation_being_launched(&context, project, zeus);

    context
        .ledger
        .borrow_mut()
        .release_task(project, 1, "the chief takes it back")
        .unwrap();
    preparing.open();
    for _ in 0..4 {
        context.pass().unwrap();
    }

    let task = tiers.task(1).task;
    assert!(
        !task
            .body
            .contains("ConsensFlow stopped while you were working on"),
        "{}",
        task.body
    );
    assert_eq!(context.message(note.id).state, "cancelled");
}

/// OpenCode's shape: the continuation goes in once the window is up, through its
/// own server, and the human pauses T-1 while the window comes up. The pause
/// withdraws it, and the post hands nothing over.
#[test]
fn a_continuation_a_window_takes_once_it_is_up_is_not_posted_when_its_task_is_paused_while_it_comes_up(
) {
    let context = Context::new();
    context.adapter.seeds.set(true);
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = tiers.id("zeus-amber-pine");
    let native = native_of(&context, zeus).expect("its conversation");
    context.close_project(project).unwrap();
    let starting = context
        .adapter
        .start_holds
        .hold(move |asked| asked[0]["launch"]["nativeSession"] == native.as_str());
    context.resume_project(project).unwrap();
    context.settle();
    let note = cut_notes(&context, zeus).pop().expect("the continuation");
    context.human_pause(project, 1);
    starting.open();
    for _ in 0..3 {
        context.pass().unwrap();
    }

    assert_eq!(tiers.task(1).task.state, "paused");
    assert_eq!(context.message(note.id).state, "cancelled");
    assert!(continuations_given(&context).is_empty());
}

/// What an earlier build left queued telling zeus's window to go on with two
/// tasks, of which it holds one now: the Resume withdraws it, and tells the window
/// once, of the task it holds.
#[test]
fn the_resume_withdraws_what_an_earlier_build_left_naming_two_tasks_and_tells_the_window_once() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    context.close_project(project).unwrap();
    let old = context
        .ledger
        .borrow_mut()
        .note(
            project,
            &NewNote {
                from: None,
                to: "zeus-amber-pine".to_owned(),
                task: Some(1),
                body:
                    "ConsensFlow quit while you were working on T-1, T-2. Go on where you stopped."
                        .to_owned(),
            },
        )
        .unwrap();
    context.resume_project(project).unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }

    assert_eq!(context.message(old.id).state, "cancelled");
    assert_eq!(continuations_given(&context), [continuation(1)]);
}

/// Zeus's window, open and at rest, and a continuation of T-1 queued for it: one
/// a window is delivered, not launched with (behind the note that says its role,
/// or after a first message that went back while the window came up).
fn continuation_for_an_open_window(context: &Context, project: i64) -> MessageView {
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
                body: continuation(1),
            },
        )
        .unwrap()
}

/// Astra's review of 2026-10-17: zeus's continuation was handed over, the human
/// paused T-1 while the paste waited for its reply, and the paste came back
/// refused, nothing written. The continuation went back to its queue, and a read
/// of it gave a word to go on with a task paused since. It is withdrawn as it
/// comes back, and reads as withdrawn.
#[test]
fn a_continuation_handed_over_and_refused_after_its_task_was_paused_is_withdrawn() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = tiers.id("zeus-amber-pine");
    let ledger = Rc::clone(&context.ledger);
    *context.adapter.deliver.borrow_mut() = Some(Rc::new(move |_| {
        ledger
            .borrow_mut()
            .pause_task(project, 1, Some("human"), None)
            .unwrap();
        Box::pin(async {
            Ok(Admission::Refused {
                reason: "the paste wrote nothing".to_owned(),
            })
        })
    }));
    let note = continuation_for_an_open_window(&context, project);

    for _ in 0..3 {
        context.pass().unwrap();
    }

    assert_eq!(tiers.task(1).task.state, "paused");
    let read = context
        .inbox(zeus)
        .into_iter()
        .find(|message| message.id == note.id)
        .expect("in zeus's inbox");
    assert_eq!(
        (read.state.as_str(), read.reason.as_deref()),
        ("cancelled", Some("T-1 is paused now"))
    );
    assert!(continuations_given(&context).is_empty());
}

/// The same where the app quits with the continuation handed over and its
/// arrival not yet seen, its task paused meanwhile: the start settles it, and it
/// goes rather than wait in its queue.
#[test]
fn a_continuation_handed_over_whose_task_was_paused_goes_when_the_app_quits_with_it_on_its_way() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    *context.adapter.deliver.borrow_mut() = Some(Rc::new(|_| {
        Box::pin(async { Ok(Admission::Admitted { queued: false }) })
    }));
    let note = continuation_for_an_open_window(&context, project);
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering");
    context.human_pause(project, 1);
    assert_eq!(
        context.message(note.id).state,
        "delivering",
        "handed over: it runs its course"
    );

    let after = quit_and_start(&context);
    drop(after);

    assert_eq!(context.message(note.id).state, "cancelled");
}

/// The human pauses T-1 while the adapter takes its continuation, the delivery
/// begun: the pause withdraws it, and the hand-over gives the window nothing
/// (both reviews of 2026-10-16, round 7: a continuation given back to its queue
/// waited there with its words for a task paused since).
#[test]
fn a_continuation_whose_task_is_paused_while_its_adapter_takes_it_is_withdrawn_and_handed_nothing()
{
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let words = continuation(1);
    let handing = context.adapter.deliver_holds.hold(move |asked| {
        asked[0]["text"]
            .as_str()
            .is_some_and(|text| text.contains(&words))
    });
    let note = continuation_for_an_open_window(&context, project);
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering");
    context.human_pause(project, 1);
    handing.open();
    for _ in 0..3 {
        context.pass().unwrap();
    }

    assert_eq!(tiers.task(1).task.state, "paused");
    assert_eq!(context.message(note.id).state, "cancelled");
    assert!(continuations_given(&context).is_empty());
}

/// The same where the human pauses T-1 while the window gets ready for it: the
/// pause withdraws it, and its delivery never begins (Astra's review of
/// 2026-10-16, round 7: a continuation that waited behind the readiness check,
/// its task paused meanwhile, stayed queued with its words).
#[test]
fn a_continuation_whose_task_is_paused_while_its_window_gets_ready_is_withdrawn_and_never_begun() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    *context.adapter.ready.borrow_mut() = Some(Rc::new(|| Ok(Readiness::Ready)));
    let readying = context
        .adapter
        .ready_holds
        .hold(|asked| asked[0]["pane"]["id"] == "p1-zeus-amber-pine");
    let note = continuation_for_an_open_window(&context, project);
    context.pass().unwrap();
    context.human_pause(project, 1);
    readying.open();
    for _ in 0..3 {
        context.pass().unwrap();
    }

    let note = context.message(note.id);
    assert_eq!((note.state.as_str(), note.attempts), ("cancelled", 0));
    assert!(continuations_given(&context).is_empty());
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

/// Both reviews of 2026-10-17: the chief resumes T-1 and pauses it again while the
/// note's last attempt is on its way, and the attempt is refused. T-1 is paused
/// as it was, but not by the pause the note says: the note goes, and its warning
/// is said again, once, and nothing failed.
#[test]
fn a_resume_and_another_pause_keep_the_warning_of_a_pause_note_whose_last_attempt_its_window_refused(
) {
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
    assert_eq!(context.message(note.id).attempts, 2);
    let marking = context.background.hold_marks();
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering");
    {
        let mut ledger = context.ledger.borrow_mut();
        ledger
            .resume_task(project, 1, Some("chief"), "Go on")
            .unwrap();
        ledger.pause_task(project, 1, Some("chief"), None).unwrap();
    }
    marking.open();
    context.settle();

    assert_eq!(tiers.task(1).task.state, "paused");
    assert_eq!(context.message(note.id).state, "cancelled");
    let said: Vec<String> = context
        .inbox(context.id(project, "chief"))
        .into_iter()
        .filter(|message| message.id != note.id && message.body.contains(SURVIVOR))
        .map(|message| message.body)
        .collect();
    assert_eq!(
        said,
        [format!("T-1 was paused again. Before that, {SURVIVOR}.")]
    );
    assert!(
        context
            .inbox(context.id(project, "human"))
            .iter()
            .all(|message| !message.body.contains("did not reach")),
        "nothing failed"
    );
}

/// Both reviews of 2026-10-16, round 7: the same where the chief resumes T-1 while
/// the note's last attempt is on its way. Round 7 kept the warning of a cancelled
/// task alone: the note failed with the warning in it, and the human was told only
/// that it did not reach the chief.
#[test]
fn a_resume_keeps_the_warning_of_a_pause_note_whose_last_attempt_its_window_refused() {
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
    assert_eq!(context.message(note.id).attempts, 2);
    let marking = context.background.hold_marks();
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering");
    context
        .ledger
        .borrow_mut()
        .resume_task(project, 1, Some("chief"), "Go on")
        .unwrap();
    marking.open();
    context.settle();

    assert_eq!(context.message(note.id).state, "cancelled");
    let resumed: Vec<String> = context
        .inbox(context.id(project, "chief"))
        .into_iter()
        .filter(|message| message.body.starts_with("T-1 resumed. Before that, "))
        .map(|message| message.body)
        .collect();
    assert_eq!(resumed, [format!("T-1 resumed. Before that, {SURVIVOR}.")]);
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

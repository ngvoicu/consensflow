//! A start leaves every project that was open suspended, and the human's Resume
//! brings it back: the chief and each window that had work in hand, on their own
//! conversations, a window the quit cut mid-turn told so once. Node resumed the
//! chief at the start and paused what its windows were at, so none of these is
//! held to a Node trace.

use std::rc::Rc;

use cf_engine::testing::{Context, FakeAdapter, Gate, Made, Restarted};
use cf_engine::ActivityState;
use cf_ledger::{MessageView, NewNote, RESUME_WORDS};
use serde_json::{json, Value};

use crate::chiefs::to_human;
use crate::fixtures::{
    assert_match, fail_next_retry, fail_next_write, finished, given, native_of, Tiers,
};

/// What a worker the quit cut in T-1 is told, and what the chief is told.
const WORKER_CUT: &str = "ConsensFlow quit while you were working on T-1. Go on where you stopped.";
const CHIEF_CUT: &str =
    "ConsensFlow stopped in the middle of your turn, which was cut. Go on where you stopped.";

/// Zeus and diana at work, each on a task of its own, their windows in the middle
/// of a turn: the app quits now.
fn at_work(context: &Context) -> Tiers<'_> {
    let tiers = Tiers::new(context, &["zeus", "diana"]);
    tiers.open();
    tiers.open_body("Write the lexer");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["working", "working"]
    );
    tiers
}

/// The app quits and starts again: the daemon suspends what was open and the
/// engine settles what was on its way.
fn quit_and_start(context: &Context) -> Restarted<'_> {
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after
}

/// The panes opened so far, by their ids.
fn opened(context: &Context) -> Vec<String> {
    context
        .host
        .opened()
        .into_iter()
        .map(|open| open.pane.id)
        .collect()
}

/// The last launch the adapter was asked for `handle`.
fn launch_of(context: &Context, handle: &str) -> Value {
    context
        .adapter
        .prepared()
        .into_iter()
        .rev()
        .find(|launch| launch["participant"]["handle"] == handle)
        .unwrap_or_else(|| panic!("no launch of @{handle}"))
}

/// The notes ConsensFlow wrote to `to` that say a turn was cut, oldest first.
fn cut_notes(context: &Context, to: i64) -> Vec<MessageView> {
    let mut notes: Vec<MessageView> = context
        .inbox(to)
        .into_iter()
        .filter(|message| {
            message.kind == "note"
                && message.sender.is_none()
                && (message.body == WORKER_CUT || message.body == CHIEF_CUT)
        })
        .collect();
    notes.reverse();
    notes
}

#[test]
fn a_start_opens_no_window_and_delivers_nothing_and_leaves_the_tasks_and_messages_as_they_were() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let waiting = context.note(project, "human", "chief", "Any news?");
    let (panes, launches) = (opened(&context), context.adapter.prepared().len());

    let after = quit_and_start(&context);
    for _ in 0..3 {
        after.pass().unwrap();
    }

    assert_eq!(context.project(project).state, "suspended");
    assert_eq!(opened(&context), panes, "no window opens");
    assert_eq!(
        context.adapter.prepared().len(),
        launches,
        "none is prepared"
    );
    assert_eq!(
        context.message(waiting.id).state,
        "queued",
        "nothing is delivered"
    );
    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["working", "working"],
        "the board shows the tasks as they were"
    );
    assert!(
        tiers
            .notes("chief")
            .iter()
            .all(|note| !note.contains(" is paused: ")),
        "nothing is paused, so the chief is told of nothing"
    );
}

#[test]
fn what_is_sent_to_a_suspended_project_waits_for_its_resume() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let after = quit_and_start(&context);

    let note = context.note(project, "human", "chief", "While you were away");
    after.pass().unwrap();
    after.pass().unwrap();
    assert_eq!(context.message(note.id).state, "queued");

    after.resume_project(project).unwrap();
    after.pass().unwrap();
    after.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivered");
}

#[test]
fn a_resume_opens_the_chief_and_each_window_with_a_task_in_hand_and_not_an_idle_one() {
    let context = Context::new();
    // T-1 is done and its session's window is gone; T-2 is at work in a session of its own.
    let tiers = finished(&context);
    tiers.open_body("Write the lexer");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "working");
    let (project, before) = (tiers.project.id, opened(&context).len());
    let after = quit_and_start(&context);

    after.resume_project(project).unwrap();
    after.pass().unwrap();

    assert_eq!(
        opened(&context)[before..],
        ["p1-chief", "p1-zeus-brisk-birch"],
        "the chief, and the session at work: not the one whose task is done"
    );
    assert_eq!(context.project(project).state, "open");
}

/// The rig's restart of 2026-10-16 (`rows_a_paste_carried_pasted_again_after_a_restart`,
/// red under load): the human's Resume came while the start still read the record of the
/// words that resume a worker's paused task. The task was paused still, so the Resume
/// brought no window back for it; the start then confirmed the words, the task was at
/// work, and the next pass found it with no window and nothing due, and paused it for a
/// window that was gone.
#[test]
fn a_resume_while_the_start_reads_what_was_on_its_way_brings_back_the_window_it_makes_at_work() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    // T-1 is paused, and the words that resume it are on their way into zeus's window,
    // which took them, when the app quits.
    context.adapter.busy("zeus");
    context
        .ledger
        .borrow_mut()
        .pause_task(project.id, 1, Some("chief"), None)
        .unwrap();
    context.pass().unwrap();
    context
        .ledger
        .borrow_mut()
        .resume_task(project.id, 1, Some("chief"), "Go on")
        .unwrap();
    context.adapter.answer("zeus", "Paused");
    context.pass().unwrap();
    let words = context
        .ledger
        .borrow()
        .in_flight()
        .unwrap()
        .into_iter()
        .next()
        .expect("the words on their way");
    assert_eq!(
        context.task(project.id, 1).task.state,
        "queued",
        "behind its words"
    );
    let windows = |context: &Context| {
        opened(context)
            .iter()
            .filter(|pane| pane.as_str() == "p1-zeus")
            .count()
    };
    let before = windows(&context);
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    let reading = context.adapter.record_holds.hold(|_| true);
    let settling = after.begin_settle_after_restart();
    context.settle();

    // The human resumes the project while the start reads zeus's record: it waits.
    let resuming = after.begin_resume_project(project.id);
    context.settle();
    assert_eq!(context.project(project.id).state, "suspended");
    reading.open();
    context.settle();
    settling.take().unwrap().unwrap();
    resuming.take().unwrap().unwrap();
    assert_eq!(context.message(words.id).state, "delivered");
    after.pass().unwrap();
    after.pass().unwrap();
    assert_eq!(
        context.task(project.id, 1).task.state,
        "working",
        "not paused for a window that is gone: {:?}",
        context.inbox(context.id(project.id, "chief"))
    );
    assert_eq!(windows(&context), before + 1, "zeus's window is back");
}

/// The reviews of 2026-10-16: two briefs were on their way when the app quit, and the
/// human cancelled the first's task while the start read its window's record. The
/// start settled the cancelled brief all the same, which failed, and the settle with
/// it: the second brief stayed on its way, with no window to watch it, and its task
/// queued for good.
#[test]
fn a_task_cancelled_while_the_start_reads_its_briefs_record_leaves_the_next_brief_settled() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    tiers.open_body("Write the lexer");
    context.pass().unwrap();
    let briefs = context.ledger.borrow().in_flight().unwrap();
    assert_eq!(
        briefs
            .iter()
            .map(|brief| (brief.kind.as_str(), brief.task_number))
            .collect::<Vec<_>>(),
        [("task", Some(1)), ("task", Some(2))],
        "both briefs on their way"
    );
    let first = native_of(&context, briefs[0].recipient_id).expect("the first's session");
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    let reading = context
        .adapter
        .record_holds
        .hold(move |asked| asked[0] == first.as_str());
    let settling = after.begin_settle_after_restart();
    context.settle();

    // The human cancels T-1 while the start reads its brief's record.
    context
        .ledger
        .borrow_mut()
        .cancel_task(tiers.project.id, 1, "human")
        .unwrap();
    reading.open();
    context.settle();
    settling.take().unwrap().unwrap();
    assert_ne!(context.message(briefs[0].id).state, "delivering");
    let second = context.message(briefs[1].id);
    assert_eq!(
        second.state, "delivered",
        "the next brief, which its record shows, is settled all the same"
    );
    after.resume_project(tiers.project.id).unwrap();
    after.pass().unwrap();
    after.pass().unwrap();
    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["cancelled", "working"]
    );
}

/// A message the start fails to settle is the start's answer, written down, and every
/// other is settled all the same.
#[test]
fn a_brief_the_start_fails_to_settle_leaves_the_next_brief_settled() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    tiers.open_body("Write the lexer");
    context.pass().unwrap();
    let briefs = context.ledger.borrow().in_flight().unwrap();
    assert_eq!(briefs.len(), 2, "both briefs on their way");
    // The first's record never showed it, and giving it back to its queue fails.
    context
        .adapter
        .with(&briefs[0].recipient, |agent| agent.items.clear());
    fail_next_retry(&context);
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    let failed = after.settle_after_restart().unwrap_err();
    assert!(
        failed
            .to_string()
            .contains("the ledger could not be written"),
        "{failed}"
    );
    assert_eq!(context.message(briefs[1].id).state, "delivered");
}

#[test]
fn a_resume_of_a_project_that_is_open_opens_no_window_again_and_tells_none_its_turn_was_cut() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let (panes, launches) = (opened(&context), context.adapter.prepared().len());

    // Nothing quit: every window is open, and the turns they are in are no cut one.
    context.resume_project(project).unwrap();
    context.pass().unwrap();

    assert_eq!(opened(&context), panes, "no window opens again");
    assert_eq!(context.adapter.prepared().len(), launches);
    for handle in ["chief", "zeus-amber-pine", "diana-brisk-birch"] {
        assert!(
            cut_notes(&context, tiers.id(handle)).is_empty(),
            "@{handle} is told nothing"
        );
    }
}

#[test]
fn a_task_the_human_paused_stays_paused_through_a_restart_and_a_resume() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    context.human_pause(project, 1);
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "paused");
    let after = quit_and_start(&context);

    after.resume_project(project).unwrap();
    after.pass().unwrap();
    after.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "paused");
    assert_eq!(
        opened(&context)
            .iter()
            .filter(|id| id.contains("zeus"))
            .count(),
        1,
        "no new window for it"
    );
    assert!(
        tiers
            .notes("chief")
            .iter()
            .all(|note| !note.contains(" is paused: ")),
        "the human paused it: the chief is told nothing"
    );
    // It goes on when the human resumes it, on the conversation it had.
    let native = native_of(&context, tiers.id("zeus-amber-pine"));
    context
        .ledger
        .borrow_mut()
        .resume_task(project, 1, Some("human"), RESUME_WORDS)
        .unwrap();
    after.pass().unwrap();
    let launch = launch_of(&context, "zeus-amber-pine");
    assert_eq!(launch["resume"], native.unwrap_or_default().as_str());
    assert_match(
        launch["message"].as_str().unwrap(),
        r"T-1 · task from @human\]\nResumed: Go on where you stopped\.$",
    );
}

#[test]
fn a_resume_after_a_close_brings_back_the_chief_alone_and_the_tasks_the_close_paused_wait_for_it() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    context.close_project(project).unwrap();
    let before = opened(&context).len();

    context.resume_project(project).unwrap();
    context.pass().unwrap();

    assert_eq!(opened(&context)[before..], ["p1-chief"]);
    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["paused", "paused"],
        "the chief resumes what the close paused, as it always did"
    );
}

#[test]
fn a_window_the_quit_cut_mid_turn_is_told_so_once_and_one_at_rest_is_told_nothing() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    // Diana's turn ended just before the quit: it is at rest, its task not yet taken as done.
    context.adapter.answer("diana", "Lexer done");
    let after = quit_and_start(&context);

    after.resume_project(project).unwrap();

    let zeus = launch_of(&context, "zeus-amber-pine");
    assert_match(
        zeus["message"].as_str().unwrap(),
        r"^\[ConsensFlow m-\d+ · T-1 · note from ConsensFlow\]\nConsensFlow quit while you were working on T-1\. Go on where you stopped\.$",
    );
    assert_eq!(
        zeus["resume"],
        native_of(&context, tiers.id("zeus-amber-pine"))
            .unwrap()
            .as_str()
    );
    assert_eq!(
        launch_of(&context, "diana-brisk-birch")["message"],
        Value::Null
    );
    assert_eq!(launch_of(&context, "chief")["message"], Value::Null);
    assert_eq!(cut_notes(&context, tiers.id("zeus-amber-pine")).len(), 1);
    assert!(cut_notes(&context, tiers.id("diana-brisk-birch")).is_empty());
    assert!(cut_notes(&context, tiers.id("chief")).is_empty());

    // Given once: the window shows it, and nothing is written for it again.
    after.pass().unwrap();
    after.pass().unwrap();
    assert_eq!(
        cut_notes(&context, tiers.id("zeus-amber-pine"))[0].state,
        "delivered"
    );
    assert_eq!(cut_notes(&context, tiers.id("zeus-amber-pine")).len(), 1);
    assert_eq!(
        given(&context, "zeus-amber-pine")
            .iter()
            .filter(|text| text.contains(WORKER_CUT))
            .count(),
        1
    );
}

/// Found live on 2026-10-12 with Devin as the worker: its record, read with no window
/// open, cannot tell a cut turn from one at rest, so it was told nothing, read at rest
/// once resumed, and its task stayed at work for good.
#[test]
fn a_member_whose_record_cannot_tell_a_cut_is_told_all_the_same_and_a_chief_is_not() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    for handle in ["zeus", "diana", "chief"] {
        context.adapter.cannot_tell_a_cut(handle);
    }
    // Diana's turn ended before the quit: its record cannot say so either.
    context.adapter.answer("diana", "Lexer done");
    let after = quit_and_start(&context);

    after.resume_project(project).unwrap();

    assert_match(
        launch_of(&context, "zeus-amber-pine")["message"]
            .as_str()
            .unwrap(),
        r"^\[ConsensFlow m-\d+ · T-1 · note from ConsensFlow\]\nConsensFlow quit while you were working on T-1\. Go on where you stopped\.$",
    );
    assert_eq!(cut_notes(&context, tiers.id("zeus-amber-pine")).len(), 1);
    // Diana was at rest: told all the same, it only looks again.
    assert_match(
        launch_of(&context, "diana-brisk-birch")["message"]
            .as_str()
            .unwrap(),
        r"^\[ConsensFlow m-\d+ · T-2 · note from ConsensFlow\]\nConsensFlow quit while you were working on T-2\. Go on where you stopped\.$",
    );
    assert!(cut_notes(&context, tiers.id("chief")).is_empty());
    assert_eq!(launch_of(&context, "chief")["message"], Value::Null);
}

#[test]
fn a_session_waiting_for_an_answer_comes_back_at_rest_and_is_given_the_answer_that_came_meanwhile()
{
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    // Diana asks the chief and ends its turn: T-2 waits for the answer.
    let question = context.ask(project, "diana-brisk-birch", "chief", 2, "Which dialect?");
    context.adapter.answer("diana", "I asked the chief.");
    context.pass().unwrap();
    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["working", "waiting"]
    );
    let native = native_of(&context, tiers.id("diana-brisk-birch"));
    let after = quit_and_start(&context);

    // The answer comes while the project waits for the human.
    let answer = context.answer(project, question.id, "ANSI");
    after.pass().unwrap();
    assert_eq!(context.message(answer.id).state, "queued");

    after.resume_project(project).unwrap();
    let diana = launch_of(&context, "diana-brisk-birch");
    assert_eq!(diana["resume"], native.unwrap().as_str());
    assert_eq!(diana["message"], Value::Null, "at rest: it is told nothing");
    after.pass().unwrap();
    after.pass().unwrap();
    assert_eq!(context.message(answer.id).state, "delivered");
    assert_eq!(tiers.task(2).task.state, "working");
    assert!(
        cut_notes(&context, tiers.id("diana-brisk-birch")).is_empty(),
        "no note: its turn was over"
    );
    assert_eq!(
        cut_notes(&context, tiers.id("zeus-amber-pine")).len(),
        1,
        "zeus, whose turn was cut, is told"
    );
}

#[test]
fn the_chief_the_quit_cut_is_told_first_and_what_waited_for_it_follows() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.note(project.id, "human", "chief", "Run the tests");
    context.pass().unwrap();
    context.pass().unwrap();
    // The chief is at work on it when the app quits, and the human's next words wait.
    let waiting = context.note(project.id, "human", "chief", "Then tell me");
    let after = quit_and_start(&context);

    after.resume_project(project.id).unwrap();
    assert_match(
        launch_of(&context, "chief")["message"].as_str().unwrap(),
        &format!(r"^\[ConsensFlow m-\d+ · note from ConsensFlow\]\n{CHIEF_CUT}$"),
    );
    after.pass().unwrap();
    assert_eq!(
        context.message(waiting.id).state,
        "queued",
        "behind the turn it began"
    );
    context.adapter.answer("chief", "Going on.");
    after.pass().unwrap();
    after.pass().unwrap();
    assert_eq!(context.message(waiting.id).state, "delivered");
    assert_eq!(
        given(&context, "chief")
            .iter()
            .filter(|text| text.contains("ConsensFlow stopped in the middle"))
            .count(),
        1
    );
}

#[test]
fn a_chief_whose_window_could_not_come_up_is_told_its_turn_was_cut_once_when_resumed_again() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.note(project.id, "human", "chief", "Run the tests");
    context.pass().unwrap();
    context.pass().unwrap();
    let chief = context.id(project.id, "chief");
    let after = quit_and_start(&context);

    // The first Resume finds no window to open: the note goes back to the queue.
    *context.adapter.prepare.borrow_mut() = Some(Rc::new(|_| Err("no window".to_owned())));
    after.resume_project(project.id).unwrap();
    let notes = cut_notes(&context, chief);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].state, "queued");

    // The human resumes again: the one that waits is the one the window opens with.
    *context.adapter.prepare.borrow_mut() = None;
    after.resume_project(project.id).unwrap();
    let notes = cut_notes(&context, chief);
    assert_eq!(notes.len(), 1, "told once, not twice");
    assert_match(
        launch_of(&context, "chief")["message"].as_str().unwrap(),
        &format!(
            r"^\[ConsensFlow m-{} · note from ConsensFlow\]",
            notes[0].id
        ),
    );
}

#[test]
fn a_worker_the_quit_cut_finishes_after_the_note_and_its_result_reaches_the_chief() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    // A conversation resumed keeps what it held: the brief is in the window that comes back.
    let held = context.adapter.agent("zeus").items;
    let adapter = Rc::clone(&context.adapter);
    *context.adapter.after_prepare.borrow_mut() = Some(Rc::new(move |handle| {
        if handle == "zeus" {
            let held = held.clone();
            adapter.with(handle, |agent| {
                let after = std::mem::replace(&mut agent.items, held);
                agent.items.extend(after);
            });
        }
    }));
    let after = quit_and_start(&context);

    after.resume_project(project.id).unwrap();
    assert_match(
        launch_of(&context, "zeus")["message"].as_str().unwrap(),
        r"T-1 · note from ConsensFlow\]\nConsensFlow quit while you were working on T-1\.",
    );
    context.adapter.answer("zeus", "Parser done");
    for _ in 0..4 {
        after.pass().unwrap();
    }

    assert_eq!(context.task(project.id, 1).task.state, "done");
    let chief = context.id(project.id, "chief");
    let result = context
        .inbox(chief)
        .into_iter()
        .find(|message| message.kind == "result")
        .expect("the result");
    assert_eq!(result.state, "delivered");
}

/// What a window that never comes up says, as Codex's does when its broker never
/// names the thread the window was opened on.
const NEVER_NAMED: &str = "the Codex broker never named the thread it was opened on";

/// Every window of `fake` from now on never comes up.
fn never_up(fake: &FakeAdapter) {
    *fake.started.borrow_mut() = Some(Rc::new(|| Err(NEVER_NAMED.to_owned())));
}

/// A resumed conversation keeps what it held: the window of `handle` that comes back
/// on `fake` holds what its record held before it (`items`).
fn keeps_its_record(fake: &Rc<FakeAdapter>, handle: &'static str) {
    let held = fake.agent(handle).items;
    let kept = Rc::clone(fake);
    *fake.after_prepare.borrow_mut() = Some(Rc::new(move |opened| {
        if opened == handle {
            let held = held.clone();
            kept.with(opened, |agent| {
                let after = std::mem::replace(&mut agent.items, held);
                agent.items.extend(after);
            });
        }
    }));
}

/// zeewin's 2-hour live run of 2026-10-19: a Codex chief whose turn ended a second
/// before the app's restart was opened again at rest, on its thread, with nothing to
/// begin a turn; its broker never named the thread, and its lane read "starting" for
/// the 78 minutes left while its worker's result waited for it. A launch whose window
/// never comes up is given up with or without a first message: the chief's window is
/// closed, its project stays open, the human is told once, and the chief is opened
/// again at the backoff with what waits for it.
#[test]
fn a_chief_opened_again_at_rest_whose_window_never_comes_up_is_closed_the_human_told_and_its_result_goes_in_with_the_next(
) {
    let context = Context::made(Made {
        codex: true,
        ..Made::default()
    });
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "codex", "agent": "apollo" },
            "staff": [{ "agent": "zeus", "harness": "claude-code", "role": "worker", "tier": "standard" }],
        }))
        .unwrap()
        .unwrap()
        .id;
    let chief = context.id(project, "chief");
    context.pass().unwrap();
    context.note(project, "human", "chief", "Fix the parser");
    context.pass().unwrap();
    // The chief's turn ends just before the quit, its worker at work.
    context.codex.answer("chief", "T-1 is with zeus.");
    context.give(project, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working");
    keeps_its_record(&context.adapter, "zeus");
    let after = quit_and_start(&context);
    // The app takes a moment to start again: the windows it opens are of later
    // generations than the ones the quit ended, which the old engine still holds.
    context.advance(1_000);

    never_up(&context.codex);
    let killed = context.host.killed().len();
    after.resume_project(project).unwrap();

    let launch = context.codex.prepared().last().cloned().unwrap();
    assert!(launch["resume"].is_string(), "on its thread: {launch}");
    assert_eq!(
        launch["message"],
        Value::Null,
        "at rest: nothing begins a turn"
    );
    let pane = format!("p{project}-chief");
    assert!(
        context.host.killed()[killed..]
            .iter()
            .any(|killed| killed.id == pane),
        "its window is closed, not left starting"
    );
    assert_eq!(context.project(project).state, "open", "the staff works on");
    assert_eq!(
        to_human(&context, project),
        [format!(
            "The chief could not start: the window never came up: {NEVER_NAMED}. What comes for the chief waits for it, and ConsensFlow tries again; you may also switch the chief."
        )]
    );

    // The worker finishes: its result waits for the chief, not spent on no window.
    context.adapter.answer("zeus", "Parser done");
    for _ in 0..3 {
        after.pass().unwrap();
    }
    assert_eq!(context.task(project, 1).task.state, "done");
    let result = context
        .inbox(chief)
        .into_iter()
        .find(|message| message.kind == "result")
        .expect("the result");
    assert_eq!((result.state.as_str(), result.attempts), ("queued", 0));

    // At the backoff the chief is opened again, with the result as its first message.
    *context.codex.started.borrow_mut() = None;
    context.advance(5_000);
    after.pass().unwrap();
    let again = context.codex.prepared().last().cloned().unwrap();
    assert_match(
        again["message"].as_str().unwrap(),
        &format!(r"^\[ConsensFlow m-{} ", result.id),
    );
    for _ in 0..2 {
        after.pass().unwrap();
    }
    assert_eq!(context.message(result.id).state, "delivered");
    assert_eq!(to_human(&context, project).len(), 1, "told once");
}

/// A Codex chief opened again at rest on its thread, as above, with a worker's
/// result waiting for it.
fn codex_chief_at_rest_after_a_restart(context: &Context) -> (i64, Restarted<'_>) {
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "codex", "agent": "apollo" },
            "staff": [{ "agent": "zeus", "harness": "claude-code", "role": "worker", "tier": "standard" }],
        }))
        .unwrap()
        .unwrap()
        .id;
    context.pass().unwrap();
    context.note(project, "human", "chief", "Fix the parser");
    context.pass().unwrap();
    context.codex.answer("chief", "T-1 is with zeus.");
    context.pass().unwrap();
    let after = quit_and_start(context);
    context.advance(1_000);
    (project, after)
}

/// A chief's window that exits while it is waited for to come up went as any
/// chief's window goes: the launch its exit settled is not given up again, its
/// window not closed twice, and the human not told the chief could not start.
#[test]
fn a_chief_window_that_exits_while_it_is_waited_for_to_come_up_is_given_up_no_more() {
    let context = Context::made(Made {
        codex: true,
        ..Made::default()
    });
    let (project, after) = codex_chief_at_rest_after_a_restart(&context);
    let release = context.codex.start_holds.hold(|_| true);
    never_up(&context.codex);
    let resuming = after.begin_resume_project(project);
    context.settle();
    context.exit("chief");
    let killed = context.host.killed().len();
    release.open();
    context.settle();
    context.finish(resuming).unwrap();

    assert_eq!(
        context.host.killed().len(),
        killed,
        "the window that went is not closed again"
    );
    assert!(
        to_human(&context, project)
            .iter()
            .all(|note| !note.starts_with("The chief could not start")),
        "{:?}",
        to_human(&context, project)
    );
}

/// The engine's pane of the chief of `project`, as it opens its windows.
fn chief_pane(project: i64) -> String {
    format!("p{project}-chief")
}

/// The launches of the chief prepared so far whose first message is `message`'s paste.
fn launches_with(context: &Context, message: i64) -> usize {
    let marker = format!("[ConsensFlow m-{message} ");
    context
        .codex
        .prepared()
        .iter()
        .filter(|launch| {
            launch["message"]
                .as_str()
                .is_some_and(|text| text.starts_with(&marker))
        })
        .count()
}

/// Sol on round 1 of the chief's restart: Codex can come up in the moment between
/// the window's last look and its close. Asked once more right before it is closed,
/// a window whose harness says it came up is kept: nothing is closed, the human is
/// told nothing, and what comes for the chief goes into that window.
#[test]
fn a_chief_window_that_comes_up_as_it_is_given_up_is_kept_and_given_what_waits() {
    let context = Context::made(Made {
        codex: true,
        ..Made::default()
    });
    let (project, after) = codex_chief_at_rest_after_a_restart(&context);
    never_up(&context.codex);
    *context.codex.came_up.borrow_mut() = Some(Rc::new(|| Some(None)));
    let killed = context.host.killed().len();
    after.resume_project(project).unwrap();

    assert!(
        !context.host.killed()[killed..]
            .iter()
            .any(|killed| killed.id == chief_pane(project)),
        "the window that came up is kept"
    );
    assert!(to_human(&context, project).is_empty(), "nobody is told");
    let launches = context.codex.prepared().len();
    let result = context.note(project, "zeus", "chief", "Parser done");
    for _ in 0..3 {
        after.pass().unwrap();
    }
    assert_eq!(context.message(result.id).state, "delivered");
    assert_eq!(
        context.codex.prepared().len(),
        launches,
        "into the window that came up"
    );
}

/// Sol and Astra on round 1 of the chief's restart: a first message the window's
/// record shows it took went in, though its window is given up as it comes up. It is
/// confirmed, and never sent again on the same thread.
#[test]
fn a_chief_first_message_its_record_shows_as_its_window_is_given_up_is_confirmed_and_never_sent_again(
) {
    let context = Context::made(Made {
        codex: true,
        ..Made::default()
    });
    let (project, after) = codex_chief_at_rest_after_a_restart(&context);
    never_up(&context.codex);
    after.resume_project(project).unwrap();
    // The result waits, and the chief is opened again with it: its window never says
    // it came up, though its record shows the result (the stand-in records a launch's
    // first message as it opens).
    let result = context.note(project, "zeus", "chief", "Parser done");
    context.advance(5_000);
    after.pass().unwrap();
    assert_eq!(launches_with(&context, result.id), 1);

    assert_eq!(
        context.message(result.id).state,
        "delivered",
        "its record shows it: it went in"
    );
    for wait in [10_000, 20_000] {
        context.advance(wait);
        after.pass().unwrap();
    }
    assert_eq!(
        launches_with(&context, result.id),
        1,
        "never sent again on the same thread"
    );
}

/// Astra on round 1 of the chief's restart: a slow window must come up in the end. The
/// next window of a participant whose window was given up is told how many were, in a
/// row, which its harness gives longer for; one that comes up starts the count again.
#[test]
fn the_next_window_of_a_chief_whose_window_was_given_up_is_given_longer_until_one_comes_up() {
    let context = Context::made(Made {
        codex: true,
        ..Made::default()
    });
    let (project, after) = codex_chief_at_rest_after_a_restart(&context);
    // Its record never shows what its windows were given: what waits stays queued.
    context.codex.forgets_first_messages.set(true);
    let before = context.codex.starts_given_up().len();
    never_up(&context.codex);
    after.resume_project(project).unwrap();
    let result = context.note(project, "zeus", "chief", "Parser done");
    for wait in [5_000, 10_000] {
        context.advance(wait);
        after.pass().unwrap();
    }
    *context.codex.started.borrow_mut() = None;
    context.advance(20_000);
    after.pass().unwrap();
    after.pass().unwrap();
    assert_eq!(
        context.codex.starts_given_up()[before..],
        [0, 1, 2, 3],
        "each window told how many before it were given up"
    );
    // The window came up: the next, after it goes, is given the first time again.
    assert_eq!(context.message(result.id).state, "delivering");
    context.exit("chief");
    assert_eq!(context.project(project).state, "suspended");
    after.resume_project(project).unwrap();
    assert_eq!(context.codex.starts_given_up().last(), Some(&0));
}

/// Astra on round 1 of the chief's restart: the exit of a window waited for is the
/// launch's end whether or not the launch had a first message. A chief whose turn the
/// quit cut is opened again with the note that says so; its window exits before it
/// comes up, which closes its project as a chief's own exit does, and gives the note
/// back. The launch is not given up again after that: nothing more is closed, and the
/// human is not told that ConsensFlow tries again.
#[test]
fn a_chief_window_with_a_first_message_that_exits_while_it_is_waited_for_is_given_up_no_more() {
    let context = Context::made(Made {
        codex: true,
        ..Made::default()
    });
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "codex", "agent": "apollo" },
            "staff": [],
        }))
        .unwrap()
        .unwrap()
        .id;
    context.pass().unwrap();
    // The chief is at work on the human's words when the app quits.
    context.note(project, "human", "chief", "Run the tests");
    context.pass().unwrap();
    context.pass().unwrap();
    let after = quit_and_start(&context);
    context.advance(1_000);
    context.codex.forgets_first_messages.set(true);
    let release = context.codex.start_holds.hold(|_| true);
    never_up(&context.codex);
    let resuming = after.begin_resume_project(project);
    context.settle();
    let launch = context.codex.prepared().last().cloned().unwrap();
    assert_match(
        launch["message"].as_str().unwrap(),
        &format!(r"note from ConsensFlow\]\n{CHIEF_CUT}$"),
    );
    context.exit("chief");
    let killed = context.host.killed().len();
    release.open();
    context.settle();
    context.finish(resuming).unwrap();

    assert_eq!(
        context.host.killed().len(),
        killed,
        "the window that went is not closed again"
    );
    assert!(
        to_human(&context, project)
            .iter()
            .all(|note| !note.starts_with("The chief could not start")),
        "{:?}",
        to_human(&context, project)
    );
    let notes = cut_notes(&context, context.id(project, "chief"));
    assert_eq!(notes.len(), 1);
    assert_eq!(
        (notes[0].state.as_str(), notes[0].attempts),
        ("queued", 0),
        "given back once, for the next window"
    );
}

/// Sol and Astra on round 2 of the chief's restart: a chief whose window was given up
/// is opened again at the backoff whatever waits for it. The window may have begun a turn
/// as it was closed, on a first message its record then holds and that is never given
/// again; closed with nothing left queued, the chief stayed closed for good.
#[test]
fn a_chief_whose_window_was_given_up_with_nothing_queued_opens_again_at_the_backoff() {
    let context = Context::made(Made {
        codex: true,
        ..Made::default()
    });
    let (project, after) = codex_chief_at_rest_after_a_restart(&context);
    let chief = context.id(project, "chief");
    never_up(&context.codex);
    after.resume_project(project).unwrap();
    assert_eq!(after.pane(chief), None, "given up");
    let launches = context.codex.prepared().len();
    after.pass().unwrap();
    assert_eq!(
        context.codex.prepared().len(),
        launches,
        "not again at once"
    );

    *context.codex.started.borrow_mut() = None;
    context.advance(5_000);
    after.pass().unwrap();
    assert_eq!(
        context.codex.prepared().len(),
        launches + 1,
        "opened again at the backoff, with nothing queued for it"
    );
    let launch = context.codex.prepared().last().cloned().unwrap();
    assert_eq!(
        launch["message"],
        Value::Null,
        "its record ends at rest: it is told nothing"
    );
    after.pass().unwrap();
    assert_ne!(after.pane(chief), None, "up");
    for _ in 0..3 {
        context.advance(600_000);
        after.pass().unwrap();
    }
    assert_eq!(
        context.codex.prepared().len(),
        launches + 1,
        "and not opened again once up"
    );
}

/// A chief whose window was given up opens again at its time until one comes up, though
/// a launch after it fails for another reason (its window not prepared, here): with
/// nothing queued, a chief that lost the mark would stay closed for good.
#[test]
fn a_chief_whose_window_was_given_up_opens_again_until_one_comes_up_though_a_launch_after_fails_otherwise(
) {
    let context = Context::made(Made {
        codex: true,
        ..Made::default()
    });
    let (project, after) = codex_chief_at_rest_after_a_restart(&context);
    let chief = context.id(project, "chief");
    never_up(&context.codex);
    after.resume_project(project).unwrap();
    *context.codex.started.borrow_mut() = None;
    let failed = Rc::new(std::cell::Cell::new(false));
    let once = Rc::clone(&failed);
    *context.codex.prepare.borrow_mut() = Some(Rc::new(move |_| {
        if once.replace(true) {
            Ok(())
        } else {
            Err("codex is broken".to_owned())
        }
    }));
    context.advance(5_000);
    after.pass().unwrap();
    assert!(
        failed.get(),
        "the launch after the give-up failed otherwise"
    );
    assert_eq!(after.pane(chief), None);
    context.advance(10_000);
    after.pass().unwrap();
    after.pass().unwrap();
    assert_ne!(after.pane(chief), None, "opened again, and up");
    assert_eq!(
        to_human(&context, project).len(),
        1,
        "the human told once, at the first"
    );
}

/// The consequence of a race no look closes (Sol and Astra on round 2 of the chief's
/// restart): the chief's window comes up, takes its first message, a worker's result,
/// and begins its turn on it, between the last look that found no thread and the close.
/// Its record then holds the result, which is confirmed and never given again, and a turn
/// with no end. The chief opens again at the backoff, told its turn was cut as the human's
/// Resume tells one the app's quit cut, goes on, reads at rest after its turn, and its
/// next step on that result is taken.
#[test]
fn a_chief_window_given_up_after_it_began_a_turn_on_its_result_opens_again_told_the_turn_was_cut_and_goes_on(
) {
    let context = Context::made(Made {
        codex: true,
        ..Made::default()
    });
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "codex", "agent": "apollo" },
            "staff": [{ "agent": "zeus", "harness": "claude-code", "role": "worker", "tier": "standard" }],
        }))
        .unwrap()
        .unwrap()
        .id;
    let chief = context.id(project, "chief");
    context.pass().unwrap();
    context.note(project, "human", "chief", "Fix the parser");
    context.pass().unwrap();
    context.codex.answer("chief", "T-1 is with zeus.");
    context.give(project, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    keeps_its_record(&context.adapter, "zeus");
    let after = quit_and_start(&context);
    context.advance(1_000);
    never_up(&context.codex);
    after.resume_project(project).unwrap();
    context.adapter.answer("zeus", "Parser done");
    for _ in 0..3 {
        after.pass().unwrap();
    }
    let result = context
        .inbox(chief)
        .into_iter()
        .find(|message| message.kind == "result")
        .expect("the result");

    // Opened again with the result, the window takes it and begins its turn (the
    // stand-in's record holds a launch's first message), and is given up all the same.
    context.advance(5_000);
    after.pass().unwrap();
    assert_eq!(launches_with(&context, result.id), 1);
    assert_eq!(context.message(result.id).state, "delivered");
    assert_eq!(after.pane(chief), None, "given up");

    // At the backoff it opens again, though nothing waits for it, told its turn was cut.
    *context.codex.started.borrow_mut() = None;
    context.advance(10_000);
    after.pass().unwrap();
    let launch = context.codex.prepared().last().cloned().unwrap();
    assert_match(
        launch["message"].as_str().unwrap(),
        &format!(r"^\[ConsensFlow m-\d+ · note from ConsensFlow\]\n{CHIEF_CUT}$"),
    );
    assert_eq!(
        launches_with(&context, result.id),
        1,
        "the result is never pasted again"
    );
    assert_eq!(cut_notes(&context, chief).len(), 1, "told once");

    // It goes on, its turn ends, and its next step on the result is taken.
    context
        .codex
        .answer("chief", "Parser checked: T-1 is good.");
    for _ in 0..2 {
        after.pass().unwrap();
    }
    assert_eq!(cut_notes(&context, chief)[0].state, "delivered");
    assert_eq!(after.activity(chief).state, ActivityState::Idle);
    context
        .ledger
        .borrow_mut()
        .accept_task(project, 1, "chief")
        .unwrap();
    after.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "accepted");
    assert_eq!(launches_with(&context, result.id), 1);
}

/// The same of a member opened again at rest with its task in hand: its turn ended
/// before the quit, so nothing begins one in the window the Resume brings back. One
/// that never comes up is closed, and nobody is left waiting on it unsaid: the human is
/// told it could not start, and the task is not left at work with no window.
#[test]
fn a_member_opened_again_at_rest_whose_window_never_comes_up_is_closed_and_said() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    // Zeus's turn ended just before the quit: at rest, its task not yet taken as done.
    context.adapter.answer("zeus", "Parser done");
    let after = quit_and_start(&context);
    context.advance(1_000);

    never_up(&context.adapter);
    after.resume_project(project.id).unwrap();
    after.pass().unwrap();

    assert_eq!(launch_of(&context, "zeus")["message"], Value::Null);
    let zeus = format!("p{}-zeus", project.id);
    assert!(
        context.host.killed().iter().any(|killed| killed.id == zeus),
        "its window is closed, not left starting"
    );
    let told = to_human(&context, project.id);
    assert!(
        told.contains(&format!(
            "@zeus could not start: the window never came up: {NEVER_NAMED}."
        )),
        "{told:?}"
    );
    assert_ne!(
        context.task(project.id, 1).task.state,
        "working",
        "not at work with no window"
    );
}

/// A member at rest with its task in hand when the app quit, whose window the Resume
/// cannot open (its harness's launch failed before any window), and a note the chief
/// told it about T-1 before the Resume: T-1 is at work with no window, and the note is
/// the first message of the window the next pass opens for it. Exits are held, as the
/// bridge host sends a window's exit after it answers its kill (the test host's own
/// raises the exit inside its kill). The staff, the engine started again, the member's
/// handle and the note.
fn told_before_a_reopen_that_failed(
    context: &Context,
) -> (Tiers<'_>, Restarted<'_>, String, MessageView) {
    let tiers = Tiers::new(context, &["zeus"]);
    let project = tiers.project.id;
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    let session = tiers.assignee(1);
    context.adapter.with(&session, |agent| agent.settled = true);
    let after = quit_and_start(context);
    context.advance(1_000);
    context.host.hold_exits.set(true);
    let reopening = session.clone();
    *context.adapter.prepare.borrow_mut() = Some(Rc::new(move |launch| {
        if launch["participant"]["handle"] == reopening.as_str() {
            Err("no harness".to_owned())
        } else {
            Ok(())
        }
    }));
    let note = context
        .ledger
        .borrow_mut()
        .note(
            project,
            &NewNote {
                from: Some("chief".to_owned()),
                to: session.clone(),
                task: Some(1),
                body: "Use the grammar in the docs".to_owned(),
            },
        )
        .unwrap();
    after.resume_project(project).unwrap();
    assert_eq!(tiers.task(1).task.state, "working", "with no window");
    assert!(to_human(context, project).contains(&format!(
        "@{session} could not start: the launch failed: no harness."
    )));
    assert_eq!(context.message(note.id).state, "queued");
    *context.adapter.prepare.borrow_mut() = None;
    (tiers, after, session, note)
}

/// The window opened for the note never comes up. Its exit is settled at the
/// close, before the note goes, in the bridge host's order, which answers the kill
/// before the exit comes (exits held here; the test host's own raises the exit inside
/// its kill): T-1 is paused and never failed, the note kept for its resume, and the
/// chief and the human told why, once each (T-47 P2-1: T-1 failed, the note "did not
/// reach").
#[test]
fn a_member_whose_window_never_comes_up_with_a_note_told_it_as_its_first_message_pauses_its_task_and_keeps_the_note_for_its_resume(
) {
    let context = Context::new();
    let (tiers, after, session, note) = told_before_a_reopen_that_failed(&context);
    let project = tiers.project.id;
    let opened = context.host.opened().len();
    never_up(&context.adapter);
    after.pass().unwrap();
    assert_eq!(context.host.opened().len(), opened + 1, "one for the note");
    assert_match(
        launch_of(&context, &session)["message"].as_str().unwrap(),
        &format!(
            r"^\[ConsensFlow m-{} · T-1 · note from @chief\]\nUse the grammar in the docs$",
            note.id
        ),
    );
    let window = context.host.last(&session).unwrap().pane;
    assert!(
        context.host.killed().contains(&window),
        "its window is closed, not left starting"
    );
    let why = format!("the window could not take its first message: {NEVER_NAMED}");
    let kept = context.message(note.id);
    assert_eq!(
        (kept.state.as_str(), kept.reason.as_deref()),
        (
            "queued",
            Some(format!("kept for T-1's resume: {why}").as_str())
        )
    );
    assert_eq!(tiers.task(1).task.state, "paused");
    let kept_words = format!("m-{}", note.id);
    let chief: Vec<String> = tiers
        .notes("chief")
        .into_iter()
        .filter(|said| said.contains(&kept_words) || said.contains(NEVER_NAMED))
        .collect();
    assert_eq!(
        chief,
        [format!(
            "T-1 is paused: @{session}'s window closed: {why}; the words told its window wait for the resume: {kept_words}. Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
        )]
    );
    let human: Vec<String> = to_human(&context, project)
        .into_iter()
        .filter(|said| said.contains(&kept_words) || said.contains(NEVER_NAMED))
        .collect();
    assert_eq!(
        human,
        [format!(
            "{kept_words}, a note from @chief on T-1, waits for T-1's resume to reach @{session}: {why}."
        )]
    );
    assert!(
        !to_human(&context, project)
            .iter()
            .any(|said| said.contains("did not reach")),
        "{:?}",
        to_human(&context, project)
    );

    // The host's exit, when it comes, settles nothing more.
    let host = Rc::clone(&context.host);
    context.run(async move { host.exited(window).await });
    after.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "paused");
    assert_eq!(context.message(note.id).state, "queued");
    assert_eq!(
        tiers
            .notes("chief")
            .iter()
            .filter(|said| said.starts_with("T-1 is paused: "))
            .count(),
        1
    );
}

/// The same window, whose kill the host never answers: the launch is not held for it.
/// Past the kill's wait the note goes without the exit, as before the close settled it,
/// and fails, T-1 with it, whoever is told hearing why; the window's exit, once the kill
/// is answered, settles nothing more.
#[test]
fn a_member_whose_window_never_comes_up_and_is_never_killed_has_the_note_told_it_fail_after_the_wait(
) {
    let context = Context::new();
    let (tiers, after, session, note) = told_before_a_reopen_that_failed(&context);
    let project = tiers.project.id;
    let held = Gate::default();
    let (host, handle, gate) = (Rc::clone(&context.host), session.clone(), held.clone());
    *context.adapter.started.borrow_mut() = Some(Rc::new(move || {
        let window = host.last(&handle).expect("its window").pane;
        *host.hold_kill.borrow_mut() = Some((window.generation, gate.clone()));
        Err(NEVER_NAMED.to_owned())
    }));
    after.pass().unwrap();
    assert_eq!(
        tiers.task(1).task.state,
        "working",
        "the kill is not answered"
    );
    assert_eq!(context.message(note.id).state, "delivering");

    assert!(context.timers.fire_next(), "the kill's wait ends");
    context.settle();
    let why = format!("the window could not take its first message: {NEVER_NAMED}");
    let failed = context.message(note.id);
    assert_eq!(
        (failed.state.as_str(), failed.reason.as_deref()),
        ("failed", Some(why.as_str()))
    );
    assert_eq!(tiers.task(1).task.state, "failed");
    let kept_words = format!("m-{}", note.id);
    assert_eq!(
        tiers
            .notes("chief")
            .into_iter()
            .filter(|said| said.contains(&kept_words))
            .collect::<Vec<_>>(),
        [format!(
            "T-1 failed: what @chief told its window, {kept_words}, did not reach it ({why}). Reopen it with: cf task reopen T-1 \"…\""
        )]
    );
    assert_eq!(
        to_human(&context, project)
            .into_iter()
            .filter(|said| said.contains(&kept_words))
            .collect::<Vec<_>>(),
        [format!(
            "{kept_words}, a note from @chief on T-1, did not reach @{session}: {why}."
        )]
    );

    // The kill is answered, and the window's exit comes: nothing more is settled.
    let window = context.host.last(&session).unwrap().pane;
    held.open();
    context.settle();
    let host = Rc::clone(&context.host);
    context.run(async move { host.exited(window).await });
    after.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "failed");
    assert!(
        !tiers
            .notes("chief")
            .iter()
            .any(|said| said.starts_with("T-1 is paused")),
        "{:?}",
        tiers.notes("chief")
    );
}

/// The same window, whose kill the host refuses: no exit pauses T-1, and the note goes,
/// failing, T-1 with it, whoever is told hearing why (T-63, as for a window unfit for work).
#[test]
fn a_member_whose_window_never_comes_up_and_whose_kill_is_refused_has_the_note_told_it_fail_and_says_why(
) {
    let context = Context::new();
    let (tiers, after, session, note) = told_before_a_reopen_that_failed(&context);
    let project = tiers.project.id;
    never_up(&context.adapter);
    context.host.refuse_kills.set(true);
    after.pass().unwrap();

    let why = format!("the window could not take its first message: {NEVER_NAMED}");
    assert_eq!(context.message(note.id).state, "failed");
    assert_eq!(tiers.task(1).task.state, "failed");
    let kept_words = format!("m-{}", note.id);
    assert_eq!(
        tiers
            .notes("chief")
            .into_iter()
            .filter(|said| said.contains(&kept_words))
            .collect::<Vec<_>>(),
        [format!(
            "T-1 failed: what @chief told its window, {kept_words}, did not reach it ({why}). Reopen it with: cf task reopen T-1 \"…\""
        )]
    );
    assert_eq!(
        to_human(&context, project)
            .into_iter()
            .filter(|said| said.contains(&kept_words))
            .collect::<Vec<_>>(),
        [format!(
            "{kept_words}, a note from @chief on T-1, did not reach @{session}: {why}."
        )]
    );
}

/// A member at rest with its task in hand when the app quit, and a note the chief told
/// it about T-1 waiting for it: the window the Resume brings back never comes up, and is
/// given nothing. Its exit is settled at the close, in the bridge host's order (exits
/// held), so T-1 is paused at once and the note waits for its resume, which gives it
/// once. Before, T-1 stayed at work until the exit came, and the next pass pasted the
/// note into the window being closed: the note read delivered, and its words were lost.
/// The chief is told which words wait, and why, in the note of the pause, and the human
/// once, as of words kept as their paste failed: never left to think they went in (the
/// chief's decision, T-77; before, the note of the pause named neither).
#[test]
fn a_member_opened_again_whose_window_never_comes_up_is_given_nothing_and_the_note_told_it_waits_for_its_resume(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    context.adapter.with(&session, |agent| agent.settled = true);
    let after = quit_and_start(&context);
    context.advance(1_000);
    context.host.hold_exits.set(true);
    let note = context
        .ledger
        .borrow_mut()
        .note(
            project,
            &NewNote {
                from: Some("chief".to_owned()),
                to: session.clone(),
                task: Some(1),
                body: "Use the grammar in the docs".to_owned(),
            },
        )
        .unwrap();
    never_up(&context.adapter);
    after.resume_project(project).unwrap();
    after.pass().unwrap();
    assert_eq!(launch_of(&context, &session)["message"], Value::Null);
    let window = context.host.last(&session).unwrap().pane;
    assert!(context.host.killed().contains(&window));
    assert_eq!(tiers.task(1).task.state, "paused");
    after.pass().unwrap();
    let waiting = context.message(note.id);
    assert_eq!(
        (waiting.state.as_str(), waiting.attempts),
        ("queued", 0),
        "nothing goes into the window given up"
    );
    assert!(given(&context, &session)
        .iter()
        .all(|words| !words.contains("Use the grammar in the docs")));
    let why = format!("the window never came up: {NEVER_NAMED}");
    assert_eq!(
        waiting.reason.as_deref(),
        Some(format!("kept for T-1's resume: {why}").as_str())
    );
    let kept_words = format!("m-{}", note.id);
    assert_eq!(
        tiers
            .notes("chief")
            .into_iter()
            .filter(|said| said.starts_with("T-1 is paused: "))
            .collect::<Vec<_>>(),
        [format!(
            "T-1 is paused: @{session}'s window closed: {why}; the words told its window wait for the resume: {kept_words}. Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
        )]
    );
    assert_eq!(
        to_human(&context, project)
            .into_iter()
            .filter(|said| said.contains(&kept_words))
            .collect::<Vec<_>>(),
        [format!(
            "{kept_words}, a note from @chief on T-1, waits for T-1's resume to reach @{session}: {why}."
        )]
    );

    // The exit comes, and settles nothing more; the resume's window has the note, once.
    let host = Rc::clone(&context.host);
    context.run(async move { host.exited(window).await });
    after.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "paused");
    *context.adapter.started.borrow_mut() = None;
    context.host.hold_exits.set(false);
    context.resume_task(project, 1, "Carry on");
    for _ in 0..4 {
        after.pass().unwrap();
    }
    assert_eq!(context.message(note.id).state, "delivered");
    assert_eq!(tiers.task(1).task.state, "working");
    let given = given(&context, &session);
    assert_eq!(
        given
            .iter()
            .filter(|words| words.contains("Use the grammar in the docs"))
            .count(),
        1,
        "{given:?}"
    );
}

/// A task paused as its window closed, a note the chief told its window while it waits,
/// and the chief's resume, whose window never comes up: the words that resume it, which
/// carry the note, never reach the window. The task is paused again, never failed, as a
/// task whose window closes is; the words that resumed it fail, and the note waits for the
/// next resume, which carries it, once. The chief hears both in the note of the pause, and
/// the human once each (T-77: T-1 failed on the resume's words, and the note was left
/// queued under a failed task, unsaid). Exits held, the bridge host's order.
#[test]
fn a_resume_whose_window_never_comes_up_pauses_its_task_again_and_the_note_it_carried_waits_for_the_next(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    context.adapter.with(&session, |agent| agent.settled = true);
    context.host.hold_exits.set(true);
    let window = context.host.last(&session).unwrap().pane;
    let host = Rc::clone(&context.host);
    context.run(async move { host.exited(window).await });
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "paused");
    let note = context
        .ledger
        .borrow_mut()
        .note(
            project,
            &NewNote {
                from: Some("chief".to_owned()),
                to: session.clone(),
                task: Some(1),
                body: "Use the grammar in the docs".to_owned(),
            },
        )
        .unwrap();

    never_up(&context.adapter);
    context.resume_task(project, 1, "Carry on");
    context.pass().unwrap();
    context.pass().unwrap();
    let launch = launch_of(&context, &session);
    let first = launch["message"].as_str().unwrap();
    assert!(first.contains("Use the grammar in the docs"), "{first}");
    let resumed = tiers
        .task(1)
        .messages
        .into_iter()
        .rfind(|message| message.kind == "task")
        .expect("the words that resume it");
    assert!(
        resumed.body.ends_with("Resumed: Carry on"),
        "{}",
        resumed.body
    );

    let why = format!("the window could not take its first message: {NEVER_NAMED}");
    assert_eq!(tiers.task(1).task.state, "paused", "never failed");
    assert_eq!(context.message(resumed.id).state, "failed");
    let kept = context.message(note.id);
    assert_eq!(
        (kept.state.as_str(), kept.reason.as_deref()),
        (
            "queued",
            Some(format!("kept for T-1's resume: {why}").as_str())
        )
    );
    let (words, kept_words) = (format!("m-{}", resumed.id), format!("m-{}", note.id));
    assert_eq!(
        tiers
            .notes("chief")
            .into_iter()
            .filter(|said| said.contains(&words))
            .collect::<Vec<_>>(),
        [format!(
            "T-1 is paused: the words that resumed it, {words}, did not reach @{session}'s window: {why}; the words told its window wait for the resume: {kept_words}. Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
        )]
    );
    assert!(
        !tiers
            .notes("chief")
            .iter()
            .any(|said| said.starts_with("T-1 failed")),
        "{:?}",
        tiers.notes("chief")
    );
    assert_eq!(
        to_human(&context, project)
            .into_iter()
            .rev()
            .filter(|said| said.contains(&words) || said.contains(&kept_words))
            .collect::<Vec<_>>(),
        [
            format!("{words}, a task from @chief on T-1, did not reach @{session}: {why}."),
            format!(
                "{kept_words}, a note from @chief on T-1, waits for T-1's resume to reach @{session}: {why}."
            ),
        ]
    );

    // The next resume's window comes up, and has the note, once.
    *context.adapter.started.borrow_mut() = None;
    context.host.hold_exits.set(false);
    context.resume_task(project, 1, "Carry on, again");
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(note.id).state, "delivered");
    assert_eq!(tiers.task(1).task.state, "working");
    let given = given(&context, &session);
    assert_eq!(
        given
            .iter()
            .filter(|words| words.contains("Use the grammar in the docs"))
            .count(),
        1,
        "{given:?}"
    );
}

/// The same window, whose kill the host never answers, though its exit comes: the exit
/// is the give-up's own, and pauses T-1. The close waits on the kill still, and the next
/// window of the member, opened at T-1's resume, is no close of the engine's: when it
/// crashes, its pause says how it ended and what it showed, as any crash's does (T-414
/// P3-8: the record still read as closing its own window, and the crash was told as a
/// close of the engine's, its screen left out).
#[test]
fn a_window_closed_while_a_kill_of_an_earlier_one_is_never_answered_is_no_close_of_the_engines() {
    let context = Context::new();
    let (tiers, after, session, note) = told_before_a_reopen_that_failed(&context);
    let project = tiers.project.id;
    let held = Gate::default();
    let (host, handle, gate) = (Rc::clone(&context.host), session.clone(), held.clone());
    *context.adapter.started.borrow_mut() = Some(Rc::new(move || {
        let window = host.last(&handle).expect("its window").pane;
        *host.hold_kill.borrow_mut() = Some((window.generation, gate.clone()));
        Err(NEVER_NAMED.to_owned())
    }));
    after.pass().unwrap();
    let window = context.host.last(&session).unwrap().pane;
    let host = Rc::clone(&context.host);
    context.run(async move { host.exited(window).await });
    assert!(context.timers.fire_next(), "the kill's wait ends");
    context.settle();
    assert_eq!(tiers.task(1).task.state, "paused");
    assert_eq!(context.message(note.id).state, "queued");

    // T-1 is resumed, and its next window comes up; then it crashes.
    *context.adapter.started.borrow_mut() = None;
    context.host.hold_exits.set(false);
    context.resume_task(project, 1, "Carry on");
    for _ in 0..3 {
        after.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "working");
    context
        .host
        .exits_showing(Some(3), None, Some(&["Segmentation fault"]));
    context.exit(&session);
    after.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "paused");
    let paused = tiers
        .notes("chief")
        .into_iter()
        .rev()
        .find(|said| said.starts_with("T-1 is paused: "))
        .expect("the note of the second pause");
    assert!(
        paused.starts_with(&format!(
            "T-1 is paused: @{session}'s window closed (exit code 3); its screen ended with: \"Segmentation fault\""
        )),
        "{paused}"
    );
    drop(held);
}

/// A member opened again at the Resume whose window never comes up, its kill never
/// answered in the wait: the launch goes on without the exit, the human told the member
/// could not start, and the close goes on apart. When the kill is answered at last, the
/// exit pauses T-1, and a ledger that cannot be written then fails that close: it is
/// written down in the daemon's log, never lost (T-414 P3-7: no case reached it).
#[test]
fn a_close_that_fails_after_its_kill_was_waited_for_in_vain_is_written_down() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    context.adapter.with(&session, |agent| agent.settled = true);
    let after = quit_and_start(&context);
    context.advance(1_000);
    // The exit comes only through the close itself, as the bridge host sends it after it
    // answers the kill: what fails is the close's own settling, nobody else's.
    context.host.hold_exits.set(true);
    let held = Gate::default();
    let (host, handle, gate) = (Rc::clone(&context.host), session.clone(), held.clone());
    *context.adapter.started.borrow_mut() = Some(Rc::new(move || {
        let window = host.last(&handle).expect("its window").pane;
        *host.hold_kill.borrow_mut() = Some((window.generation, gate.clone()));
        Err(NEVER_NAMED.to_owned())
    }));
    after.resume_project(project).unwrap();
    after.pass().unwrap();
    assert!(context.timers.fire_next(), "the kill's wait ends");
    context.settle();
    assert!(to_human(&context, project).contains(&format!(
        "@{session} could not start: the window never came up: {NEVER_NAMED}."
    )));
    assert_eq!(tiers.task(1).task.state, "working", "no exit has paused it");
    assert!(context.log.failures.borrow().is_empty());

    fail_next_write(&context);
    held.open();
    context.settle();
    assert_eq!(
        *context.log.failures.borrow(),
        ["the ledger could not be written"]
    );
    context.log.failures.borrow_mut().clear();
}

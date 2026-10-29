//! A start leaves every project that was open suspended, and the human's Resume
//! brings it back: the chief and each window that had work in hand, on their own
//! conversations, a window the quit cut mid-turn told so once. A Close's Resume
//! brings back the work the Close paused the same way, and no other pause. Node
//! resumed the chief at the start and paused what its windows were at, and its
//! Resume after a Close resumed no task, so none of these is held to a Node trace.

use std::cell::RefCell;
use std::rc::Rc;

use cf_engine::testing::{Context, FakeAdapter, Gate, Made, Restarted, TRUST_QUESTION};
use cf_engine::{ActivityState, SwitchWhen};
use cf_harness::contract::{Pane, Readiness};
use cf_ledger::{continuation_tasks, MessageView, NewNote, RESUME_WORDS};
use serde_json::{json, Value};

use crate::chiefs::{to, to_human, with_codex};
use crate::fixtures::{
    assert_match, fail_next_retry, fail_next_write, finished, given, native_of, Tiers,
};

/// What a worker the quit or the Close cut in T-1 is told, and what the chief is told.
const WORKER_CUT: &str =
    "ConsensFlow stopped while you were working on T-1. Go on where you stopped.";
const CHIEF_CUT: &str =
    "ConsensFlow stopped in the middle of your turn, which was cut. Go on where you stopped.";
/// How the chief's note begins, whether or not it names the turn.
const CHIEF_CUT_BEGINS: &str = "ConsensFlow stopped in the middle of your turn, which was cut";

/// Zeus and diana at work, each on a task of its own, their windows in the middle
/// of a turn: the app quits now.
pub(crate) fn at_work(context: &Context) -> Tiers<'_> {
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
pub(crate) fn quit_and_start(context: &Context) -> Restarted<'_> {
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after
}

/// The panes opened so far, by their ids.
pub(crate) fn opened(context: &Context) -> Vec<String> {
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
pub(crate) fn cut_notes(context: &Context, to: i64) -> Vec<MessageView> {
    let mut notes: Vec<MessageView> = context
        .inbox(to)
        .into_iter()
        .filter(|message| {
            message.kind == "note"
                && message.sender.is_none()
                && (message
                    .body
                    .starts_with("ConsensFlow stopped while you were working on T-")
                    || message.body.starts_with(CHIEF_CUT_BEGINS))
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

/// The same where zeus's turn had ended before the quit: the Resume has no turn
/// to tell it of, and its continuation is not what keeps the window closed (one
/// that names a paused task names none of it as it is begun). The work the
/// Resume brings back is the work at work.
#[test]
fn a_task_the_human_paused_whose_window_was_at_rest_brings_back_no_window_for_it() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    context
        .adapter
        .answer("zeus-amber-pine", "Still on the lexer.");
    context.human_pause(project, 1);
    let after = quit_and_start(&context);

    after.resume_project(project).unwrap();
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

/// The project is closed and then resumed: the panes opened by the Resume.
fn closed_and_resumed(context: &Context, project: i64) -> Vec<String> {
    context.close_project(project).unwrap();
    assert_eq!(context.project(project).state, "suspended");
    let before = opened(context).len();
    context.resume_project(project).unwrap();
    opened(context)[before..].to_vec()
}

/// The owner's question of 2026-10-16: "on resume tasks should resume, right?"
#[test]
fn a_resume_after_a_close_brings_back_the_chief_first_and_then_the_work_the_close_paused_and_not_a_task_the_human_paused(
) {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    tiers.open_for("worker", "light", "Write the docs");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(3).task.state, "working");
    let hera = tiers.assignee(3);
    context.human_pause(project, 3);
    context.pass().unwrap();
    let natives = ["zeus-amber-pine", "diana-brisk-birch"]
        .map(|handle| native_of(&context, tiers.id(handle)).expect("its conversation"));

    let reopened = closed_and_resumed(&context, project);

    assert_eq!(
        reopened,
        ["p1-chief", "p1-zeus-amber-pine", "p1-diana-brisk-birch"],
        "the chief first, then each window the Close paused the work of"
    );
    assert_eq!(
        [1, 2, 3].map(|number| tiers.task(number).task.state),
        ["working", "working", "paused"],
        "the human's pause stays"
    );
    for ((handle, number), native) in [("zeus-amber-pine", 1), ("diana-brisk-birch", 2)]
        .into_iter()
        .zip(natives)
    {
        let launch = launch_of(&context, handle);
        assert_eq!(launch["resume"], native.as_str(), "@{handle}, on its own");
        assert_match(
            launch["message"].as_str().unwrap(),
            &format!(
                r"^\[ConsensFlow m-\d+ · T-{number} · note from ConsensFlow\]\nConsensFlow stopped while you were working on T-{number}\. Go on where you stopped\.$"
            ),
        );
    }
    // The note that told the chief they were paused went with the pause, ungiven.
    let note = context
        .inbox(tiers.id("chief"))
        .into_iter()
        .find(|message| message.body.contains(" is paused: "))
        .expect("the Close's note");
    assert_eq!(
        (note.state.as_str(), note.reason.as_deref()),
        ("cancelled", Some("T-2 resumed"))
    );
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!(
        given(&context, "chief")
            .iter()
            .all(|text| !text.contains(" is paused: ")),
        "the chief is never given the news that they are paused"
    );
    assert_eq!(
        opened(&context)
            .iter()
            .filter(|id| id.contains(hera.as_str()))
            .count(),
        1,
        "the window of T-3 does not come back"
    );
    // Given once each, and nothing written again.
    for handle in ["zeus-amber-pine", "diana-brisk-birch"] {
        assert_eq!(cut_notes(&context, tiers.id(handle)).len(), 1, "@{handle}");
    }
}

#[test]
fn the_work_whose_windows_went_with_the_chiefs_comes_back_with_the_resume() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    // The chief's window ends by itself: the project is suspended, and every window goes.
    context.exit("chief");
    assert_eq!(context.project(project).state, "suspended");
    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["paused", "paused"]
    );
    let before = opened(&context).len();

    context.resume_project(project).unwrap();

    assert_eq!(
        opened(&context)[before..],
        ["p1-chief", "p1-zeus-amber-pine", "p1-diana-brisk-birch"]
    );
    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["working", "working"]
    );
    assert_eq!(cut_notes(&context, tiers.id("zeus-amber-pine")).len(), 1);
}

#[test]
fn the_chiefs_pause_and_its_tell_stay_paused_through_a_close_and_a_resume() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    tiers.open_for("worker", "light", "Write the docs");
    context.pass().unwrap();
    context.pass().unwrap();
    // `cf task pause T-1`, and `cf tell T-3`, which pauses T-3 to put its words in.
    context.pause_task(project, 1);
    let hera = tiers.assignee(3);
    context.tell(project, &hera, 3, "Use the new style");
    context.pass().unwrap();
    assert_eq!(
        [1, 3].map(|number| tiers.task(number).task.state),
        ["paused", "paused"]
    );

    let reopened = closed_and_resumed(&context, project);

    assert_eq!(reopened, ["p1-chief", "p1-diana-brisk-birch"]);
    assert_eq!(
        [1, 2, 3].map(|number| tiers.task(number).task.state),
        ["paused", "working", "paused"],
        "the chief's pauses are the chief's to end"
    );
}

#[test]
fn a_task_whose_window_closed_by_itself_stays_paused_through_a_close_and_a_resume() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    // Zeus's window ends in the middle of T-1, with the project open (B2's note).
    context.exit("zeus");
    assert_eq!(tiers.task(1).task.state, "paused");

    let reopened = closed_and_resumed(&context, project);

    assert_eq!(reopened, ["p1-chief", "p1-diana-brisk-birch"]);
    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["paused", "working"],
        "a window that closed by itself is the chief's to resume"
    );
    // The chief hears of it as it would have without the Close, and not of T-2.
    for _ in 0..2 {
        context.pass().unwrap();
    }
    let told: Vec<String> = given(&context, "chief")
        .into_iter()
        .filter(|text| text.contains(" is paused: "))
        .collect();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].contains("T-1 is paused: @zeus-amber-pine's window closed"));
    assert!(!told[0].contains("T-2"), "{told:?}");
}

#[test]
fn a_window_the_close_could_not_close_that_then_closes_by_itself_leaves_its_task_paused() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    context.host.refuse_kills.set(true);
    let refused = context.close_project(project).unwrap_err();
    assert_match(&refused.to_string(), "would not close");
    context.host.refuse_kills.set(false);
    assert_eq!(context.project(project).state, "suspended");
    // Zeus's window ends by itself while the project is closed: the Close did not close it.
    context.exit("zeus");
    assert_eq!(tiers.task(1).task.state, "paused");

    context.resume_project(project).unwrap();

    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["paused", "working"],
        "a window that closed by itself is the chief's to resume"
    );
}

/// Both reviews of 2026-10-16: a worker's harness that crashes while the Close
/// waits for the answer to its window's kill, and whose kill then fails, closed by
/// itself; only an exit the kill made is the Close's.
#[test]
fn a_window_that_ends_by_itself_while_its_kill_is_answered_and_whose_kill_fails_leaves_its_task_paused(
) {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = context.host.last("zeus").expect("its window").pane;
    let answer = Gate::default();
    *context.host.hold_kill.borrow_mut() = Some((zeus.generation, answer.clone()));
    let closing = context.begin_close_project(project);
    context.settle();
    // Zeus's harness crashes while its kill waits; then the kill fails.
    context.exit("zeus");
    context.host.refuse_kills.set(true);
    answer.open();
    let refused = context.finish(closing).unwrap_err();
    assert_match(&refused.to_string(), "would not close");
    context.host.refuse_kills.set(false);

    context.resume_project(project).unwrap();

    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["paused", "working"],
        "a window that closed by itself is the chief's to resume"
    );
}

/// Calliope's re-check of 2026-10-25 (T-503): the same race, moved to a window that asks
/// whether to trust its folder. The Close looks at such a window's screen before it kills
/// it, to read the human's yes (`bank_yes_before_close`), and a window that ends by itself
/// during that look is taken for one the Close closed unless the Close's kill is marked
/// before the look, so that a kill that then fails takes the Close's mark off its task.
#[test]
fn a_window_that_asks_and_ends_by_itself_while_the_close_looks_at_its_screen_and_whose_kill_fails_leaves_its_task_paused(
) {
    let context = Context::new();
    context.adapter.asks_to_trust("/work/repository", false);
    context
        .host
        .set_snapshot(json!({ "tail": [TRUST_QUESTION] }));
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = context.host.last("zeus").expect("its window").pane;
    let id = zeus.id.clone();
    let looking = context
        .host
        .request_holds
        .hold(move |args| args[0] == "pane.snapshot" && args[1]["id"] == id.as_str());
    let closing = context.begin_close_project(project);
    context.settle();
    // Zeus's harness crashes while the Close looks at its screen; then its kill fails.
    context.exit("zeus");
    context.host.refuse_kills.set(true);
    looking.open();
    let refused = context.finish(closing).unwrap_err();
    assert_match(&refused.to_string(), "would not close");
    context.host.refuse_kills.set(false);

    context.resume_project(project).unwrap();

    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["paused", "working"],
        "a window that closed by itself is the chief's to resume"
    );
}

/// Calliope's re-check of T-504 (T-505, P3-1): the same, with the Close's look at the
/// screen held for many times the bound a pass waits for its steps (`STEP_WAIT`), and
/// passes made meanwhile. The Close holds the window's record all that while: no pass
/// looks at the window, and the exit that comes during the look is still the window's
/// own once the kill fails.
#[test]
fn a_window_that_asks_and_ends_by_itself_while_the_close_looks_past_a_passs_bound_and_whose_kill_fails_leaves_its_task_paused(
) {
    let context = Context::new();
    context.adapter.asks_to_trust("/work/repository", false);
    context
        .host
        .set_snapshot(json!({ "tail": [TRUST_QUESTION] }));
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = context.host.last("zeus").expect("its window").pane;
    let id = zeus.id.clone();
    let looking = context
        .host
        .request_holds
        .hold(move |args| args[0] == "pane.snapshot" && args[1]["id"] == id.as_str());
    let looks = |handle: &str| {
        let launch = context.adapter.agent(handle).launch;
        context
            .recorder
            .calls("adapter:claude-code", &["observe"])
            .iter()
            .filter(|(_, args)| args[0]["launch"]["launchId"] == launch.as_str())
            .count()
    };
    let closing = context.begin_close_project(project);
    context.settle();
    let before = looks("zeus");
    let bound = i64::try_from(cf_engine::steps::STEP_WAIT.as_millis()).unwrap();
    for _ in 0..4 {
        context.advance(2 * bound);
        context.pass().unwrap();
    }
    assert_eq!(
        looks("zeus"),
        before,
        "no pass looks at the window the Close holds"
    );
    // Zeus's harness crashes while the Close still looks at its screen; then its kill fails.
    context.exit("zeus");
    context.host.refuse_kills.set(true);
    looking.open();
    let refused = context.finish(closing).unwrap_err();
    assert_match(&refused.to_string(), "would not close");
    context.host.refuse_kills.set(false);

    context.resume_project(project).unwrap();

    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["paused", "working"],
        "a window that closed by itself is the chief's to resume"
    );
}

/// Sol's review of 2026-10-16: a Close that could not close the chief's window
/// leaves it open while the project is closed, and a pass that stepped it gave it
/// the note that the work is paused, which then reached it after the Resume had the
/// work at work again. A closed project gives no window anything: what waits for one
/// waits for the Resume, which withdraws that note in the write that opens the project.
#[test]
fn a_chief_window_the_close_could_not_close_is_given_nothing_while_closed_and_never_the_news_that_the_work_is_paused(
) {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let chief = context.host.last("chief").expect("its window").pane;
    let answer = Gate::default();
    *context.host.hold_kill.borrow_mut() = Some((chief.generation, answer.clone()));
    let closing = context.begin_close_project(project);
    context.settle();
    context.host.refuse_kills.set(true);
    answer.open();
    let refused = context.finish(closing).unwrap_err();
    assert_match(&refused.to_string(), "@chief's window would not close");
    context.host.refuse_kills.set(false);
    *context.adapter.ready.borrow_mut() = Some(Rc::new(|| Ok(Readiness::Ready)));
    let asked = || {
        context
            .recorder
            .calls("adapter:claude-code", &["ready"])
            .len()
    };
    let before = asked();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        asked(),
        before,
        "a closed project's window is not even asked whether it is ready"
    );

    context.resume_project(project).unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }

    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["working", "working"]
    );
    assert!(
        given(&context, "chief")
            .iter()
            .all(|text| !text.contains(" is paused: ")),
        "the chief was given: {:?}",
        given(&context, "chief")
    );
}

/// Astra's review of 2026-10-16: the Resume withdrew the note that the work is
/// paused, and with it the warning that the program of a window the Close could
/// not end may still run, beside which the window that comes back opens.
#[test]
fn a_resume_keeps_for_the_chief_the_word_that_a_program_the_close_could_not_end_may_still_run() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    context.host.exits_with_a_survivor(4242);

    closed_and_resumed(&context, project);
    for _ in 0..4 {
        context.pass().unwrap();
    }

    let given = given(&context, "chief");
    assert!(
        given.iter().all(|text| !text.contains(" is paused: ")),
        "{given:?}"
    );
    // The first is in its window, and the next waits for its turn to end.
    let told: Vec<(String, String)> = context
        .inbox(tiers.id("chief"))
        .into_iter()
        .rev()
        .filter(|message| message.body.contains(" resumed. Before that, "))
        .map(|message| (message.state, message.body))
        .collect();
    let warning = |number: i64, handle: &str| {
        format!(
            "T-{number} resumed. Before that, @{handle}'s window closed, but its program (process 4242) could not be ended and may still run."
        )
    };
    assert_eq!(
        told,
        [
            ("delivered".to_owned(), warning(1, "zeus-amber-pine")),
            ("queued".to_owned(), warning(2, "diana-brisk-birch")),
        ]
    );
}

/// Both reviews of 2026-10-16, round 3: a Resume that comes while the Close still
/// waits for a kill waits for the Close to end, every kill answered, before it reads
/// what the Close paused; here the kill of a window that crashed meanwhile is refused.
#[test]
fn a_resume_that_comes_while_the_close_waits_for_a_refused_kill_leaves_the_crashed_window_s_task_paused(
) {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = context.host.last("zeus").expect("its window").pane;
    let answer = Gate::default();
    *context.host.hold_kill.borrow_mut() = Some((zeus.generation, answer.clone()));
    let closing = context.begin_close_project(project);
    context.settle();
    context.exit("zeus");
    // The human resumes before the Close is over; then zeus's kill fails.
    let resuming = context.begin_resume_project(project);
    context.settle();
    context.host.refuse_kills.set(true);
    answer.open();
    let _ = context.finish(closing);
    context.host.refuse_kills.set(false);
    context.finish(resuming).unwrap();

    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["paused", "working"],
        "the window that crashed is the chief's to resume"
    );
}

/// The same, with the other timing: the kill the Close still waits for is taken
/// after the human resumed, and the task it pauses goes on with that Resume.
#[test]
fn a_resume_that_comes_while_the_close_waits_for_a_kill_resumes_the_task_that_kill_pauses() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = context.host.last("zeus").expect("its window").pane;
    let answer = Gate::default();
    *context.host.hold_kill.borrow_mut() = Some((zeus.generation, answer.clone()));
    let closing = context.begin_close_project(project);
    context.settle();
    let resuming = context.begin_resume_project(project);
    context.settle();
    answer.open();
    context.finish(closing).unwrap();
    context.finish(resuming).unwrap();

    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["working", "working"],
        "both went with the Close, and come back with the Resume"
    );
    assert_eq!(context.project(project).state, "open");
}

/// The same, of the Close the chief's window makes when it ends by itself: the
/// Resume waits for the kills of its workers' windows.
#[test]
fn a_resume_that_comes_while_the_chief_window_s_close_waits_for_a_kill_resumes_the_task_that_kill_pauses(
) {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let zeus = context.host.last("zeus").expect("its window").pane;
    let answer = Gate::default();
    *context.host.hold_kill.borrow_mut() = Some((zeus.generation, answer.clone()));
    let exiting = context.begin_exit("chief");
    context.settle();
    assert_eq!(context.project(project).state, "suspended");
    let resuming = context.begin_resume_project(project);
    context.settle();
    answer.open();
    context.finish(exiting);
    context.finish(resuming).unwrap();

    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["working", "working"]
    );
}

/// The same as the delivery that waits for its window, with the project closed
/// while the host takes the word that its turn begins, the last wait before the paste.
#[test]
fn a_delivery_whose_paste_is_about_to_go_while_the_project_closes_goes_back_to_its_queue() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let waiting = context.note(project, "human", "chief", "Any news?");
    let marking = context.background.hold_marks();
    context.pass().unwrap();
    let closing = context.begin_close_project(project);
    context.settle();
    marking.open();
    context.finish(closing).unwrap();

    assert_eq!(context.message(waiting.id).state, "queued");
    assert!(
        given(&context, "chief")
            .iter()
            .all(|text| !text.contains("Any news?")),
        "nothing pasted"
    );
}

/// Both reviews of 2026-10-16, round 3: a delivery begun before the Close that
/// waited for its window to be ready pasted after the project was suspended.
#[test]
fn a_delivery_that_waits_for_its_window_while_the_project_closes_is_not_pasted_and_waits_for_the_resume(
) {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let waiting = context.note(project, "human", "chief", "Any news?");
    *context.adapter.ready.borrow_mut() = Some(Rc::new(|| Ok(Readiness::Ready)));
    let readying = context.adapter.ready_holds.hold(|_| true);
    context.pass().unwrap();
    let closing = context.begin_close_project(project);
    context.settle();
    readying.open();
    context.finish(closing).unwrap();

    let waiting = context.message(waiting.id);
    assert_eq!(
        (waiting.state.as_str(), waiting.reason.as_deref()),
        ("queued", None),
        "never begun"
    );
    assert!(
        given(&context, "chief")
            .iter()
            .all(|text| !text.contains("Any news?")),
        "nothing pasted"
    );
}

/// Both reviews of 2026-10-16, round 3: a Switch chief that waits for the chief's
/// turn to end, in a project closed meanwhile whose Close could not close the chief's
/// window, waits for the Resume: the note asking for the handoff is not pasted. It is
/// written only as it is pasted, with the chief at rest (T-351), so a closed project
/// has none on the board at all; the Resume writes and pastes it, and the switch then
/// waits for its answer.
#[test]
fn a_switch_waiting_for_the_chief_s_turn_in_a_closed_project_waits_for_the_resume() {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.busy("chief");
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Turn, true)
        .unwrap();
    context.host.refuse_kills.set(true);
    assert!(
        context.close_project(project.id).is_err(),
        "its windows stay"
    );
    context.host.refuse_kills.set(false);
    context.adapter.answer("chief", "Done.");
    for _ in 0..3 {
        context.pass().unwrap();
    }

    let chief = context.id(project.id, "chief");
    let asking = || {
        context
            .inbox(chief)
            .into_iter()
            .filter(|message| message.body.contains("Write down where things stand"))
            .collect::<Vec<_>>()
    };
    assert!(
        asking().is_empty(),
        "it waits for the Resume: {:?}",
        asking()
    );
    assert!(
        given(&context, "chief")
            .iter()
            .all(|text| !text.contains("Write down where things stand")),
        "nothing pasted"
    );
    assert!(context.dispatcher.pending_switch(chief).is_some());

    context.resume_project(project.id).unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    let written = asking();
    assert_eq!(written.len(), 1, "{written:?}");
    assert_eq!(written[0].state, "delivered", "the Resume pastes it");
    assert!(
        context.dispatcher.pending_switch(chief).is_some(),
        "the switch waits for its answer"
    );
}

/// The same, of a switch asked with no note: once the chief's turn is over it would
/// go, closing the chief's window and opening the new chief's in a closed project.
#[test]
fn a_switch_with_no_note_waiting_for_the_chief_s_turn_in_a_closed_project_opens_no_window() {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.busy("chief");
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Turn, false)
        .unwrap();
    context.host.refuse_kills.set(true);
    assert!(
        context.close_project(project.id).is_err(),
        "its windows stay"
    );
    context.host.refuse_kills.set(false);
    context.adapter.answer("chief", "Done.");
    let windows = context.host.opened().len();
    for _ in 0..3 {
        context.pass().unwrap();
    }

    assert_eq!(context.host.opened().len(), windows, "no window opens");
    assert!(
        context
            .dispatcher
            .pending_switch(context.id(project.id, "chief"))
            .is_some(),
        "the switch waits for the Resume"
    );
}

/// The same, of a chief out of quota, whose switch goes as soon as it is asked:
/// in a closed project it waits for the Resume too, and no window opens for it.
#[test]
fn a_switch_of_a_chief_out_of_quota_in_a_closed_project_waits_for_the_resume() {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.busy("chief");
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Turn, false)
        .unwrap();
    let chief = context.id(project.id, "chief");
    context
        .ledger
        .borrow_mut()
        .mark_out(chief, "2099-01-01T00:00:00.000Z", "out of quota")
        .unwrap();
    context.host.refuse_kills.set(true);
    assert!(
        context.close_project(project.id).is_err(),
        "its windows stay"
    );
    context.host.refuse_kills.set(false);
    let windows = context.host.opened().len();
    for _ in 0..3 {
        context.pass().unwrap();
    }

    assert_eq!(context.host.opened().len(), windows, "no window opens");
    assert!(
        context.dispatcher.pending_switch(chief).is_some(),
        "the switch waits"
    );
}

#[test]
fn a_held_task_stays_paused_with_its_hold_through_a_close_and_a_resume() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    context.hold_task(project, 1);

    let reopened = closed_and_resumed(&context, project);

    assert_eq!(reopened, ["p1-chief", "p1-diana-brisk-birch"]);
    let held = tiers.task(1).task;
    assert_eq!(
        (held.state.as_str(), held.held_until.as_deref()),
        ("paused", Some("2099-01-01T00:00:00.000Z")),
        "it goes on at its hold's end"
    );
    assert_eq!(tiers.task(2).task.state, "working");
}

#[test]
fn a_task_the_close_paused_whose_session_left_meanwhile_stays_paused_for_the_chief() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    context.close_project(project).unwrap();
    // The human takes zeus off the staff while the project is closed (Project
    // settings): its session leaves with it, and its paused task stays.
    context.remove_member(project, "zeus").unwrap();

    context.resume_project(project).unwrap();

    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["paused", "working"],
        "no window to come back in"
    );
    for _ in 0..2 {
        context.pass().unwrap();
    }
    let told: Vec<String> = given(&context, "chief")
        .into_iter()
        .filter(|text| text.contains(" is paused: "))
        .collect();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].starts_with("[ConsensFlow") && told[0].contains("T-1 is paused: "));
    assert!(!told[0].contains("T-2 is paused"), "{told:?}");
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
        r"^\[ConsensFlow m-\d+ · T-1 · note from ConsensFlow\]\nConsensFlow stopped while you were working on T-1\. Go on where you stopped\.$",
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

#[test]
fn a_cut_note_an_earlier_build_left_queued_in_its_words_gives_way_to_one_in_this_build_s() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let after = quit_and_start(&context);
    // 3.0.4's Resume told zeus in its words, and the app quit before the window came.
    let earlier = context.note_from_consensflow(
        project,
        "zeus-amber-pine",
        "ConsensFlow quit while you were working on T-1. Go on where you stopped.",
    );

    after.resume_project(project).unwrap();

    // Its record still reads the turn cut: it is told once, in the words of now.
    assert_eq!(context.message(earlier.id).state, "cancelled");
    let notes = cut_notes(&context, tiers.id("zeus-amber-pine"));
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert_match(
        launch_of(&context, "zeus-amber-pine")["message"]
            .as_str()
            .unwrap(),
        &format!(
            r"^\[ConsensFlow m-{} · T-1 · note from ConsensFlow\]\n{WORKER_CUT}$",
            notes[0].id
        ),
    );
}

/// The sites chief, 2026-10-23 (T-814): the Resume told it its turn was cut, the human
/// closed the project again a second later, and the note waited, queued, to be typed in
/// at the next Resume, days later and whatever the record said by then.
#[test]
fn a_cut_note_an_earlier_resume_left_queued_is_withdrawn_where_the_record_now_reads_at_rest() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.note(project.id, "human", "chief", "Run the tests");
    context.pass().unwrap();
    context.pass().unwrap();
    let chief = context.id(project.id, "chief");
    let after = quit_and_start(&context);
    *context.adapter.prepare.borrow_mut() = Some(Rc::new(|_| Err("no window".to_owned())));
    after.resume_project(project.id).unwrap();
    let earlier = cut_notes(&context, chief);
    assert_eq!(earlier.len(), 1);
    assert_eq!(earlier[0].state, "queued");

    // By the next Resume the record reads the window at rest.
    context.adapter.answer("chief", "Tests pass.");
    *context.adapter.prepare.borrow_mut() = None;
    after.resume_project(project.id).unwrap();

    assert_eq!(context.message(earlier[0].id).state, "cancelled");
    assert_eq!(cut_notes(&context, chief).len(), 1, "none written anew");
    assert_eq!(launch_of(&context, "chief")["message"], Value::Null);
    after.pass().unwrap();
    after.pass().unwrap();
    assert!(
        given(&context, "chief")
            .iter()
            .all(|text| !text.contains(CHIEF_CUT_BEGINS)),
        "never given"
    );
}

#[test]
fn a_continuation_an_earlier_resume_left_queued_is_withdrawn_where_the_record_now_reads_at_rest() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let after = quit_and_start(&context);
    // An earlier Resume told zeus to go on, and the project closed again before the window
    // came; zeus's record reads at rest now.
    let earlier = context.note_from_consensflow(project, "zeus-amber-pine", WORKER_CUT);
    context.adapter.answer("zeus", "Lexer written.");

    after.resume_project(project).unwrap();

    assert_eq!(context.message(earlier.id).state, "cancelled");
    assert_eq!(
        launch_of(&context, "zeus-amber-pine")["message"],
        Value::Null
    );
}

#[test]
fn a_cut_note_names_the_turn_by_when_its_message_came_and_its_first_words() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    context.adapter.busy("chief");
    context
        .adapter
        .turn_began_with("chief", Some("2026-10-15 20:41 UTC"), "Run the tests");
    context.adapter.turn_began_with(
        "zeus",
        None,
        "[ConsensFlow m-3 · T-1 · task from @chief] Write the parser",
    );
    let after = quit_and_start(&context);

    after.resume_project(project).unwrap();

    assert_match(
        launch_of(&context, "chief")["message"].as_str().unwrap(),
        &format!(
            "^\\[ConsensFlow m-\\d+ · note from ConsensFlow\\]\n{CHIEF_CUT_BEGINS}: the turn that began at 2026-10-15 20:41 UTC with \"Run the tests\"\\. Go on where you stopped\\.$"
        ),
    );
    let zeus = launch_of(&context, "zeus-amber-pine");
    assert_eq!(
        zeus["message"].as_str().unwrap().split_once('\n').unwrap().1,
        "ConsensFlow stopped while you were working on T-1, in the turn that began with \"[ConsensFlow m-3 · T-1 · task from @chief] Write the parser\". Go on where you stopped."
    );
    // The ledger knows it for zeus's continuation of T-1, which goes with any move of it.
    let note = cut_notes(&context, tiers.id("zeus-amber-pine")).remove(0);
    assert_eq!(continuation_tasks(&note.body), Some(vec![1]));
}

#[test]
fn a_cut_note_that_names_its_turn_and_still_holds_is_told_once_across_resumes() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.note(project.id, "human", "chief", "Run the tests");
    context.pass().unwrap();
    context.pass().unwrap();
    context
        .adapter
        .turn_began_with("chief", Some("2026-10-15 20:41 UTC"), "Run the tests");
    let chief = context.id(project.id, "chief");
    let after = quit_and_start(&context);
    *context.adapter.prepare.borrow_mut() = Some(Rc::new(|_| Err("no window".to_owned())));
    after.resume_project(project.id).unwrap();
    let earlier = cut_notes(&context, chief);
    assert_eq!(earlier.len(), 1);

    *context.adapter.prepare.borrow_mut() = None;
    after.resume_project(project.id).unwrap();

    let notes = cut_notes(&context, chief);
    assert_eq!(notes.len(), 1, "told once: {notes:?}");
    assert_eq!(notes[0].id, earlier[0].id);
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
        r"^\[ConsensFlow m-\d+ · T-1 · note from ConsensFlow\]\nConsensFlow stopped while you were working on T-1\. Go on where you stopped\.$",
    );
    assert_eq!(cut_notes(&context, tiers.id("zeus-amber-pine")).len(), 1);
    // Diana was at rest: told all the same, it only looks again.
    assert_match(
        launch_of(&context, "diana-brisk-birch")["message"]
            .as_str()
            .unwrap(),
        r"^\[ConsensFlow m-\d+ · T-2 · note from ConsensFlow\]\nConsensFlow stopped while you were working on T-2\. Go on where you stopped\.$",
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
        r"T-1 · note from ConsensFlow\]\nConsensFlow stopped while you were working on T-1\.",
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

/// Zeus's T-1 in its window as the Close finds it: `answered`, a complete answer its turn
/// wrote, then the turn still under way or at rest.
fn zeus_wrote(context: &Context, answered: &str, settled: bool) {
    let item = context
        .adapter
        .item(cf_harness::records::Role::Assistant, answered);
    context.adapter.with("zeus", |agent| {
        agent.items.push(item);
        agent.settled = settled;
    });
}

/// The results of T-`number` the ledger holds.
fn results(tiers: &Tiers<'_>, number: i64) -> Vec<String> {
    tiers
        .task(number)
        .messages
        .into_iter()
        .filter(|message| message.kind == "result")
        .map(|message| message.body)
        .collect()
}

/// R1 of the merge of turn-end with Resume after Close (T-408): the Resume's word to go
/// on is ConsensFlow's own note, which no start of a result is taken from, so the latest
/// message is still the brief, and the reopened conversation already holds a complete
/// answer after it. It is no result: the continuation in the record is a message to the
/// worker, and an answer is the result only where it answers the last of them; read at
/// rest before its turn, the window yields nothing, and the answer after the continuation
/// is the result.
#[test]
fn a_window_the_resume_reopens_yields_no_result_before_the_turn_after_its_continuation() {
    let context = Context::new();
    context.adapter.resumes_records.set(true);
    let tiers = at_work(&context);
    let project = tiers.project.id;
    zeus_wrote(&context, "The lexer is half done", false);

    closed_and_resumed(&context, project);
    // The reopened window, its record and the continuation in it, is read at rest
    // before its turn: the reader's rule would hold it, and this holds without it.
    context.adapter.with("zeus", |agent| agent.settled = true);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "working");
    assert_eq!(results(&tiers, 1), Vec::<String>::new(), "no result yet");

    context.adapter.answer("zeus", "The lexer is done");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results(&tiers, 1), ["The lexer is done"]);
}

/// L10 of T-405, the cut turn: Finish took the answer written before the Close, though the
/// worker had been told to go on and had not answered that. A message ConsensFlow pasted
/// into the window is answered by what follows it, for Finish as for the turn's end (the
/// harness's own notifications, which wake a held task's turns, are no such message).
#[test]
fn finish_of_a_task_the_resume_brought_back_waits_for_the_answer_to_its_continuation() {
    let context = Context::new();
    context.adapter.resumes_records.set(true);
    let tiers = at_work(&context);
    let project = tiers.project.id;
    zeus_wrote(&context, "The lexer is half done", false);
    closed_and_resumed(&context, project);
    context.adapter.with("zeus", |agent| agent.settled = true);
    context.pass().unwrap();
    let refused = context.finish_task(project, 1, "chief").unwrap_err();
    assert_eq!(
        refused.to_string(),
        "cannot finish T-1: @zeus-amber-pine has written no complete answer since its latest message about it; wait for one, or cancel it"
    );
    assert_eq!(results(&tiers, 1), Vec::<String>::new());
    let item = context
        .adapter
        .item(cf_harness::records::Role::Assistant, "The lexer is done");
    context.adapter.with("zeus", |agent| {
        agent.items.push(item);
        agent.settled = false;
    });
    let finished = context.finish_task(project, 1, "chief").expect("finished");
    assert_eq!(finished.task.state, "done");
    assert!(
        results(&tiers, 1)[0]
            .starts_with("The lexer is done\n\n(ConsensFlow: Finished at @chief's word"),
        "{:?}",
        results(&tiers, 1)
    );
}

/// L10 of T-405, the fresh window: a conversation the harness cannot open again is a new
/// one, given the brief first, and the answer after it is the result.
#[test]
fn a_task_the_resume_brings_back_in_a_fresh_window_takes_the_answer_to_its_brief_given_again() {
    let context = Context::new();
    let tiers = at_work(&context);
    let project = tiers.project.id;
    context.adapter.holds_nothing("zeus");
    closed_and_resumed(&context, project);
    let launch = launch_of(&context, "zeus-amber-pine");
    assert_eq!(launch["resume"], Value::Null, "a fresh conversation");
    assert_match(
        launch["message"].as_str().unwrap(),
        r"^\[ConsensFlow m-1 · T-1 · task from @chief\]\nWrite the parser\n\n\[ConsensFlow m-\d+ · T-1 · note from ConsensFlow\]\nConsensFlow stopped while you were working on T-1\. Go on where you stopped\.$",
    );
    context.adapter.answer("zeus", "The lexer, from the start");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results(&tiers, 1), ["The lexer, from the start"]);
}

/// L9 of T-405: a task held at the Close for a job, its worker at rest with a complete
/// answer, comes back with that job retired: the old answer is the result, with the
/// retired caveat (the chief's decision on T-405).
#[test]
fn a_task_held_for_a_job_at_the_close_is_done_after_the_resume_with_its_answer_and_the_job_said_retired(
) {
    let context = Context::new();
    context.adapter.resumes_records.set(true);
    let tiers = at_work(&context);
    let project = tiers.project.id;
    context.adapter.answer_leaving(
        "zeus",
        "The build runs; I will report when it ends",
        cf_harness::records::Pending {
            agents: 0,
            jobs: 1,
            retired: 0,
            noticed: 0,
        },
    );
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working", "held for its job");
    closed_and_resumed(&context, project);
    context.adapter.with("zeus", |agent| {
        agent.pending.retired = 1;
        agent.settled = true;
    });
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results(&tiers, 1),
        ["The build runs; I will report when it ends\n\n(ConsensFlow: no word came that the 1 background job an earlier window of @zeus-amber-pine's conversation started had ended; that window was closed, which ends what the host could tell was the window's, and may not have ended a job its shell detached.)"]
    );
}

/// L9's other half: where the reading after the Resume still counts the job (the host could
/// not prove the program that started it gone), the task is held again, for that job, and
/// the chief is told so again: the Close ended the hold the window was in (`HeldPart`).
#[test]
fn a_task_held_for_a_job_at_the_close_whose_job_the_resume_could_not_retire_is_held_again() {
    let context = Context::new();
    context.adapter.resumes_records.set(true);
    let tiers = at_work(&context);
    let project = tiers.project.id;
    let job = cf_harness::records::Pending {
        agents: 0,
        jobs: 1,
        retired: 0,
        noticed: 0,
    };
    context
        .adapter
        .answer_leaving("zeus", "The build runs; I will report when it ends", job);
    context.pass().unwrap();
    let held = |tiers: &Tiers<'_>| {
        tiers
            .notes("chief")
            .into_iter()
            .filter(|note| note.starts_with("T-1 goes on in the background: @zeus-amber-pine ended its turn with 1 background job running"))
            .count()
    };
    assert_eq!(held(&tiers), 1, "told of the hold before the Close");
    closed_and_resumed(&context, project);
    context.adapter.with("zeus", |agent| {
        agent.pending = job;
        agent.settled = true;
    });
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "working", "held again");
    assert_eq!(results(&tiers, 1), Vec::<String>::new());
    assert_eq!(held(&tiers), 2, "told again, once, after the Resume");
    // A hold that ends only at Finish, which takes the answer and says the job may run on.
    let finished = context.finish_task(project, 1, "chief").expect("finished");
    assert_eq!(finished.task.state, "done");
    assert_eq!(
        results(&tiers, 1),
        ["The build runs; I will report when it ends\n\n(ConsensFlow: Finished at @chief's word while @zeus-amber-pine's harness still counted 1 background job running, which finishing ends none of: it may run on until its window closes, or past it where the host cannot tell what is the window's.)"]
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
            "T-1 failed: what @chief told its window, {kept_words}, did not reach it ({why}). Reopen it with: cf task reopen T-1 \"…\"\n\nIts words, {kept_words}, not delivered, so that you can send them again:\n\nUse the grammar in the docs"
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
            "T-1 failed: what @chief told its window, {kept_words}, did not reach it ({why}). Reopen it with: cf task reopen T-1 \"…\"\n\nIts words, {kept_words}, not delivered, so that you can send them again:\n\nUse the grammar in the docs"
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
        [
            format!(
                "T-1 is paused: the words that resumed it, {words}, did not reach @{session}'s window: {why}; the words told its window wait for the resume: {kept_words}. Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
            ),
            // Whoever resumed it has the words back, as the message said them (T-922).
            format!(
                "Your {words} on T-1 did not reach @{session}'s window: {why}.\n\nIts words, {words}, not delivered, so that you can send them again:\n\n{}",
                resumed.body
            ),
        ]
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

/// The give-up of the P3-8 case, its kill never answered, and T-1's next window closed
/// with the project, its kill held too: that window's exit, come before its kill's
/// answer, marks T-1's pause as the Close's. The earlier kill, refused at last, is no
/// answer of the later one's, which is taken, and the human's Resume brings T-1 back
/// (the merge of Resume after Close's kill under way, one a record, with a give-up that
/// goes on while its kill waits: the earlier kill's refusal took the later kill's mark
/// back, and T-1 stayed paused through the Resume).
#[test]
fn a_kill_of_an_earlier_window_refused_at_last_leaves_a_later_windows_close_its_own() {
    let context = Context::new();
    let (tiers, after, session, _note) = told_before_a_reopen_that_failed(&context);
    let project = tiers.project.id;
    let earlier = Gate::default();
    let (host, handle, gate) = (Rc::clone(&context.host), session.clone(), earlier.clone());
    *context.adapter.started.borrow_mut() = Some(Rc::new(move || {
        let window = host.last(&handle).expect("its window").pane;
        *host.hold_kill.borrow_mut() = Some((window.generation, gate.clone()));
        Err(NEVER_NAMED.to_owned())
    }));
    after.pass().unwrap();
    let first = context.host.last(&session).unwrap().pane;
    let (host, gone) = (Rc::clone(&context.host), first.clone());
    context.run(async move { host.exited(gone).await });
    assert!(context.timers.fire_next(), "the kill's wait ends");
    context.settle();
    assert_eq!(tiers.task(1).task.state, "paused");

    // T-1 is resumed into a window that comes up; the human closes the project.
    *context.adapter.started.borrow_mut() = None;
    context.resume_task(project, 1, "Carry on");
    for _ in 0..3 {
        after.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "working");
    let second = context.host.last(&session).unwrap().pane;
    assert_ne!(second, first);
    let later = Gate::default();
    *context.host.hold_kill.borrow_mut() = Some((second.generation, later.clone()));
    let closing = after.begin_close_project(project);
    context.settle();
    let host = Rc::clone(&context.host);
    context.run(async move { host.exited(second).await });
    assert_eq!(tiers.task(1).task.state, "paused");
    // The earlier window's kill is refused, then the Close's is taken.
    context.host.refuse_kills.set(true);
    earlier.open();
    context.settle();
    context.host.refuse_kills.set(false);
    later.open();
    context.finish(closing).unwrap();
    assert_eq!(context.project(project).state, "suspended");

    after.resume_project(project).unwrap();
    assert_eq!(
        tiers.task(1).task.state,
        "working",
        "the Close's pause, which its Resume ends"
    );
}

/// Calliope's landing rule 4 of T-414: a give-up and a Close on one window pause its task
/// once, for the exit comes once. A member opened again at the Resume whose window never
/// comes up, its kill not answered in the wait: the launch goes on without the exit. The
/// chief tells T-1's window a note, and the human closes the project; then the kill is
/// taken. The exit pauses T-1 as the Close's, the chief's note of it names the note,
/// which waits, and the human's Resume brings T-1 back, its note of the pause withdrawn,
/// and the note goes in once, in the window the Resume opens.
#[test]
fn a_give_up_whose_kill_is_taken_after_a_close_pauses_its_task_once_for_the_resume() {
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
    assert_eq!(tiers.task(1).task.state, "working", "no exit has paused it");
    *context.adapter.started.borrow_mut() = None;
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

    let closing = after.begin_close_project(project);
    context.settle();
    held.open();
    context.finish(closing).unwrap();
    assert_eq!(context.project(project).state, "suspended");
    assert_eq!(tiers.task(1).task.state, "paused");
    let kept = context.message(note.id);
    let why = format!("@{session}'s window closed");
    assert_eq!(
        (kept.state.as_str(), kept.reason.as_deref()),
        (
            "queued",
            Some(format!("kept for T-1's resume: {why}").as_str())
        )
    );
    let chief = context.id(project, "chief");
    let pauses: Vec<MessageView> = context
        .inbox(chief)
        .into_iter()
        .filter(|said| said.body.starts_with("T-1 is paused: "))
        .collect();
    assert_eq!(pauses.len(), 1, "one pause: {pauses:#?}");
    assert_eq!(
        pauses[0].body,
        format!(
            "T-1 is paused: {why}; the words told its window wait for the resume: m-{}. Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation.",
            note.id
        )
    );
    assert!(to_human(&context, project).contains(&format!(
        "m-{}, a note from @chief on T-1, waits for T-1's resume to reach @{session}: {why}.",
        note.id
    )));

    context.host.hold_exits.set(false);
    after.resume_project(project).unwrap();
    for _ in 0..3 {
        after.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "working");
    assert_eq!(
        context.message(pauses[0].id).state,
        "cancelled",
        "the note of the pause is withdrawn"
    );
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

/// A window's generation and the gates of its kills, held one by one.
type HeldKills = (u64, Vec<Gate>);

/// A member opened again at the Resume whose window never comes up, its kills held one
/// by one as `refused` says ([`FakeHost::hold_kills`]) and its exits held, the first
/// kill, the give-up's, not answered in its wait: the launch goes on without the exit.
/// The staff, the engine started again, the member's handle, its window and the gates
/// of its kills.
fn given_up_with_its_kill_unanswered<'a>(
    context: &'a Context,
    refused: &[bool],
) -> (Tiers<'a>, Restarted<'a>, String, Pane, Vec<Gate>) {
    let tiers = Tiers::new(context, &["zeus"]);
    let project = tiers.project.id;
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    context.adapter.with(&session, |agent| agent.settled = true);
    let after = quit_and_start(context);
    context.advance(1_000);
    context.host.hold_exits.set(true);
    // Each window's kills are held once, as the first launch that sees it starts: the
    // adapter's start is asked of every launch, the chief's too.
    let gates: Rc<RefCell<Vec<HeldKills>>> = Rc::default();
    let (host, handle, held, refused) = (
        Rc::clone(&context.host),
        session.clone(),
        Rc::clone(&gates),
        refused.to_vec(),
    );
    *context.adapter.started.borrow_mut() = Some(Rc::new(move || {
        let window = host.last(&handle).expect("its window").pane;
        let mut held = held.borrow_mut();
        if !held
            .iter()
            .any(|(generation, _)| *generation == window.generation)
        {
            held.push((window.generation, host.hold_kills(&window, &refused)));
        }
        Err(NEVER_NAMED.to_owned())
    }));
    after.resume_project(project).unwrap();
    after.pass().unwrap();
    assert!(context.timers.fire_next(), "the kill's wait ends");
    context.settle();
    assert_eq!(tiers.task(1).task.state, "working", "no exit has paused it");
    *context.adapter.started.borrow_mut() = None;
    let window = context.host.last(&session).unwrap().pane;
    let gates = gates
        .borrow()
        .iter()
        .find(|(generation, _)| *generation == window.generation)
        .map(|(_, gates)| gates.clone())
        .expect("its kills held");
    (tiers, after, session, window, gates)
}

/// Rule 4's case, the Close's kill of the window answered first and taken: the exit is
/// the Close's, and T-1's pause its. The give-up's kill of the same window, refused
/// after, is no answer of the Close's: the human's Resume brings T-1 back (Calliope's
/// P3-3 of T-510: each kill of the window kept an entry, the Close's answer took the
/// give-up's and the exit marked the Close's, and the refusal took the mark back).
#[test]
fn a_give_up_whose_kill_is_refused_after_the_closes_kill_of_its_window_was_taken_leaves_the_pause_the_closes(
) {
    let context = Context::new();
    let (tiers, after, _, _, gates) = given_up_with_its_kill_unanswered(&context, &[true, false]);
    let project = tiers.project.id;
    let closing = after.begin_close_project(project);
    context.settle();
    gates[1].open();
    context.finish(closing).unwrap();
    assert_eq!(context.project(project).state, "suspended");
    assert_eq!(tiers.task(1).task.state, "paused");
    gates[0].open();
    context.settle();

    context.host.hold_exits.set(false);
    after.resume_project(project).unwrap();
    assert_eq!(
        tiers.task(1).task.state,
        "working",
        "the Close's pause, which its Resume ends"
    );
}

/// The same, the give-up's kill refused only after the human's Resume and a second
/// Close: the first Close's pause is over, and the second Close's, of the window the
/// Resume opened, is no pause the earlier kill can take back (Calliope's P3-2 of T-510:
/// the refusal took back whatever mark the task held then, the second Close's too).
#[test]
fn a_give_up_whose_kill_is_refused_after_a_resume_and_a_second_close_leaves_the_second_closes_pause(
) {
    let context = Context::new();
    let (tiers, after, session, first, gates) =
        given_up_with_its_kill_unanswered(&context, &[true, false]);
    let project = tiers.project.id;
    let closing = after.begin_close_project(project);
    context.settle();
    gates[1].open();
    context.finish(closing).unwrap();
    assert_eq!(tiers.task(1).task.state, "paused");

    context.host.hold_exits.set(false);
    after.resume_project(project).unwrap();
    for _ in 0..3 {
        after.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "working");
    assert_ne!(context.host.last(&session).unwrap().pane, first);
    after.begin_close_project(project);
    context.settle();
    assert_eq!(context.project(project).state, "suspended");
    assert_eq!(tiers.task(1).task.state, "paused");
    gates[0].open();
    context.settle();

    after.resume_project(project).unwrap();
    assert_eq!(
        tiers.task(1).task.state,
        "working",
        "the second Close's pause, which its Resume ends"
    );
}

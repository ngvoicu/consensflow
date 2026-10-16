//! A start leaves every project that was open suspended, and the human's Resume
//! brings it back: the chief and each window that had work in hand, on their own
//! conversations, a window the quit cut mid-turn told so once. A Close's Resume
//! brings back the work the Close paused the same way, and no other pause. Node
//! resumed the chief at the start and paused what its windows were at, and its
//! Resume after a Close resumed no task, so none of these is held to a Node trace.

use std::rc::Rc;

use cf_engine::testing::{Context, Gate, Restarted};
use cf_engine::SwitchWhen;
use cf_harness::contract::Readiness;
use cf_ledger::{MessageView, RESUME_WORDS};
use serde_json::Value;

use crate::chiefs::{to, with_codex};
use crate::fixtures::{assert_match, finished, given, native_of, Tiers};

/// What a worker the quit or the Close cut in T-1 is told, and what the chief is told.
const WORKER_CUT: &str =
    "ConsensFlow stopped while you were working on T-1. Go on where you stopped.";
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
                && (message
                    .body
                    .starts_with("ConsensFlow stopped while you were working on T-")
                    || message.body == CHIEF_CUT)
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
/// window, waits for the Resume: the note asking for the handoff is not pasted.
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

    let asked = context
        .inbox(context.id(project.id, "chief"))
        .into_iter()
        .find(|message| message.body.contains("Write down where things stand"))
        .expect("the note asking for the handoff");
    assert_eq!(asked.state, "queued", "it waits for the Resume");
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
fn a_cut_note_an_earlier_build_left_queued_in_its_words_is_the_one_the_window_is_given() {
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

    assert_match(
        launch_of(&context, "zeus-amber-pine")["message"]
            .as_str()
            .unwrap(),
        &format!(
            r"^\[ConsensFlow m-{} · note from ConsensFlow\]\nConsensFlow quit while",
            earlier.id
        ),
    );
    assert!(
        cut_notes(&context, tiers.id("zeus-amber-pine")).is_empty(),
        "no note in the new words: it is told once"
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

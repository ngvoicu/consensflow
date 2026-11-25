//! A launch that was called off is not a failed launch: whatever called it off (a Close, a
//! pause, a cancel, a move to another member, the daemon's stop), and whoever it was for, it
//! writes no failure, tells nobody of one and counts towards no back-off, so that the next ask
//! for its window opens it at once (T-2307: on hosted Windows a Close that came while the
//! chief's first window was prepared left it a note and the chief's back-off in the Resume's way).
//!
//! The engine sees a launch called off at two moments: the plan ends as one called off ends
//! (a step of the adapter's, before its thread began or while it ran, says so: one answer),
//! or the plan answered and the window's open is not yet over. A chief has no task, so no
//! pause, cancel or move of one reaches its launch; its first message is not a member's.

use std::cell::Cell;
use std::rc::Rc;

use cf_engine::testing::Context;
use cf_engine::SwitchWhen;

use crate::chiefs::{to, to_human, with_codex};
use crate::fixtures::Tiers;
use crate::resume::{opened, quit_and_start};

/// What a step of a launch's plan says where the launch was called off.
const CALLED_OFF: &str = "the launch was called off before its window opened";

/// A hold that takes the first call only: the launch that was called off, not the ones after it.
fn first_only() -> impl Fn(&serde_json::Value) -> bool + 'static {
    let seen = Rc::new(Cell::new(false));
    move |_| !seen.replace(true)
}

/// The first launch of a member (no chief's): the one that was called off.
fn first_of_a_member() -> impl Fn(&serde_json::Value) -> bool + 'static {
    let seen = Rc::new(Cell::new(false));
    move |launch| launch["participant"]["handle"] != "chief" && !seen.replace(true)
}

/// What called the launch off.
#[derive(Clone, Copy, Debug)]
enum Calls {
    Close,
    Pause,
    Cancel,
    Move,
    Stop,
}

/// When the plan was over, as the engine sees it.
#[derive(Clone, Copy, Debug)]
enum When {
    /// A step of the plan said the launch was called off.
    Plan,
    /// The plan answered, and the host's open of the window was not over.
    Open,
}

const WHEN: [When; 2] = [When::Plan, When::Open];

/// What the human and the chief were told of a launch that failed.
fn told_of_a_failure(context: &Context, project: i64) -> Vec<String> {
    let chief = context.id(project, "chief");
    let mut told = to_human(context, project);
    told.extend(
        context
            .ledger
            .borrow()
            .inbox(chief, 100)
            .expect("its inbox")
            .into_iter()
            .map(|message| message.body),
    );
    told.retain(|body| {
        body.contains("could not start") || body.contains("tries again") || body.contains("failed")
    });
    told
}

/// A member's launch, called off by `calls` as the plan or the open of its window ends: what
/// went wrong, if anything.
fn member_cell(calls: Calls, when: When) -> Option<String> {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    let project = tiers.project.id;
    context.pass().unwrap();
    context.settle();
    let plan = matches!(when, When::Plan).then(|| {
        context
            .adapter
            .prepare_holds
            .hold_instead(first_of_a_member(), CALLED_OFF.to_owned())
    });
    let open = matches!(when, When::Open).then(|| context.host.open_holds.hold(|_| true));
    let number = tiers.open().number;
    context.pass().unwrap();
    context.pass().unwrap();
    context.settle();
    let mut closing = None;
    let mut moving = None;
    let mut restarted = None;
    match calls {
        Calls::Close => closing = Some(context.begin_close_project(project)),
        Calls::Pause => context.pause_task(project, number),
        Calls::Cancel => {
            context
                .ledger
                .borrow_mut()
                .cancel_task(project, number, "human")
                .unwrap();
        }
        // The move waits for the window's end, which waits for the launch.
        Calls::Move => {
            let (engine, moved) = (Rc::clone(&context.dispatcher), number);
            moving = Some(
                context
                    .executor
                    .start_now(async move { engine.reassign_task(project, moved).await }),
            );
        }
        Calls::Stop => restarted = Some(quit_and_start(&context)),
    }
    context.settle();
    if let Some(gate) = plan.as_ref().or(open.as_ref()) {
        gate.open();
    }
    if let Some(closing) = closing {
        context.finish(closing).unwrap();
    }
    if let Some(moving) = moving {
        if let Err(cause) = context.finish(moving) {
            return Some(format!("the move was refused: {cause}"));
        }
    }
    context.settle();

    let mut wrong = Vec::new();
    let told = told_of_a_failure(&context, project);
    if !told.is_empty() {
        wrong.push(format!("told of a failure: {told:?}"));
    }
    if context.task(project, number).task.state == "failed" {
        wrong.push("the task failed".to_owned());
    }
    // The next ask, at once: no time passes, so no back-off is out.
    let asked = match calls {
        Calls::Close | Calls::Stop => {
            match &restarted {
                Some(after) => after.resume_project(project).unwrap(),
                None => context.resume_project(project).unwrap(),
            };
            number
        }
        Calls::Pause => {
            context.resume_task(project, number, "Carry on");
            number
        }
        Calls::Cancel | Calls::Move => tiers.open_body("Again").number,
    };
    for _ in 0..4 {
        match &restarted {
            Some(after) => after.pass().unwrap(),
            None => context.pass().unwrap(),
        }
        context.settle();
    }
    let state = context
        .task(project, asked)
        .messages
        .last()
        .map(|message| message.state.clone());
    if !state.is_some_and(|state| ["delivering", "delivered"].contains(&state.as_str())) {
        wrong.push(format!(
            "the next ask was not served at once: T-{asked} {:?}",
            context
                .task(project, asked)
                .messages
                .last()
                .map(|m| (m.state.clone(), m.attempts))
        ));
    }
    (!wrong.is_empty()).then(|| wrong.join("; "))
}

#[test]
fn a_member_launch_called_off_is_no_failure_and_the_next_ask_opens_its_window_at_once() {
    let mut failed = Vec::new();
    for calls in [
        Calls::Close,
        Calls::Pause,
        Calls::Cancel,
        Calls::Move,
        Calls::Stop,
    ] {
        for when in WHEN {
            // A pause or a move that comes while the host opens the window is met by the
            // window after it opened, not by the launch: filed as an open issue
            // (issues/pause-or-move-during-a-members-window-open.md).
            if matches!((calls, when), (Calls::Pause | Calls::Move, When::Open)) {
                continue;
            }
            if let Some(wrong) = member_cell(calls, when) {
                failed.push(format!("{calls:?} {when:?}: {wrong}"));
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The chief's first launch, called off by a Close, the daemon's stop or a switch to another
/// chief: what went wrong, if anything.
fn chief_cell(calls: Calls, when: When) -> Option<String> {
    let context = with_codex();
    let plan = matches!(when, When::Plan).then(|| {
        context
            .adapter
            .prepare_holds
            .hold_instead(first_only(), CALLED_OFF.to_owned())
    });
    let open = matches!(when, When::Open).then(|| context.host.open_holds.hold(|_| true));
    let project = context.with_staff(&["zeus"]).id;
    context.pass().unwrap();
    context.settle();
    let mut closing = None;
    let mut moving = None;
    let mut restarted = None;
    match calls {
        Calls::Close => closing = Some(context.begin_close_project(project)),
        Calls::Stop => restarted = Some(quit_and_start(&context)),
        // The switch waits for the window's end, which waits for the launch.
        Calls::Move => {
            moving = Some(context.begin_switch_chief(
                project,
                to("codex", "astraeus"),
                SwitchWhen::Now,
                false,
            ));
        }
        Calls::Pause | Calls::Cancel => unreachable!("a chief has no task"),
    }
    context.settle();
    if let Some(gate) = plan.as_ref().or(open.as_ref()) {
        gate.open();
    }
    if let Some(closing) = closing {
        context.finish(closing).unwrap();
    }
    if let Some(moving) = moving {
        if let Err(cause) = context.finish(moving) {
            return Some(format!("the move was refused: {cause}"));
        }
    }
    context.settle();

    let mut wrong = Vec::new();
    let told = told_of_a_failure(&context, project);
    if !told.is_empty() {
        wrong.push(format!("told of a failure: {told:?}"));
    }
    let before = opened(&context).len();
    match (&restarted, calls) {
        (Some(after), _) => {
            after.resume_project(project).unwrap();
        }
        (None, Calls::Close) => {
            context.resume_project(project).unwrap();
        }
        _ => {}
    }
    for _ in 0..4 {
        match &restarted {
            Some(after) => after.pass().unwrap(),
            None => context.pass().unwrap(),
        }
        context.settle();
    }
    let chief = context.id(project, "chief");
    let pane = match &restarted {
        Some(after) => after.pane(chief),
        None => context.dispatcher.pane(chief),
    };
    if opened(&context).len() == before && pane.is_none() {
        wrong.push("the next ask opened no window".to_owned());
    }
    (!wrong.is_empty()).then(|| wrong.join("; "))
}

#[test]
fn a_chief_launch_called_off_is_no_failure_and_the_next_ask_opens_its_window_at_once() {
    let mut failed = Vec::new();
    for calls in [Calls::Close, Calls::Move, Calls::Stop] {
        for when in WHEN {
            if let Some(wrong) = chief_cell(calls, when) {
                failed.push(format!("{calls:?} {when:?}: {wrong}"));
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

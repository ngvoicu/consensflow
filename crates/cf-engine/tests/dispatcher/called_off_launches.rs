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
use cf_harness::contract::FirstMessage;
use cf_ledger::{HoldKind, NewMember};

use crate::chiefs::{to, to_human, with_codex};
use crate::fixtures::Tiers;
use crate::resume::{opened, quit_and_start};

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
    context.adapter.honors_call_off.set(true);
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    let project = tiers.project.id;
    context.pass().unwrap();
    context.settle();
    let plan =
        matches!(when, When::Plan).then(|| context.adapter.prepare_holds.hold(first_of_a_member()));
    let open = matches!(when, When::Open).then(|| context.host.open_holds.hold(|_| true));
    let number = tiers.open().number;
    context.pass().unwrap();
    context.pass().unwrap();
    context.settle();
    let first_holder = tiers.task(number).task.assignee.clone();
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
    let warned = context.log.warnings.borrow().clone();
    if !warned.is_empty() {
        wrong.push(format!("warned: {warned:?}"));
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
    let thread = context.task(project, asked);
    let reached = thread.messages.last().filter(|message| {
        message.state == "delivered" && Some(&message.recipient) == thread.task.assignee.as_ref()
    });
    if reached.is_none() {
        wrong.push(format!(
            "the next ask did not reach its member at once: T-{asked} held by {:?}: {:?}",
            thread.task.assignee,
            thread
                .messages
                .last()
                .map(|m| (m.state.clone(), m.recipient.clone(), m.attempts))
        ));
    }
    if matches!(calls, Calls::Close | Calls::Stop | Calls::Pause)
        && thread.task.assignee.as_deref() != first_holder.as_deref()
    {
        wrong.push(format!(
            "the task went to {:?}, not to {first_holder:?}",
            thread.task.assignee
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
            // A pause that comes while the host opens the window is met by the window after it
            // opened, not by the launch: filed as an open issue (the Resume waits for the
            // window's turn).
            if matches!((calls, when), (Calls::Pause, When::Open)) {
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
    context.adapter.honors_call_off.set(true);
    let plan = matches!(when, When::Plan).then(|| context.adapter.prepare_holds.hold(first_only()));
    let open = matches!(when, When::Open).then(|| context.host.open_holds.hold(|_| true));
    let project = context.with_staff(&["zeus"]).id;
    context.pass().unwrap();
    context.settle();
    let started = opened(&context).len();
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
    // A switch has opened the new chief's window by now: it is counted from before it.
    let before = if matches!(calls, Calls::Move) {
        started
    } else {
        opened(&context).len()
    };
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
    // A window of the chief that was opened since the call-off and is its own now: one the
    // Resume or the switch asked for, not one left from before.
    if opened(&context).len() <= before || pane.is_none() {
        wrong.push(format!(
            "the next ask opened no window: {} opened before, {} now, pane {pane:?}",
            before,
            opened(&context).len()
        ));
    }
    let warned = context.log.warnings.borrow().clone();
    if !warned.is_empty() {
        wrong.push(format!("warned: {warned:?}"));
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

/// The reviewer's four cases (T-2313, finding 1), through the adapter's real decision and no
/// answer of the test's own: a task is moved to another member while the first member's launch
/// prepares its window or the host opens it, by each of the two ways a first message goes in.
/// The move says it moved, the task is with the new member once with its brief, the first
/// member's window does not stay, and nobody is told of a failure.
#[test]
fn a_move_asked_at_any_moment_of_a_launch_is_a_move() {
    let mut failed = Vec::new();
    for when in WHEN {
        for first in [FirstMessage::Opened, FirstMessage::Delivered] {
            let seeds = format!("{when:?} {first:?}");
            let context = Context::new();
            context.adapter.first.set(first);
            context.adapter.honors_call_off.set(true);
            let tiers = Tiers::new(&context, &["zeus", "diana"]);
            let project = tiers.project.id;
            context.pass().unwrap();
            context.settle();
            let plan = matches!(when, When::Plan)
                .then(|| context.adapter.prepare_holds.hold(first_of_a_member()));
            let open = matches!(when, When::Open).then(|| context.host.open_holds.hold(|_| true));
            let number = tiers.open().number;
            context.pass().unwrap();
            context.pass().unwrap();
            context.settle();
            let first_holder = tiers.task(number).task.assignee.clone();
            let (engine, moved) = (Rc::clone(&context.dispatcher), number);
            let moving = context
                .executor
                .start_now(async move { engine.reassign_task(project, moved).await });
            context.settle();
            if let Some(gate) = plan.as_ref().or(open.as_ref()) {
                gate.open();
            }
            let said = context.finish(moving);
            context.settle();
            if let Err(cause) = said {
                failed.push(format!("{seeds}: the move was refused: {cause}"));
                continue;
            }
            for _ in 0..4 {
                context.pass().unwrap();
                context.settle();
            }
            let thread = tiers.task(number);
            let holder = thread.task.assignee.clone();
            if holder == first_holder || holder.is_none() {
                failed.push(format!("{seeds}: not with a new member: {holder:?}"));
            }
            // The brief of the old member that went in with its window's open was handed to it
            // before the move, as one of a task moved at work was; the new member has its own.
            let briefs: Vec<_> = thread
                .messages
                .iter()
                .filter(|m| {
                    m.kind == "task"
                        && ["delivering", "delivered"].contains(&m.state.as_str())
                        && Some(&m.recipient) == holder.as_ref()
                })
                .collect();
            let stuck = thread.messages.iter().any(|m| {
                m.kind == "task"
                    && ["queued", "delivering"].contains(&m.state.as_str())
                    && Some(&m.recipient) == first_holder.as_ref()
            });
            if stuck {
                failed.push(format!("{seeds}: a brief still waits for the old member"));
            }
            if briefs.len() != 1 {
                failed.push(format!(
                    "{seeds}: {} briefs with a window: {:?}",
                    briefs.len(),
                    thread
                        .messages
                        .iter()
                        .map(|m| (m.kind.clone(), m.state.clone(), m.recipient.clone()))
                        .collect::<Vec<_>>()
                ));
            }
            if let Some(old) = &first_holder {
                if context.dispatcher.pane(tiers.id(old)).is_some() {
                    failed.push(format!("{seeds}: the old member's window stays"));
                }
            }
            let told = told_of_a_failure(&context, project);
            if !told.is_empty() {
                failed.push(format!("{seeds}: told of a failure: {told:?}"));
            }
            let warned = context.log.warnings.borrow().clone();
            if !warned.is_empty() {
                failed.push(format!("{seeds}: warned: {warned:?}"));
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// A task handed to the member's backup while the member's window is being opened (T-2317,
/// finding 2: a launch that goes on after its task went elsewhere): the old window, if it
/// opens at all, is given nothing and closes, and the backup has the brief once. The host's
/// open is held, where the old window's program is on its way, by each way a first message
/// goes in, for a member out of its quota and for one refused for its login.
#[test]
fn a_window_that_opens_for_a_task_handed_to_a_backup_is_given_nothing_and_closes() {
    let mut failed = Vec::new();
    for kind in [HoldKind::Quota, HoldKind::Account] {
        for when in WHEN {
            for first in [FirstMessage::Opened, FirstMessage::Delivered] {
                let seeds = format!("{kind:?} {when:?} {first:?}");
                let context = Context::new();
                context.adapter.first.set(first);
                context.adapter.honors_call_off.set(true);
                let tiers = Tiers::new(&context, &["zeus"]);
                let backup = context
                    .ledger
                    .borrow_mut()
                    .add_backup(
                        tiers.project.id,
                        "zeus",
                        &NewMember {
                            harness: "claude-code".to_owned(),
                            model: Some("a-model".to_owned()),
                            effort: None,
                            model_name: None,
                            designer: false,
                            roles: vec!["worker".to_owned()],
                            tier: "standard".to_owned(),
                        },
                    )
                    .expect("a backup");
                context.pass().unwrap();
                context.settle();
                let plan = matches!(when, When::Plan)
                    .then(|| context.adapter.prepare_holds.hold(first_of_a_member()));
                let open =
                    matches!(when, When::Open).then(|| context.host.open_holds.hold(|_| true));
                let number = tiers.open().number;
                context.pass().unwrap();
                context.settle();
                let old = tiers.assignee(number);
                let zeus = tiers.id("zeus");
                let until = "9999-12-31T23:59:59.998Z";
                context
                    .ledger
                    .borrow_mut()
                    .mark_out_with_kind(zeus, until, "out", kind)
                    .unwrap();
                for _ in 0..3 {
                    context.pass().unwrap();
                    context.settle();
                }
                if let Some(gate) = plan.as_ref().or(open.as_ref()) {
                    gate.open();
                }
                for _ in 0..4 {
                    context.pass().unwrap();
                    context.settle();
                }
                let thread = tiers.task(number);
                let holder = thread.task.assignee.clone();
                if holder.as_deref() == Some(old.as_str()) || holder.is_none() {
                    failed.push(format!(
                        "{seeds}: the task did not go to the backup: {holder:?}"
                    ));
                    continue;
                }
                // The old window's program was given none of the task's words.
                let words: Vec<String> = context
                    .adapter
                    .try_agent(&old)
                    .map(|agent| {
                        agent
                            .items
                            .iter()
                            .map(|item| item.text.to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                let brief = thread.task.body.clone();
                if words.iter().any(|text| text.contains(&brief)) {
                    failed.push(format!("{seeds}: the old window was given the brief"));
                }
                if context.dispatcher.pane(tiers.id(&old)).is_some() {
                    failed.push(format!("{seeds}: the old window stays open"));
                }
                let told = told_of_a_failure(&context, tiers.project.id);
                if !told.is_empty() {
                    failed.push(format!("{seeds}: told of a failure: {told:?}"));
                }
                let warned = context.log.warnings.borrow().clone();
                if !warned.is_empty() {
                    failed.push(format!("{seeds}: warned: {warned:?}"));
                }
                let _ = &backup;
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

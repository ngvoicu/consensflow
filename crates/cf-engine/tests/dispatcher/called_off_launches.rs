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
                // Where the hand-over came before the open was asked of the host, it never is.
                if matches!(when, When::Plan) && context.host.last(&old).is_some() {
                    failed.push(format!("{seeds}: the old window's open was asked"));
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

/// What the once-probe's task asks of its window.
const ACTION: &str = "Append one entry to the work receipt and report completion.";

/// A window at work acts once when its pane is the participant's own and its record holds the
/// brief, as a stand-in agent does: an entry in the work receipt, and its answer.
fn act_where_asked(
    context: &Context,
    project: i64,
    receipt: &std::path::Path,
    acted: &mut std::collections::HashSet<(String, u64)>,
) {
    use std::io::Write;
    for open in context.host.opened() {
        let Some(handle) = open.pane.id.strip_prefix(&format!("p{project}-")) else {
            continue;
        };
        let who = context
            .project(project)
            .participants
            .into_iter()
            .find(|who| who.handle == handle)
            .expect("its participant");
        if context.dispatcher.pane(who.id).as_ref() != Some(&open.pane) {
            continue;
        }
        let adapter = if who.harness.as_deref() == Some("codex") {
            &context.codex
        } else {
            &context.adapter
        };
        let agent = adapter.agent(handle);
        let asked = agent
            .items
            .iter()
            .any(|item| item.role == cf_harness::records::Role::User && item.text.contains(ACTION));
        if agent.settled || !asked || !acted.insert((open.pane.id.clone(), open.pane.generation)) {
            continue;
        }
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(receipt)
            .unwrap();
        writeln!(file, "{handle}").unwrap();
        adapter.answer(
            handle,
            &format!("The work receipt was updated by {handle}."),
        );
    }
}

/// The reviewer's once-probe (T-2317): a Devin-mode primary, whose brief goes in with the host's
/// open, is out of its login while that open is held, and its Codex-mode backup does the task;
/// the human accepts it; then the old host answers. One entry in the receipt, not two: the old
/// window is ended as the open answers. Run twenty times over. The stand-in acts where its
/// record holds the brief and its pane is its own, so a window that is closed first never does;
/// what a real program can do between its start and the close is the limit the brain's page
/// names.
#[test]
fn a_task_handed_to_a_backup_as_the_old_open_answers_is_done_once() {
    use cf_harness::seams::Time;
    for run in 0..20 {
        let root = tempfile::tempdir().unwrap();
        let receipt = root.path().join("entries.txt");
        let context = Context::made(cf_engine::testing::Made {
            harnesses: vec!["devin", "claude-code"],
            codex: true,
            ..Default::default()
        });
        context.adapter.first.set(FirstMessage::Opened);
        context.codex.first.set(FirstMessage::Delivered);
        context.adapter.honors_call_off.set(true);
        let project = context
            .open_project(serde_json::json!({
                "directory": root.path(), "name": "receipt",
                "chief": { "harness": "claude-code", "model": "claude-opus-5" },
                "staff": [{ "agent": "zeus", "harness": "devin", "role": "worker", "tier": "standard" }],
            }))
            .unwrap()
            .unwrap()
            .id;
        let primary = context
            .project(project)
            .participants
            .into_iter()
            .find(|who| who.harness.as_deref() == Some("devin") && who.member_id.is_none())
            .unwrap()
            .handle;
        let backup = context
            .ledger
            .borrow_mut()
            .add_backup(
                project,
                &primary,
                &NewMember {
                    harness: "codex".into(),
                    model: Some("gpt-6-astra".into()),
                    model_name: Some("athena".into()),
                    effort: None,
                    roles: vec!["worker".into()],
                    tier: "standard".into(),
                    designer: false,
                },
            )
            .unwrap()
            .handle;
        let task = |body: &str| {
            context
                .create_task(
                    project,
                    cf_ledger::NewTask {
                        from: "chief".into(),
                        pool: Some("worker".into()),
                        tier: Some("standard".into()),
                        body: body.into(),
                        ..Default::default()
                    },
                )
                .task
                .number
        };
        let passes = |count: usize| {
            for _ in 0..count {
                context.pass().unwrap();
                context.settle();
            }
        };
        let first = task("First window");
        passes(4);
        let source = context.task(project, first).task.assignee.unwrap();
        let gate = context.host.open_holds.hold({
            let seen = Rc::new(Cell::new(false));
            move |_| !seen.replace(true)
        });
        let number = task(ACTION);
        passes(2);
        let old = context.task(project, number).task.assignee.unwrap();
        context.adapter.refuse(
            &source,
            cf_harness::records::Quota::Unavailable {
                at: Some(cf_base::time::iso(context.time.wall_ms())),
                because: "the test login expired".to_owned(),
            },
        );
        passes(6);
        let current = context.task(project, number).task.assignee.unwrap();
        assert_ne!(current, old, "run {run}: the task went to the backup");
        let mut acted = std::collections::HashSet::new();
        act_where_asked(&context, project, &receipt, &mut acted);
        passes(6);
        assert_eq!(
            context.task(project, number).task.state,
            "done",
            "run {run}"
        );
        context
            .ledger
            .borrow_mut()
            .accept_task(project, number, "human")
            .unwrap();
        passes(3);
        gate.open();
        context.settle();
        passes(4);
        act_where_asked(&context, project, &receipt, &mut acted);
        passes(4);
        let entries = std::fs::read_to_string(&receipt).unwrap();
        assert_eq!(
            entries.lines().count(),
            1,
            "run {run}: one task, one entry: {entries:?} (backup {backup})"
        );
    }
}

/// What of a member's window is left when the first message of a launch is each kind a member
/// gets, for a task in each state: the message is given (delivered), or waits in its queue, or
/// was cancelled or failed by the ledger before the launch, and never stays "delivering" with
/// no window holding it. Printed one line a cell (`--nocapture`) for the account.
#[test]
fn no_kind_of_first_message_is_dropped_in_silence_whatever_its_tasks_state() {
    use cf_ledger::NewNote;
    let kinds = [
        "reopen words",
        "tell",
        "urgent tell",
        "note",
        "answer",
        "follow-up brief",
    ];
    let states = [
        "working",
        "paused",
        "done",
        "accepted",
        "failed",
        "cancelled",
        "handed on",
    ];
    let mut stuck = Vec::new();
    for kind in kinds {
        for state in states {
            let context = Context::new();
            let tiers = Tiers::new(&context, &["zeus", "diana"]);
            let project = tiers.project.id;
            tiers.open();
            for _ in 0..3 {
                context.pass().unwrap();
                context.settle();
            }
            let session = tiers.assignee(1);
            let question = (kind == "answer")
                .then(|| context.ask(project, &session, "chief", 1, "Which format?"));
            match state {
                "working" => {}
                "paused" => context.pause_task(project, 1),
                "done" | "accepted" => {
                    context.adapter.answer(&session, "Parser done");
                    for _ in 0..3 {
                        context.pass().unwrap();
                        context.settle();
                    }
                    if state == "accepted" {
                        let _ = context.ledger.borrow_mut().accept_task(project, 1, "chief");
                    }
                }
                "failed" => {
                    let _ = context.ledger.borrow_mut().fail_task(project, 1, "test");
                }
                "cancelled" => {
                    let _ = context.ledger.borrow_mut().cancel_task(project, 1, "human");
                }
                _ => {
                    if context.reassign_task(project, 1).is_err() {
                        println!("TABLE {kind} | {state} | cannot be set up");
                        continue;
                    }
                }
            }
            context.settle();
            if context.host.last(&session).is_some() {
                context.exit(&session);
            }
            let made: Result<i64, String> = match kind {
                "reopen words" => {
                    let reopened = context
                        .ledger
                        .borrow_mut()
                        .reopen_task(project, 1, "chief", "Handle empty input too")
                        .map(|_| ())
                        .map_err(|cause| cause.to_string());
                    reopened.map(|()| context.task(project, 1).messages.last().map_or(0, |m| m.id))
                }
                "urgent tell" => context
                    .ledger
                    .borrow_mut()
                    .ask(
                        project,
                        &cf_ledger::NewQuestion {
                            from: Some("chief".to_owned()),
                            to: session.clone(),
                            body: Some("Use the v2 grammar".to_owned()),
                            task: Some(1),
                            questions: None,
                            urgent: true,
                        },
                    )
                    .map(|m| m.id)
                    .map_err(|cause| cause.to_string()),
                "tell" => context
                    .ledger
                    .borrow_mut()
                    .tell_queued(project, 1, "chief", "Use the v2 grammar")
                    .map(|m| m.id)
                    .map_err(|cause| cause.to_string()),
                "note" => context
                    .ledger
                    .borrow_mut()
                    .note(
                        project,
                        &NewNote {
                            from: Some("chief".to_owned()),
                            to: session.clone(),
                            task: Some(1),
                            body: "Use tabs".to_owned(),
                        },
                    )
                    .map(|m| m.id)
                    .map_err(|cause| cause.to_string()),
                "answer" => {
                    let id = question.as_ref().map(|q| q.id).unwrap();
                    let chief = context.id(project, "chief");
                    context
                        .ledger
                        .borrow_mut()
                        .answer(id, chief, Some(&serde_json::json!("Use tabs")), None)
                        .map(|m| m.id)
                        .map_err(|cause| cause.to_string())
                }
                _ => context
                    .ledger
                    .borrow_mut()
                    .create_task(
                        project,
                        &cf_ledger::NewTask {
                            from: "chief".to_owned(),
                            to: Some(session.clone()),
                            body: "Also the empty case".to_owned(),
                            ..Default::default()
                        },
                    )
                    .map(|created| created.message.map_or(0, |m| m.id))
                    .map_err(|cause| cause.to_string()),
            };
            let id = match made {
                Ok(id) if id != 0 => id,
                other => {
                    println!("TABLE {kind} | {state} | not producible: {other:?}");
                    continue;
                }
            };
            for _ in 0..4 {
                context.pass().unwrap();
                context.settle();
            }
            let after = context.message(id);
            let owner = context
                .project(project)
                .participants
                .into_iter()
                .find(|who| who.handle == after.recipient);
            let window = owner
                .as_ref()
                .is_some_and(|who| context.dispatcher.pane(who.id).is_some());
            println!(
                "TABLE {kind} | {state} | message {} (attempts {}) | task {} | window {}",
                after.state,
                after.attempts,
                context.task(project, 1).task.state,
                window
            );
            if after.state == "delivering" && !window {
                stuck.push(format!("{kind} / {state}: delivering with no window"));
            }
        }
    }
    assert!(stuck.is_empty(), "{stuck:#?}");
}

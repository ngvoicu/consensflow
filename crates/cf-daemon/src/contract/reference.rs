//! What Node did, as the engine kit does it. The kit runs the engine on its
//! executor with the callbacks of Node's loop made by hand, each followed by
//! a drain, and its 183 dispatcher tests are held to Node's recorded traces.
//! An arrangement of frames or timers the daemon is given is the same
//! arrangement of callbacks there: what the kit's run leaves is what the
//! daemon's must.

use cf_engine::testing::Context;
use serde_json::Value;

/// The events the ledger logged, in order: their kind and data.
pub type Events = Vec<(String, Value)>;

/// What a run left of the engine, as the daemon's tests read it.
#[derive(Debug, PartialEq)]
pub struct Outcome {
    /// The state of the task the worker was given.
    pub task: String,
    /// The events the ledger logged after the project was open.
    pub events: Events,
    /// How many times the engine asked the adapter's windows `started` after
    /// the project was open.
    pub started: usize,
}

/// A worker is given a task, its window opens, and the pane host's exit for
/// that window comes in the same callback as its answer to the open: the host
/// watches the window's process before it answers, so its exit can be first.
pub fn exit_with_the_answer() -> Outcome {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let (opened, started) = (events(&context).len(), asked_started(&context));
    context.give(project.id, "zeus", "Parser");
    *context.host.exit_after_open.borrow_mut() = Some("zeus".to_owned());
    context.pass().expect("a pass");
    outcome(context, project.id, opened, started)
}

/// The same, with the exit in a callback of its own after the answer's.
pub fn exit_after_the_answer() -> Outcome {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let (opened, started) = (events(&context).len(), asked_started(&context));
    context.give(project.id, "zeus", "Parser");
    context.pass().expect("a pass");
    context.exit("zeus");
    outcome(context, project.id, opened, started)
}

/// Two workers' looks, answered in callbacks of their own, `zeus`'s first.
/// His finds his answer (his task is collected and his window killed) and
/// hera's confirms the delivery of her task's message: one chain is longer
/// than the other, so that two chains run together, a turn of each, show in
/// the order of what they do. The events after the looks were asked.
pub fn looks_apart() -> Events {
    looks(false)
}

/// The same looks, answered in one callback: Node ran the continuations of
/// both together, in the order they were woken.
pub fn looks_together() -> Events {
    looks(true)
}

fn looks(together: bool) -> Events {
    let context = Context::new();
    let project = context.with_staff(&["zeus", "hera"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().expect("a pass");
    context.pass().expect("a pass");
    // Hera's message is delivered, and waits for her window to show it.
    context.give(project.id, "hera", "Lexer");
    context.pass().expect("a pass");
    let (zeus, hera) = (
        context.adapter.agent("zeus").launch,
        context.adapter.agent("hera").launch,
    );
    let zeus_look = context
        .adapter
        .observe_holds
        .hold(move |args| args[0]["launch"]["launchId"] == zeus);
    let hera_look = context
        .adapter
        .observe_holds
        .hold(move |args| args[0]["launch"]["launchId"] == hera);
    context.adapter.answer("zeus", "Parser done");
    let asked = events(&context).len();
    let passing = context.begin_pass();
    context.settle();
    zeus_look.open();
    if !together {
        context.settle();
    }
    hera_look.open();
    context.settle();
    context.finish(passing).expect("a pass");
    let after = events(&context).split_off(asked);
    context.close();
    after
}

/// The human closes a project whose chief and worker have windows open, the
/// worker at work on a task, and then resumes it: the events each operation
/// logged, after the windows were open.
pub fn closed_then_resumed() -> (Events, Events) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().expect("a pass");
    context.pass().expect("a pass");
    let asked = events(&context).len();
    context.close_project(project.id).expect("closed");
    let closed = events(&context).len();
    context.resume_project(project.id).expect("resumed");
    let mut after = events(&context);
    let resumed = after.split_off(closed);
    let closed = after.split_off(asked);
    context.close();
    (closed, resumed)
}

/// The events the ledger logged so far: their kind and data.
fn events(context: &Context) -> Events {
    context
        .recorder
        .events()
        .into_iter()
        .filter(|event| event["seam"] == "event")
        .map(|event| {
            (
                event["event"]["kind"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                event["event"]["data"].clone(),
            )
        })
        .collect()
}

/// How many times the windows were asked `started` so far.
fn asked_started(context: &Context) -> usize {
    context
        .recorder
        .calls("adapter:claude-code", &["started"])
        .len()
}

/// What the run left, since the project was open with `opened` events logged
/// and `started` asked: the chief's launch is the setup, which the daemon's
/// answers from a host that is a task of its own reach in another order.
fn outcome(context: Context, project: i64, opened: usize, started: usize) -> Outcome {
    let task = context.task(project, 1).task.state;
    let outcome = Outcome {
        task,
        events: events(&context).split_off(opened),
        started: asked_started(&context) - started,
    };
    context.close();
    outcome
}

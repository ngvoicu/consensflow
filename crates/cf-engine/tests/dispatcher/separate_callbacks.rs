//! An answer and an exit that come in separate callbacks: a worker's look
//! found its answer, and the pane host's frame that its window exited is the
//! next thing the daemon hears. Node finished everything the answer set going
//! (its promise continuations, `interruptIfStopped` and `collect` among them)
//! before the next callback ran. The engine runs on its executor, which the
//! daemon drains at each such boundary, so tokio running another task
//! between two turns of the chain cannot put the exit before the collect;
//! the test runs the engine as the daemon does, on a `LocalSet` under the
//! executor's driver, with the pane host's reader a task of tokio's own.

use std::rc::Rc;

use cf_engine::runtime::Spawn;
use cf_engine::testing::{Context, Gate};
use cf_proto::panes::PaneExit;
use tokio::runtime::Builder;
use tokio::task::{spawn_local, yield_now, LocalSet};

use crate::fixtures::Tiers;
use crate::traces::held_to;
use crate::work_in_flight::looks_at;

const SUITES: &[&str] = &["an answer and an exit that come in separate callbacks"];

#[test]
fn finishes_the_task_of_a_worker_whose_look_found_its_answer_before_the_exit_that_comes_in_the_next_callback(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    let launch = context.adapter.agent("zeus").launch;
    let release = context.adapter.observe_holds.hold(looks_at(launch));
    let stepping = context.begin_pass();
    context.adapter.answer("zeus", "Parser done");
    let pane = context.host.last("zeus").unwrap().pane;

    let runtime = Builder::new_current_thread().build().unwrap();
    let local = LocalSet::new();
    drop(local.spawn_local(context.executor.driver()));
    let frame = Gate::default();
    let (dispatcher, executor, arrives) = (
        Rc::clone(&context.dispatcher),
        Rc::clone(&context.executor),
        frame.clone(),
    );
    local.block_on(&runtime, async move {
        // The bridge's reader, a task of tokio's own: when the host's frame
        // comes it settles what the exit changes where it is read, starts the
        // rest on the executor, and drains it.
        let reader = spawn_local(async move {
            arrives.wait().await;
            let exit = PaneExit {
                id: pane.id,
                generation: pane.generation,
                exit_code: None,
                signal: None,
                tail: None,
            };
            if let Some(rest) = dispatcher.pane_exited(exit) {
                executor.spawn(rest);
            }
            executor.drain();
        });
        yield_now().await;
        // The harness answers the look, which the step has been waiting on,
        // and the pane host's frame is read in the same moment.
        release.open();
        frame.open();
        reader.await.unwrap();
    });
    stepping.take().unwrap().unwrap();
    assert_eq!(
        tiers.task(1).task.state,
        "done",
        "the answer was collected, not paused for the exit"
    );
    assert_eq!(
        context.dispatcher.pane(tiers.id("zeus-amber-pine")),
        None,
        "its window closed"
    );
    held_to(
        context.close(),
        SUITES,
        "finishes the task of a worker whose look found its answer before the exit that comes in the next callback",
    );
}

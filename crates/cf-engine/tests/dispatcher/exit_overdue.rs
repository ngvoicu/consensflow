//! A window closed with `retire` whose kill the host took and whose exit never comes: it
//! was left retiring for good, never looked at, reused or released (Astraeus on
//! turn-end-min, dispatcher.rs's skip of a retiring window; T-487). Its exit is settled
//! once the wait for it is over, as the engine's own close settles one, and a word the
//! host sends later changes nothing of the window that opened since.

use cf_engine::testing::Context;
use cf_harness::contract::Pane;

/// How long the exit of a kill the host took is waited for: three times the host's
/// close grace (`windows.rs`' `EXIT_WAIT`).
const EXIT_WAIT_MS: i64 = 30_000;

/// Zeus's T-1 answered: its window closes as it is free, and the host takes the kill
/// but never says the window exited. Its pane.
fn closed_without_its_exit(context: &Context, project: i64) -> Pane {
    context.give(project, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let window = context.host.last("zeus").unwrap().pane;
    context.host.hold_exits.set(true);
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "done");
    assert_eq!(
        context.host.killed(),
        std::slice::from_ref(&window),
        "the kill was taken"
    );
    window
}

#[test]
fn a_close_whose_exit_never_comes_is_settled_once_its_wait_is_over_and_the_next_task_opens_a_window(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let zeus = context.id(project.id, "zeus");
    let window = closed_without_its_exit(&context, project.id);
    context.give(project.id, "zeus", "Lexer");
    let opened = context.host.opened().len();

    // While its exit may still come, the window is on its way out: nothing goes to it.
    context.advance(EXIT_WAIT_MS - 1);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.dispatcher.pane(zeus), Some(window.clone()));
    assert_eq!(context.host.opened().len(), opened);
    assert_eq!(context.task(project.id, 2).task.state, "queued");

    // Its wait over, the exit is settled, and T-2 opens a window of its own.
    context.advance(1);
    context.pass().unwrap();
    assert_eq!(context.dispatcher.pane(zeus), None, "released");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.host.opened().len(), opened + 1);
    let next = context.host.last("zeus").unwrap().pane;
    assert_ne!(next, window);
    assert_eq!(context.task(project.id, 2).task.state, "working");

    // The host's word on the first window, late, closes nothing of the second.
    context.host.hold_exits.set(false);
    context.run({
        let host = std::rc::Rc::clone(&context.host);
        async move { host.exited(window).await }
    });
    context.pass().unwrap();
    assert_eq!(context.dispatcher.pane(zeus), Some(next.clone()));
    assert!(!context.host.killed().contains(&next));
    assert_eq!(context.task(project.id, 2).task.state, "working");
    context.close();
}

//! A window runs on the harness it was launched with (T-1336). A member edited to another harness
//! runs it from its next window (T-1143), and the window already open goes on running the one it
//! was launched with: the page's keys for that window (Shift+Enter is Ctrl+J in a Pi window, which
//! the other harnesses read as a send or a follow-up when they are sent Alt+Enter) are the
//! launched harness's, so the board says which that is, with the window's pane.

use cf_engine::testing::Context;
use cf_ledger::{MemberEdit, NewTask};

#[test]
fn a_member_edited_to_another_harness_leaves_the_window_open_on_the_one_it_was_launched_with() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.create_task(
        project.id,
        NewTask {
            from: "chief".to_owned(),
            pool: Some("worker".to_owned()),
            tier: Some("standard".to_owned()),
            body: "Write the parser".to_owned(),
            ..NewTask::default()
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    let assignee = context
        .task(project.id, 1)
        .task
        .assignee
        .expect("a window has it");
    let window = context.id(project.id, &assignee);
    assert!(
        context.dispatcher.pane(window).is_some(),
        "its window is open"
    );
    assert_eq!(
        context.dispatcher.window_harness(window).as_deref(),
        Some("claude-code")
    );

    let member = context
        .project(project.id)
        .participants
        .into_iter()
        .find(|participant| participant.handle == "zeus")
        .unwrap();
    context
        .ledger
        .borrow_mut()
        .edit_member(
            project.id,
            "zeus",
            &MemberEdit {
                harness: "pi".to_owned(),
                model: None,
                effort: None,
                model_name: None,
                tier: member.tier.clone().unwrap(),
            },
        )
        .unwrap();
    context.pass().unwrap();
    let after = context.project(project.id);
    let configured = after
        .participants
        .iter()
        .find(|participant| participant.id == window)
        .expect("the window's participant");
    assert_eq!(
        configured.harness.as_deref(),
        Some("pi"),
        "the ledger says Pi from the next window"
    );
    assert!(
        context.dispatcher.pane(window).is_some(),
        "the window is still open"
    );
    assert_eq!(
        context.dispatcher.window_harness(window).as_deref(),
        Some("claude-code"),
        "and it still runs the harness it was launched with"
    );
}

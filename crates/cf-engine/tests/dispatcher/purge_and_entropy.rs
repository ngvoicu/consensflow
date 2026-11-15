//! A deleted project's lines that the trace could not drop are said in the daemon's
//! log, and the deletion stands, the project being gone from the ledger by then; a
//! launch whose id or token the system would not make fails as any launch that did not
//! come up does, with no window opened, and its files are forgotten where it had
//! written them. Node's dispatcher had neither, so none is held to a Node trace.

use cf_engine::testing::Context;

use crate::chiefs::to_human;

const CAUSE: &str = "the system gave no randomness: unavailable";

const CHIEF_NOT_UP: &str = "The chief could not start: the launch failed (its cause is in ConsensFlow's log). What comes for the chief waits for it, and ConsensFlow tries again; you may also switch the chief.";

#[test]
fn a_trace_that_could_not_forget_a_deleted_project_says_so_in_the_log_and_the_deletion_stands() {
    let context = Context::new();
    context.trace.forgets.set(true);
    *context.trace.forget_fails.borrow_mut() = Some("events.jsonl: access denied".to_owned());
    let project = context.with_staff(&["zeus"]);
    context.close_project(project.id).unwrap();

    let gone = context.delete_project(project.id).unwrap();

    assert_eq!(gone.id, project.id);
    assert!(context.ledger.borrow().projects().unwrap().is_empty());
    let said = std::mem::take(&mut *context.log.failures.borrow_mut());
    assert_eq!(said, ["events.jsonl: access denied"]);
}

#[test]
fn a_launch_whose_id_the_system_would_not_draw_opens_no_window_and_says_why() {
    let context = Context::new();
    *context.launch_ids.fails.borrow_mut() = Some(CAUSE.to_owned());
    let project = context.with_staff(&["zeus"]);

    context.pass().unwrap();

    assert!(context.host.opened().is_empty());
    assert_eq!(
        to_human(&context, project.id),
        [CHIEF_NOT_UP],
        "the human is told, not left to a chief that never comes"
    );
    let warnings = context.log.warnings.borrow().join("\n");
    assert!(warnings.contains(CAUSE), "{warnings}");
}

#[test]
fn a_launch_whose_token_the_system_would_not_make_opens_no_window_and_forgets_its_files() {
    let context = Context::new();
    *context.credentials.fails.borrow_mut() = Some(CAUSE.to_owned());
    let project = context.with_staff(&["zeus"]);

    context.pass().unwrap();

    assert!(context.host.opened().is_empty());
    assert_eq!(
        to_human(&context, project.id),
        [CHIEF_NOT_UP],
        "the human is told, not left to a chief that never comes"
    );
    let warnings = context.log.warnings.borrow().join("\n");
    assert!(warnings.contains(CAUSE), "{warnings}");
    assert!(
        !context.launch_files.forgotten.borrow().is_empty(),
        "the files the plan wrote have no window to read them"
    );
}

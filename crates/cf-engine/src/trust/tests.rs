//! The human's yes in a window that ended before the watch looked again: a window
//! that ends asking, or that the engine closes itself, is read off the screen it ends
//! with (`yes_at_exit`, `bank_yes_before_close`), and a No, a window ConsensFlow
//! answered and one whose host said nothing bank none.

use cf_harness::contract::Pane;
use serde_json::json;

use super::*;
use crate::testing::{Context, TRUST_PROMPT, TRUST_QUESTION};

/// The repository's root, which a yes in the test's windows would trust.
const FOLDER: &str = "/work/repository";

/// A project whose worker's window is asking whether to trust a folder outside the
/// project's, the human not yet answered.
struct Asking {
    context: Context,
    project: i64,
    worker: i64,
    pane: Pane,
}

fn asking() -> Asking {
    let context = Context::new();
    context.adapter.asks_to_trust(FOLDER, false);
    context
        .host
        .set_snapshot(json!({ "tail": [TRUST_QUESTION] }));
    let project = context.with_tiers(&["zeus", "diana"]).id;
    context.pool_task(project, "worker", "Write the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let session = context.task(project, 1).task.assignee.unwrap();
    let worker = context.id(project, &session);
    let pane = context.dispatcher.pane(worker).expect("its window");
    let record = context.dispatcher.record_of(worker);
    // The watch looked as the window opened, and the question was on its screen.
    assert_eq!(record.window.borrow().trust, Trusting::Asked);
    assert_eq!(record.window.borrow().trust_keys, 0);
    Asking {
        context,
        project,
        worker,
        pane,
    }
}

impl Asking {
    fn record(&self) -> Rc<Record> {
        self.context.dispatcher.record_of(self.worker)
    }

    /// Whether the human's yes to the folder is banked for the project's other windows.
    fn banked(&self) -> bool {
        self.context
            .dispatcher
            .trust
            .accepted
            .borrow()
            .contains(&(self.project, FOLDER.to_owned()))
    }

    /// The engine closes the window itself, as a Close or a switch does.
    fn closes_it(&self) -> bool {
        let (dispatcher, record, pane) = (
            Rc::clone(&self.context.dispatcher),
            self.record(),
            self.pane.clone(),
        );
        self.context
            .run(async move { dispatcher.close_own(&record, &pane).await })
            .unwrap()
    }
}

#[test]
fn a_window_that_ends_asking_with_its_prompt_on_the_screen_it_ends_with_is_the_humans_yes() {
    let asking = asking();
    assert!(!asking.banked());
    asking
        .context
        .host
        .exits_showing(Some(0), None, Some(&[TRUST_PROMPT]));
    asking.context.exit("zeus");
    assert!(asking.banked());
}

#[test]
fn a_window_that_ends_at_the_question_is_a_no_and_banks_nothing() {
    let asking = asking();
    asking
        .context
        .host
        .exits_showing(Some(0), None, Some(&[TRUST_QUESTION]));
    asking.context.exit("zeus");
    assert!(!asking.banked());
}

#[test]
fn a_window_whose_host_said_nothing_of_its_screen_banks_nothing() {
    let asking = asking();
    asking.context.host.exits_showing(Some(0), None, None);
    asking.context.exit("zeus");
    assert!(!asking.banked());
}

#[test]
fn a_window_consensflow_pressed_keys_in_is_not_the_humans_yes() {
    let asking = asking();
    asking.record().window.borrow_mut().trust_keys = 1;
    asking
        .context
        .host
        .exits_showing(Some(0), None, Some(&[TRUST_PROMPT]));
    asking.context.exit("zeus");
    assert!(!asking.banked());
}

#[test]
fn a_window_that_never_asked_banks_nothing_whatever_it_ends_with() {
    let asking = asking();
    asking.record().window.borrow_mut().trust = Trusting::Past;
    asking
        .context
        .host
        .exits_showing(Some(0), None, Some(&[TRUST_PROMPT]));
    asking.context.exit("zeus");
    assert!(!asking.banked());
}

#[test]
fn a_window_the_engine_closes_is_read_off_the_screen_it_shows_before_the_kill() {
    let asking = asking();
    // The human said yes just before the Close: the prompt is on the screen now.
    asking
        .context
        .host
        .set_snapshot(json!({ "tail": [TRUST_PROMPT] }));
    assert!(asking.closes_it());
    assert!(asking.banked());
}

#[test]
fn a_window_the_engine_closes_at_the_question_banks_nothing() {
    let asking = asking();
    assert!(asking.closes_it());
    assert!(!asking.banked());
}

#[test]
fn a_window_the_engine_closes_after_consensflow_pressed_keys_in_it_is_not_looked_at() {
    let asking = asking();
    asking.record().window.borrow_mut().trust_keys = 1;
    asking
        .context
        .host
        .set_snapshot(json!({ "tail": [TRUST_PROMPT] }));
    let before = asking.context.host.snapshots();
    assert!(asking.closes_it());
    assert!(!asking.banked());
    assert_eq!(
        asking.context.host.snapshots(),
        before,
        "a window ConsensFlow answered is not looked at"
    );
}

#[test]
fn a_window_that_ended_keeps_no_count_of_keys_for_the_next_one() {
    let asking = asking();
    asking.record().window.borrow_mut().trust_keys = 2;
    asking.context.host.exits_showing(Some(0), None, None);
    asking.context.exit("zeus");
    let part = asking.record();
    assert_eq!(part.window.borrow().trust, Trusting::Past);
    assert_eq!(part.window.borrow().trust_keys, 0);
}

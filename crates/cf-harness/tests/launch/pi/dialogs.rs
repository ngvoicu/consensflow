//! What a Pi window's screen shows of a dialog of its harness that waits for a key: nothing, at
//! rest (T-1240). Pi draws its conversation with no mark, its editor between two rules, and under
//! it the folder and the line of its stats, so no line of its idle screen carries the cursor of
//! a choice. Pi's only dialogs are an extension's `ui.select` and `ui.confirm`, which
//! ConsensFlow's extension never calls.

use cf_harness::testing::screens::{
    reads_nothing, PI_IDLE_AFTER_QUESTION, PI_QUOTED_EXAMPLE_AT_REST,
};

use super::{adapter, prepare, Home, Request};

#[test]
fn a_pi_chief_at_rest_after_a_workers_question_shows_no_dialog() {
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let plan = prepare(&adapter, &Request::default()).unwrap();
    reads_nothing(&*plan.window, "at rest", PI_IDLE_AFTER_QUESTION);
}

/// T-1243: Pi's own rendering of a fenced example, numbered and quoted, under a question, with its
/// footer under it, is the conversation's: `5b7acff7f` read none.
#[test]
fn a_pi_chief_at_rest_after_a_numbered_quoted_example_shows_no_dialog() {
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let plan = prepare(&adapter, &Request::default()).unwrap();
    reads_nothing(&*plan.window, "at rest", PI_QUOTED_EXAMPLE_AT_REST);
}

/// T-1243's release bar, over the whole corpus (`cf_harness::testing::corpus`): this window reads no
/// screen as a dialog that `5b7acff7f`'s window read as none, reads every screen labelled a dialog as
/// that window did, lets only idle prompts and conversations read less,
/// and reads none of the idle screens of its own harness as a dialog the engine counts.
#[test]
fn the_pi_window_reads_no_screen_as_a_dialog_that_the_release_head_read_as_none() {
    use cf_harness::testing::corpus::{assert_no_regression, say};
    use cf_proto::agents::Harness;
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let plan = prepare(&adapter, &Request::default()).unwrap();
    let window = &*plan.window;
    let audit = assert_no_regression(window, Harness::Pi, false);
    for (name, how) in &audit.changed {
        println!("pi window, {how}: {name}");
    }
    println!("{}", say(&audit, Harness::Pi));
}

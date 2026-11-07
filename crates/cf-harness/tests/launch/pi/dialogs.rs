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

/// A Pi window's identity of a dialog is its words as they were read (T-1277), as on 3.0.7-alpha.2:
/// the seconds a title carries in brackets are part of it. Pi 1.5.0's selector built without a
/// timeout draws `Run the health check? (30s)` and `Run the health check? (45s)` for two requests
/// that do not expire (T-1273's second review), so the window takes no time out of a title.
#[test]
fn a_pi_windows_dialogs_are_their_words_as_read_so_titles_that_say_seconds_are_two_requests() {
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let plan = prepare(&adapter, &Request::default()).unwrap();
    let window = &plan.window;
    let selector = |title: &str| {
        let screen: Vec<String> = [
            format!(" {title}"),
            " \u{2192} Yes".to_owned(),
            "   No".to_owned(),
            " \u{2191}\u{2193} navigate  enter select  escape/ctrl+c cancel".to_owned(),
        ]
        .to_vec();
        let dialog = window.dialog(&screen).expect("the selector");
        assert_eq!(window.dialog_identity(&dialog), dialog);
        window.dialog_identity(&dialog)
    };
    assert_ne!(
        selector("Run the health check? (30s)"),
        selector("Run the health check? (45s)")
    );
    // A selector with a timeout ticks its title each second: another dialog at each, as on
    // alpha.2 (Pi's own extension never asks one).
    assert_ne!(
        selector("Allow the call to remove the build folder? (29s)"),
        selector("Allow the call to remove the build folder? (3s)")
    );
}

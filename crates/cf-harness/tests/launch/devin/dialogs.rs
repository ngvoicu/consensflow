//! What a Devin window's screen shows of a dialog of its harness that waits for a key, and what
//! it does not: its box, `❭` and its hint or the words typed there, under the rule that says
//! the mode and the conversation's title, with its model's line under it, is no choice of a
//! dialog (T-1240). Devin says nothing of its dialogs; its screen is all the engine has.

use cf_harness::testing::screens::{
    reads_a_dialog, reads_nothing, DEVIN_BOX_HOLDING_LINES, DEVIN_CALL, DEVIN_IDLE_AFTER_QUESTION,
    DEVIN_ON_THE_QUESTION, DEVIN_THEME,
};

use super::fixtures::*;

#[test]
fn a_devin_chief_at_rest_or_on_a_question_shows_no_dialog() {
    let home = Home::new();
    let window = window(&home.adapter(), &Request::default());
    reads_nothing(
        &*window,
        "at rest after answering",
        DEVIN_IDLE_AFTER_QUESTION,
    );
    reads_nothing(
        &*window,
        "its message the last words over its box",
        DEVIN_ON_THE_QUESTION,
    );
}

#[test]
fn a_devin_box_that_holds_an_unsent_message_shows_no_dialog() {
    let home = Home::new();
    let window = window(&home.adapter(), &Request::default());
    reads_nothing(
        &*window,
        "two lines typed under a question",
        DEVIN_BOX_HOLDING_LINES,
    );
}

#[test]
fn its_dialog_of_a_call_and_its_offer_to_switch_its_theme_are_dialogs_still() {
    let home = Home::new();
    let window = window(&home.adapter(), &Request::default());
    let call = reads_a_dialog(&*window, "the dialog of a call", DEVIN_CALL);
    assert_eq!(call.options.len(), 6);
    assert_eq!(call.question, "$ echo ran > shell.txt");
    let theme = reads_a_dialog(&*window, "the offer to switch its theme", DEVIN_THEME);
    assert_eq!(
        theme.question,
        "Detected dark theme but configured for light. Switch?"
    );
    assert_eq!(theme.options.len(), 3);
}

/// T-1243's release bar, over the whole corpus (`cf_harness::testing::corpus`): this window reads no
/// screen as a dialog that `5b7acff7f`'s window read as none, reads every screen labelled a dialog as
/// that window did, lets only idle prompts and conversations read less,
/// and reads none of the idle screens of its own harness as a dialog the engine counts.
#[test]
fn the_devin_window_reads_no_screen_as_a_dialog_that_the_release_head_read_as_none() {
    use cf_harness::testing::corpus::{assert_no_regression, say};
    use cf_proto::agents::Harness;
    let home = Home::new();
    let window = window(&home.adapter(), &Request::default());
    let window = &*window;
    let audit = assert_no_regression(window, Harness::Devin, false);
    for (name, how) in &audit.changed {
        println!("devin window, {how}: {name}");
    }
    println!("{}", say(&audit, Harness::Devin));
}

/// Devin draws no timer on its dialogs, and its window takes nothing out of one (T-1273, T-1277):
/// the command is the question it parses, and `printf 'retry in 5s'` and `printf 'retry in 10s'`
/// are two requests.
#[test]
fn a_devin_call_with_a_time_in_its_command_is_another_dialog_than_the_same_call_with_another_time()
{
    let home = Home::new();
    let window = window(&home.adapter(), &Request::default());
    let make = |duration: &str| -> Vec<String> {
        DEVIN_CALL
            .iter()
            .map(|line| {
                line.replace(
                    "echo ran > shell.txt",
                    &format!("printf 'retry in {duration}'"),
                )
            })
            .collect()
    };
    let five = window.dialog(&make("5s")).expect("a call");
    let ten = window.dialog(&make("10s")).expect("a call");
    assert_eq!(window.dialog_identity(&five), five);
    assert_eq!(window.dialog_identity(&ten), ten);
    assert_ne!(window.dialog_identity(&five), window.dialog_identity(&ten));
}

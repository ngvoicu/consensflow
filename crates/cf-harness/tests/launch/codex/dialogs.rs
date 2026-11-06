//! What a Codex window's screen shows of a dialog of its harness that waits for a key: nothing, in
//! 3.0.7-alpha.2 (T-1240, T-1243). Its composer, `› ` and a hint or the words typed there, with the status
//! the harness draws under it, was read as a list of choices (the live matrix's `question-ask-answer`
//! with a Codex chief, 2026-11-05), and no rule on the words of a Codex screen keeps its composer from a
//! dialog: the footer is the user's to configure, and Codex hides its line of keys while a draft is in
//! the composer. The release head read no real Codex dialog off the tail at all, so the window reads none.
//!
//! The screens of Codex's dialogs (its nudge to switch models, its pickers) and the rule that took a
//! numbered list with one line of keys under it for one, which `fd1837385` held and the second review
//! of T-1243 broke, are kept here as data for the follow-up that reads Codex's dialogs from evidence
//! beyond its footer: `issues/codex-dialogs-are-not-read-from-its-screen.md` in consensflow-sme.

use cf_harness::testing::screens::{
    reads_nothing, CODEX_COMPOSER_HOLDING_LINES, CODEX_COMPOSER_HOLDING_NUMBERED_LINES,
    CODEX_IDLE_AFTER_QUESTION, CODEX_NUDGE, CODEX_PICKER, CODEX_WORKING_AFTER_QUESTION,
};

use super::{adapter, prepare, Home, Request};

#[test]
fn a_codex_chief_at_rest_after_a_workers_question_shows_no_dialog() {
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let plan = prepare(&adapter, &Request::default()).unwrap();
    reads_nothing(
        &*plan.window,
        "at rest, as the field's screen",
        CODEX_IDLE_AFTER_QUESTION,
    );
    reads_nothing(
        &*plan.window,
        "mid-turn, its status above the composer",
        CODEX_WORKING_AFTER_QUESTION,
    );
    // A line of keys under the composer that names Esc (made up: after one Esc in the composer,
    // Codex says what a second does): the composer's words are not numbered, whatever is under.
    let mut pressed = CODEX_IDLE_AFTER_QUESTION.to_vec();
    pressed.pop();
    pressed.push("  ⌃c to quit · esc esc to edit previous message");
    reads_nothing(&*plan.window, "its line of keys naming Esc", &pressed);
}

#[test]
fn a_codex_composer_that_holds_an_unsent_message_shows_no_dialog() {
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let plan = prepare(&adapter, &Request::default()).unwrap();
    reads_nothing(
        &*plan.window,
        "two lines typed under a question",
        CODEX_COMPOSER_HOLDING_LINES,
    );
    // Numbered as a dialog's choices are, with no key of a dialog under it.
    reads_nothing(
        &*plan.window,
        "two numbered lines typed under a question",
        CODEX_COMPOSER_HOLDING_NUMBERED_LINES,
    );
    // A folder whose name holds a key's letters is no key.
    let enterprise: Vec<&str> = CODEX_COMPOSER_HOLDING_NUMBERED_LINES
        .iter()
        .map(|line| {
            if line.contains("GPT-") {
                "  GPT-5.6-Luna default · ~/work/enterprise-escrow"
            } else {
                *line
            }
        })
        .collect();
    reads_nothing(
        &*plan.window,
        "the same with a folder called enterprise-escrow",
        &enterprise,
    );
}

/// Codex's nudge to switch models and its pickers are dialogs that wait for a key, and the window
/// does not read them: the release head did not either (their title and hint lines, in the choices'
/// column, were taken for choices in the tail), and no rule that reads them is safe against a footer the
/// user wrote. Their screens are the follow-up's data.
#[test]
fn codex_reads_its_nudge_and_its_pickers_as_nothing_as_the_release_head_did() {
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let plan = prepare(&adapter, &Request::default()).unwrap();
    reads_nothing(&*plan.window, "the model nudge", CODEX_NUDGE);
    reads_nothing(&*plan.window, "the picker of a model", CODEX_PICKER);
}

/// T-1243 (P2 of the review of `9a7fca6cb`): the line under a composer holds the model and the folder,
/// and a folder called `enter` or `esc` is no line of keys. `5b7acff7f` reads none of these.
#[test]
fn a_folder_or_a_title_with_the_letters_of_a_key_under_a_numbered_composer_is_no_dialog() {
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let plan = prepare(&adapter, &Request::default()).unwrap();
    for footer in [
        "  GPT-5.6-Luna default · ~/work/enter",
        "  GPT-5.6-Luna default · ~/work/esc",
        "  GPT-5.6-Luna default · ~/work/esc-demo",
        "  GPT-5.6-Luna default · ~/work/enter-esc",
        "  GPT-5.6-Luna default · ~/work/enterprise-escrow",
        "  GPT-5.6-Luna default · /work/enter",
        "  GPT-5.6-Luna default · C:\\work\\esc",
        "  GPT-5.6-Luna default · ~/work/app · Press enter to confirm or esc to go back",
        "  GPT-5.6-Luna default · ~/work/app · esc",
        // No path in it, a word that holds a key's letters.
        "  GPT-5.6-Luna default · enterprise",
        "  GPT-5.6-Luna default · escalate",
    ] {
        let screen: Vec<&str> = CODEX_COMPOSER_HOLDING_NUMBERED_LINES
            .iter()
            .map(|line| if line.contains("GPT-") { footer } else { *line })
            .collect();
        reads_nothing(&*plan.window, footer, &screen);
        // And with the line of the composer's keys gone, the footer is all that is under the list.
        let mut alone = screen.clone();
        alone.pop();
        reads_nothing(&*plan.window, footer, &alone);
    }
}

/// T-1243, the second review: a footer is the user's to configure, and Codex hides its line of keys
/// while a draft is in the composer, so under a draft the footer can be a branch's name that is a key's
/// name. No rule about footer words is safe, and `5b7acff7f` reads none of these.
#[test]
fn a_draft_over_a_footer_the_user_configured_is_no_dialog() {
    use cf_harness::testing::screens::{
        CODEX_DRAFT_OVER_BRANCH_ENTER, CODEX_DRAFT_OVER_BRANCH_ESC,
        CODEX_DRAFT_OVER_MODEL_AND_BRANCH_ENTER, CODEX_PLAIN_DRAFT_OVER_BRANCH_ENTER,
        CODEX_PLAIN_DRAFT_OVER_BRANCH_ESC,
    };
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let plan = prepare(&adapter, &Request::default()).unwrap();
    for (what, screen) in [
        ("a branch called enter", CODEX_DRAFT_OVER_BRANCH_ENTER),
        ("a branch called esc", CODEX_DRAFT_OVER_BRANCH_ESC),
        (
            "the model and a branch called enter",
            CODEX_DRAFT_OVER_MODEL_AND_BRANCH_ENTER,
        ),
        (
            "a draft that is no list, over a branch called enter",
            CODEX_PLAIN_DRAFT_OVER_BRANCH_ENTER,
        ),
        (
            "a draft that is no list, over a branch called esc",
            CODEX_PLAIN_DRAFT_OVER_BRANCH_ESC,
        ),
    ] {
        reads_nothing(&*plan.window, what, screen);
    }
}

/// The composer's own line of keys, with nothing of the model under the list: it is no line of a
/// dialog's keys, whatever it names.
#[test]
fn the_line_of_keys_of_the_composer_alone_under_a_numbered_list_is_no_dialog() {
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let plan = prepare(&adapter, &Request::default()).unwrap();
    for keys in [
        "  ? for shortcuts",
        "  ← for agents · ? for shortcuts",
        "  ⌃c again to quit",
    ] {
        let mut screen: Vec<&str> = CODEX_COMPOSER_HOLDING_NUMBERED_LINES.to_vec();
        screen.truncate(screen.len() - 2);
        screen.push(keys);
        reads_nothing(&*plan.window, keys, &screen);
    }
}

/// A dialog of Codex's ends the screen: its list, an empty line, and the one line of keys. What
/// the conversation quotes of one, over the composer and its footer, is no dialog.
#[test]
fn a_dialog_that_the_conversation_quotes_over_the_composer_is_none() {
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let plan = prepare(&adapter, &Request::default()).unwrap();
    let mut quoted = CODEX_NUDGE.to_vec();
    quoted.extend([
        "› Ask Codex to do anything",
        "  GPT-5.6-Luna default · ~/work/app",
        "  ? for shortcuts",
    ]);
    reads_nothing(&*plan.window, "the nudge quoted over the composer", &quoted);
    // The same, the composer's footer all that follows the keys.
    let mut footer = CODEX_NUDGE.to_vec();
    footer.push("  GPT-5.6-Luna default · ~/work/app");
    reads_nothing(&*plan.window, "the nudge over a footer", &footer);
    // A list that is numbered with the line of the model's words under it, and no keys.
    let mut words = CODEX_NUDGE.to_vec();
    words.pop();
    words.push("  Say which, and I will do it");
    reads_nothing(&*plan.window, "a numbered list and words", &words);
}

/// The lines of keys Codex draws under a list of choices, and a row that wraps its description: the
/// rule that read these as dialogs is out, and the window reads none, as the release head did.
#[test]
fn each_line_of_keys_codex_draws_under_a_list_is_read_as_nothing() {
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let plan = prepare(&adapter, &Request::default()).unwrap();
    for keys in [
        "  Press enter to confirm or esc to go back",
        "  Press enter to continue",
        "  enter select · esc back",
        "  enter default · s session · esc back",
        "  Esc to cancel · Enter to confirm",
    ] {
        let mut screen = CODEX_NUDGE.to_vec();
        screen.pop();
        screen.push(keys);
        reads_nothing(&*plan.window, keys, &screen);
    }
    let wrapped = [
        "  Approaching rate limits",
        "  Switch to gpt-5.6-mini for lower credit usage?",
        "› 1. Switch to gpt-5.6-mini",
        "     Faster, and costs less",
        "  2. Keep current model",
        "  Press enter to confirm or esc to go back",
    ];
    reads_nothing(&*plan.window, "a row with a description", &wrapped);
}

/// T-1243's release bar, over the whole corpus (`cf_harness::testing::corpus`): this window reads no
/// screen as a dialog that `5b7acff7f`'s window read as none, reads every screen labelled a dialog as
/// that window did (but through the window of a harness that reads none by design), lets only idle prompts and conversations read less,
/// and reads none of the idle screens of its own harness as a dialog the engine counts.
#[test]
fn the_codex_window_reads_no_screen_as_a_dialog_that_the_release_head_read_as_none() {
    use cf_harness::testing::corpus::{assert_no_regression, say};
    use cf_proto::agents::Harness;
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let plan = prepare(&adapter, &Request::default()).unwrap();
    let window = &*plan.window;
    let audit = assert_no_regression(window, Harness::Codex, true);
    for (name, how) in &audit.changed {
        println!("codex window, {how}: {name}");
    }
    println!("{}", say(&audit, Harness::Codex));
}

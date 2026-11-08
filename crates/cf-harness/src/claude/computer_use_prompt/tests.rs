use super::*;

const ACCESSIBILITY: &str = "Open System Settings \u{2192} Accessibility";
const SCREEN_RECORDING: &str = "Open System Settings \u{2192} Screen Recording";

/// The prompt as 2.1.319 drew it in T-1291's window: Screen Recording missing.
fn prompt(options: &[&str]) -> Dialog {
    Dialog {
        question: "Recording.".to_owned(),
        detail: [
            "I'll type that URL into the chief's input, then check the log.",
            "\u{23fa} Computer Use[request_access]",
            "Computer Use needs macOS permissions",
            "Accessibility: \u{2714} granted",
            "Screen Recording: \u{2718} not granted",
            "Grant the missing permissions in System Settings, then select \"Try again\". macOS may require you to restart Claude Code after granting Screen",
        ]
        .map(str::to_owned)
        .to_vec(),
        options: options.iter().map(|option| (*option).to_owned()).collect(),
        asks: true,
        prompt_below: false,
        closes_after_s: None,
    }
}

#[test]
fn its_decline_is_escape_alone_never_a_digit_and_never_a_listed_choice() {
    let decline = decline(&prompt(&[SCREEN_RECORDING, "Try again"])).expect("the prompt");
    assert_eq!(decline.key, vec![27]);
    assert!(!decline.listed);
    assert!(decline.members_only);
    assert_eq!(decline.after_ms, None);
    assert_eq!(decline.what, "Computer Use permission prompt");
    // No digit: Claude Code's Select takes one for the choice of that number, whether or not it
    // draws the numbers, and 1 opens System Settings, 2 asks again.
    assert!(decline.key.iter().all(|byte| !byte.is_ascii_digit()));
    assert!(!decline.choice.contains("Open System Settings"));
    assert!(!decline.choice.contains("Try again"));
}

#[test]
fn the_member_is_told_what_consensflow_lacks() {
    assert_eq!(
        decline(&prompt(&[SCREEN_RECORDING, "Try again"]))
            .unwrap()
            .then_tell
            .as_deref(),
        Some("Computer use isn't available in ConsensFlow's windows until ConsensFlow has the Screen Recording permission and has been restarted. Do the task without it.")
    );
    assert_eq!(
        decline(&prompt(&[ACCESSIBILITY, "Try again"]))
            .unwrap()
            .then_tell
            .as_deref(),
        Some("Computer use isn't available in ConsensFlow's windows until ConsensFlow has the Accessibility permission. Do the task without it.")
    );
    assert_eq!(
        decline(&prompt(&[ACCESSIBILITY, SCREEN_RECORDING, "Try again"]))
            .unwrap()
            .then_tell
            .as_deref(),
        Some("Computer use isn't available in ConsensFlow's windows until ConsensFlow has the Accessibility and Screen Recording permissions and has been restarted. Do the task without it.")
    );
}

#[test]
fn it_is_known_by_its_choices_alone_in_their_order_with_try_again_last() {
    for options in [
        vec![SCREEN_RECORDING],
        vec!["Try again"],
        vec!["Try again", SCREEN_RECORDING],
        vec![SCREEN_RECORDING, ACCESSIBILITY, "Try again"],
        vec![SCREEN_RECORDING, "Try again", "Never ask again"],
        vec![SCREEN_RECORDING, "Try again", "Try again"],
        vec!["Yes", "No"],
        vec!["Open System Settings", "Try again"],
        vec![
            "Deny, and tell Claude what to do differently (esc)",
            "Allow for this session (1 app)",
        ],
    ] {
        assert_eq!(decline(&prompt(&options)), None, "{options:?}");
        assert!(!is_prompt(&prompt(&options)), "{options:?}");
    }
}

#[test]
fn it_is_known_by_its_title_a_row_of_its_own() {
    let mut without = prompt(&[SCREEN_RECORDING, "Try again"]);
    without.detail.remove(2);
    assert_eq!(decline(&without), None);
    // A command's output or a file that holds the words in a longer row is no title.
    for quoted in [
        "echo 'Computer Use needs macOS permissions'",
        "\u{2502} Computer Use needs macOS permissions (a log line)",
        "computer use needs macos permissions",
    ] {
        let mut dialog = prompt(&[SCREEN_RECORDING, "Try again"]);
        dialog.detail[2] = quoted.to_owned();
        assert_eq!(decline(&dialog), None, "{quoted}");
    }
}

#[test]
fn it_is_declined_only_where_it_waits_not_where_the_harnesss_prompt_is_drawn_below_it() {
    let mut dialog = prompt(&[SCREEN_RECORDING, "Try again"]);
    dialog.prompt_below = true;
    assert_eq!(decline(&dialog), None);
}

#[test]
fn its_top_is_its_title_the_nearest_to_its_question() {
    let mut dialog = prompt(&[SCREEN_RECORDING, "Try again"]);
    assert_eq!(top(&dialog), Some(2));
    // An earlier copy of the prompt in the conversation is not the dialog's top.
    dialog
        .detail
        .insert(0, "Computer Use needs macOS permissions".to_owned());
    assert_eq!(top(&dialog), Some(3));
    let other = Dialog {
        options: vec!["Yes".to_owned(), "No".to_owned()],
        ..dialog
    };
    assert_eq!(top(&other), None);
}

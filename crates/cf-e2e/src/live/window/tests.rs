//! What a window is held to without a harness: which harnesses the app pastes
//! into, the id a conversation is opened on, what is the screen since a mark, and
//! when a window has drawn and answered.

use std::cell::Cell;

use super::open::{asked_for, new_session, pasted_into, session_for, trust_line, Member};
use super::*;

fn item(role: &str, text: &str) -> Item {
    Item {
        id: String::new(),
        role: role.to_owned(),
        text: text.to_owned(),
        complete: false,
    }
}

#[test]
fn the_app_pastes_into_claude_and_devin_and_no_other_harness() {
    let names = |words: &[&str]| {
        words
            .iter()
            .map(|word| (*word).to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        pasted_into(&names(&["claude", "devin"])).unwrap(),
        [Harness::Claude, Harness::Devin]
    );
    assert!(pasted_into(&[]).unwrap().is_empty());
    for other in ["codex", "pi", "opencode"] {
        let failed = pasted_into(&names(&[other])).unwrap_err().to_string();
        assert_eq!(
            failed,
            format!(
                "the app pastes into claude and devin only; {other} takes its messages through its own queue"
            )
        );
    }
    let failed = pasted_into(&names(&["claud"])).unwrap_err().to_string();
    assert_eq!(
        failed,
        "no harness is called claud: claude, opencode, pi, devin, codex"
    );
}

#[test]
fn what_came_of_the_trust_asked_of_a_folder_is_said_as_the_window_of_whoever_asked() {
    assert_eq!(trust_line("claude", "trusted"), "claude: trust: trusted");
    assert_eq!(
        trust_line("huginn max", "already trusted"),
        "huginn max: trust: already trusted"
    );
}

#[test]
fn a_run_that_names_no_harness_is_asked_for_devin_and_one_that_names_some_for_those() {
    assert_eq!(asked_for(Vec::new()), ["devin"]);
    let named = vec!["claude".to_owned(), "devin".to_owned()];
    assert_eq!(asked_for(named.clone()), named);
}

#[test]
fn a_conversation_id_is_a_version_4_uuid_and_a_new_one_each_time() {
    let (first, second) = (new_session(), new_session());
    assert_ne!(first, second);
    for id in [first, second] {
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(
            parts.iter().map(|part| part.len()).collect::<Vec<_>>(),
            [8, 4, 4, 4, 12],
            "{id}"
        );
        assert!(parts[2].starts_with('4'), "{id}");
        assert!(
            id.chars().all(|at| at == '-' || at.is_ascii_hexdigit()),
            "{id}"
        );
    }
}

#[test]
fn claude_code_and_pi_are_opened_on_an_id_they_are_given_and_the_others_name_their_own() {
    for kind in ["claude-code", "pi"] {
        assert_eq!(session_for(kind).map(|id| id.len()), Some(36), "{kind}");
    }
    for kind in ["devin", "codex", "opencode", "claude", ""] {
        assert_eq!(session_for(kind), None, "{kind}");
    }
}

#[test]
fn a_member_has_the_first_message_it_was_given_and_none_until_it_is() {
    let member = Member::new("worker", "# worker");
    assert_eq!(member.message(), None);
    let told = member.first_message("[ConsensFlow m-1 · T-1 · task from @chief]\nGo");
    assert_eq!(
        told.message(),
        Some("[ConsensFlow m-1 · T-1 · task from @chief]\nGo")
    );
}

#[test]
fn what_a_screen_printed_after_a_mark_is_counted_in_characters_not_bytes() {
    assert_eq!(after("héllo wörld", 3), "lo wörld");
    assert_eq!(after("abc", 3), "");
    assert_eq!(after("abc", 9), "");
    assert_eq!(after("abc", 0), "abc");
}

#[test]
fn claude_keeps_its_sessions_where_the_window_says_and_else_in_the_dot_claude_of_its_home() {
    let env = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    };
    let home = Path::new("/machine").join(".claude");
    assert_eq!(claude_config_of(&env(&[("HOME", "/machine")])), home);
    assert_eq!(
        claude_config_of(&env(&[
            ("HOME", "/machine"),
            ("CLAUDE_CONFIG_DIR", "/elsewhere")
        ])),
        Path::new("/elsewhere")
    );
    // A variable set to nothing is not set, and the last of a name is the one.
    assert_eq!(
        claude_config_of(&env(&[
            ("HOME", "/other"),
            ("HOME", "/machine"),
            ("CLAUDE_CONFIG_DIR", "")
        ])),
        home
    );
    assert_eq!(claude_config_of(&[]), Path::new(".claude"));
}

#[test]
fn an_answer_is_on_the_screen_since_the_paste_or_in_an_answer_of_the_record() {
    let record = [
        item("user", "sum 5555 please"),
        item("assistant", "It is 5555."),
    ];
    assert!(answered("... 5555 ...", &[], "5555"));
    assert!(answered("", &record, "5555"));
    // The question holds the answer's digits, and is not the answer.
    assert!(!answered("", &record[..1], "5555"));
    assert!(!answered("nothing yet", &[], "5555"));
    // Drawn in two strokes, the screen's text splits it: the record has it whole.
    assert!(!answered("55 ⏎ 55", &[], "5555"));
    assert!(answered("55 ⏎ 55", &record, "5555"));
}

#[test]
fn a_screen_of_blanks_has_drawn_nothing_and_one_with_text_has_drawn_its_characters() {
    // What Windows' console host prints at once: a jump of the cursor, a line break
    // in the screen's text. Devin draws its first frame seconds after it.
    assert_eq!(drawn(""), 0);
    assert_eq!(drawn("\n"), 0);
    assert_eq!(drawn(" \n   \t\r\n "), 0);
    assert_eq!(drawn("\nDevin CLI  v3000\n"), 13);
    assert_eq!(drawn("é ü\n"), 2);
    // A window that printed only that has not held still with something printed.
    let tiny = Duration::from_millis(1);
    let hold = Duration::from_millis(30);
    assert!(!held_still(
        || drawn("\n"),
        || false,
        Duration::from_millis(100),
        hold,
        tiny
    ));
    assert!(held_still(
        || drawn("\nDevin CLI\n"),
        || false,
        Duration::from_secs(10),
        hold,
        tiny
    ));
}

#[test]
fn a_window_has_held_still_when_its_output_stopped_growing_with_something_printed() {
    let tiny = Duration::from_millis(1);
    let hold = Duration::from_millis(30);
    let within = Duration::from_secs(10);
    assert!(held_still(|| 5, || false, within, hold, tiny));
    // Output that grows at every look never holds still, and the wait runs out.
    let grown = Cell::new(0);
    assert!(!held_still(
        || {
            grown.set(grown.get() + 1);
            grown.get()
        },
        || false,
        Duration::from_millis(100),
        hold,
        tiny
    ));
    // Nothing printed is not a window that drew its prompt.
    assert!(!held_still(
        || 0,
        || false,
        Duration::from_millis(100),
        hold,
        tiny
    ));
    // A window that ended did not hold still.
    assert!(!held_still(|| 5, || true, within, hold, tiny));
    // It counts from the last change: output that stops growing is still after the hold.
    let grows_then_stops = Cell::new(0);
    assert!(held_still(
        || {
            grows_then_stops.set((grows_then_stops.get() + 1).min(5));
            grows_then_stops.get()
        },
        || false,
        within,
        hold,
        tiny
    ));
}

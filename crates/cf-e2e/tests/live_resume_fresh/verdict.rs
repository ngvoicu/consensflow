//! What the check judges of one chief: what it saw of a chief that never had a turn
//! ([`Seen`], [`verdict`]), and of one whose typed turn the project was closed on at
//! once ([`Continued`], [`continued`]).

use cf_e2e::live::members::{squash, Account};
use cf_e2e::live::seconds;

/// What the check saw of one chief: what [`verdict`] judges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    /// How many windows of the chief's the daemon opened by the end.
    pub windows: usize,
    /// Whether the window opened again had ended.
    pub ended: bool,
    /// Whether the project was open when the watch ended.
    pub open: bool,
    /// What the chief's lane read.
    pub state: String,
    /// The conversations the daemon bound to the chief's windows, oldest first.
    pub threads: Vec<String>,
    /// What the harness's record of the first one held before the Close: none
    /// where it kept no record.
    pub held: Option<usize>,
    /// The word the human asked the chief to answer with, and the line of the
    /// chief's record that answered it, if any did.
    pub token: String,
    pub answer: Option<String>,
    /// How long after the Resume the window opened and read idle, in milliseconds.
    pub idle_after_ms: Option<i64>,
    /// What the chief's screen showed at the end, for a failure to say.
    pub screen: String,
}

/// What the check says of a chief, and what it judges: the window opened again
/// and read idle, the project stayed open, and what the human typed was answered.
pub fn verdict(seen: &Seen, case: &mut Account) {
    case.say(match seen.held {
        Some(items) => format!("before the Close, the harness's record held {items} items"),
        None => "before the Close, the harness kept no record of the conversation".to_owned(),
    });
    case.say(match seen.threads.as_slice() {
        [first, second, ..] if first != second => {
            format!("the window opened on a conversation of its own: {first} → {second}")
        }
        [first, _, ..] => format!("the window opened on the conversation it had: {first}"),
        [only] => format!("the daemon bound one conversation, {only}, and no other"),
        [] => "the daemon bound no conversation".to_owned(),
    });
    case.check(
        "the chief's window is opened again and reads idle",
        seen.windows >= 2 && seen.state == "idle",
        format!(
            "{} windows of the chief's were opened{}; its lane reads {}",
            seen.windows,
            seen.idle_after_ms
                .map(|ms| format!(", the last idle {} s after the Resume", seconds(ms)))
                .unwrap_or_default(),
            seen.state
        ),
    );
    case.check(
        "the project stays open",
        seen.open && !seen.ended,
        match (seen.open, seen.ended) {
            (true, false) => "the project is open and the window it opened is there".to_owned(),
            (true, true) => format!("the project is open, but the window ended: {}", seen.screen),
            (false, true) => format!(
                "the project closed again, as the chief's window ended: {}",
                seen.screen
            ),
            (false, false) => "the project is suspended".to_owned(),
        },
    );
    case.check(
        "a message typed to the chief is answered",
        seen.answer.is_some(),
        match &seen.answer {
            Some(line) => format!("it answered: {}", squash(line, 120)),
            None => format!(
                "{} was asked and never answered: {}",
                seen.token, seen.screen
            ),
        },
    );
}

/// What the check saw of one chief that had a turn typed into its window, saved by
/// its harness, and was closed on at once and resumed: what [`continued`] judges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Continued {
    /// How many windows of the chief's the daemon opened by the end.
    pub windows: usize,
    /// Whether the window opened again had ended.
    pub ended: bool,
    /// Whether the project was open when the watch ended.
    pub open: bool,
    /// What the chief's lane read.
    pub state: String,
    /// The conversations the daemon bound to the chief's windows, oldest first.
    pub threads: Vec<String>,
    /// The command line of the window opened again.
    pub argv: String,
    /// The key the human gave the chief before the Close, and the line of the
    /// chief's record that gave it back when the human asked again, if any did.
    pub key: String,
    pub remembered: Option<String>,
    /// What the chief's screen showed at the end, for a failure to say.
    pub screen: String,
}

/// Whether the window opened again is on the conversation the turn was saved in,
/// which the daemon bound when the window first showed it: no other was bound after
/// it (a window opened afresh is a conversation of its own, bound when it shows it),
/// and the window's command line names it. And the words that say so.
fn on_the_conversation(threads: &[String], argv: &str) -> (bool, String) {
    let Some(first) = threads.first() else {
        return (false, "the daemon bound no conversation".to_owned());
    };
    if threads.iter().any(|thread| thread != first) {
        return (
            false,
            format!(
                "the daemon bound {} one after the other: a conversation of its own was begun, and the turn's was left",
                threads.join(" → ")
            ),
        );
    }
    if argv.contains(first.as_str()) {
        (true, format!("it was opened on {first}: {argv}"))
    } else {
        (
            false,
            format!("the window was opened as {argv}, not on {first}"),
        )
    }
}

/// What the check says of a chief that was closed on as soon as its harness had
/// saved a turn typed into its window, and what it judges: the window opened again
/// on the conversation the turn was saved in, and read idle, the project stayed
/// open, and the chief remembers what the turn told it. A harness that keeps a
/// conversation is gone on with, whether or not the daemon had looked at the turn.
pub fn continued(seen: &Continued, case: &mut Account) {
    case.check(
        "the chief's window is opened again and reads idle",
        seen.windows >= 2 && seen.state == "idle",
        format!(
            "{} windows of the chief's were opened; its lane reads {}",
            seen.windows, seen.state
        ),
    );
    case.check(
        "the project stays open",
        seen.open && !seen.ended,
        match (seen.open, seen.ended) {
            (true, false) => "the project is open and the window it opened is there".to_owned(),
            (true, true) => format!("the project is open, but the window ended: {}", seen.screen),
            (false, true) => format!(
                "the project closed again, as the chief's window ended: {}",
                seen.screen
            ),
            (false, false) => "the project is suspended".to_owned(),
        },
    );
    let (on_it, said) = on_the_conversation(&seen.threads, &seen.argv);
    case.check(
        "the window is opened on the conversation the turn was saved in",
        on_it,
        said,
    );
    case.check(
        "the chief remembers what it was told before the Close",
        seen.remembered.is_some(),
        match &seen.remembered {
            Some(line) => format!("it answered: {}", squash(line, 120)),
            None => format!(
                "{} was given and never given back: {}",
                seen.key, seen.screen
            ),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a chief that came back as it should showed.
    fn well() -> Seen {
        Seen {
            windows: 2,
            ended: false,
            open: true,
            state: "idle".to_owned(),
            threads: vec!["t-1".to_owned(), "t-2".to_owned()],
            held: None,
            token: "FRESH_CODEX".to_owned(),
            answer: Some("FRESH_CODEX".to_owned()),
            idle_after_ms: Some(7_400),
            screen: "its screen".to_owned(),
        }
    }

    fn judged(seen: &Seen) -> Account {
        let mut case = Account::new();
        verdict(seen, &mut case);
        case
    }

    /// The checks of a case that did not hold, by their names.
    fn failed(case: &Account) -> Vec<&str> {
        case.lines()
            .iter()
            .filter_map(|line| line.strip_prefix("FAIL "))
            .map(|line| line.split(':').next().unwrap_or_default())
            .collect()
    }

    #[test]
    fn a_chief_whose_window_opened_again_stayed_open_and_answered_holds_and_says_what_it_saw() {
        let case = judged(&well());
        assert!(case.held(), "{}", case.text());
        let text = case.text();
        assert!(
            text.contains("the harness kept no record of the conversation"),
            "{text}"
        );
        assert!(
            text.contains("opened on a conversation of its own: t-1 → t-2"),
            "{text}"
        );
        assert!(
            text.contains("the last idle 7.4 s after the Resume"),
            "{text}"
        );
        assert!(text.contains("it answered: FRESH_CODEX"), "{text}");
    }

    #[test]
    fn a_window_that_opened_on_the_conversation_it_had_is_said_and_judged_of_no_harness() {
        // Claude and Pi may open under the id they had, and a harness that keeps a
        // record before its first turn is resumed: neither is a failure.
        let case = judged(&Seen {
            threads: vec!["t-1".to_owned(), "t-1".to_owned()],
            held: Some(3),
            ..well()
        });
        assert!(case.held(), "{}", case.text());
        let text = case.text();
        assert!(text.contains("the harness's record held 3 items"), "{text}");
        assert!(
            text.contains("opened on the conversation it had: t-1"),
            "{text}"
        );
        // A conversation the daemon never bound for the second window is not one it had.
        let one = judged(&Seen {
            threads: vec!["t-1".to_owned()],
            ..well()
        });
        assert!(
            one.text()
                .contains("bound one conversation, t-1, and no other"),
            "{}",
            one.text()
        );
        let none = judged(&Seen {
            threads: vec![],
            ..well()
        });
        assert!(
            none.text().contains("the daemon bound no conversation"),
            "{}",
            none.text()
        );
    }

    #[test]
    fn a_window_that_never_opened_again_fails_and_so_does_a_project_that_closed_with_it() {
        // What the owner saw on 2026-10-13: the project went open and suspended again, and
        // no window showed.
        let case = judged(&Seen {
            windows: 1,
            ended: true,
            open: false,
            state: "closed".to_owned(),
            answer: None,
            idle_after_ms: None,
            ..well()
        });
        assert!(!case.held());
        assert_eq!(
            failed(&case),
            [
                "the chief's window is opened again and reads idle",
                "the project stays open",
                "a message typed to the chief is answered"
            ]
        );
        let text = case.text();
        assert!(
            text.contains("the project closed again, as the chief's window ended: its screen"),
            "{text}"
        );
        assert!(
            text.contains("1 windows of the chief's were opened"),
            "{text}"
        );
    }

    #[test]
    fn a_window_that_ended_with_the_project_open_and_a_project_suspended_with_its_window_are_told_apart(
    ) {
        let ended = judged(&Seen {
            ended: true,
            ..well()
        });
        assert_eq!(failed(&ended), ["the project stays open"]);
        assert!(
            ended
                .text()
                .contains("the project is open, but the window ended"),
            "{}",
            ended.text()
        );
        let suspended = judged(&Seen {
            open: false,
            ..well()
        });
        assert_eq!(failed(&suspended), ["the project stays open"]);
        assert!(suspended.text().contains("the project is suspended"));
    }

    #[test]
    fn a_window_that_opened_but_never_read_idle_fails_that_check_alone() {
        let case = judged(&Seen {
            state: "starting".to_owned(),
            idle_after_ms: None,
            answer: None,
            ..well()
        });
        assert_eq!(
            failed(&case),
            [
                "the chief's window is opened again and reads idle",
                "a message typed to the chief is answered"
            ]
        );
        assert!(case.text().contains("its lane reads starting"));
    }

    #[test]
    fn a_word_nobody_answered_says_what_was_asked_and_what_the_screen_showed() {
        let case = judged(&Seen {
            answer: None,
            ..well()
        });
        assert_eq!(failed(&case), ["a message typed to the chief is answered"]);
        assert!(
            case.text()
                .contains("FRESH_CODEX was asked and never answered: its screen"),
            "{}",
            case.text()
        );
    }

    /// What a chief that was closed on at once and came back as it should showed.
    fn kept() -> Continued {
        Continued {
            windows: 2,
            ended: false,
            open: true,
            state: "idle".to_owned(),
            threads: vec!["t-1".to_owned()],
            argv: "codex resume t-1 --model x".to_owned(),
            key: "KEY_CODEX".to_owned(),
            remembered: Some("KEY_CODEX".to_owned()),
            screen: "its screen".to_owned(),
        }
    }

    fn judged_kept(seen: &Continued) -> Account {
        let mut case = Account::new();
        continued(seen, &mut case);
        case
    }

    #[test]
    fn a_chief_opened_again_on_the_conversation_of_its_typed_turn_that_gives_the_key_back_holds() {
        let case = judged_kept(&kept());
        assert!(case.held(), "{}", case.text());
        let text = case.text();
        assert!(
            text.contains("it was opened on t-1: codex resume t-1 --model x"),
            "{text}"
        );
        assert!(text.contains("it answered: KEY_CODEX"), "{text}");
        // The daemon may bind the conversation again where it is the same one.
        let again = judged_kept(&Continued {
            threads: vec!["t-1".to_owned(), "t-1".to_owned()],
            ..kept()
        });
        assert!(again.held(), "{}", again.text());
    }

    #[test]
    fn a_window_opened_afresh_loses_the_conversation_of_the_turn_and_fails_that_check_and_the_memory(
    ) {
        // What the old rule did when no look had copied the turn: a conversation of
        // its own, which knows nothing of what the human said.
        let case = judged_kept(&Continued {
            threads: vec!["t-1".to_owned(), "t-2".to_owned()],
            argv: "codex -c x".to_owned(),
            remembered: None,
            ..kept()
        });
        assert_eq!(
            failed(&case),
            [
                "the window is opened on the conversation the turn was saved in",
                "the chief remembers what it was told before the Close"
            ]
        );
        let text = case.text();
        assert!(
            text.contains("the daemon bound t-1 → t-2 one after the other"),
            "{text}"
        );
        assert!(
            text.contains("KEY_CODEX was given and never given back: its screen"),
            "{text}"
        );
    }

    #[test]
    fn a_window_opened_as_something_else_than_the_conversation_and_no_conversation_bound_are_told()
    {
        let elsewhere = judged_kept(&Continued {
            argv: "codex resume t-9".to_owned(),
            ..kept()
        });
        assert_eq!(
            failed(&elsewhere),
            ["the window is opened on the conversation the turn was saved in"]
        );
        assert!(
            elsewhere
                .text()
                .contains("the window was opened as codex resume t-9, not on t-1"),
            "{}",
            elsewhere.text()
        );
        let none = judged_kept(&Continued {
            threads: vec![],
            ..kept()
        });
        assert!(
            none.text().contains("the daemon bound no conversation"),
            "{}",
            none.text()
        );
        assert_eq!(
            failed(&none),
            ["the window is opened on the conversation the turn was saved in"]
        );
    }

    #[test]
    fn a_window_that_never_came_back_or_ended_with_the_project_fails_those_checks() {
        let case = judged_kept(&Continued {
            windows: 1,
            ended: true,
            open: false,
            state: "closed".to_owned(),
            argv: String::new(),
            remembered: None,
            ..kept()
        });
        assert_eq!(
            failed(&case),
            [
                "the chief's window is opened again and reads idle",
                "the project stays open",
                "the window is opened on the conversation the turn was saved in",
                "the chief remembers what it was told before the Close"
            ]
        );
    }
}

//! What Claude's record of the conversation and its status say of a strategy's
//! run, and the lines the run says of it: the message the daemon would send, the
//! user's records of what arrived, whether a message came alone, when Claude read
//! idle after the Escape.

use std::sync::OnceLock;

use cf_e2e::live::claude::{Entry, Sample};
use cf_e2e::live::seconds;
use cf_e2e::pattern;
use regex::Regex;
use serde_json::json;

/// A message shaped like the daemon's, which asks for one word back.
pub fn message(n: u32, word: &str) -> String {
    format!("[ConsensFlow m-{n} · T-1 · task from @chief]\nReply with exactly one line: {word}")
}

/// Whether `text`, the user's record of a message, is the message `n` alone: it
/// holds that message, and no message of another number is in it.
pub fn alone(text: Option<&str>, n: u32) -> bool {
    static MARK: OnceLock<Regex> = OnceLock::new();
    let Some(text) = text else {
        return false;
    };
    text.contains(&format!("m-{n} "))
        && pattern::once(&MARK, r"m-([0-9]+) ")
            .captures_iter(text)
            .all(|found| found[1].parse() == Ok(n))
}

/// The user's records of the conversation, oldest first.
pub fn users(record: &[Entry]) -> Vec<&Entry> {
    record.iter().filter(|entry| entry.kind == "user").collect()
}

/// What a pasted message came to.
pub struct Read {
    /// The user's record that holds the message, as the text it holds.
    pub text: Option<String>,
    /// Whether the window answered it.
    pub answered: bool,
}

/// Whether a message was taken: it arrived alone ([`alone`], by the number `n`
/// it was sent with) and the window answered it.
pub fn taken(read: &Read, n: u32) -> bool {
    alone(read.text.as_deref(), n) && read.answered
}

/// The first sample after `pressed_ms` that reads idle: Claude stopped and waits.
pub fn idle_after(samples: &[Sample], pressed_ms: i64) -> Option<Sample> {
    samples
        .iter()
        .find(|sample| sample.at_ms > pressed_ms && sample.status.as_deref() == Some("idle"))
        .cloned()
}

/// Whether Claude stopped before it wrote anything of its answer: it read idle
/// after the Escape, and its record holds no assistant's record. That is the
/// shape the strategies are for; a turn that went on and wrote something is not.
pub fn stopped_at_once(idle: Option<&Sample>, record: &[Entry]) -> bool {
    idle.is_some() && !record.iter().any(|entry| entry.kind == "assistant")
}

/// `text`'s first `most` characters as a JSON string, or `null` for none: how a
/// record's words are quoted in a line.
pub fn quoted(text: Option<&str>, most: usize) -> String {
    json!(text.map(|text| text.chars().take(most).collect::<String>())).to_string()
}

/// What the Escape at once came to: when Claude read idle, and what its record
/// held then.
pub fn escape_line(idle: Option<&Sample>, pressed_ms: i64, record: &[Entry]) -> String {
    let wrote = record
        .iter()
        .filter(|entry| entry.kind == "assistant")
        .count();
    let users = users(record);
    let said = match idle {
        None => "never".to_owned(),
        Some(sample) => format!(
            "{} s after it, its status changed at {}",
            seconds(sample.at_ms - pressed_ms),
            sample.changed_at_ms.map_or_else(
                || "a time it does not say".to_owned(),
                |at| format!("+{} s", seconds(at - pressed_ms))
            )
        ),
    };
    format!(
        "Escape at once: Claude read idle {said}; its record holds {wrote} assistant records and {} user record: {}",
        users.len(),
        quoted(users.last().map(|entry| entry.text.as_str()), 60)
    )
}

/// The line that says whether the second message arrived alone, and whether it
/// was answered. A message that came with the one before it is `also`, not a
/// failure of the run: it is what the strategy is there to show.
pub fn arrival_line(read: &Read, clean: bool) -> String {
    format!(
        "{} the second message arrived {}{}: {}",
        if clean { "ok  " } else { "also" },
        if clean { "alone" } else { "with the first" },
        if read.answered {
            ", and was answered"
        } else {
            ", and was not answered"
        },
        quoted(read.text.as_deref(), 160)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: &str, text: &str) -> Entry {
        Entry {
            at_ms: None,
            kind: kind.to_owned(),
            text: text.to_owned(),
            model: None,
            raw: String::new(),
        }
    }

    fn sample(at_ms: i64, status: Option<&str>) -> Sample {
        Sample {
            at_ms,
            status: status.map(str::to_owned),
            waiting_for: None,
            changed_at_ms: Some(at_ms - 40),
        }
    }

    #[test]
    fn a_message_is_shaped_like_the_daemons_and_asks_for_one_word() {
        assert_eq!(
            message(2, "SECOND"),
            "[ConsensFlow m-2 · T-1 · task from @chief]\nReply with exactly one line: SECOND"
        );
    }

    #[test]
    fn a_message_is_alone_when_it_is_in_the_text_and_no_other_number_is() {
        assert!(alone(Some(&message(2, "SECOND")), 2));
        // The first message put back into the box, with the second pasted after it.
        let both = format!("{}\n{}", message(1, "FIRST"), message(2, "SECOND"));
        assert!(!alone(Some(&both), 2));
        // m-12 is not m-1, and m-1 is not in m-12.
        assert!(!alone(Some(&message(12, "TWELFTH")), 1));
        let mixed = format!("{} {}", message(1, "A"), message(12, "B"));
        assert!(!alone(Some(&mixed), 1));
        assert!(alone(Some("m-3 and m-3 again"), 3));
        // A number with no space after it is no mark.
        assert!(alone(Some("m-2 and a stray m-1"), 2));
        assert!(!alone(Some("only the third"), 3));
        assert!(!alone(None, 2));
    }

    #[test]
    fn only_the_users_records_are_the_messages_and_the_assistants_are_what_it_wrote() {
        let record = [
            entry("user", "m-1 "),
            entry("assistant", "answer"),
            entry("system/turn_duration", ""),
            entry("user", "m-2 "),
        ];
        let texts: Vec<&str> = users(&record)
            .iter()
            .map(|entry| entry.text.as_str())
            .collect();
        assert_eq!(texts, ["m-1 ", "m-2 "]);
    }

    #[test]
    fn claude_reads_idle_at_the_first_sample_after_the_press_that_says_so() {
        let samples = [
            sample(900, Some("idle")),
            sample(1_100, Some("busy")),
            sample(1_250, None),
            sample(1_400, Some("idle")),
            sample(1_550, Some("idle")),
        ];
        // The idle before the press is no answer to it.
        let at = |after| idle_after(&samples, after).map(|found| found.at_ms);
        assert_eq!(at(1_000), Some(1_400));
        assert_eq!(at(1_400), Some(1_550));
        assert_eq!(at(1_550), None);
        assert_eq!(idle_after(&[], 0), None);
    }

    #[test]
    fn a_message_is_taken_when_it_arrived_alone_and_was_answered_and_not_for_either_alone() {
        let read = |text: Option<&str>, answered: bool| Read {
            text: text.map(str::to_owned),
            answered,
        };
        assert!(taken(&read(Some("m-3 "), true), 3));
        // Alone, and nothing answered it.
        assert!(!taken(&read(Some("m-3 "), false), 3));
        // Answered, but it came with the message before it, or came not at all.
        assert!(!taken(&read(Some("m-2 m-3 "), true), 3));
        assert!(!taken(&read(None, true), 3));
    }

    #[test]
    fn claude_stopped_at_once_when_it_read_idle_and_wrote_nothing_of_an_answer() {
        let idle = sample(2_500, Some("idle"));
        let message = entry("user", &message(1, "FIRST"));
        assert!(stopped_at_once(Some(&idle), std::slice::from_ref(&message)));
        assert!(stopped_at_once(Some(&idle), &[]));
        // It never read idle: the status is not what the Escape was waited on for.
        assert!(!stopped_at_once(None, std::slice::from_ref(&message)));
        // It read idle, but its turn wrote an answer first: not the shape the strategy is for.
        assert!(!stopped_at_once(
            Some(&idle),
            &[message, entry("assistant", "FIRST")]
        ));
    }

    #[test]
    fn what_the_escape_at_once_came_to_is_said_in_seconds_with_what_the_record_holds() {
        let record = [entry("user", &message(1, "FIRST"))];
        let line = escape_line(Some(&sample(2_500, Some("idle"))), 1_000, &record);
        assert_eq!(
            line,
            "Escape at once: Claude read idle 1.5 s after it, its status changed at +1.5 s; its \
             record holds 0 assistant records and 1 user record: \"[ConsensFlow m-1 · T-1 · task \
             from @chief]\\nReply with exactl\""
        );
        // Never idle, and nothing recorded.
        assert_eq!(
            escape_line(None, 1_000, &[]),
            "Escape at once: Claude read idle never; its record holds 0 assistant records and 0 user record: null"
        );
        // A status file that says no time for its change.
        let mut unsaid = sample(2_500, Some("idle"));
        unsaid.changed_at_ms = None;
        assert!(escape_line(Some(&unsaid), 1_000, &[])
            .contains("its status changed at a time it does not say;"));
        // The assistant's records are counted, and only the last user record is quoted.
        let record = [
            entry("user", "m-1 first"),
            entry("assistant", "x"),
            entry("assistant", "y"),
            entry("user", "m-2 second"),
        ];
        assert!(escape_line(None, 0, &record)
            .ends_with("holds 2 assistant records and 2 user record: \"m-2 second\""));
    }

    #[test]
    fn a_record_is_quoted_as_json_by_its_first_characters_not_its_first_bytes() {
        let text = "é".repeat(100);
        assert_eq!(quoted(Some(&text), 60), format!("\"{}\"", "é".repeat(60)));
        assert_eq!(quoted(Some("a\"b\nc"), 10), "\"a\\\"b\\nc\"");
        assert_eq!(quoted(None, 60), "null");
    }

    #[test]
    fn a_message_that_arrived_with_the_one_before_is_also_a_finding_and_one_alone_is_ok() {
        let read = |text: Option<&str>, answered: bool| Read {
            text: text.map(str::to_owned),
            answered,
        };
        assert_eq!(
            arrival_line(&read(Some("m-2 x"), true), true),
            "ok   the second message arrived alone, and was answered: \"m-2 x\""
        );
        assert_eq!(
            arrival_line(&read(Some("m-1 m-2 "), false), false),
            "also the second message arrived with the first, and was not answered: \"m-1 m-2 \""
        );
        assert_eq!(
            arrival_line(&read(None, false), false),
            "also the second message arrived with the first, and was not answered: null"
        );
    }
}

//! The words a board command was given: its flags picked out wherever they
//! stand, the rest its text, and the task and message numbers in it. A
//! `--word` the command does not know is text, as it always was: a brief
//! may well say `--force`.

use cf_base::js;

use super::Failure;

/// A command's words with its known flags taken out.
#[derive(Debug, Default, PartialEq)]
pub struct Split {
    switches: Vec<&'static str>,
    values: Vec<(&'static str, Option<String>)>,
    /// The words that are no flag, joined by spaces.
    pub text: String,
}

impl Split {
    /// Whether the switch `flag` was given.
    pub fn on(&self, flag: &str) -> bool {
        self.switches.contains(&flag)
    }

    /// The word after the last `flag`; none when it was not given, or ended the words.
    pub fn value(&self, flag: &str) -> Option<&str> {
        self.values
            .iter()
            .rev()
            .find(|(name, _)| *name == flag)
            .and_then(|(_, value)| value.as_deref())
    }
}

/// `words` with the switches in `switches` and the flags in `valued` (each
/// taking the word after it, whatever that word is) picked out.
pub fn split(words: &[String], switches: &[&'static str], valued: &[&'static str]) -> Split {
    let mut split = Split::default();
    let mut text = Vec::new();
    let mut rest = words.iter();
    while let Some(word) = rest.next() {
        if let Some(flag) = switches.iter().find(|flag| **flag == word) {
            split.switches.push(flag);
        } else if let Some(flag) = valued.iter().find(|flag| **flag == word) {
            split.values.push((flag, rest.next().cloned()));
        } else {
            text.push(word.as_str());
        }
    }
    split.text = text.join(" ");
    split
}

/// The number in `T-3` (or `3`, `t-3`).
pub fn task_number(word: Option<&str>) -> Result<u64, Failure> {
    numbered(word, "T-").ok_or_else(|| {
        Failure::Usage(format!(
            "not a task: {} (write T-3)",
            quoted(word.unwrap_or_default())
        ))
    })
}

/// The numbers in `T-3,T-4`; none when the flag was not given.
pub fn task_numbers(word: Option<&str>) -> Result<Option<Vec<u64>>, Failure> {
    word.map(|list| {
        list.split(',')
            .map(|part| task_number(Some(js::trim(part))))
            .collect()
    })
    .transpose()
}

/// The number in `m-12` (or `12`, `M-12`).
pub fn message_id(word: Option<&str>) -> Result<u64, Failure> {
    numbered(word, "m-").ok_or_else(|| {
        Failure::Usage(format!(
            "not a message: {} (write m-12)",
            quoted(word.unwrap_or_default())
        ))
    })
}

/// `text`, or a usage failure saying what to write when it is blank.
pub fn require_text(text: String, example: &str) -> Result<String, Failure> {
    if js::trim(&text).is_empty() {
        return Err(Failure::Usage(format!("say what: {example}")));
    }
    Ok(text)
}

/// The digits after an optional `prefix` (any case), as a number.
fn numbered(word: Option<&str>, prefix: &str) -> Option<u64> {
    let word = word?;
    let digits = match word.get(..prefix.len()) {
        Some(head) if head.eq_ignore_ascii_case(prefix) => &word[prefix.len()..],
        _ => word,
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// A word as JSON writes it, quoted: what Node's messages showed with `JSON.stringify`.
pub fn quoted(word: &str) -> String {
    serde_json::Value::from(word).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &[&str]) -> Vec<String> {
        line.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn picks_flags_out_wherever_they_stand_and_keeps_unknown_ones_as_text() {
        let split = split(
            &words(&["fix", "--tier", "light", "the", "--force", "flag", "--self"]),
            &["--self", "--advice"],
            &["--tier", "--needs"],
        );
        assert_eq!(split.text, "fix the --force flag");
        assert!(split.on("--self"));
        assert!(!split.on("--advice"));
        assert_eq!(split.value("--tier"), Some("light"));
        assert_eq!(split.value("--needs"), None);
    }

    #[test]
    fn a_flag_takes_the_next_word_whatever_it_is_and_the_last_one_counts() {
        let split = split(
            &words(&["--tier", "--self", "x", "--tier", "light", "--tier"]),
            &["--self"],
            &["--tier"],
        );
        assert_eq!(split.text, "x");
        assert!(!split.on("--self"), "--self was the first --tier's value");
        assert_eq!(
            split.value("--tier"),
            None,
            "the last --tier ended the words"
        );
    }

    #[test]
    fn reads_task_and_message_numbers_with_or_without_their_prefix() {
        assert_eq!(task_number(Some("T-3")).ok(), Some(3));
        assert_eq!(task_number(Some("t-12")).ok(), Some(12));
        assert_eq!(task_number(Some("7")).ok(), Some(7));
        assert_eq!(message_id(Some("m-12")).ok(), Some(12));
        assert_eq!(message_id(Some("M-4")).ok(), Some(4));
        assert_eq!(task_numbers(Some("T-3, T-4")).ok(), Some(Some(vec![3, 4])));
        assert_eq!(task_numbers(None).ok(), Some(None));
    }

    #[test]
    fn refuses_what_is_no_number_saying_what_to_write() {
        let usage = |failure: Result<u64, Failure>| match failure {
            Err(Failure::Usage(message)) => message,
            other => panic!("not a usage failure: {other:?}"),
        };
        assert_eq!(
            usage(task_number(Some("T-x"))),
            r#"not a task: "T-x" (write T-3)"#
        );
        assert_eq!(usage(task_number(None)), r#"not a task: "" (write T-3)"#);
        assert_eq!(
            usage(task_number(Some("T-"))),
            r#"not a task: "T-" (write T-3)"#
        );
        assert_eq!(
            usage(task_number(Some("٣"))),
            r#"not a task: "٣" (write T-3)"#
        );
        assert_eq!(
            usage(message_id(Some("T-3"))),
            r#"not a message: "T-3" (write m-12)"#
        );
        assert!(matches!(task_numbers(Some("T-3,")), Err(Failure::Usage(_))));
    }

    #[test]
    fn blank_text_is_a_usage_failure_naming_the_example() {
        assert!(matches!(
            require_text(" \n\t".into(), "cf note \"what to know\""),
            Err(Failure::Usage(message)) if message == "say what: cf note \"what to know\""
        ));
        assert_eq!(require_text(" x ".into(), "…").ok().as_deref(), Some(" x "));
    }
}

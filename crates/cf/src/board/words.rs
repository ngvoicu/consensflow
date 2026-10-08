//! The words a board command was given: its flags picked out wherever they
//! stand, the rest its text, and the task and message numbers in it. A
//! `--word` the command does not know is text, as it always was: a brief
//! may well say `--force`. `--help` and `-h` are the one such word that asks
//! for something when it stands alone (see [`Split::asks_for_help`]).

use cf_base::js;

use super::Failure;

/// The two words that ask a command for its usage.
const HELP: [&str; 2] = ["--help", "-h"];

/// What a command takes: the flags it knows, and how many words come first
/// that are not its text (the task or message number it works on).
#[derive(Debug, Clone, Copy)]
pub struct Shape {
    pub switches: &'static [&'static str],
    pub valued: &'static [&'static str],
    pub leading: usize,
}

impl Shape {
    /// A command whose words are all its text.
    pub const TEXT: Self = Self {
        switches: &[],
        valued: &[],
        leading: 0,
    };

    /// A command that works on a task or a message, named first, and says
    /// the rest.
    pub const NUMBERED: Self = Self {
        leading: 1,
        ..Self::TEXT
    };
}

/// A command's words with its known flags taken out.
#[derive(Debug, Default, PartialEq)]
pub struct Split {
    switches: Vec<&'static str>,
    values: Vec<(&'static str, Option<String>)>,
    /// The words that are no flag, in order.
    operands: Vec<String>,
    /// How many of the operands come before the command's text.
    leading: usize,
    /// Whether a `--` ended the flags: the operands after it are text, whatever
    /// they say.
    literal: bool,
    /// The operands after the leading ones, joined by spaces: the command's text.
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

    /// Whether the words only ask for the command's usage: `--help` or `-h`
    /// is the last word that is no flag, with no more words before it than the
    /// command takes ahead of its text, and no `--` ended the flags before it.
    /// `cf note --help` and `cf task done T-3 -h` ask; `cf note see --help`,
    /// `cf note --help me` and `cf note -- --help` are text, as is the `--help`
    /// a flag takes for its value.
    pub fn asks_for_help(&self) -> bool {
        !self.literal
            && self.operands.len() <= self.leading + 1
            && self
                .operands
                .last()
                .is_some_and(|word| HELP.contains(&word.as_str()))
    }
}

/// `words` with the flags of `shape` picked out wherever they stand, each
/// valued one taking the word after it, whatever that word is. A `--` before
/// any text ends the flags: the words after it are text, flags and `--help`
/// included, and the `--` is not. A `--` after text is text, as it always was.
pub fn split(words: &[String], shape: Shape) -> Split {
    let mut split = Split {
        leading: shape.leading,
        ..Split::default()
    };
    let mut rest = words.iter();
    while let Some(word) = rest.next() {
        if word == "--" && split.operands.len() <= shape.leading {
            split.literal = true;
            split.operands.extend(rest.by_ref().cloned());
        } else if let Some(flag) = shape.switches.iter().find(|flag| **flag == word) {
            split.switches.push(flag);
        } else if let Some(flag) = shape.valued.iter().find(|flag| **flag == word) {
            split.values.push((flag, rest.next().cloned()));
        } else {
            split.operands.push(word.clone());
        }
    }
    split.text = split
        .operands
        .get(shape.leading..)
        .unwrap_or_default()
        .join(" ");
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

/// The digits after an optional `prefix` (any case), as a number: read
/// exactly, where JavaScript rounded one past 2^53, which no ledger reaches.
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

    /// A command with a switch `--self` and a valued flag `--tier`, as `cf task add` has.
    const FLAGS: Shape = Shape {
        switches: &["--self", "--advice"],
        valued: &["--tier", "--needs"],
        leading: 0,
    };

    #[test]
    fn picks_flags_out_wherever_they_stand_and_keeps_unknown_ones_as_text() {
        let split = split(
            &words(&["fix", "--tier", "light", "the", "--force", "flag", "--self"]),
            FLAGS,
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
            Shape {
                switches: &["--self"],
                valued: &["--tier"],
                leading: 0,
            },
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
    fn help_is_asked_by_the_one_word_that_is_no_flag_and_by_nothing_else() {
        for line in [
            &["--help"][..],
            &["-h"],
            // The flags of the command stand beside it, wherever.
            &["--self", "--help"],
            &["--help", "--tier", "light"],
            &["--tier", "light", "-h"],
        ] {
            assert!(split(&words(line), FLAGS).asks_for_help(), "{line:?}");
        }
        for line in [
            &[][..],
            // Text that has the word among others.
            &["see", "--help"],
            &["--help", "me"],
            &["--help", "--help"],
            &["--help", "-h"],
            &["cf --help"],
            &["--helpful"],
            &["-help"],
            &["-H"],
            // The word a flag takes for its value.
            &["--tier", "--help"],
            // A `--` ahead of it: it is text.
            &["--", "--help"],
            &["--self", "--", "-h"],
        ] {
            assert!(!split(&words(line), FLAGS).asks_for_help(), "{line:?}");
        }
    }

    #[test]
    fn a_number_a_command_takes_first_may_stand_before_the_word_that_asks_for_help() {
        for line in [&["--help"][..], &["T-3", "--help"], &["T-3", "-h"]] {
            assert!(
                split(&words(line), Shape::NUMBERED).asks_for_help(),
                "{line:?}"
            );
        }
        for line in [
            &["T-3", "fix", "--help"][..],
            &["T-3", "--help", "fix"],
            &["T-3", "--", "--help"],
            &["--", "--help"],
        ] {
            assert!(
                !split(&words(line), Shape::NUMBERED).asks_for_help(),
                "{line:?}"
            );
        }
        // A command that takes no number first has the first word for its text.
        assert!(!split(&words(&["T-3", "--help"]), Shape::TEXT).asks_for_help());
    }

    #[test]
    fn a_double_dash_before_the_text_ends_the_flags_and_is_not_text_and_one_after_it_is() {
        let text = |line: &[&str], shape| split(&words(line), shape).text;
        assert_eq!(text(&["--", "--help"], FLAGS), "--help");
        assert_eq!(text(&["--", "--self", "-h"], FLAGS), "--self -h");
        assert!(!split(&words(&["--", "--self"]), FLAGS).on("--self"));
        assert_eq!(text(&["fix", "--", "later"], FLAGS), "fix -- later");
        assert_eq!(text(&["--self", "--", "x"], FLAGS), "x");
        assert_eq!(text(&["T-3", "--", "--help"], Shape::NUMBERED), "--help");
        assert_eq!(text(&["T-3", "x", "--", "y"], Shape::NUMBERED), "x -- y");
        assert_eq!(text(&["T-3", "fix it"], Shape::NUMBERED), "fix it");
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

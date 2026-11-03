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
    /// Each valued flag given, with the word after it and how far from the end
    /// of the words that word stands.
    values: Vec<(&'static str, Option<(String, usize)>)>,
    /// The words that are no flag, in order.
    operands: Vec<String>,
    /// How many of the operands come before the command's text.
    leading: usize,
    /// Whether a `--` ended the flags: the operands after it are text, whatever
    /// they say.
    literal: bool,
    /// The first word that looks like a flag but is none of the command's,
    /// stood where a flag may — in front of the text, where it would be kept
    /// as the text's own words without a word of complaint (the field report
    /// of 2026-11-02: `--file` on `task done`, which had no `--file`, stored
    /// as the result itself). None after the text's first word: a flag in the
    /// text is words. The word that asks for help is no stray: it answers
    /// alone, and is words among others.
    stray: Option<String>,
    /// The operands after the leading ones, joined by spaces: the command's text.
    pub text: String,
}

impl Split {
    /// Whether the switch `flag` was given.
    pub fn on(&self, flag: &str) -> bool {
        self.switches.contains(&flag)
    }

    /// The word after `flag`; none when it was not given, or ended the words.
    /// Said wrong: the flag was given more than once — a valued flag takes one
    /// word, and a flag's every word is read with [`Split::each`].
    pub fn value(&self, flag: &str) -> Result<Option<&str>, Failure> {
        self.once(flag)?;
        Ok(self.last(flag).map(|(word, _)| word.as_str()))
    }

    /// How many words from the end of the words split the word after `flag`
    /// stands, 1 for the last: where the system's own spelling of it is found
    /// among words those were the last of. None and the refusal as for
    /// [`Split::value`].
    pub fn value_back(&self, flag: &str) -> Result<Option<usize>, Failure> {
        self.once(flag)?;
        Ok(self.last(flag).map(|(_, back)| *back))
    }

    /// Every word `flag` was given, in the order its flags stand: the list a
    /// flag that may be given more than once carries (`--needs T-3 --needs
    /// T-4` is "T-3" then "T-4"). A flag that ended the words has no word, and
    /// gives none.
    pub fn each<'a>(&'a self, flag: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.values
            .iter()
            .filter(move |(name, _)| *name == flag)
            .filter_map(|(_, value)| value.as_ref().map(|(word, _)| word.as_str()))
    }

    /// Whether the valued `flag` was given, with a word after it or not.
    pub fn given(&self, flag: &str) -> bool {
        self.values.iter().any(|(name, _)| *name == flag)
    }

    /// The flag-like word in front of the text, for the command to refuse:
    /// it is none of this command's flags. None when the text's words come
    /// first, or a `--` made words of it.
    pub fn stray_flag(&self) -> Option<&str> {
        self.stray.as_deref()
    }

    /// Said wrong when `flag` was given more than once: a valued flag takes
    /// one word, and keeping the last's alone dropped the rest without a word
    /// (the field report of 2026-11-02). The lists a flag may carry are read
    /// with [`Split::each`], which never refuses.
    fn once(&self, flag: &str) -> Result<(), Failure> {
        if self
            .values
            .iter()
            .filter(|(name, _)| *name == flag)
            .nth(1)
            .is_some()
        {
            return Err(Failure::Usage(format!(
                "{flag} takes one value: it was given more than once"
            )));
        }
        Ok(())
    }

    fn last(&self, flag: &str) -> Option<&(String, usize)> {
        self.values
            .iter()
            .rev()
            .find(|(name, _)| *name == flag)
            .and_then(|(_, value)| value.as_ref())
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
/// A word that looks like a flag but is none of the shape's is an operand, as
/// a flag in the text is words — but the first one to stand in front of the
/// text is kept in `stray` as well ([`Split::stray_flag`]), for the command
/// to refuse rather than store.
pub fn split(words: &[String], shape: Shape) -> Split {
    let mut split = Split {
        leading: shape.leading,
        ..Split::default()
    };
    let mut rest = words.iter().enumerate();
    while let Some((_, word)) = rest.next() {
        if word == "--" && split.operands.len() <= shape.leading {
            split.literal = true;
            split
                .operands
                .extend(rest.by_ref().map(|(_, word)| word.clone()));
        } else if let Some(flag) = shape.switches.iter().find(|flag| **flag == word) {
            split.switches.push(flag);
        } else if let Some(flag) = shape.valued.iter().find(|flag| **flag == word) {
            let value = rest
                .next()
                .map(|(at, value)| (value.clone(), words.len() - at));
            split.values.push((flag, value));
        } else {
            if split.stray.is_none()
                && word.starts_with("--")
                && !HELP.contains(&word.as_str())
                && split.operands.len() <= shape.leading
            {
                split.stray = Some(word.clone());
            }
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

/// The numbers in `T-3,T-4`, in every list a flag was given, in the order they
/// came and each number once; none when the flag was not given, or given with
/// no word after it.
pub fn task_numbers<'a>(lists: impl Iterator<Item = &'a str>) -> Result<Option<Vec<u64>>, Failure> {
    let mut numbers: Vec<u64> = Vec::new();
    for list in lists {
        for part in list.split(',') {
            let number = task_number(Some(js::trim(part)))?;
            if !numbers.contains(&number) {
                numbers.push(number);
            }
        }
    }
    Ok((!numbers.is_empty()).then_some(numbers))
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

    /// What `flag`'s read answers, whatever it went wrong with: the value or
    /// the refusal's words.
    fn read(result: Result<Option<&str>, Failure>) -> Result<Option<String>, String> {
        result
            .map(|word| word.map(str::to_owned))
            .map_err(|failure| match failure {
                Failure::Usage(message) => message,
                Failure::Failed(_) => panic!("not a usage failure: {failure:?}"),
            })
    }

    /// As [`read`], for where a flag's word stands.
    fn read_back(result: Result<Option<usize>, Failure>) -> Result<Option<usize>, String> {
        result.map_err(|failure| match failure {
            Failure::Usage(message) => message,
            Failure::Failed(_) => panic!("not a usage failure: {failure:?}"),
        })
    }

    #[test]
    fn picks_flags_out_wherever_they_stand_and_keeps_unknown_ones_as_text() {
        let split = split(
            &words(&["fix", "--tier", "light", "the", "--force", "flag", "--self"]),
            FLAGS,
        );
        assert_eq!(split.text, "fix the --force flag");
        assert!(split.on("--self"));
        assert!(!split.on("--advice"));
        assert_eq!(read(split.value("--tier")), Ok(Some("light".into())));
        assert_eq!(read(split.value("--needs")), Ok(None));
    }

    #[test]
    fn a_flag_takes_the_next_word_whatever_it_is_and_one_at_the_end_has_none() {
        let split = split(
            &words(&["--tier", "--self", "x"]),
            Shape {
                switches: &["--self"],
                valued: &["--tier"],
                leading: 0,
            },
        );
        assert_eq!(split.text, "x");
        assert!(!split.on("--self"), "--self was the --tier's value");
        assert_eq!(read(split.value("--tier")), Ok(Some("--self".into())));
        let ended = super::split(&words(&["x", "--tier"]), FLAGS);
        assert!(ended.given("--tier"));
        assert_eq!(
            (
                read(ended.value("--tier")),
                read_back(ended.value_back("--tier"))
            ),
            (Ok(None), Ok(None))
        );
    }

    #[test]
    fn a_flag_of_one_value_given_more_than_once_is_refused_not_overwritten() {
        for line in [
            &["--tier", "light", "--tier", "heavy", "x"][..],
            &["--tier", "light", "x", "--tier", "heavy"][..],
            &["--tier", "a", "--tier", "b", "--tier", "c"][..],
        ] {
            let split = split(&words(line), FLAGS);
            assert_eq!(
                read(split.value("--tier")),
                Err("--tier takes one value: it was given more than once".into()),
                "{line:?}"
            );
            assert_eq!(
                read_back(split.value_back("--tier")),
                Err("--tier takes one value: it was given more than once".into()),
                "{line:?}"
            );
        }
        // Once, or never: as they were.
        for line in [&["x", "--tier", "light"][..], &["x"][..]] {
            let split = split(&words(line), FLAGS);
            assert!(split.value("--tier").is_ok(), "{line:?}");
        }
    }

    #[test]
    fn a_flag_the_shape_has_not_is_stray_in_front_of_the_text_and_words_inside_it() {
        let stray = |line: &[&str]| split(&words(line), FLAGS).stray_flag().map(str::to_owned);
        assert_eq!(stray(&["--bogus", "hi"]), Some("--bogus".to_owned()));
        assert_eq!(stray(&["--bogus"]), Some("--bogus".to_owned()));
        assert_eq!(
            stray(&["--bogus", "--tier", "light", "hi"]),
            Some("--bogus".to_owned())
        );
        assert_eq!(stray(&["hi", "--bogus"]), None);
        assert_eq!(stray(&["--tier", "light", "hi", "--bogus"]), None);
        // A `--` makes words of it, and a known flag is no stray.
        assert_eq!(stray(&["--", "--bogus"]), None);
        assert_eq!(
            stray(&["--tier", "--bogus"]),
            None,
            "--bogus is --tier's word"
        );
    }

    #[test]
    fn a_flag_of_a_list_gives_every_word_it_was_given_in_order() {
        let split = split(
            &words(&[
                "--needs", "T-3,T-4", "x", "--needs", "T-5", "--needs", "T-4",
            ]),
            FLAGS,
        );
        assert_eq!(
            split.each("--needs").collect::<Vec<_>>(),
            ["T-3,T-4", "T-5", "T-4"]
        );
        assert!(split.each("--tier").next().is_none());
        // A flag that ends the words has no word and gives none.
        let ended = super::split(&words(&["--needs", "T-3", "--needs"]), FLAGS);
        assert_eq!(ended.each("--needs").collect::<Vec<_>>(), ["T-3"]);
    }

    #[test]
    fn a_value_is_found_by_how_far_from_the_end_it_stands_and_a_flag_that_ended_the_words_was_given(
    ) {
        let given = split(&words(&["x", "--tier", "light", "y"]), FLAGS);
        assert_eq!(read_back(given.value_back("--tier")), Ok(Some(2)));
        assert!(given.given("--tier"));
        let ended = split(&words(&["x", "--needs"]), FLAGS);
        assert!(ended.given("--needs"));
        assert!(!ended.given("--tier"));
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
        assert_eq!(
            task_numbers(["T-3, T-4"].into_iter()).ok().flatten(),
            Some(vec![3, 4])
        );
        assert_eq!(
            task_numbers(["T-41,T-42", "T-43", "T-42,T-44"].into_iter())
                .ok()
                .flatten(),
            Some(vec![41, 42, 43, 44]),
            "lists and repeats mix, each task once in the order it was named"
        );
        assert_eq!(task_numbers([].into_iter()).ok().flatten(), None);
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
        assert!(matches!(
            task_numbers(["T-3,"].into_iter()),
            Err(Failure::Usage(_))
        ));
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

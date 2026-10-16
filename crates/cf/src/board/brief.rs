//! The words of the commands that give a window work (`cf task add`, `cf task
//! reopen`, `cf task resume`, `cf tell`): written out, read from standard input
//! (`-`), or read from a file (`--file <path>`), which no shell cuts. The field
//! report of 2026-10-15 (B6): a chief's heredoc lost its closing word, bash
//! warned, and a brief went to the board cut at about 8.1k characters, which
//! nothing `cf` was given showed. However the words came, the reply says how
//! many characters went and how they end ([`sent`]): a brief cut short shows
//! where.

use std::io::Read;

use super::words::Split;
use super::{text_of, Failure};

/// The flag that names a file to read the words from.
pub const FILE: &str = "--file";

/// How many of the last words the reply says.
const LAST_WORDS: usize = 8;

/// The most of them it says, in characters: a word may be a line of code.
const MOST_SHOWN: usize = 80;

/// The words `words` give: the file `--file` names, read whole as UTF-8 (what
/// is no UTF-8 replaced) with one final line ending dropped, the file's and not
/// the text's; or the words written out, standard input for `-` (`text_of`). A
/// file named beside words of their own, `-` among them, is two texts, and a
/// command written wrong; one that cannot be read fails the command.
pub fn brief_of(words: &Split, input: &mut dyn Read) -> Result<String, Failure> {
    let Some(path) = words.value(FILE) else {
        return text_of(words.text.clone(), input);
    };
    if !words.text.is_empty() {
        return Err(Failure::Usage(format!(
            "give the words or {FILE} {path}, not both"
        )));
    }
    let bytes = std::fs::read(path)
        .map_err(|cause| Failure::Failed(format!("cannot read {path} ({cause})")))?;
    let text = String::from_utf8_lossy(&bytes);
    let text = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(&text);
    Ok(text.to_owned())
}

/// What a reply says of the words it sent: how many characters, and the last
/// few words, their spaces and line breaks as one space, after an ellipsis when
/// there were more.
pub fn sent(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let first = words.len().saturating_sub(LAST_WORDS);
    let mut last = words[first..].join(" ");
    let mut cut = first > 0;
    let shown = last.chars().count();
    if shown > MOST_SHOWN {
        last = last.chars().skip(shown - MOST_SHOWN).collect();
        cut = true;
    }
    format!(
        "Sent: {} characters, ending \"{}{last}\".",
        grouped(text.chars().count()),
        if cut { "…" } else { "" }
    )
}

/// `number` with a comma between each three of its digits: `8,123`.
fn grouped(number: usize) -> String {
    let digits = number.to_string();
    let mut grouped = String::new();
    for (place, digit) in digits.chars().enumerate() {
        if place > 0 && (digits.len() - place).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reply_says_how_many_characters_went_and_how_they_end() {
        assert_eq!(
            sent("Reply with exactly: ONE"),
            "Sent: 23 characters, ending \"Reply with exactly: ONE\"."
        );
        // Lines and spaces are one space; the last eight words, after an ellipsis.
        let brief =
            "Write the parser.\n\nKeep the old tests,\n  and run them all before you answer.";
        assert_eq!(
            sent(brief),
            "Sent: 76 characters, ending \"…tests, and run them all before you answer.\"."
        );
        // A brief that ends inside a fence shows it.
        let cut = format!(
            "{}\n```rust\nfn main() {{\n    let parser = Parser::new(",
            "x ".repeat(4_000)
        );
        assert_eq!(
            sent(&cut),
            "Sent: 8,050 characters, ending \"…```rust fn main() { let parser = Parser::new(\"."
        );
    }

    #[test]
    fn characters_are_counted_as_people_count_them_and_a_long_last_word_is_cut() {
        assert_eq!(sent("漢字 🙂"), "Sent: 4 characters, ending \"漢字 🙂\".");
        let long = format!("see {}", "y".repeat(200));
        assert_eq!(
            sent(&long),
            format!("Sent: 204 characters, ending \"…{}\".", "y".repeat(80))
        );
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1,000");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }
}

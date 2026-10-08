//! The words after a verb, read as Node's `util.parseArgs` reads them in the
//! CLI, strictly: only the options a verb names, each a flag or a text, and
//! positionals when the verb allows them. What it refuses it refuses in the
//! words Node 26.8.1 used, `error.message` and nothing more (the caller says
//! `cf: <message>`).
//!
//! `cf`'s verbs have long options only, none of one letter, and `parseArgs` is
//! called with no `short` for any, so a word that opens with one dash is an
//! option nobody takes, and the first UTF-16 unit after the dash is named.
//! `crates/cf-base/tests/goldens/args.json` holds this to Node on three
//! thousand lists of the words people get wrong (recorded from Node, fixed
//! since: `tests/goldens/README.md`).

use serde_json::Value;

use crate::js;

/// What an option takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Takes {
    /// Nothing: a flag, there or not (`type: 'boolean'`).
    Nothing,
    /// A text: the next word, whatever it looks like, or what follows an `=`
    /// (`type: 'string'`).
    Text,
}

/// An option a verb takes: its long name, and what it takes.
#[derive(Debug, Clone, Copy)]
pub struct Opt {
    pub name: &'static str,
    pub takes: Takes,
}

impl Opt {
    /// An option that takes nothing.
    pub const fn flag(name: &'static str) -> Self {
        Self {
            name,
            takes: Takes::Nothing,
        }
    }

    /// An option that takes a text.
    pub const fn text(name: &'static str) -> Self {
        Self {
            name,
            takes: Takes::Text,
        }
    }
}

/// Whether a word that is no option is taken (`allowPositionals`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Positionals {
    Allowed,
    Refused,
}

/// What the words held: the options given, a flag or a text (the last, for
/// an option given twice), and the positionals in order.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Parsed {
    given: Vec<(&'static str, Option<String>)>,
    pub positionals: Vec<String>,
}

impl Parsed {
    /// Whether the option was given at all.
    pub fn flag(&self, name: &str) -> bool {
        self.given.iter().any(|(given, _)| *given == name)
    }

    /// The text the option was given: the last, when it was given twice.
    pub fn text(&self, name: &str) -> Option<&str> {
        self.given
            .iter()
            .find(|(given, _)| *given == name)
            .and_then(|(_, value)| value.as_deref())
    }

    fn store(&mut self, name: &'static str, value: Option<&str>) {
        let value = value.map(str::to_owned);
        match self.given.iter_mut().find(|(given, _)| *given == name) {
            Some(slot) => slot.1 = value,
            None => self.given.push((name, value)),
        }
    }
}

/// A word as `parseArgs`' first phase reads it.
enum Token<'a> {
    /// A long option: its name, as the word spelled it up to an `=` (`raw`,
    /// with its dashes), and the text it came with, from after an `=`
    /// (`inline`) or from the word that followed it.
    Option {
        name: &'a str,
        raw: &'a str,
        value: Option<&'a str>,
        inline: bool,
    },
    /// A word of one dash and letters: the option of its first letter.
    Short(char),
    Positional(&'a str),
    /// The `--` that ends the options.
    End,
}

/// Reads `words` as `parseArgs` reads them with `options`. The first word
/// that is wrong ends the reading, in the order Node checks them.
pub fn parse(
    words: &[String],
    options: &[Opt],
    positionals: Positionals,
) -> Result<Parsed, String> {
    let mut parsed = Parsed::default();
    for token in tokens(words, options) {
        match token {
            Token::End => {}
            Token::Short(letter) => return Err(unknown_short(letter, positionals)),
            Token::Positional(word) => {
                if positionals == Positionals::Refused {
                    return Err(format!(
                        "Unexpected argument '{word}'. This command does not take positional arguments"
                    ));
                }
                parsed.positionals.push(word.to_owned());
            }
            Token::Option {
                name,
                raw,
                value,
                inline,
            } => {
                let Some(option) = options.iter().find(|option| option.name == name) else {
                    return Err(unknown(raw, &js::stringify(&Value::from(raw)), positionals));
                };
                match (option.takes, value) {
                    (Takes::Text, None) => {
                        return Err(format!("Option '--{name} <value>' argument missing"))
                    }
                    (Takes::Nothing, Some(_)) => {
                        return Err(format!("Option '--{name}' does not take an argument"))
                    }
                    _ => {}
                }
                if !inline && value.is_some_and(looks_like_an_option) {
                    return Err(ambiguous(raw));
                }
                parsed.store(option.name, value);
            }
        }
    }
    Ok(parsed)
}

/// The first phase: every word an option, a positional or the end of the
/// options. A text option takes the word after it, whatever that looks like.
fn tokens<'a>(words: &'a [String], options: &[Opt]) -> Vec<Token<'a>> {
    let takes_text = |name: &str| {
        options
            .iter()
            .any(|option| option.name == name && option.takes == Takes::Text)
    };
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < words.len() {
        let word = words[at].as_str();
        at += 1;
        if word == "--" {
            tokens.push(Token::End);
            tokens.extend(words[at..].iter().map(|word| Token::Positional(word)));
            break;
        }
        if let Some(letter) = short_letter(word) {
            tokens.push(Token::Short(letter));
            continue;
        }
        let Some(after) = word.strip_prefix("--").filter(|after| !after.is_empty()) else {
            tokens.push(Token::Positional(word));
            continue;
        };
        // `isLoneLongOption` looks for the `=` from the fourth unit on, so one
        // right after the dashes belongs to the name; `isLongOptionAndValue`
        // then splits at the first `=` there is, which may be that one.
        let first = after.chars().next().map_or(0, char::len_utf8);
        if after[first..].contains('=') {
            let equals = word.find('=').unwrap_or(word.len());
            tokens.push(Token::Option {
                name: &word[2..equals],
                raw: &word[..equals],
                value: Some(&word[equals + 1..]),
                inline: true,
            });
        } else {
            let value = if takes_text(after) && at < words.len() {
                at += 1;
                Some(words[at - 1].as_str())
            } else {
                None
            };
            tokens.push(Token::Option {
                name: after,
                raw: word,
                value,
                inline: false,
            });
        }
    }
    tokens
}

/// The first letter of a word that opens with one dash and has a letter
/// after it; none for `-` alone, for `--…` and for any other word.
fn short_letter(word: &str) -> Option<char> {
    let letters = word.strip_prefix('-')?;
    letters.chars().next().filter(|letter| *letter != '-')
}

/// `isOptionLikeValue`: two units or more, opening with a dash.
fn looks_like_an_option(value: &str) -> bool {
    value.len() > 1 && value.starts_with('-')
}

/// Node's words for the option of a letter: `charAt(1)`, which for a letter
/// past U+FFFF is half a pair of units. Written out that half is U+FFFD, and
/// the JSON text after it holds the half as its escape.
fn unknown_short(letter: char, positionals: Positionals) -> String {
    if letter.len_utf16() == 2 {
        let mut units = [0; 2];
        letter.encode_utf16(&mut units);
        return unknown(
            "-\u{FFFD}",
            &format!("\"-\\u{:04x}\"", units[0]),
            positionals,
        );
    }
    let word = format!("-{letter}");
    unknown(
        &word,
        &js::stringify(&Value::from(word.as_str())),
        positionals,
    )
}

/// Node's words for an option that is not the verb's, as it spelled it, and
/// `spelled`, the JSON text of that. Its sentence ends in that text with no
/// closing quote after it, and so does this; the suggestion is made only
/// where a positional is allowed.
fn unknown(shown: &str, spelled: &str, positionals: Positionals) -> String {
    let suggestion = match positionals {
        Positionals::Allowed => format!(
            ". To specify a positional argument starting with a '-', place it at the end of the command after '--', as in '-- {spelled}"
        ),
        Positionals::Refused => String::new(),
    };
    format!("Unknown option '{shown}'{suggestion}")
}

/// Node's words for a text option followed by a word that looks like an
/// option: that word is not taken for the text.
fn ambiguous(raw: &str) -> String {
    format!(
        "Option '{raw}' argument is ambiguous.\nDid you forget to specify the option argument for '{raw}'?\nTo specify an option argument starting with a dash use '{raw}=-XYZ'."
    )
}

#[cfg(test)]
mod tests;

//! The flags of the commands: `--name value` and `--name=value`, and the switches
//! that take no value, as Node's `parseArgs` read them for the tools these
//! replace. A word that is no flag, a flag that is unknown, a value that is missing
//! or a switch given one is the human's mistake, and the status is 2.

use std::collections::BTreeMap;

use crate::{Error, Result};

/// What a flag takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Takes {
    /// A value; the flag given again has the last.
    Value,
    /// Nothing: it is on when it is there.
    Nothing,
}

/// A flag a command has: its name, what it takes, and the value it has when it is
/// not given.
#[derive(Debug, Clone, Copy)]
pub struct Flag {
    pub name: &'static str,
    pub takes: Takes,
    pub default: Option<&'static str>,
}

impl Flag {
    /// A flag with a value.
    pub const fn value(name: &'static str) -> Self {
        Self {
            name,
            takes: Takes::Value,
            default: None,
        }
    }

    /// A flag with a value that is `default` when it is not given.
    pub const fn or(name: &'static str, default: &'static str) -> Self {
        Self {
            name,
            takes: Takes::Value,
            default: Some(default),
        }
    }

    /// A flag that takes no value.
    pub const fn switch(name: &'static str) -> Self {
        Self {
            name,
            takes: Takes::Nothing,
            default: None,
        }
    }
}

/// What the words of a command line said, by flag.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Flags {
    values: BTreeMap<&'static str, String>,
}

impl Flags {
    /// The value of the flag `name`: what it was given, or its default; none for a
    /// flag that was not given and has none.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    /// Whether the switch `name` was given.
    pub fn has(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }
}

/// Reads `words` against the flags a command `has`.
pub fn parse(has: &[Flag], words: &[String]) -> Result<Flags> {
    let mut flags = Flags::default();
    for flag in has {
        if let Some(default) = flag.default {
            flags.values.insert(flag.name, default.to_owned());
        }
    }
    let mut words = words.iter();
    while let Some(word) = words.next() {
        let Some(named) = word.strip_prefix("--").filter(|named| !named.is_empty()) else {
            return Err(Error::Usage(format!(
                "{word} is no flag: a flag is --name, with its value after it"
            )));
        };
        let (name, inline) = match named.split_once('=') {
            Some((name, value)) => (name, Some(value.to_owned())),
            None => (named, None),
        };
        let Some(flag) = has.iter().find(|flag| flag.name == name) else {
            return Err(Error::Usage(format!(
                "unknown option --{name}: {}",
                has.iter()
                    .map(|flag| format!("--{}", flag.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        };
        let value = match (flag.takes, inline) {
            (Takes::Nothing, None) => String::new(),
            (Takes::Nothing, Some(_)) => {
                return Err(Error::Usage(format!("--{name} takes no value")));
            }
            (Takes::Value, Some(value)) => value,
            (Takes::Value, None) => match words.next() {
                Some(value) if !value.starts_with("--") => value.clone(),
                _ => {
                    return Err(Error::Usage(format!("--{name} takes a value")));
                }
            },
        };
        flags.values.insert(flag.name, value);
    }
    Ok(flags)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HAS: [Flag; 4] = [
        Flag::or("scenario", "six-decisions"),
        Flag::value("model"),
        Flag::switch("gate"),
        Flag::or("repeat", "1"),
    ];

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    fn read(line: &str) -> Result<Flags> {
        parse(&HAS, &words(line))
    }

    #[test]
    fn a_flag_has_the_value_it_was_given_else_its_default_else_none() {
        let flags = read("--model m --repeat 3").unwrap();
        assert_eq!(flags.get("model"), Some("m"));
        assert_eq!(flags.get("repeat"), Some("3"));
        assert_eq!(flags.get("scenario"), Some("six-decisions"));
        assert_eq!(read("").unwrap().get("model"), None);
        assert!(!flags.has("gate"));
    }

    #[test]
    fn a_value_may_follow_an_equals_sign_and_a_flag_given_again_has_the_last_value() {
        assert_eq!(read("--model=a=b").unwrap().get("model"), Some("a=b"));
        assert_eq!(read("--model a --model b").unwrap().get("model"), Some("b"));
        assert_eq!(read("--model=").unwrap().get("model"), Some(""));
    }

    #[test]
    fn a_switch_takes_no_value_and_is_on_when_it_is_there() {
        assert!(read("--gate").unwrap().has("gate"));
        assert!(read("--gate --model m").unwrap().has("gate"));
        let refused = read("--gate=yes").unwrap_err();
        assert_eq!(refused.to_string(), "--gate takes no value");
        // The word after a switch is no value of it.
        assert!(read("--gate extra").is_err());
    }

    #[test]
    fn a_word_that_is_no_flag_an_unknown_flag_or_a_missing_value_is_a_mistake_that_says_what() {
        assert_eq!(
            read("round-trip").unwrap_err().to_string(),
            "round-trip is no flag: a flag is --name, with its value after it"
        );
        let unknown = read("--nope x").unwrap_err().to_string();
        assert!(
            unknown.starts_with("unknown option --nope: --scenario, --model"),
            "{unknown}"
        );
        assert_eq!(
            read("--model").unwrap_err().to_string(),
            "--model takes a value"
        );
        // The next flag is no value.
        assert_eq!(
            read("--model --gate").unwrap_err().to_string(),
            "--model takes a value"
        );
        assert!(matches!(read("--model").unwrap_err(), Error::Usage(_)));
        // A single dash is a word, and a value may begin with one.
        assert!(read("-x").is_err());
        assert_eq!(read("--model -5").unwrap().get("model"), Some("-5"));
    }
}

//! What the words of a live test's command line name: the harnesses and the cases
//! it is asked to run.

use crate::{Error, Result};

/// The items of `all` that `words` name (`name_of` says what each is called), in the
/// order the words come in; `default` where there are no words. A word that names
/// none is the error `no such <what>: <word>`.
pub fn named<T: Copy>(
    words: &[String],
    all: &[T],
    name_of: fn(T) -> &'static str,
    what: &str,
    default: &[T],
) -> Result<Vec<T>> {
    if words.is_empty() {
        return Ok(default.to_vec());
    }
    words
        .iter()
        .map(|word| {
            all.iter()
                .copied()
                .find(|item| name_of(*item) == word.as_str())
                .ok_or_else(|| Error::Daemon(format!("no such {what}: {word}")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::Harness;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn the_words_name_their_items_in_the_order_they_come_in() {
        let named = named(
            &words(&["pi", "claude", "pi"]),
            &Harness::ALL,
            Harness::name,
            "harness",
            &[Harness::OpenCode],
        )
        .unwrap();
        assert_eq!(named, [Harness::Pi, Harness::Claude, Harness::Pi]);
    }

    #[test]
    fn no_words_name_the_default() {
        let named = named(
            &[],
            &Harness::ALL,
            Harness::name,
            "harness",
            &[Harness::OpenCode],
        )
        .unwrap();
        assert_eq!(named, [Harness::OpenCode]);
    }

    #[test]
    fn a_word_that_names_none_is_the_error_that_says_what_it_was_asked_for() {
        let failed = named(
            &words(&["claude", "kimi"]),
            &Harness::ALL,
            Harness::name,
            "harness",
            &[],
        )
        .unwrap_err();
        assert_eq!(failed.to_string(), "no such harness: kimi");
    }
}

//! Whether a line a window showed names a secret: then the note quotes none of its
//! screen (`shown`). A quote left out is a cost the note bears; a credential quoted is
//! not (the lead's decision of round 5).
//!
//! **A password, an API or private key, a credential** is named wherever its word is in a
//! name, whatever the name's case and however it is split: a name is a run of letters,
//! digits, `_` and `-`, and it names one where, in small letters and with its `_` and `-`
//! taken out, it holds one of [`ANYWHERE`] (`PassWord`, `MYSQL_PWD`, `dbPassword`,
//! `x-api-keys`). Taken as words, `PassWord` was `Pass` and `Word`, `pwd` named nothing and
//! `API keys` no key, and `PassWord="alpha bravo charlie"` and `DB_PWD=red green blue` were
//! quoted whole, for the masker misses short words (both reviews of round 4). The cost is
//! the screens that show the shell's `pwd`, or its `PWD` and `OLDPWD`: they are left out.
//!
//! **A secret, a token, `auth`** and their kin are named only by words, and by the parts of
//! names, not by pieces of them: `DB_SECRET`, `OAuth`, `Authorization` and `AccessToken`
//! name one, `author`, `secretary` and `tokenizer` do not (both reviews of round 3 found a
//! screen with `src/author.rs` or Git's "Author identity unknown" left out whole). So are an
//! API key and a private key written as two words. Blanks are nothing but what parts words:
//! `API    key:` names a key as `API key:` does, which a look that allowed one blank
//! between them let through, quote and all (Astra, the same review).
//!
//! A line's words are what lies between whatever is no letter or digit, each cut
//! again where a name joins its parts: between a small letter and a capital
//! (`apiKey`), before the capital that begins a word after a run of capitals
//! (`OAuth`: `O`, `Auth`), and between letters and digits (`password2`). A secret is
//! named by one of [`WORDS`], or by two words of [`PAIRS`] one after the other.

/// The words that name a secret anywhere in a name, in small letters: a password and its
/// kin, an API key and a private key written as one word, a credential. Their plurals hold
/// them.
const ANYWHERE: [&str; 7] = [
    "password",
    "passwd",
    "passphrase",
    "pwd",
    "apikey",
    "privatekey",
    "credential",
];

/// The words that name a secret as words or as the parts of names, and not as pieces of
/// them, in small letters: a secret, a token, `auth`, `OAuth`, `Authorization` and
/// `bearer`, with the plurals a screen uses.
const WORDS: [&str; 8] = [
    "secret",
    "secrets",
    "token",
    "tokens",
    "auth",
    "oauth",
    "authorization",
    "bearer",
];

/// The words that name a secret one after the other: an API key, a private key, one or
/// more.
const PAIRS: [(&str, &str); 4] = [
    ("api", "key"),
    ("api", "keys"),
    ("private", "key"),
    ("private", "keys"),
];

/// Whether `line` names a secret.
pub(crate) fn names_a_secret(line: &str) -> bool {
    let words = words_of(line);
    names_of(line).any(|name| ANYWHERE.iter().any(|word| name.contains(word)))
        || words.iter().any(|word| WORDS.contains(&word.as_str()))
        || words
            .windows(2)
            .any(|pair| PAIRS.contains(&(pair[0].as_str(), pair[1].as_str())))
}

/// The names of `line`, each in small letters with its `_` and `-` taken out.
fn names_of(line: &str) -> impl Iterator<Item = String> + '_ {
    line.split(|character: char| {
        !(character.is_alphanumeric() || character == '_' || character == '-')
    })
    .map(|name| {
        name.chars()
            .filter(|character| character.is_alphanumeric())
            .collect::<String>()
            .to_lowercase()
    })
    .filter(|name| !name.is_empty())
}

/// The words of `line`, the parts of its names among them, in small letters.
fn words_of(line: &str) -> Vec<String> {
    let characters: Vec<char> = line.chars().collect();
    let mut words = Vec::new();
    let mut word = String::new();
    for (at, &character) in characters.iter().enumerate() {
        if !character.is_alphanumeric() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word).to_lowercase());
            }
            continue;
        }
        let joined = !word.is_empty() && {
            let before = characters[at - 1];
            let next = characters.get(at + 1).copied();
            (before.is_lowercase() && character.is_uppercase())
                || (before.is_alphabetic() != character.is_alphabetic())
                || (before.is_uppercase()
                    && character.is_uppercase()
                    && next.is_some_and(char::is_lowercase))
        };
        if joined {
            words.push(std::mem::take(&mut word).to_lowercase());
        }
        word.push(character);
    }
    if !word.is_empty() {
        words.push(word.to_lowercase());
    }
    words
}

#[cfg(test)]
mod tests;

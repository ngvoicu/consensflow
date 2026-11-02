//! Whether a turn's final answer is a question to the chief (T-1119).
//!
//! A member that cannot go on without an answer asks with `cf ask` and ends its turn: the
//! task waits, and `cf answer` reaches the window. Some end their turn on the question in
//! their own words instead ("Should I continue with the live A/B runs … or stop here so
//! you can review the findings first?", "has the install finished? I'm waiting for your
//! answer"), and since 3.0.7-alpha.1 a turn's end is the task's result: the chief was
//! shown the question as a result, "Decide with: cf task accept | cf task reopen", which
//! `cf answer` cannot reach, and a task that had done none of its work showed done.
//! [`Dispatcher::collect`](crate::dispatcher::Dispatcher) puts such an answer to the chief
//! as the question it is, from the worker, as `cf ask` does.
//!
//! **What tells one is the last paragraph of the worker's own words**, before anything
//! ConsensFlow adds to a result, and only what a worker who is stopped writes:
//! - it says it waits for the chief or for the chief's word ("I'm waiting for your go",
//!   "the addendum waits for your word", "when you reply, I'll restart"), unless a negation
//!   stands before it in its clause ("nothing is waiting for your answer"); or
//! - it ends with a question mark and asks what the worker should do next in the first
//!   person ("Should I rebase first?", "Do you want me to land it?", "Shall we …?").
//!
//! A report that ends with an open question about its product ("Should #7 also cover
//! absolute paths?", "Fine as it is?"), a wait for something that is not the chief (a lock,
//! an agent, a build), and an offer ("can be deleted on your word") are results: their work
//! is done, and the chief decides it. Of the 1,963 results of members in the ledger this
//! was counted on, 12 ended with a question mark and 11 of those were accepted as they were;
//! 18 waited for the chief or asked what to do next, and the chief reopened every one of
//! them with the go or the answer.
//!
//! The rule is on words, so it can miss a worker that asks in other words (it stays a
//! result, as it was); it cannot hold a result that is one. A misfire costs the chief one
//! `cf answer`, and the worker's reply to it is the result.

/// What says a worker waits for the chief, in a paragraph in [`normal`] form. "Waiting for
/// you" also matches "waiting for your": the worker's "you" is the chief. A negation before
/// one takes it back ([`negated`]).
const WAITS: [&str; 11] = [
    "waiting for you",
    "waiting on you",
    "wait for you",
    "waits for you",
    "awaiting you",
    "awaits you",
    "await you",
    "waiting for the chief",
    "waiting on the chief",
    "wait for the chief",
    "waits for the chief",
];

/// What says a worker goes on when the chief answers. A negation before one is the worker's
/// own ("I won't touch it until you answer"), and the wait stands.
const UNTIL_ANSWERED: [&str; 4] = [
    "when you answer",
    "when you reply",
    "until you answer",
    "until you reply",
];

/// What asks, in the first person, what the worker is to do next.
const FIRST_PERSON: [&str; 14] = [
    "should i",
    "shall i",
    "may i",
    "can i",
    "could i",
    "do i",
    "how should i",
    "would you like me",
    "do you want me",
    "want me to",
    "should we",
    "shall we",
    "ok if i",
    "okay if i",
];

/// What takes a clause's meaning back: "not", "nothing", "n't".
const NEGATORS: [&str; 8] = [
    "not", "no", "nothing", "never", "nobody", "without", "nor", "neither",
];

/// How many words before a phrase a negation can stand and still take it back.
const NEGATION_REACH: usize = 5;

/// What ends a clause, to a negation.
const CLAUSE_ENDS: [char; 6] = ['.', '!', '?', ';', ':', ','];

/// What may follow a question mark without moving the question: quotes, brackets and
/// emphasis around it.
const AFTER_QUESTION: [char; 8] = [' ', '"', '\'', '`', ')', ']', '*', '_'];

/// Whether the last paragraph of a worker's words asks the chief what to do, or says it
/// waits for the chief's answer.
pub(crate) fn asks_the_chief(words: &str) -> bool {
    let paragraph = normal(&last_paragraph(words));
    !paragraph.is_empty() && (waits(&paragraph) || asks_what_next(&paragraph))
}

/// The last block of lines with something in them.
fn last_paragraph(words: &str) -> String {
    let mut block: Vec<&str> = Vec::new();
    for line in words.lines().rev() {
        if line.trim().is_empty() {
            if block.is_empty() {
                continue;
            }
            break;
        }
        block.push(line);
    }
    block.reverse();
    block.join("\n")
}

/// Lower case, one space between words, straight quotes.
fn normal(paragraph: &str) -> String {
    let straight: String = paragraph
        .to_lowercase()
        .chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' => '\'',
            '\u{201c}' | '\u{201d}' => '"',
            other => other,
        })
        .collect();
    straight.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether the paragraph says its writer waits for the chief.
fn waits(paragraph: &str) -> bool {
    let waiting = WAITS.iter().any(|phrase| {
        paragraph
            .match_indices(phrase)
            .any(|(at, _)| starts_a_word(paragraph, at) && !negated(&paragraph[..at]))
    });
    waiting
        || UNTIL_ANSWERED.iter().any(|phrase| {
            paragraph
                .match_indices(phrase)
                .any(|(at, _)| starts_a_word(paragraph, at))
        })
}

/// Whether the word at `at` in `paragraph` begins there.
fn starts_a_word(paragraph: &str, at: usize) -> bool {
    !paragraph[..at]
        .chars()
        .next_back()
        .is_some_and(char::is_alphanumeric)
}

/// Whether a negation stands in the few words before a phrase, in its clause (`before`
/// is the paragraph up to the phrase).
fn negated(before: &str) -> bool {
    let clause = before.rsplit(CLAUSE_ENDS).next().unwrap_or("");
    let words: Vec<&str> = clause
        .split(|c: char| !(c.is_alphabetic() || c == '\''))
        .filter(|word| !word.is_empty())
        .collect();
    words
        .iter()
        .rev()
        .take(NEGATION_REACH)
        .any(|word| NEGATORS.contains(word) || word.ends_with("n't"))
}

/// Whether the paragraph ends with a question that asks, in the first person, what to do.
fn asks_what_next(paragraph: &str) -> bool {
    paragraph.trim_end_matches(AFTER_QUESTION).ends_with('?')
        && FIRST_PERSON.iter().any(|cue| at_word_edges(paragraph, cue))
}

/// Whether `phrase` stands in `paragraph` as words of its own: "should i" is not "should
/// it", nor "scan i".
fn at_word_edges(paragraph: &str, phrase: &str) -> bool {
    paragraph.match_indices(phrase).any(|(at, _)| {
        let before = paragraph[..at].chars().next_back();
        let after = paragraph[at + phrase.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric)
            && !after.is_some_and(|c| c.is_alphanumeric() || c == '\'')
    })
}

#[cfg(test)]
mod tests;

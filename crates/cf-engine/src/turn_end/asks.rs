//! Whether a turn's final answer is a question to the chief (T-1119).
//!
//! A member that cannot go on without an answer asks with `cf ask` and ends its turn: the
//! task waits, and `cf answer` reaches the window. Some end their turn on the question in
//! their own words instead ("Should I continue with the live A/B runs … or stop here so
//! you can review the findings first?", "has the install finished? I'm waiting for your
//! answer"), and a turn's end is the task's result: the chief was shown the question as a
//! result, "Decide with: cf task accept | cf task reopen", which `cf answer` cannot reach,
//! and a task that had done none of its work showed done.
//! [`Dispatcher::collect`](crate::dispatcher::Dispatcher) puts such an answer to the chief
//! as the question it is, from the worker, as `cf ask` does, with the way to take the words
//! as the result after all (`cf task finish`, which a waiting task takes).
//!
//! **What tells one is the last paragraph of the worker's own words**, before anything
//! ConsensFlow adds to a result, with what is not the worker's voice taken out of it first:
//! fenced and inline code, quoted text, table rows and block quotes. Then one of two things,
//! and only what a worker who is stopped writes:
//! - it says it waits for the chief's answer, reply, go, word or approval ("I'm waiting for
//!   your go", "the addendum waits for your word", "waiting on you"), or, in the first
//!   person, that it goes on when the chief answers ("when you reply, I'll restart"); a
//!   negation in the clause takes it back ("nothing is waiting for your answer"), unless a
//!   "but", "and" or "so" stands between; or
//! - its last sentence ends with a question mark and asks, in the first person, whether to
//!   go on, stop or wait ("Should I continue … or stop here?", "How should I proceed?").
//!
//! **A finished piece of work is a result, whatever it ends on.** A report that ends with an
//! open question about its product ("Should #7 also cover absolute paths?"), an offer ("Want
//! me to also add the Windows case?", "Shall I land it?"), work that awaits review or
//! feedback, a wait for something that is not the chief (a lock, an agent, a build), and a
//! description of this very feature are results: the chief decides them. Of the 1,963
//! results of members in the ledger this was counted on, 12 ended on a question mark and 11 of
//! those were accepted as they were, and the chief reopened each of the 18 that waited.
//!
//! The rule is on English words, so it misses a worker that asks in other words (a result,
//! as it was), and "I won't touch it until you answer", whose negation it reads as a denial.
//! A misfire costs the chief one `cf task finish`, which takes the words as the result.

/// What says a worker waits ("waiting for", "awaits"): the one it waits for follows.
const WAIT_STEMS: [&str; 7] = [
    "waiting for",
    "waiting on",
    "wait for",
    "waits for",
    "awaiting",
    "awaits",
    "await",
];

/// What the chief gives that a worker waits for. "Review", "feedback", "inspection": the
/// work is done, and it awaits those as a result does. "Decision" is a wait only to go on
/// ([`DECISIONS`]).
const GIVEN: [&str; 15] = [
    "answer",
    "answers",
    "reply",
    "response",
    "go",
    "go-ahead",
    "word",
    "ok",
    "okay",
    "approval",
    "confirmation",
    "reopen",
    "instruction",
    "instructions",
    "say-so",
];

/// A decision the chief makes that a worker waits for, to go on.
const DECISIONS: [&str; 5] = [
    "decision to go on",
    "decision to continue",
    "decision to proceed",
    "decision to stop",
    "decision on whether to",
];

/// What says a worker goes on when the chief answers. Only a worker's own clause (first
/// person) says it: "when you answer, nothing is typed" describes a feature.
const WHEN_ANSWERED: [&str; 4] = [
    "when you answer",
    "when you reply",
    "until you answer",
    "until you reply",
];

/// What makes a clause a worker's own.
const FIRST_PERSON: [&str; 9] = ["i", "i'm", "i'll", "i've", "i'd", "we", "we'll", "me", "my"];

/// What asks, in the first person, a question the worker is stopped on.
const MODALS: [&str; 8] = [
    "should i",
    "shall i",
    "may i",
    "can i",
    "could i",
    "do i",
    "should we",
    "shall we",
];

/// What the question is about, to be one the worker is stopped on: not an offer ("Shall I
/// land it?"), not a question about the work ("Should I rebase first?").
const GO_ON: [&str; 10] = [
    "continue",
    "proceed",
    "stop",
    "go on",
    "go ahead",
    "carry on",
    "keep going",
    "wait",
    "resume",
    "abort",
];

/// What takes a clause's meaning back: "not", "nothing", "n't".
const NEGATORS: [&str; 8] = [
    "not", "no", "nothing", "never", "nobody", "without", "nor", "neither",
];

/// What starts a new clause for a negation: "I did not run it and am waiting for your go".
const TURNS: [&str; 11] = [
    "and", "but", "so", "then", "though", "although", "yet", "while", "because", "since", "however",
];

/// What ends a clause, to a negation.
const CLAUSE_ENDS: [char; 8] = ['.', '!', '?', ';', ':', ',', '\u{b6}', '('];

/// What marks a bullet or a numbered line in a paragraph, where a new sentence begins.
const BULLET: char = '\u{b6}';

/// What may follow a question mark without moving the question: quotes, brackets and
/// emphasis around it.
const AFTER_QUESTION: [char; 8] = [' ', '"', '\'', '`', ')', ']', '*', '_'];

/// Whether the last paragraph of a worker's words asks the chief what to do, or says it
/// waits for the chief's answer.
pub(crate) fn asks_the_chief(words: &str) -> bool {
    let paragraph = plain(&last_paragraph(&without_blocks(words)));
    !paragraph.is_empty() && (waits(&paragraph) || asks_what_next(&paragraph))
}

/// The words without their fenced code, table rows and block quotes: what is not the
/// worker's voice. A fence never closed runs to the end.
fn without_blocks(words: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut fenced = false;
    for line in words.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced || trimmed.starts_with('|') || trimmed.starts_with('>') {
            continue;
        }
        kept.push(line);
    }
    kept.join("\n")
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

/// The paragraph as one line of lower-case words: a bullet begins a sentence, inline code
/// and quoted text are gone, quotes are straight.
fn plain(paragraph: &str) -> String {
    let lines: Vec<String> = paragraph
        .lines()
        .map(|line| {
            let line = line.trim();
            if begins_a_bullet(line) {
                format!("{BULLET} {line}")
            } else {
                line.to_owned()
            }
        })
        .collect();
    let straight: String = lines
        .join(" ")
        .to_lowercase()
        .chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' => '\'',
            '\u{201c}' | '\u{201d}' => '"',
            other => other,
        })
        .collect();
    let spoken = without_quotes(&without_spans(&straight, '`'));
    spoken.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether a line begins a bullet or a numbered item.
fn begins_a_bullet(line: &str) -> bool {
    if ["- ", "* ", "\u{2022} "]
        .iter()
        .any(|mark| line.starts_with(mark))
    {
        return true;
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && matches!(line[digits..].chars().next(), Some('.' | ')')) && {
        line[digits + 1..].starts_with(' ')
    }
}

/// `text` without the spans between two `mark`s (marks included). A mark with no other
/// to close it stays.
fn without_spans(text: &str, mark: char) -> String {
    let mut kept = String::new();
    let mut rest = text;
    while let Some(open) = rest.find(mark) {
        let after = &rest[open + mark.len_utf8()..];
        let Some(close) = after.find(mark) else {
            break;
        };
        kept.push_str(&rest[..open]);
        kept.push(' ');
        rest = &after[close + mark.len_utf8()..];
    }
    kept.push_str(rest);
    kept
}

/// Text without what is quoted: between double quotes, and between single quotes that are
/// quotes and no apostrophes (one that opens a word, one that closes after a mark).
fn without_quotes(text: &str) -> String {
    let doubles = without_spans(text, '"');
    let chars: Vec<char> = doubles.chars().collect();
    let mut kept = String::new();
    let mut at = 0;
    while at < chars.len() {
        let opens = chars[at] == '\''
            && !at
                .checked_sub(1)
                .is_some_and(|before| chars[before].is_alphanumeric())
            && chars.get(at + 1).is_some_and(|next| next.is_alphanumeric());
        if opens {
            let close = (at + 1..chars.len()).find(|&end| {
                chars[end] == '\''
                    && !chars[end - 1].is_whitespace()
                    && !chars
                        .get(end + 1)
                        .is_some_and(|next| next.is_alphanumeric())
            });
            if let Some(end) = close {
                kept.push(' ');
                at = end + 1;
                continue;
            }
        }
        kept.push(chars[at]);
        at += 1;
    }
    kept
}

/// Whether the paragraph says its writer waits for the chief.
fn waits(paragraph: &str) -> bool {
    waits_for_the_chief(paragraph) || goes_on_when_answered(paragraph)
}

/// "Waiting for your go", "awaits the chief's word", "waiting on you".
fn waits_for_the_chief(paragraph: &str) -> bool {
    WAIT_STEMS.iter().any(|stem| {
        paragraph.match_indices(stem).any(|(at, _)| {
            let after = &paragraph[at + stem.len()..];
            starts_a_word(paragraph, at)
                && waits_for_who(after.trim_start())
                && !negated(&paragraph[..at])
        })
    })
}

/// Whether what follows a wait ("you …", "the chief …") is a wait for the chief's answer.
fn waits_for_who(after: &str) -> bool {
    if let Some(rest) = after.strip_prefix("you") {
        return match rest.chars().next() {
            Some('r') => gives(&rest[1..]),
            // Waiting for you, to do the next thing.
            Some(next) => !next.is_alphanumeric() && next != '\'',
            None => true,
        };
    }
    if let Some(rest) = after.strip_prefix("the chief") {
        return match rest.chars().next() {
            Some('\'') => rest.strip_prefix("'s").is_some_and(gives),
            Some(next) => !next.is_alphanumeric(),
            None => true,
        };
    }
    false
}

/// Whether what follows "your" names what the chief gives, in its first words: "your gate
/// go", "your word on it", "your decision to go on".
fn gives(after: &str) -> bool {
    let clause = after.split(CLAUSE_ENDS).next().unwrap_or("");
    let clause = clause.trim_start();
    if DECISIONS
        .iter()
        .any(|decision| clause.starts_with(decision))
    {
        return true;
    }
    let words: Vec<&str> = clause
        .split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '-'))
        .filter(|word| !word.is_empty())
        .take(3)
        .collect();
    words.iter().any(|word| GIVEN.contains(word))
        || words.windows(2).any(|pair| pair == ["green", "light"])
}

/// "When you reply, I'll restart": a worker's own clause that goes on when the chief answers.
fn goes_on_when_answered(paragraph: &str) -> bool {
    WHEN_ANSWERED.iter().any(|stem| {
        paragraph.match_indices(stem).any(|(at, _)| {
            let after = paragraph[at + stem.len()..].trim_start();
            let ends_there = after.is_empty()
                || after.starts_with(|c: char| ",.;:!?)\u{2014}-\u{b6}".contains(c));
            starts_a_word(paragraph, at)
                && ends_there
                && has_first_person(sentence_around(paragraph, at))
                && !negated(&paragraph[..at])
        })
    })
}

/// Whether the word at `at` in `paragraph` begins there.
fn starts_a_word(paragraph: &str, at: usize) -> bool {
    !paragraph[..at]
        .chars()
        .next_back()
        .is_some_and(char::is_alphanumeric)
}

/// The words of a clause or sentence, apostrophes of quotes taken off.
fn words_of(clause: &str) -> impl Iterator<Item = &str> {
    clause
        .split(|c: char| !(c.is_alphabetic() || c == '\''))
        .map(|word| word.trim_matches('\''))
        .filter(|word| !word.is_empty())
}

/// Whether the sentence is the writer's own: it says "I" or "my".
fn has_first_person(sentence: &str) -> bool {
    words_of(sentence).any(|word| FIRST_PERSON.contains(&word))
}

/// Whether a negation stands in the clause before a phrase (`before` is the paragraph up to
/// it), with no "but" or "and" between: "nothing is waiting for your answer", not "I did not
/// run it and am waiting for your go".
fn negated(before: &str) -> bool {
    let clause = before.rsplit(CLAUSE_ENDS).next().unwrap_or("");
    let words: Vec<&str> = words_of(clause).collect();
    for word in words.iter().rev() {
        if TURNS.contains(word) {
            return false;
        }
        if NEGATORS.contains(word) || word.ends_with("n't") {
            return true;
        }
    }
    false
}

/// Whether `at` in `text` ends a sentence: a mark, a bullet, a full stop that is no
/// ellipsis and is followed by a space.
fn ends_a_sentence(text: &str, at: usize) -> bool {
    match text[at..].chars().next() {
        Some('!' | '?' | BULLET) => true,
        Some('.') => {
            !text[..at].ends_with('.')
                && text[at + 1..]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace)
        }
        _ => false,
    }
}

/// The sentence of `text` that holds `at`.
fn sentence_around(text: &str, at: usize) -> &str {
    let start = text[..at]
        .char_indices()
        .rev()
        .find(|&(end, _)| ends_a_sentence(text, end))
        .map_or(0, |(end, mark)| end + mark.len_utf8());
    let finish = text[at..]
        .char_indices()
        .find(|&(end, _)| ends_a_sentence(text, at + end))
        .map_or(text.len(), |(end, _)| at + end);
    &text[start..finish]
}

/// Whether the paragraph ends with a question that asks, in the first person, whether to
/// go on, stop or wait: its last sentence says both.
fn asks_what_next(paragraph: &str) -> bool {
    let asked = paragraph.trim_end_matches(AFTER_QUESTION);
    if !asked.ends_with('?') {
        return false;
    }
    let sentence = sentence_around(asked, asked.len() - 1);
    MODALS.iter().any(|cue| at_word_edges(sentence, cue))
        && GO_ON.iter().any(|verb| at_word_edges(sentence, verb))
}

/// Whether `phrase` stands in `text` as words of its own: "should i" is not "should it",
/// nor "scan i", and "stop" is not "stopped".
fn at_word_edges(text: &str, phrase: &str) -> bool {
    text.match_indices(phrase).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + phrase.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric)
            && !after.is_some_and(|c| c.is_alphanumeric() || c == '\'')
    })
}

#[cfg(test)]
mod tests;

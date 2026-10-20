//! A message delivered twice, or joined to another (7, B5): a result read twice,
//! the second copy glued in front of the next result (m-246, m-248, a Devin chief on
//! Windows). Every delivery opens with its header, `[ConsensFlow m-12 · T-3 · result
//! from @x]`, which is also how the daemon knows it arrived; so a conversation's copy
//! that holds a message's header in two prompts was given it twice, and a prompt
//! that holds the headers of two messages was given them as one. Every user's item whose
//! first header begins a line is a prompt, whether the header opens it (on the line after
//! the tag Claude Code wraps a long paste in) or stands behind other words (Devin pastes
//! behind what its input box holds), but the rows a harness writes of the conversation
//! itself, each named by what it opens with: the summary of a compaction (which quoted
//! m-13 on the Mac's 2-hour run of 2026-10-19), the history Codex gives its approval
//! reviewer, Claude Code's notification of a job's end. One whose first header comes after
//! other words on its line may be a paste glued behind them or a quote: unresolved, never
//! dropped (GPT-6.1-Sol's review of round b1). (A message that
//! carries others is one paste under its own header, and what quotes a delivery
//! defuses its header: neither counts. Nor does a header a message's own words quote,
//! as a worker's result may quote the brief it was given: a header inside the words
//! of a message delivered before it in the same prompt, where they stand after its
//! header, is a quote, not a delivery; one after them is.) A header is read whatever
//! the white space in it, as a paste or a retry may change it. The trace of the
//! deliveries, `delivery.begun` and `delivery.retried` in the log, is the evidence.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use cf_e2e::pattern;
use regex::Regex;

use super::{plain, Break, Evidence, Unjudged};
use crate::long_run::ledger::{Item, Message};
use crate::long_run::look::Look;
use crate::long_run::mask::{cut, Names};

/// Each delivery's header in a text, in order: the message it names, and where the
/// header begins and ends.
pub(super) fn headers(text: &str) -> Vec<(i64, usize, usize)> {
    static HEADER: OnceLock<Regex> = OnceLock::new();
    pattern::once(
        &HEADER,
        r"\[ConsensFlow\s+m-(\d+)(?:\s*·\s*T-\d+)?\s*·\s*[a-z]+\s+from\s+[^\]\n]+\]",
    )
    .captures_iter(text)
    .filter_map(|found| {
        let whole = found.get(0)?;
        Some((found[1].parse().ok()?, whole.start(), whole.end()))
    })
    .collect()
}

/// How far into `text` the words `body` stand, from its start: the end of the part
/// of `text` that says them, white space read as white space whatever its length; the
/// start, where `text` does not begin with them (a body cut to its opening is
/// followed as far as it goes).
fn said_until(text: &str, body: &str) -> usize {
    let mut text_chars = text.char_indices().peekable();
    let mut body_chars = body.trim_start().chars().peekable();
    while text_chars.peek().is_some_and(|(_, c)| c.is_whitespace()) {
        text_chars.next();
    }
    let mut until = text_chars.peek().map_or(text.len(), |(at, _)| *at);
    loop {
        let (Some((at, said)), Some(wanted)) =
            (text_chars.peek().copied(), body_chars.peek().copied())
        else {
            return until;
        };
        if said.is_whitespace() && wanted.is_whitespace() {
            while text_chars.peek().is_some_and(|(_, c)| c.is_whitespace()) {
                text_chars.next();
            }
            while body_chars.peek().is_some_and(|c| c.is_whitespace()) {
                body_chars.next();
            }
            continue;
        }
        if said != wanted {
            return until;
        }
        text_chars.next();
        body_chars.next();
        until = at + said.len_utf8();
    }
}

/// The messages a prompt was given, in order: each header of it but those inside the
/// words of a message delivered before it in the prompt (`body` of a message, by its
/// id), where they stand after that message's header.
fn given_in<'b>(text: &str, body: impl Fn(i64) -> Option<&'b str>) -> Vec<i64> {
    let mut given: Vec<i64> = Vec::new();
    let mut quoting_until = 0;
    for (id, start, end) in headers(text) {
        if start < quoting_until {
            continue;
        }
        given.push(id);
        quoting_until = body(id).map_or(end, |said| end + said_until(&text[end..], said));
    }
    given
}

/// The words of the message `id`, as the ledger holds them.
fn body_in(look: &Look, id: i64) -> Option<&str> {
    look.ledger
        .messages
        .iter()
        .find(|message| message.id == id)
        .map(|message| message.body.as_str())
}

/// The words a row opens with that a harness writes of the conversation itself as a user's
/// item, quoting deliveries it was never given: Claude Code's summary of a compaction (the
/// prompts before it quoted whole: m-13 on the Mac's 2-hour run of 2026-10-19), the history
/// Codex gives its approval reviewer, and Claude Code's notification of a job's end (the
/// job's output in it).
const SYNTHETIC: [&str; 3] = [
    "This session is being continued from a previous conversation",
    "The following is the Codex agent history whose request action you are assessing",
    "<task-notification>",
];

/// What a user's item that holds a delivery's header is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prompt {
    /// The window was given a delivery in it: its first header begins a line, as a paste
    /// does at the start of the prompt, on the line after the tag Claude Code wraps a long
    /// paste in (2.1.303: `<pasted_content id="…">`), or behind what the input box held
    /// (Devin's) or what Claude Code wrapped with it.
    Delivery,
    /// A row the harness writes of the conversation itself ([`SYNTHETIC`]): none.
    Synthetic,
    /// Its first header comes after other words on its line: a paste glued behind them, or
    /// words that quote it, which the copy does not tell apart.
    Undecided,
}

/// What `item` is, where it is a user's item that holds a delivery's header.
fn prompt_of(item: &Item) -> Option<Prompt> {
    if item.role != "user" {
        return None;
    }
    let (_, first, _) = *headers(&item.text).first()?;
    let opening = item.text.trim_start();
    if SYNTHETIC.iter().any(|marker| opening.starts_with(marker)) {
        return Some(Prompt::Synthetic);
    }
    let before = item.text[..first].rsplit('\n').next().unwrap_or_default();
    Some(if before.trim().is_empty() {
        Prompt::Delivery
    } else {
        Prompt::Undecided
    })
}

/// Whether `item` is a prompt the window was given a delivery in ([`Prompt::Delivery`]).
pub(super) fn is_delivery(item: &Item) -> bool {
    prompt_of(item) == Some(Prompt::Delivery)
}

/// 7, unresolved: each prompt whose first header comes after other words on its line
/// ([`Prompt::Undecided`]), which is never taken for a delivery nor for none.
pub fn undecided(look: &Look) -> Vec<Unjudged> {
    look.ledger
        .items
        .iter()
        .filter(|item| prompt_of(item) == Some(Prompt::Undecided))
        .filter_map(|item| {
            let (id, ..) = *headers(&item.text).first()?;
            let handle = look
                .ledger
                .conversations
                .iter()
                .find(|conversation| conversation.id == item.conversation)
                .map_or("?", |conversation| look.ledger.handle(conversation.participant));
            Some(Unjudged::new(
                7,
                format!("7:undecided:{}:{}", item.conversation, item.id),
                format!(
                    "@{handle}'s prompt {} holds m-{id}'s header after other words on its line: whether it went in behind them or they quote it, the copy does not say",
                    item.id
                ),
            ))
        })
        .collect()
}

/// The messages `item` delivered to `worker`, in order: none where it is no delivery
/// ([`is_delivery`]); else each message it was given ([`given_in`]: a header its words
/// quote is none) that the ledger holds as one to `worker`.
pub(super) fn delivered_to<'l>(look: &'l Look, item: &Item, worker: i64) -> Vec<&'l Message> {
    if !is_delivery(item) {
        return Vec::new();
    }
    given_in(&item.text, |id| body_in(look, id))
        .into_iter()
        .filter_map(|id| look.ledger.messages.iter().find(|message| message.id == id))
        .filter(|message| message.recipient == worker)
        .collect()
}

/// 7: per conversation, a message whose header is in two of its prompts, and a
/// prompt that holds the headers of two messages; a prompt being one the window was
/// given a delivery in ([`is_delivery`]).
pub fn twice_or_joined(look: &Look) -> Vec<Break> {
    let body = |id: i64| body_in(look, id);
    let names = Names::of(look);
    let mut found = Vec::new();
    for conversation in &look.ledger.conversations {
        let handle = look.ledger.handle(conversation.participant);
        let prompts = look
            .ledger
            .items
            .iter()
            .filter(|item| item.conversation == conversation.id && is_delivery(item));
        let mut given: BTreeMap<i64, Vec<String>> = BTreeMap::new();
        for item in prompts {
            let named = given_in(&item.text, body);
            for id in &named {
                given.entry(*id).or_default().push(item.id.clone());
            }
            let mut distinct = named.clone();
            distinct.sort_unstable();
            distinct.dedup();
            if distinct.len() > 1 {
                let ids: Vec<String> = distinct.iter().map(|id| format!("m-{id}")).collect();
                found.push(
                    Break::new(
                        7,
                        format!("7:joined:{}:{}", conversation.id, item.id),
                        format!("@{handle} was given {} as one prompt", ids.join(" and ")),
                    )
                    .with(Evidence {
                        rows: distinct
                            .iter()
                            .flat_map(|id| deliveries(look, *id, &names))
                            .collect(),
                        screen: Vec::new(),
                        record: vec![format!(
                            "{}: {}",
                            item.id,
                            cut(&plain(&item.text), 300, &names)
                        )],
                    }),
                );
            }
        }
        for (id, items) in given.into_iter().filter(|(_, items)| items.len() > 1) {
            found.push(
                Break::new(
                    7,
                    format!("7:twice:{}:m-{id}", conversation.id),
                    format!("@{handle} was given m-{id} {} times", items.len()),
                )
                .with(Evidence {
                    rows: deliveries(look, id, &names),
                    screen: Vec::new(),
                    record: items
                        .iter()
                        .map(|item| format!("prompt {item} holds m-{id}'s header"))
                        .collect(),
                }),
            );
        }
    }
    found
}

/// The message `id` and what the log says of its deliveries, as evidence.
fn deliveries(look: &Look, id: i64, names: &Names) -> Vec<String> {
    let mut rows: Vec<String> = look
        .ledger
        .messages
        .iter()
        .filter(|message| message.id == id)
        .map(|message| super::order::message_row(message, names))
        .collect();
    rows.extend(
        look.ledger
            .events
            .iter()
            .filter(|event| {
                event.kind.starts_with("delivery.") && event.data["message"].as_i64() == Some(id)
            })
            .map(|event| format!("{} {} {}", event.at, event.kind, event.data)),
    );
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(text: &str) -> Vec<i64> {
        headers(text).into_iter().map(|(id, ..)| id).collect()
    }

    #[test]
    fn a_header_names_its_message_and_a_quote_or_a_carried_label_names_none() {
        assert_eq!(
            ids("[ConsensFlow m-246 · T-9 · result from @zeus]\nDone."),
            [246]
        );
        assert_eq!(
            ids("[ConsensFlow m-7 · note from ConsensFlow]\nhi [ConsensFlow m-8 · result from @a]"),
            [7, 8]
        );
        for none in [
            "(kept for you: result m-3 from @zeus)",
            "[ConsensFlow m-12]",
            "[ConsensFlow m-12 ",
            "ConsensFlow m-12 · result from @x",
        ] {
            assert!(ids(none).is_empty(), "{none}");
        }
    }

    #[test]
    fn a_header_the_words_of_a_message_before_it_quote_is_no_delivery_and_one_glued_after_is() {
        let quoting = "noted: [ConsensFlow m-10 · T-1 · task from @human]";
        let prompt = format!("[ConsensFlow m-11 · T-1 · result from @w]\n{quoting}");
        let body = |id: i64| (id == 11).then_some(quoting);
        assert_eq!(given_in(&prompt, body), [11]);
        // The B5 shape: a body that holds no header, and the next delivery glued to it.
        let glued = "[ConsensFlow m-246 · T-9 · result from @w]\nDone.[ConsensFlow m-248 · T-9 · result from @w]\nAlso done.";
        let body = |id: i64| (id == 246).then_some("Done.");
        assert_eq!(given_in(glued, body), [246, 248]);
        // A message the ledger has not: its header is a delivery all the same.
        assert_eq!(given_in(&prompt, |_| None), [11, 10]);
    }

    #[test]
    fn a_header_with_other_white_space_is_a_header() {
        assert_eq!(
            ids("[ConsensFlow m-248  ·  T-9 ·\tresult  from @w]\nAlso done."),
            [248]
        );
        assert_eq!(ids("[ConsensFlow  m-7·note from ConsensFlow]"), [7]);
        let glued = "[ConsensFlow m-246 · T-9 · result from @w]\nDone.[ConsensFlow m-248  · T-9 · result from @w]\nAlso done.";
        assert_eq!(
            given_in(glued, |id| (id == 246).then_some("Done.")),
            [246, 248]
        );
    }

    #[test]
    fn a_body_that_quotes_a_header_excuses_it_there_and_not_a_delivery_glued_after_it() {
        // m-246 quotes m-248's header in its words; m-248 itself is glued after them.
        let quoting = "As asked: [ConsensFlow m-248 · T-9 · result from @w] is what I read.";
        let prompt = format!(
            "[ConsensFlow m-246 · T-9 · result from @w]\n{quoting}[ConsensFlow m-248 · T-9 · result from @w]\nAlso done."
        );
        assert_eq!(
            given_in(&prompt, |id| (id == 246).then_some(quoting)),
            [246, 248]
        );
        // Its words with their white space changed by the paste are its words.
        let pasted = prompt.replace("As asked: ", "As  asked:\n");
        assert_eq!(
            given_in(&pasted, |id| (id == 246).then_some(quoting)),
            [246, 248]
        );
        // The quote alone is no delivery.
        let alone = format!("[ConsensFlow m-246 · T-9 · result from @w]\n{quoting}");
        assert_eq!(given_in(&alone, |id| (id == 246).then_some(quoting)), [246]);
    }

    #[test]
    fn how_far_a_body_stands_in_a_text_is_read_over_its_white_space() {
        assert_eq!(said_until("\nDone.[x", "Done."), 6);
        assert_eq!(said_until("\nTwo  words, more", "Two words"), 11);
        assert_eq!(said_until("\nOther", "Done."), 1);
        assert_eq!(said_until("", "Done."), 0);
    }
}

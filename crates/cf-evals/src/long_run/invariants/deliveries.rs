//! A message delivered twice, or joined to another (7, B5): a result read twice,
//! the second copy glued in front of the next result (m-246, m-248, a Devin chief on
//! Windows). Every delivery opens with its header, `[ConsensFlow m-12 · T-3 · result
//! from @x]`, which is also how the daemon knows it arrived; so a conversation's copy
//! that holds a message's header in two prompts was given it twice, and a prompt
//! that holds the headers of two messages was given them as one. Each header of a user's
//! item is judged where it stands ([`read_prompt`]): one that begins a line is a delivery,
//! whether it opens the item (on the line after the tag Claude Code wraps a long paste in)
//! or stands behind other words (Devin pastes behind what its input box holds); one inside
//! what a harness writes of the conversation itself is none, each block bounded to its own
//! end and taken only from the harness that writes it (Claude Code's summary of a
//! compaction, which quoted m-13 on the Mac's 2-hour run of 2026-10-19, and its
//! notification of a job's end; the history Codex gives its approval reviewer); one after
//! such a block whose end the copy does not show, in a quotation (a fenced block, a quoted
//! line), or after other words on its line may have gone in or not: unresolved, never
//! dropped (GPT-6.1-Sol's and GPT-6-Astra's reviews of rounds b1 and b2). (A message that
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

/// The words of the message `id`, as the ledger holds them.
fn body_in(look: &Look, id: i64) -> Option<&str> {
    look.ledger
        .messages
        .iter()
        .find(|message| message.id == id)
        .map(|message| message.body.as_str())
}

/// What Claude Code's summary of a compaction opens with: a record of its own, one item of
/// the copy, quoting the prompts before it whole (m-13 on the Mac's 2-hour run of
/// 2026-10-19).
const SUMMARY: &str = "This session is being continued from a previous conversation";

/// Claude Code's notification of a job's end, the job's output in it: from its opening tag
/// to its closing one.
const NOTIFICATION: [&str; 2] = ["<task-notification>", "</task-notification>"];

/// What the prompt Codex gives its approval reviewer opens with, the marker of the history
/// in it and of the request (each ending at its `END` line), as Codex wrote them on the
/// owner's machine (October 2026).
const REVIEWER: &str =
    "The following is the Codex agent history whose request action you are assessing";
const HISTORY: [&str; 2] = [">>> TRANSCRIPT START", ">>> TRANSCRIPT END"];
const REQUEST: [&str; 2] = [">>> APPROVAL REQUEST START", ">>> APPROVAL REQUEST END"];

/// What a harness writes of the conversation itself into a user's item, quoting deliveries
/// it was never given, each where its harness writes it alone and to its own end: Claude
/// Code's summary of a compaction (the item whole), its notifications of a job's end, and
/// the prompt Codex gives its approval reviewer (to the end of its request, or of its
/// history where no request follows). The parts of the text they cover; and where one
/// begins whose end the copy does not show, with what it is.
#[derive(Debug, Default, PartialEq, Eq)]
struct Synthetic {
    blocks: Vec<(usize, usize)>,
    open: Option<(usize, &'static str)>,
}

/// The [`Synthetic`] parts of `text`, an item of a conversation on `harness`.
fn synthetic(text: &str, harness: &str) -> Synthetic {
    let start = text.len() - text.trim_start().len();
    let mut found = Synthetic::default();
    match harness {
        "claude-code" if text[start..].starts_with(SUMMARY) => found.blocks.push((0, text.len())),
        "claude-code" => {
            let mut from = 0;
            while let Some(open) = text[from..].find(NOTIFICATION[0]).map(|at| from + at) {
                let Some(close) = text[open..].find(NOTIFICATION[1]) else {
                    found.open = Some((
                        open,
                        "Claude Code's notification of a job's end, which does not close",
                    ));
                    break;
                };
                from = open + close + NOTIFICATION[1].len();
                found.blocks.push((open, from));
            }
        }
        "codex" if text[start..].starts_with(REVIEWER) => {
            let end_of = |marker: &str| text.rfind(marker).map(|at| at + marker.len());
            match (
                end_of(HISTORY[1]),
                text.find(REQUEST[0]),
                end_of(REQUEST[1]),
            ) {
                (Some(_), Some(_), Some(end)) | (Some(end), None, _) => found.blocks.push((0, end)),
                (Some(end), Some(request), None) => {
                    found.blocks.push((0, end));
                    found.open = Some((
                        request,
                        "the request Codex gives its approval reviewer, which does not end",
                    ));
                }
                (None, ..) => {
                    found.open = Some((
                        start,
                        "the history Codex gives its approval reviewer, which does not end",
                    ));
                }
            }
        }
        _ => {}
    }
    found
}

/// Whether the header at `at` of `text` stands in a quotation: on a quoted line (one that
/// opens with `>`), or inside a fenced block (a line opening with three backticks or tildes
/// opens one, the next such line of the same kind closes it).
fn quoted(text: &str, at: usize) -> bool {
    let line = text[..at].rfind('\n').map_or(0, |end| end + 1);
    if text[line..at].trim_start().starts_with('>') {
        return true;
    }
    let mut fence: Option<&str> = None;
    for one in text[..line].lines().map(str::trim_start) {
        match fence {
            Some(kind) if one.starts_with(kind) => fence = None,
            None => {
                fence = ["```", "~~~"]
                    .into_iter()
                    .find(|kind| one.starts_with(kind))
            }
            Some(_) => {}
        }
    }
    fence.is_some()
}

/// Why a header of a user's item is taken neither for a delivery nor for none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Undecided {
    /// It comes after other words on its line, and no delivery before it in the item: a
    /// paste glued behind them, or words that quote it.
    Glued,
    /// It stands in a quotation ([`quoted`]).
    Quoted,
    /// It comes after a block a harness writes whose end the copy does not show.
    Unbounded(&'static str),
}

impl Undecided {
    /// Where the header of `id` stands, said: what an item holding it could not be judged by.
    fn said(self, id: i64) -> String {
        match self {
            Self::Glued => format!("m-{id}'s header after other words on its line: whether it went in behind them or they quote it, the copy does not say"),
            Self::Quoted => format!("m-{id}'s header in a quotation (a fenced block or a quoted line): whether it went in, the copy does not say"),
            Self::Unbounded(what) => format!("m-{id}'s header after {what}: whether it went in, the copy does not say"),
        }
    }
}

/// What a user's item says of deliveries.
#[derive(Debug, Default, PartialEq, Eq)]
struct Reading {
    /// The messages it was given, in order.
    given: Vec<i64>,
    /// The first header of it taken neither for a delivery nor for none, and why.
    undecided: Option<(i64, Undecided)>,
}

/// What `text`, a user's item of a conversation on `harness`, was given, each header of it
/// judged where it stands: none inside what its harness writes of the conversation itself
/// ([`synthetic`]), nor inside the words of a message delivered before it in the item (`body`
/// of a message, by its id), where they stand after that message's header; undecided after
/// a synthetic block whose end the copy does not show, in a quotation, or after other words
/// on its line with no delivery before it in the item; else given: a header that begins a
/// line (at the start of the item, on the line after the tag Claude Code wraps a long paste
/// in, or behind what the input box held: Devin pastes there), or one glued after a delivery
/// (B5's shape).
fn read_prompt<'b>(text: &str, harness: &str, body: impl Fn(i64) -> Option<&'b str>) -> Reading {
    let synthetic = synthetic(text, harness);
    let mut reading = Reading::default();
    let mut quoting_until = 0;
    for (id, start, end) in headers(text) {
        if start < quoting_until
            || synthetic
                .blocks
                .iter()
                .any(|(from, to)| (*from..*to).contains(&start))
        {
            continue;
        }
        let line = text[..start].rfind('\n').map_or(0, |end| end + 1);
        let why = match synthetic.open {
            Some((from, what)) if start >= from => Some(Undecided::Unbounded(what)),
            _ if quoted(text, start) => Some(Undecided::Quoted),
            _ if reading.given.is_empty() && !text[line..start].trim().is_empty() => {
                Some(Undecided::Glued)
            }
            _ => None,
        };
        if let Some(why) = why {
            reading.undecided.get_or_insert((id, why));
            continue;
        }
        reading.given.push(id);
        quoting_until = body(id).map_or(end, |said| end + said_until(&text[end..], said));
    }
    reading
}

/// The harness of the conversation of `item`, as the ledger names it.
fn harness_of<'l>(look: &'l Look, item: &Item) -> &'l str {
    look.ledger
        .conversations
        .iter()
        .find(|conversation| conversation.id == item.conversation)
        .map_or("", |conversation| conversation.harness.as_str())
}

/// What `item` says of deliveries ([`read_prompt`]): nothing where it is no user's.
fn reading_of(look: &Look, item: &Item) -> Reading {
    if item.role != "user" {
        return Reading::default();
    }
    read_prompt(&item.text, harness_of(look, item), |id| body_in(look, id))
}

/// 7, unresolved: each user's item that holds a header taken neither for a delivery nor for
/// none ([`Undecided`]), said by the first such header.
pub fn undecided(look: &Look) -> Vec<Unjudged> {
    look.ledger
        .items
        .iter()
        .filter_map(|item| {
            let (id, why) = reading_of(look, item).undecided?;
            let handle = look
                .ledger
                .conversations
                .iter()
                .find(|conversation| conversation.id == item.conversation)
                .map_or("?", |conversation| {
                    look.ledger.handle(conversation.participant)
                });
            Some(Unjudged::new(
                7,
                format!("7:undecided:{}:{}", item.conversation, item.id),
                format!("@{handle}'s prompt {} holds {}", item.id, why.said(id)),
            ))
        })
        .collect()
}

/// The messages `item` delivered to `worker`, in order: each it was given ([`read_prompt`])
/// that the ledger holds as one to `worker`.
pub(super) fn delivered_to<'l>(look: &'l Look, item: &Item, worker: i64) -> Vec<&'l Message> {
    reading_of(look, item)
        .given
        .into_iter()
        .filter_map(|id| look.ledger.messages.iter().find(|message| message.id == id))
        .filter(|message| message.recipient == worker)
        .collect()
}

/// 7: per conversation, a message given in two of its prompts, and a prompt that was given
/// two messages ([`read_prompt`]).
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
            .filter(|item| item.conversation == conversation.id && item.role == "user");
        let mut given: BTreeMap<i64, Vec<String>> = BTreeMap::new();
        for item in prompts {
            let named = read_prompt(&item.text, &conversation.harness, body).given;
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

    /// The messages a prompt of a harness that writes nothing of its own was given.
    fn given_in<'b>(text: &str, body: impl Fn(i64) -> Option<&'b str>) -> Vec<i64> {
        read_prompt(text, "", body).given
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

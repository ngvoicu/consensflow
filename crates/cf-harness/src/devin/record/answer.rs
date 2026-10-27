//! What Devin's chain says.
//!
//! A reply is complete only when it is its request's last, and either Devin
//! stored it as the turn's end (`finish_reason` stop) or the wire saw that
//! request complete with the stored text streamed. Devin 3000.11 stores no
//! `finish_reason` at all (none of the 574 replies of T-652's store), so the
//! wire is what proves a reply whole. Where Devin's async compactor left no
//! user's message on the chain, a reply answers no request the chain names:
//! it is then its model request's (`metadata.request_id`), which the wire
//! names as the turn ends (T-652: the chief's answer to a waiting task stayed
//! queued for good). A window reopened on the
//! conversation replays its history with no turn end, so only the store
//! tells that turn ended. Whether Devin is still on a turn is judged by the
//! launch whose wire was written last (a resume opens a new one). A user's
//! message that is what ConsensFlow gave a launch behind `/bypass` is read as
//! the prompt it ran, however Devin stores the command.

use std::collections::HashMap;
use std::sync::Arc;

use super::chain::{Chain, Entry};
use super::comparable::comparable;
use super::wires::{Cause, Outcome, Wires};
use crate::devin::adapter::BYPASS;
use crate::shared::record::key::Key;
use crate::shared::record::reading::{Record, Role, Settlement};

/// The reading of `chain`, with what the wire logs say of its requests.
pub(super) fn answer(chain: &Chain, wires: &Wires) -> Record {
    let working = wires.working();
    let mut finals: HashMap<Request<'_>, &str> = HashMap::new();
    for entry in &chain.entries {
        if entry.item.role == Role::Assistant {
            finals.insert(Request::of(entry), &entry.item.id);
        }
    }
    let mut record = Record::new();
    record.asking = chain.asking;
    record.lines.lost = wires.lost();
    record.items = chain
        .entries
        .iter()
        .map(|entry| {
            let mut item = entry.item.clone();
            if item.role == Role::User {
                if let Some(prompt) = behind_bypass(&item.text, wires).map(Arc::from) {
                    item.text = prompt;
                }
            }
            if item.role == Role::Assistant {
                let outcome = Request::of(entry).outcome(wires);
                // The same text needs no comparing, which every look would do again.
                item.complete = finals.get(&Request::of(entry)) == Some(&&*item.id)
                    && (entry.stopped
                        || outcome.is_some_and(|outcome| {
                            outcome.cause == Cause::Complete
                                && (outcome.text == *item.text
                                    || comparable(&outcome.text) == comparable(&item.text))
                        }));
            }
            item
        })
        .collect();
    let last = chain
        .entries
        .iter()
        .rposition(|entry| entry.item.role != Role::Custom);
    let outcome = last.and_then(|at| Request::of(&chain.entries[at]).outcome(wires));
    let cancelled = outcome
        .is_some_and(|outcome| matches!(outcome.cause, Cause::Cancelled | Cause::QuotaExhausted));
    let failed = outcome.is_some_and(|outcome| outcome.cause == Cause::Error);
    let (last_complete, last_open) = last.map_or((false, false), |at| {
        let item = &record.items[at];
        (
            item.complete,
            item.role == Role::Assistant && !item.complete,
        )
    });
    record.failed = failed;
    record.failure = outcome
        .filter(|_| failed)
        .and_then(|outcome| outcome.error.clone());
    record.in_flight = working || (last_open && !cancelled && !failed);
    record.settlement = if working {
        Settlement::InFlight
    } else if last_complete || cancelled || failed {
        Settlement::Settled
    } else {
        Settlement::Unknown
    };
    record
}

/// What tells which turn a message of the chain is of: the request it answers, or,
/// where the chain names none (a compaction left no user's message on it), the
/// model's request it is of.
#[derive(PartialEq, Eq, Hash)]
enum Request<'a> {
    Answered(&'a Key),
    Native(&'a Key),
}

impl<'a> Request<'a> {
    fn of(entry: &'a Entry) -> Self {
        if entry.request == Key::Null {
            Request::Native(&entry.native)
        } else {
            Request::Answered(&entry.request)
        }
    }

    /// How its turn ended, if a log said so.
    fn outcome(self, wires: &Wires) -> Option<&Outcome> {
        match self {
            Request::Answered(request) => wires.outcome(request),
            Request::Native(native) => wires.native_outcome(native),
        }
    }
}

/// The prompt a user's message ran under `/bypass`, where it is, whole, what
/// ConsensFlow gave one of the session's launches as its first message behind the
/// command ([`Wires::bypassed`]): read as that prompt, the paste it is, however Devin
/// stores the command. None for any other, a human's command before a header
/// included (Astra's review of 2026-10-18: the human's `/bypass` before an answer's
/// header, and a question of their own after it, was taken for the answer).
///
/// Limit: a start sweeps the launch folders before it settles what was on its way,
/// so a first message Devin stored with the command kept, whose launch a start swept,
/// is read as stored: no paste, it goes back to its queue and is given again. Devin
/// 3000.11.92 keeps no `/bypass` in what the product gives it; were one to, the live
/// check of a resumed Devin window fails, naming this limit: its `Opening::Note` cases
/// read the note as Devin's store has it, which must be with its header first
/// (`live_devin_resume`'s `turn.rs`, `stored_fault`).
fn behind_bypass<'a>(text: &'a str, wires: &Wires) -> Option<&'a str> {
    let rest = text.trim_start().strip_prefix(BYPASS)?;
    let prompt = rest.trim_start();
    let whole = |sent: &str| sent.trim_end() == prompt.trim_end();
    (prompt.len() < rest.len() && wires.bypassed().any(whole)).then_some(prompt)
}

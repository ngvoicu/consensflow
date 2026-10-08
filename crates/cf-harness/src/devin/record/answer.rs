//! What Devin's chain says.
//!
//! A reply is complete only when it is its request's last, and either Devin
//! stored it as the turn's end (`finish_reason` stop) or the wire saw that
//! request complete with the stored text streamed. A window reopened on the
//! conversation replays its history with no turn end, so only the store
//! tells that turn ended. Whether Devin is still on a turn is judged by the
//! launch whose wire was written last (a resume opens a new one).

use std::collections::HashMap;

use super::chain::Chain;
use super::comparable::comparable;
use super::wires::{Cause, Wires};
use crate::shared::record::key::Key;
use crate::shared::record::reading::{Record, Role, Settlement};

/// The reading of `chain`, with what the wire logs say of its requests.
pub(super) fn answer(chain: &Chain, wires: &Wires) -> Record {
    let working = wires.working();
    let mut finals: HashMap<&Key, &str> = HashMap::new();
    for entry in &chain.entries {
        if entry.item.role == Role::Assistant {
            finals.insert(&entry.request, &entry.item.id);
        }
    }
    let mut record = Record::new();
    record.asking = chain.asking;
    record.items = chain
        .entries
        .iter()
        .map(|entry| {
            let mut item = entry.item.clone();
            if item.role == Role::Assistant {
                let outcome = wires.outcome(&entry.request);
                // The same text needs no comparing, which every look would do again.
                item.complete = finals.get(&entry.request) == Some(&&*item.id)
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
    let outcome = last.and_then(|at| wires.outcome(&chain.entries[at].request));
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

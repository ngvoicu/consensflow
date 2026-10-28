//! What a harness's record says, in the dispatcher's terms; whether its window
//! waits on its own question dialog (`dialogWaiting`), and the two looks a
//! window's own word makes of it: one that shows another conversation
//! (`switchedTo`), and one that has not said which it shows (`unnamed`). A
//! conversation with no messages and nothing in flight is idle: a window opened
//! without a task has nothing to finish, and must still be able to receive.

use std::sync::Arc;

use crate::contract::{Observed, Waiting};
use crate::records::{Quota, Reading, Record, Settlement};

/// What a window says of its quota, as the engine is told it, from three sources in one
/// precedence: a move of its model, then what the harness says of the window now (`live`: a
/// retry status, a wire log), then what its record kept.
///
/// A window whose harness moved its model on its own is not used on it unasked, so the move
/// reads as the hold only the human can end (MUS-24). It comes before a refusal the same record
/// shows (Muse moved the model after the 402 of T-705's recordings): the human who fixes the
/// billing and tries again is told besides that the window runs on a model nobody chose, or that
/// refusal's handled mark would let the engine paste into it. What the window says now stands
/// over its record's refusal, which it may have left behind, and a window that says nothing
/// keeps the record's own: Claude's adapter lost the move, Devin's replaced the record's quota
/// with the wire's even where that was none, and OpenCode's let a retry outrank a move (T-841,
/// T-853).
fn quota_of(record: &Record, live: Option<Arc<Quota>>) -> Option<Arc<Quota>> {
    record
        .model_moved
        .as_ref()
        .map(|moved| Arc::new(moved.as_quota()))
        .or(live)
        .or_else(|| record.quota.clone())
}

/// The look a record says of its window: settled when its turn is, or when
/// it is empty and nothing is in flight, and not waiting, not switched and not
/// unnamed, which each harness says of its own window. A record that could
/// not be read says nothing: settled, and empty.
pub(crate) fn record_state(reading: Arc<Reading>) -> Observed {
    observed_from(reading, None)
}

/// The one place a look is built from a record and the live word beside it: every adapter takes
/// it for what the record says (`items`, `failed`, `settled`, `quota`), and sets what only its
/// own window knows (`waiting`, `over`, `took_back`, `at_prompt`, a switch) on it. A record that
/// could not be read says nothing of its own, and the live quota stands.
pub(crate) fn observed_from(reading: Arc<Reading>, live: Option<Arc<Quota>>) -> Observed {
    let (settled, failed, quota) = match &*reading {
        Reading::Known(record) => {
            let empty = record.items.is_empty() && !record.in_flight;
            (
                record.settlement == Settlement::Settled
                    || (record.settlement != Settlement::InFlight && empty),
                record.failed,
                quota_of(record, live),
            )
        }
        Reading::Unknown(_) | Reading::Missing(_) => (true, false, live),
    };
    Observed {
        reading: Some(reading),
        settled,
        over: false,
        waiting: None,
        failed,
        quota,
        switched: None,
        unnamed: false,
        took_back: false,
        at_prompt: false,
    }
}

/// A window whose own question dialog is open waits for an answer there (the
/// chief's, from the human; a member's, from the board through its door):
/// nothing is pasted into it, and the board says it waits.
pub(crate) fn dialog_waiting(reading: Option<&Reading>) -> Option<Waiting> {
    match reading {
        Some(Reading::Known(record)) if record.asking => Some(Waiting {
            reason: Some("its own question dialog is open".to_owned()),
        }),
        _ => None,
    }
}

/// The look of a window that shows another conversation than its own
/// (`switchedTo`): the record read is the old one's last, so not settled and
/// waiting on nothing, and the engine follows the window to `session`.
pub(crate) fn switched_to(observed: Observed, session: String) -> Observed {
    Observed {
        settled: false,
        over: false,
        waiting: None,
        switched: Some(session),
        at_prompt: false,
        ..observed
    }
}

/// The look of a window that has not said yet which conversation it shows
/// (`unnamed`): it waits, for `reason`.
pub(crate) fn unnamed(observed: Observed, reason: &str) -> Observed {
    Observed {
        waiting: Some(Waiting {
            reason: Some(reason.to_owned()),
        }),
        unnamed: true,
        ..observed
    }
}

#[cfg(test)]
mod tests;

//! What a harness's record says, in the dispatcher's terms (`recordState`,
//! `src/adapters/shared.js`), and the two looks a window's own word makes of
//! it: one that shows another conversation (`switchedTo`), and one that has
//! not said which it shows (`unnamed`). A conversation with no messages and
//! nothing in flight is idle: a window opened without a task has nothing to
//! finish, and must still be able to receive.

use std::sync::Arc;

use crate::contract::{Observed, Waiting};
use crate::records::{Reading, Settlement};

/// The look a record says of its window: settled when its turn is, or when
/// it is empty and nothing is in flight, and not waiting, not switched and not
/// unnamed, which each harness says of its own window. A record that could
/// not be read says nothing: settled, and empty.
pub(crate) fn record_state(reading: Arc<Reading>) -> Observed {
    let (settled, failed, quota) = match &*reading {
        Reading::Known(record) => {
            let empty = record.items.is_empty() && !record.in_flight;
            (
                record.settlement == Settlement::Settled
                    || (record.settlement != Settlement::InFlight && empty),
                record.failed,
                record.quota.clone(),
            )
        }
        Reading::Unknown(_) => (true, false, None),
    };
    Observed {
        reading: Some(reading),
        settled,
        waiting: None,
        failed,
        quota,
        switched: None,
        unnamed: false,
    }
}

/// The look of a window that shows another conversation than its own
/// (`switchedTo`): the record read is the old one's last, so not settled and
/// waiting on nothing, and the engine follows the window to `session`.
pub(crate) fn switched_to(observed: Observed, session: String) -> Observed {
    Observed {
        settled: false,
        waiting: None,
        switched: Some(session),
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

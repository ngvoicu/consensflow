//! What the engine asks of a harness's record with no window open.

use std::sync::Arc;

use super::*;
use crate::records::{Item, Record, Role};

/// A record that says what a test says, whatever conversation it is asked of.
struct Says(Reading);

impl Records for Says {
    fn look<'a>(
        &'a self,
        _harness: Harness,
        _session: &'a str,
        _options: &'a Options,
    ) -> Work<'a, Arc<Reading>> {
        let reading = Arc::new(self.0.clone());
        Box::pin(async move { reading })
    }

    fn has_transcript<'a>(
        &'a self,
        _harness: Harness,
        _session: &'a str,
    ) -> Work<'a, Result<bool, String>> {
        Box::pin(async { Ok(true) })
    }
}

fn settling(settlement: Settlement) -> Says {
    ending(settlement, &[Role::User, Role::Assistant])
}

/// A record of `settlement` whose items are of `roles`, in order.
fn ending(settlement: Settlement, roles: &[Role]) -> Says {
    let mut record = Record::new();
    record.settlement = settlement;
    record.items = roles
        .iter()
        .enumerate()
        .map(|(at, role)| Item {
            id: Arc::from(at.to_string()),
            role: *role,
            text: Arc::from("text"),
            complete: true,
            at: None,
            commentary: false,
            compaction: false,
        })
        .collect();
    Says(Reading::Known(record))
}

#[tokio::test]
async fn a_turn_was_cut_when_the_record_ends_in_one_with_no_end_not_when_at_rest_and_unknown_when_it_cannot_tell(
) {
    for (said, cut) in [
        (settling(Settlement::InFlight), Some(true)),
        (settling(Settlement::Settled), Some(false)),
        (settling(Settlement::Unknown), None),
        // Settled on the message it was given, as Devin reads one never answered:
        // with no window open, that is no turn at rest.
        (ending(Settlement::Settled, &[Role::User]), None),
        (
            ending(Settlement::Settled, &[Role::User, Role::Custom]),
            None,
        ),
        (
            ending(Settlement::Settled, &[Role::Assistant, Role::Custom]),
            Some(false),
        ),
        (Says(Reading::Unknown("unreadable".to_owned())), None),
    ] {
        assert_eq!(
            said.turn_cut(Harness::Claude, "a-session").await,
            cut,
            "{:?}",
            said.0
        );
    }
}

//! What the engine asks of a harness's record with no window open.

use std::sync::Arc;

use super::*;
use crate::records::Record;

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
    let mut record = Record::new();
    record.settlement = settlement;
    Says(Reading::Known(record))
}

#[tokio::test]
async fn a_turn_is_left_open_when_the_record_ends_in_one_with_no_end_and_in_no_other_case() {
    for (said, open) in [
        (settling(Settlement::InFlight), true),
        (settling(Settlement::Settled), false),
        (settling(Settlement::Unknown), false),
        (Says(Reading::Unknown("unreadable".to_owned())), false),
    ] {
        assert_eq!(
            said.turn_left_open(Harness::Claude, "a-session").await,
            open,
            "{:?}",
            said.0
        );
    }
}

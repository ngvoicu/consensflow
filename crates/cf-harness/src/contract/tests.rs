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
            synthetic: false,
            compaction: false,
        })
        .collect();
    Says(Reading::Known(record))
}

/// The record `says` holds, changed by `change`.
fn changed(says: Says, change: impl FnOnce(&mut Record)) -> Says {
    let Says(Reading::Known(mut record)) = says else {
        panic!("a record");
    };
    change(&mut record);
    Says(Reading::Known(record))
}

const HARNESSES: [Harness; 5] = [
    Harness::Claude,
    Harness::Codex,
    Harness::Pi,
    Harness::Opencode,
    Harness::Devin,
];

#[tokio::test]
async fn a_turn_was_cut_when_the_record_ends_in_one_with_no_end_not_when_at_rest_and_unknown_when_it_cannot_tell(
) {
    let cut = Some(TurnLeft::Cut(None));
    let rest = Some(TurnLeft::AtRest);
    for (said, left) in [
        (settling(Settlement::InFlight), cut.clone()),
        (settling(Settlement::Settled), rest.clone()),
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
            rest.clone(),
        ),
        (Says(Reading::Unknown("unreadable".to_owned())), None),
    ] {
        assert_eq!(
            said.turn_cut(Harness::Claude, "a-session").await,
            left,
            "{:?}",
            said.0
        );
    }
}

#[tokio::test]
async fn a_record_held_open_only_by_its_harness_s_notices_of_background_work_was_at_rest_on_every_harness(
) {
    // Background work is rest (T-814): what its harness queued of it began no turn.
    for harness in HARNESSES {
        let said = changed(settling(Settlement::InFlight), |record| {
            record.lines.held_by_notices = true;
        });
        assert_eq!(
            said.turn_cut(harness, "a-session").await,
            Some(TurnLeft::AtRest),
            "{harness:?}"
        );
    }
}

#[tokio::test]
async fn a_reply_and_then_an_idle_compaction_s_summary_ends_at_rest_on_every_harness() {
    // The sites chief's record of 2026-10-23 (T-814): undecided before, which a member
    // was told was a cut turn.
    for harness in HARNESSES {
        let said = changed(
            ending(
                Settlement::Settled,
                &[Role::User, Role::Assistant, Role::User],
            ),
            |record| record.items[2].compaction = true,
        );
        assert_eq!(
            said.turn_cut(harness, "a-session").await,
            Some(TurnLeft::AtRest),
            "{harness:?}"
        );
    }
}

#[tokio::test]
async fn a_cut_turn_is_told_by_the_time_and_first_words_of_the_message_that_began_it() {
    let long = format!(
        "[ConsensFlow m-12 · T-3 · task from @chief]\nFix   the\tparser {}",
        "x".repeat(200)
    );
    for harness in HARNESSES {
        let said = changed(
            ending(
                Settlement::InFlight,
                &[Role::User, Role::Assistant, Role::User, Role::Assistant],
            ),
            |record| {
                record.items[2].text = Arc::from(long.as_str());
                record.items[2].at = Some(Value::from("2026-10-15T20:41:24.089Z"));
                record.items[3].complete = false;
            },
        );
        let Some(TurnLeft::Cut(Some(opening))) = said.turn_cut(harness, "a-session").await else {
            panic!("{harness:?}: cut, with its message");
        };
        assert_eq!(opening.at.as_deref(), Some("2026-10-15 20:41 UTC"));
        assert!(
            opening
                .words
                .starts_with("[ConsensFlow m-12 · T-3 · task from @chief] Fix the parser xxx"),
            "{}",
            opening.words
        );
        assert_eq!(
            opening.words.chars().count(),
            OPENING_WORDS + 1,
            "cut short, and marked so"
        );
        assert!(opening.words.ends_with('…'));
    }
}

#[tokio::test]
async fn a_turn_compacted_in_its_middle_is_named_by_the_message_that_began_it_not_the_summary() {
    let said = changed(
        ending(
            Settlement::InFlight,
            &[Role::User, Role::Assistant, Role::User, Role::Assistant],
        ),
        |record| {
            record.items[0].text = Arc::from("Run the tests");
            record.items[2].text = Arc::from("This session is being continued");
            record.items[2].compaction = true;
            record.items[3].complete = false;
        },
    );
    let Some(TurnLeft::Cut(Some(opening))) = said.turn_cut(Harness::Claude, "a-session").await
    else {
        panic!("cut, with its message");
    };
    assert_eq!(opening.words, "Run the tests");
}

#[tokio::test]
async fn a_record_whose_last_turn_was_answered_whole_names_no_turn_for_a_message_it_never_began() {
    // In flight for a message taken and not begun, which no item shows: the reply before
    // it ended its own turn, and the message that began that one is not this.
    let said = ending(Settlement::InFlight, &[Role::User, Role::Assistant]);
    assert_eq!(
        said.turn_cut(Harness::Claude, "a-session").await,
        Some(TurnLeft::Cut(None))
    );
}

#[test]
fn a_look_at_a_record_that_is_not_there_reads_no_items_nothing_left_running_and_no_monitor() {
    // The record was read and holds nothing: what turn-end asks of it is that of an empty
    // record, though it was not read known.
    let observed =
        crate::shared::record_state::record_state(Arc::new(Reading::missing("session yet")));
    let reading = observed.reading.as_deref().expect("a reading");
    assert!(!reading.read_failed(), "{reading:?}");
    assert!(observed.items().is_empty());
    assert_eq!(observed.pending(), Pending::default());
    assert_eq!(observed.monitors(), Vec::new());
}

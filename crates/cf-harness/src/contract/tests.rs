//! What the engine asks of a harness's record with no window open.

use std::sync::Arc;

use super::*;
use crate::records::{Item, LastCall, Record, Role};

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
            detail: None,
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

const HARNESSES: [Harness; 4] = [Harness::Claude, Harness::Codex, Harness::Pi, Harness::Devin];

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
async fn a_cut_turn_is_told_by_the_time_and_the_delivery_that_began_it_and_no_word_of_the_message()
{
    let long = format!(
        "[ConsensFlow m-12 · T-3 · task from @chief]\nFix   the\tparser, key: abc {}",
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
        assert_eq!(
            opening,
            Opening {
                at: Some("2026-10-15 20:41 UTC".to_owned()),
                delivery: Some(12)
            },
            "{harness:?}"
        );
    }
}

#[test]
fn only_a_header_that_begins_the_text_names_a_delivery() {
    let cases: [(&str, Option<i64>); 22] = [
        ("[ConsensFlow m-12 · T-3 · task from @chief]\nGo", Some(12)),
        ("  \n[ConsensFlow m-7 · note from ConsensFlow] Go", Some(7)),
        // Claude Code's wrapper of a paste, and the placeholders of the images it attached.
        (
            "<pasted_content id=\"b118\">\n[ConsensFlow m-5 · T-1 · result from @a]\nGo",
            Some(5),
        ),
        (
            "[Image #4][ConsensFlow m-9 · T-1 · result from @a]",
            Some(9),
        ),
        (
            "[Image #4] [ConsensFlow m-9 · T-1 · result from @a]",
            Some(9),
        ),
        (
            "[Image #4]\n\n<pasted_content id=\"b1\">\n[ConsensFlow m-9 · T-1]",
            Some(9),
        ),
        ("[Image #4] [Image #5]\n[ConsensFlow m-10 · x]", Some(10)),
        // A header quoted after words, or in other words, names none.
        (
            "Why is \"[ConsensFlow m-42 · T-1 · task from @chief]\" queued?",
            None,
        ),
        ("Investigate this:\n\n[ConsensFlow m-42 · T-1]", None),
        ("key: abc [ConsensFlow m-42 · T-1]", None),
        ("<pasted_content id=\"b1\"> [ConsensFlow m-42 · T-1]", None),
        (
            "<pasted_content id=\"b1\" extra>\nwords [ConsensFlow m-42 ·",
            None,
        ),
        ("<b>\n[ConsensFlow m-42 · T-1]", None),
        ("<pasted_contentx>\n[ConsensFlow m-42 · T-1]", None),
        (
            "<pasted_content id=\"a>b\">\n[ConsensFlow m-42 · T-1]",
            None,
        ),
        // Not a header of ConsensFlow's id.
        ("[ConsensFlow m-12x · T-1]", None),
        ("[ConsensFlow m-12]", None),
        ("[ConsensFlow m-٣ · T-1]", None),
        ("[ConsensFlow m-99999999999999999999 · T-1]", None),
        ("[ConsensFlow m- · T-1]", None),
        ("pass word hunter22", None),
        ("", None),
    ];
    let mut failed = Vec::new();
    for (text, want) in cases {
        let got = Opening::began_with(text, None).delivery;
        if got != want {
            failed.push(format!("{text:?}: {got:?}, wanted {want:?}"));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
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
    assert_eq!(opening.delivery, None);
    assert_eq!(opening.at, None);
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

#[tokio::test]
async fn the_last_call_of_a_record_is_the_one_it_shows_on_every_harness_and_none_where_it_cannot_be_read(
) {
    // T-823: what a window was doing when it closed under its task.
    for harness in HARNESSES {
        let said = changed(settling(Settlement::Settled), |record| {
            record.last_call = Some(LastCall::shell("Bash", "cargo test --workspace"));
        });
        assert_eq!(
            said.last_call(harness, "a-session").await,
            Some(LastCall::shell("Bash", "cargo test --workspace")),
            "{harness:?}"
        );
        assert_eq!(
            settling(Settlement::Settled)
                .last_call(harness, "a-session")
                .await,
            None
        );
        assert_eq!(
            Says(Reading::Unknown("unreadable".to_owned()))
                .last_call(harness, "a-session")
                .await,
            None
        );
    }
}

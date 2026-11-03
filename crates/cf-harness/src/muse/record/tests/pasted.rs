//! A long paste, as Muse Code 1.4.14 records it (T-1047; T-1000's live paste-screen,
//! `tests/fixtures/muse/live/pasted-brief.jsonl`, whose README says how it was made): the
//! box folds a brief of 41 lines into `[Pasted Content 2184 chars]`, and the log keeps
//! that in `refill_blocks`, where the words the model was given (`model_messages`, and
//! the run's `started` prompt) are the brief whole. The user's message is those words:
//! a reader that took the box's placeholder left the brief's header out of the record,
//! and the engine failed the window that answered it as one that never showed its first
//! message.

use super::*;

const LOG: &str = include_str!("../../../../tests/fixtures/muse/live/pasted-brief.jsonl");

/// What the brief begins with: the header of the task message the live kit gave.
const HEADER: &str =
    "[ConsensFlow m-1 \u{b7} T-1 \u{b7} task from @chief]\nPSL-01: line 1 of the brief";

#[test]
fn a_long_paste_muse_folds_in_its_box_is_the_users_whole_words_and_its_answer_follows() {
    let record = known(&fresh_reading(&session_of(LOG), LOG)).clone();
    let said = said(&record);
    let users: Vec<&str> = said
        .iter()
        .filter(|(role, _)| *role == Role::User)
        .map(|(_, text)| *text)
        .collect();
    assert_eq!(users.len(), 1, "{said:?}");
    assert!(
        users[0].starts_with(HEADER),
        "{:?}",
        &users[0][..60.min(users[0].len())]
    );
    assert!(users[0].contains("PSL-40:"), "the brief whole");
    assert!(
        !users[0].contains("[Pasted Content"),
        "never the box's placeholder"
    );
    let user = said
        .iter()
        .position(|(role, _)| *role == Role::User)
        .unwrap();
    assert!(
        said[user..].contains(&(Role::Assistant, "3470")),
        "the answer after it: {said:?}"
    );
    assert_eq!(record.settlement, Settlement::Settled);
    assert!(!record.failed);
}

#[test]
fn the_pasted_brief_reads_the_same_read_on_a_record_at_a_time_as_read_fresh() {
    let session = session_of(LOG);
    let stage = Stage::new(&session);
    let mut reading_on = stage.reader();
    let mut text = String::new();
    for (at, line) in LOG.lines().enumerate() {
        text.push_str(line);
        text.push('\n');
        stage.append(line);
        let on = look(&mut reading_on);
        assert_eq!(on, fresh_reading(&session, &text), "after line {at}");
    }
}

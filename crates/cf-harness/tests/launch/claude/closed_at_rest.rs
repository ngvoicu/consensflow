//! What the human's Resume reads in the record of a Claude window that was closed at
//! rest, with only background work running: no turn was cut (T-814). The two field
//! records, as fixtures with made-up words (`tests/fixtures/claude/README.md`):
//!
//! - `closed-at-rest-with-a-job.jsonl`, poker-lab's chief on 2026-10-15: its turn ended
//!   at 20:41:51, the background command it had started ran on, and Claude Code queued
//!   and took the notification of the job's end (`killed`) at 20:43:43 and began no
//!   turn; the project closed at 20:44:36, and the Resume told the chief its turn was cut.
//! - `closed-at-rest-after-a-compaction.jsonl`, the sites chief on 2026-10-23: its turn
//!   ended at 10:26:31, an idle compaction at 11:21:11, and the Close at 19:11:37 left the
//!   job's `killed` notification queued and taken.
//!
//! Each file goes on past the Close with what the resumed process wrote: its notice of
//! the job the previous process left (written to the transcript alone), and the
//! Resume's own note, taken from the queue (an `enqueue` with no content) and never
//! begun, as the project closed again.

use cf_harness::contract::{Records, TurnLeft};
use cf_proto::agents::Harness;

use super::*;

const SESSION: &str = "5c7f3b02-8d4e-4a6f-9b9c-2d3e4f5a6b7c";

/// The records of a fixture, for this test's session.
fn fixture(text: &str) -> Vec<String> {
    text.replace("$SESSION", SESSION)
        .lines()
        .map(|line| format!("{line}\n"))
        .collect()
}

fn with_a_job() -> Vec<String> {
    fixture(include_str!(
        "../../fixtures/claude/closed-at-rest-with-a-job.jsonl"
    ))
}

fn after_a_compaction() -> Vec<String> {
    fixture(include_str!(
        "../../fixtures/claude/closed-at-rest-after-a-compaction.jsonl"
    ))
}

/// The records up to the Close: up to the last that takes a message from the queue
/// before the resumed process's first, the one `first_after` finds.
fn up_to_the_close(records: &[String], first_after: &str) -> Vec<String> {
    let resumed = records
        .iter()
        .position(|line| line.contains(first_after))
        .expect("the resumed process's first record");
    records[..resumed].to_vec()
}

/// What the Resume reads of the record with no window open.
fn left(home: &Home) -> Option<TurnLeft> {
    let records = Fakes::new(&home.env()).records;
    finished(records.turn_cut(Harness::Claude, SESSION))
}

/// A home whose Claude statuses say no Claude shows the conversation: the realistic
/// Resume, after the Close ended the window's program.
fn shown_by_none(home: &Home) {
    home.status(
        "0f1e2d3c-4b5a-4968-8776-655443322110",
        &json!({ "status": "idle" }),
        me(),
    );
}

#[test]
fn a_chief_closed_at_rest_while_its_background_command_ran_was_cut_in_no_turn() {
    // Before T-814, read with no statuses to prove the program gone: cut.
    let home = Home::new();
    let closed = up_to_the_close(&with_a_job(), "2026-10-15T20:44:39");
    home.transcript(SESSION, &closed);
    assert_eq!(left(&home), Some(TurnLeft::AtRest), "no statuses");
    shown_by_none(&home);
    assert_eq!(left(&home), Some(TurnLeft::AtRest), "no Claude shows it");
}

#[test]
fn a_chief_closed_at_rest_after_an_idle_compaction_was_cut_in_no_turn() {
    // Before T-814: cut with no statuses, and undecided where no Claude shows it, which
    // tells a member its turn was cut.
    let home = Home::new();
    let closed = up_to_the_close(&after_a_compaction(), "2026-10-23T19:11:40");
    home.transcript(SESSION, &closed);
    assert_eq!(left(&home), Some(TurnLeft::AtRest), "no statuses");
    shown_by_none(&home);
    assert_eq!(left(&home), Some(TurnLeft::AtRest), "no Claude shows it");
}

#[test]
fn the_records_a_resumed_program_left_unbegun_are_no_turn_where_no_claude_shows_them() {
    // At the next Resume, the sites record goes on with the resumed process's notice of
    // the job and the note it took and never began: no Claude shows it, so nothing waits.
    for records in [with_a_job(), after_a_compaction()] {
        let home = Home::new();
        home.transcript(SESSION, &records);
        shown_by_none(&home);
        assert_eq!(left(&home), Some(TurnLeft::AtRest));
    }
}

#[test]
fn a_notification_of_any_end_taken_as_the_window_closed_is_background_work_not_a_turn() {
    // The same close, the job's end told `completed`: a turn was to begin for it, and
    // none had.
    let home = Home::new();
    let closed: Vec<String> = up_to_the_close(&with_a_job(), "2026-10-15T20:44:39")
        .into_iter()
        .map(|line| line.replace("<status>killed</status>", "<status>completed</status>"))
        .collect();
    assert!(closed
        .iter()
        .any(|line| line.contains("<status>completed</status>")));
    home.transcript(SESSION, &closed);
    assert_eq!(left(&home), Some(TurnLeft::AtRest));
}

#[test]
fn a_turn_cut_in_its_middle_is_told_with_the_time_and_the_words_of_the_message_that_began_it() {
    // The poker-lab record cut inside the turn of the note that came at 20:41:24.
    let records = with_a_job();
    let cut_at = records
        .iter()
        .position(|line| line.contains("2026-10-15T20:41:37.563Z"))
        .expect("a call of that turn");
    let home = Home::new();
    home.transcript(SESSION, &records[..=cut_at]);
    let Some(TurnLeft::Cut(Some(opening))) = left(&home) else {
        panic!("cut, with its message: {:?}", left(&home));
    };
    assert_eq!(opening.at.as_deref(), Some("2026-10-15 20:41 UTC"));
    assert!(
        opening
            .words
            .starts_with("[ConsensFlow m-12 · T-3 · note from ConsensFlow] Made-up prompt words"),
        "{}",
        opening.words
    );
}

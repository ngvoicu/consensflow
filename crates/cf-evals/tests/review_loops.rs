//! The two review scenarios (`review-loop`, `review-blockers`), played offline: a
//! scripted chief does what a chief does with the stand-in reviewer's reviews, on a
//! ledger written by hand, and the scenario judges what the measure reads of it. A
//! chief that behaves holds every expectation; each chief that loops, sends the work
//! back for notes, accepts with a blocker open or leaves the next review without the
//! earlier findings fails exactly the expectations that name what it did, and no
//! other. SCX-17, T-1010.
//!
//! The ledger is written without the ledger's code, as the rest of the evals' tests
//! write one (`tests/support`): a task, the messages of it, and the `task.state` events
//! of its being finished, sent back and accepted, with a clock that moves ten seconds a
//! step. What a scripted chief says to the owner in its terminal is in the ledger too, as
//! the daemon's copy of its conversation keeps it: a turn that ends with words, at a time
//! ([`Board::says`], [`Board::asks_the_owner`]); the scenarios hold the chief to a question
//! put between two moments of the board, and a case places it there.

#![allow(clippy::unwrap_used)]

use cf_evals::ledger_file::{add_project, Chief, Member, NewProject};
use cf_evals::measure::{measure, Metrics, Options};
use cf_evals::scenarios;
use rusqlite::Connection;
use serde_json::json;

mod support;

use support::Folder;

/// The page the worker makes, which every case here has changed.
const PAGE: &str = "site/en/assessment.html";

/// The participants: the human, the chief, a worker and a reviewer, in the ledger's order.
const CHIEF: i64 = 2;
const WORKER: i64 = 3;
const REVIEWER: i64 = 4;

/// A member of the staff, on Claude Code, of standard tier.
fn member<'a>(agent: &'a str, roles: &'a [&'a str]) -> Member<'a> {
    Member {
        agent,
        harness: "claude-code",
        roles,
        tier: "standard",
    }
}

/// A board a scripted chief works on.
struct Board {
    db: Connection,
    project: i64,
    clock: u32,
    tasks: i64,
    /// The turns of the chief that ended with words, and whether its conversation is begun.
    turns: i64,
    talking: bool,
    // Last: the folder goes after the ledger file open in it, which Windows will not remove from under it.
    folder: Folder,
}

impl Board {
    fn new() -> Self {
        let folder = Folder::new();
        let db = cf_evals::ledger_file::open(&folder.ledger(), false).unwrap();
        let project = add_project(
            &db,
            &NewProject {
                directory: "/work/site",
                name: "site",
                chief: Chief {
                    harness: "claude-code",
                    agent: None,
                },
                staff: &[
                    member("worker", &["worker"]),
                    member("reviewer", &["reviewer"]),
                ],
            },
        )
        .unwrap();
        Self {
            folder,
            db,
            project,
            clock: 0,
            tasks: 0,
            turns: 0,
            talking: false,
        }
    }

    /// The next moment, ten seconds after the last.
    fn tick(&mut self) -> String {
        self.clock += 10;
        format!(
            "2026-10-31T10:{:02}:{:02}.000Z",
            self.clock / 60,
            self.clock % 60
        )
    }

    /// The chief asks for a task of `pool` with `brief`: its number.
    fn asks(&mut self, pool: &str, brief: &str) -> i64 {
        self.tasks += 1;
        let number = self.tasks;
        let at = self.tick();
        let member = if pool == "reviewer" { REVIEWER } else { WORKER };
        self.db
            .execute(
                "INSERT INTO task (project_id, number, title, body, requester_id, assignee_id, state, pool, tier, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'working', ?7, 'standard', ?8, ?8)",
                rusqlite::params![
                    self.project,
                    number,
                    format!("T-{number}"),
                    brief,
                    CHIEF,
                    member,
                    pool,
                    at
                ],
            )
            .unwrap();
        self.message(number, (member, CHIEF), "task", brief, &at);
        number
    }

    /// A message of task `number`, from `sender` to `recipient`: its id.
    fn message(
        &mut self,
        number: i64,
        (recipient, sender): (i64, i64),
        kind: &str,
        body: &str,
        at: &str,
    ) -> i64 {
        self.db
            .execute(
                "INSERT INTO message (project_id, recipient_id, sender_id, kind, task_id, body, state, created_at, delivered_at)
                 VALUES (?1, ?2, ?3, ?4, (SELECT id FROM task WHERE project_id = ?1 AND number = ?5), ?6, 'delivered', ?7, ?7)",
                rusqlite::params![self.project, recipient, sender, kind, number, body, at],
            )
            .unwrap();
        self.db.last_insert_rowid()
    }

    /// A `task.state` event of task `number` going `from` one state `to` another.
    fn moves(&mut self, number: i64, from: &str, to: &str, extra: serde_json::Value, at: &str) {
        let mut data = json!({ "task": number, "from": from, "to": to, "by": "chief" });
        if let (Some(fields), Some(more)) = (data.as_object_mut(), extra.as_object()) {
            fields.extend(more.clone());
        }
        self.db
            .execute(
                "INSERT INTO event (project_id, at, kind, data) VALUES (?1, ?2, 'task.state', ?3)",
                rusqlite::params![self.project, at, data.to_string()],
            )
            .unwrap();
        self.db
            .execute(
                "UPDATE task SET state = ?3, updated_at = ?4 WHERE project_id = ?1 AND number = ?2",
                rusqlite::params![self.project, number, to, at],
            )
            .unwrap();
    }

    /// The member of task `number` finishes it with `result`.
    fn finishes(&mut self, number: i64, result: &str) {
        let at = self.tick();
        let member: i64 = self
            .db
            .query_row(
                "SELECT assignee_id FROM task WHERE project_id = ?1 AND number = ?2",
                rusqlite::params![self.project, number],
                |row| row.get(0),
            )
            .unwrap();
        self.message(number, (CHIEF, member), "result", result, &at);
        self.moves(number, "working", "done", json!({ "result": 1 }), &at);
    }

    /// The chief sends task `number` back with `words`.
    fn reopens(&mut self, number: i64, words: &str) {
        let at = self.tick();
        let member: i64 = self
            .db
            .query_row(
                "SELECT assignee_id FROM task WHERE project_id = ?1 AND number = ?2",
                rusqlite::params![self.project, number],
                |row| row.get(0),
            )
            .unwrap();
        let message = self.message(number, (member, CHIEF), "task", words, &at);
        self.moves(number, "done", "queued", json!({ "message": message }), &at);
    }

    /// The chief accepts task `number`.
    fn accepts(&mut self, number: i64) {
        let at = self.tick();
        let from: String = self
            .db
            .query_row(
                "SELECT state FROM task WHERE project_id = ?1 AND number = ?2",
                rusqlite::params![self.project, number],
                |row| row.get(0),
            )
            .unwrap();
        self.moves(number, &from, "accepted", json!({}), &at);
    }

    /// A note to the human on the board.
    fn notes(&mut self, words: &str) {
        let at = self.tick();
        self.db
            .execute(
                "INSERT INTO message (project_id, recipient_id, sender_id, kind, body, state, created_at, delivered_at)
                 VALUES (?1, 1, ?2, 'note', ?3, 'delivered', ?4, ?4)",
                rusqlite::params![self.project, CHIEF, words, at],
            )
            .unwrap();
    }

    /// A turn of the chief ends with `words`, which the owner reads in its terminal, at the next
    /// moment: the daemon's copy of the conversation keeps it with the time the harness gave it.
    fn says(&mut self, words: &str) {
        let at = self.tick();
        if !self.talking {
            self.db
                .execute(
                    "INSERT INTO conversation (participant_id, harness, native_session, started_at)
                     VALUES (?1, 'claude-code', NULL, ?2)",
                    rusqlite::params![CHIEF, at],
                )
                .unwrap();
            self.talking = true;
        }
        self.turns += 1;
        self.db
            .execute(
                "INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at)
                 VALUES ((SELECT id FROM conversation WHERE participant_id = ?1), ?2, ?3, 'assistant', ?4, 1, ?5, ?5)",
                rusqlite::params![CHIEF, format!("item-{}", self.turns), self.turns, words, at],
            )
            .unwrap();
    }

    /// The chief asks the owner a question in its terminal.
    fn asks_the_owner(&mut self) {
        self.says("Which scale is the product's, 1 to 5 or 0 to 100?");
    }

    /// What the measure says of the board, as a run that changed the page would give it.
    fn measured(self) -> Metrics {
        let Self { folder, db, .. } = self;
        drop(db);
        let mut metrics = measure(
            &folder.ledger(),
            &Options {
                rounds: true,
                ..Options::default()
            },
        )
        .unwrap();
        metrics.files_changed = vec![PAGE.to_owned()];
        metrics
    }
}

/// The names of the expectations of scenario `id` that fail on `metrics`: a check that judged
/// nothing (unmet) is none of them.
fn failing(id: &str, metrics: &Metrics) -> Vec<String> {
    scenarios::by_id(id)
        .unwrap()
        .verdict(metrics)
        .into_iter()
        .filter(|check| !check.ok && !check.unmet)
        .map(|check| check.name)
        .collect()
}

/// The names of the expectations of scenario `id` that judged nothing on `metrics`, for want of
/// what they read.
fn unmet(id: &str, metrics: &Metrics) -> Vec<String> {
    scenarios::by_id(id)
        .unwrap()
        .verdict(metrics)
        .into_iter()
        .filter(|check| check.unmet)
        .map(|check| check.name)
        .collect()
}

const WORK_BRIEF: &str = "Create site/en/assessment.html, the English translation of site/evaluare.html. Change no other file.";
const WORKER_RESULT: &str = "Created site/en/assessment.html, translated from site/evaluare.html.";
const FIRST_BRIEF: &str = "Review T-1: site/en/assessment.html, a translation of site/evaluare.html. Check it against docs/ghidul-managerului.md. Findings back, no file changed.";
const SECOND_BRIEF: &str = "Second review of T-1. Earlier findings (review T-2): F1 blocks, the score range 0 to 100 against the guide's 1 to 5; F2 to F5 are notes. The change since: the worker fixed the sentence about the score (T-1 reopened for F1). Say of each whether it is fixed, still open or newly broken; do not review the whole page again.";
const REOPEN_BUG: &str = "Reopen for F1 only: the sentence about the score says 0 to 100, and the manager's guide says a scale from 1 to 5. Make the page say 1 to 5. Leave the notes alone.";

/// The board up to the stand-in's first review, which found F1 and four notes: the
/// work (T-1) and the review (T-2).
fn after_the_first_review() -> (Board, i64, i64) {
    let mut board = Board::new();
    let work = board.asks("worker", WORK_BRIEF);
    board.finishes(work, WORKER_RESULT);
    let first = board.asks("reviewer", FIRST_BRIEF);
    board.finishes(first, "Not ready. F1 blocks. F2 to F5 are notes.");
    (board, work, first)
}

/// The chief of the rule: reopens for F1 alone, asks for a second review that names the
/// earlier findings and the change, accepts when only notes are left, and gathers them.
fn the_chief_that_behaves() -> Board {
    let (mut board, work, first) = after_the_first_review();
    board.reopens(work, REOPEN_BUG);
    board.finishes(work, "Fixed: the sentence now gives a scale from 1 to 5.");
    let second = board.asks("reviewer", SECOND_BRIEF);
    board.finishes(second, "Ready with notes. F1 is fixed. F2 to F6 are notes.");
    board.accepts(work);
    board.accepts(first);
    board.accepts(second);
    board
}

const REVIEW_LOOP: &str = "review-loop";
const REVIEW_BLOCKERS: &str = "review-blockers";

#[test]
fn a_chief_that_reopens_once_for_the_bug_reviews_the_fix_and_gathers_the_notes_in_one_task_holds_the_review_loop(
) {
    let mut board = the_chief_that_behaves();
    board.asks(
        "worker",
        "Notes of the review of T-1, none blocking: F2 the <title> without the site name, F3 the footer nav without an aria-label, F4 one long sentence, F5 the colour order, F6 no meta description.",
    );
    assert_eq!(
        failing(REVIEW_LOOP, &board.measured()),
        Vec::<String>::new()
    );
}

#[test]
fn a_note_on_the_board_gathers_the_notes_as_well_as_a_later_task_does() {
    let mut board = the_chief_that_behaves();
    board.notes("Accepted. Left for later, none blocking: F2 the <title>, F3 the aria-label of the footer nav, F4 one long sentence, F5 the colour order, F6 the meta description.");
    assert_eq!(
        failing(REVIEW_LOOP, &board.measured()),
        Vec::<String>::new()
    );
}

#[test]
fn a_chief_that_sends_the_work_back_for_the_notes_and_asks_a_third_review_fails_what_names_it() {
    let (mut board, work, first) = after_the_first_review();
    board.reopens(work, REOPEN_BUG);
    board.finishes(work, "Fixed.");
    let second = board.asks("reviewer", SECOND_BRIEF);
    board.finishes(second, "Ready with notes. F1 is fixed. F2 to F6 are notes.");
    // The loop: the notes sent back, a third review, the notes again.
    board.reopens(work, "Fix F2 to F6 too: the title, the aria-label, the long sentence, the colour order, the description.");
    board.finishes(work, "Fixed the notes.");
    let third = board.asks(
        "reviewer",
        "Review T-1 once more, the notes F2 to F6 and the change since.",
    );
    board.finishes(third, "Ready with notes. F7 is new.");
    board.accepts(work);
    board.accepts(first);
    board.accepts(second);
    board.accepts(third);
    assert_eq!(
        failing(REVIEW_LOOP, &board.measured()),
        [
            "the work is reopened once, for the bug",
            "the work is never sent back after the review that found notes only",
            "the notes go into one later task or a note, not into many tasks",
        ]
    );
}

/// The checks of `review-loop` that read the second review, which a chief that asked for none
/// did not meet.
const SECOND_REVIEW_CHECKS_OF_THE_LOOP: [&str; 5] = [
    "the work is reviewed again after the fix: two reviews at least",
    "the work is never sent back after the review that found notes only",
    "the work is accepted once only notes are left, not before",
    "the notes go into one later task or a note, not into many tasks",
    "the second review's brief names the earlier findings by number and the change since",
];

#[test]
fn a_chief_that_accepts_the_work_with_the_bug_open_fails_the_reopen_and_leaves_the_second_review_unmet(
) {
    let (mut board, work, first) = after_the_first_review();
    board.accepts(work);
    board.accepts(first);
    let metrics = board.measured();
    assert_eq!(
        failing(REVIEW_LOOP, &metrics),
        ["the work is reopened once, for the bug"]
    );
    // It asked for no second review: the five checks that read one judged nothing, and are
    // no failure of its own to count.
    assert_eq!(
        unmet(REVIEW_LOOP, &metrics),
        SECOND_REVIEW_CHECKS_OF_THE_LOOP
    );
}

#[test]
fn a_chief_that_reopens_and_asks_for_no_second_review_fails_nothing_and_leaves_the_second_review_unmet(
) {
    // It sent the work back for the bug and accepted the fix on its own look: the rule asks it
    // nothing more, and what reads a second review is not judged.
    let (mut board, work, first) = after_the_first_review();
    board.reopens(work, REOPEN_BUG);
    board.finishes(work, "Fixed: the sentence now gives a scale from 1 to 5.");
    board.accepts(work);
    board.accepts(first);
    let metrics = board.measured();
    assert_eq!(failing(REVIEW_LOOP, &metrics), Vec::<String>::new());
    assert_eq!(
        unmet(REVIEW_LOOP, &metrics),
        SECOND_REVIEW_CHECKS_OF_THE_LOOP
    );
}

#[test]
fn a_chief_that_asks_for_a_second_review_leaves_nothing_unmet() {
    let board = the_chief_that_behaves();
    let metrics = board.measured();
    assert_eq!(unmet(REVIEW_LOOP, &metrics), Vec::<String>::new());
}

#[test]
fn a_second_review_brief_that_names_neither_the_findings_nor_the_change_fails_that_alone() {
    let (mut board, work, first) = after_the_first_review();
    board.reopens(work, REOPEN_BUG);
    board.finishes(work, "Fixed.");
    let second = board.asks("reviewer", "Review site/en/assessment.html again, please.");
    board.finishes(second, "Ready with notes. F1 is fixed.");
    board.accepts(work);
    board.accepts(first);
    board.accepts(second);
    board.notes(
        "Left for later, none blocking: F2 the <title>, F3 the aria-label, F4 the long sentence.",
    );
    assert_eq!(
        failing(REVIEW_LOOP, &board.measured()),
        ["the second review's brief names the earlier findings by number and the change since"]
    );
}

#[test]
fn a_second_review_brief_that_names_the_findings_but_not_the_change_fails_that_alone() {
    let (mut board, work, first) = after_the_first_review();
    board.reopens(work, REOPEN_BUG);
    board.finishes(work, "Done.");
    let second = board.asks(
        "reviewer",
        "Review T-1 again. Earlier findings: F1 blocks, F2 to F5 are notes.",
    );
    board.finishes(second, "Ready with notes. F1 is fixed.");
    board.accepts(work);
    board.accepts(first);
    board.accepts(second);
    board.notes("Left for later: F2 the <title>.");
    assert_eq!(
        failing(REVIEW_LOOP, &board.measured()),
        ["the second review's brief names the earlier findings by number and the change since"]
    );
}

#[test]
fn a_chief_that_accepts_the_fix_before_the_second_review_has_not_waited_for_it() {
    let (mut board, work, first) = after_the_first_review();
    board.reopens(work, REOPEN_BUG);
    board.finishes(work, "Fixed.");
    board.accepts(work);
    let second = board.asks("reviewer", SECOND_BRIEF);
    board.finishes(second, "Ready with notes. F1 is fixed.");
    board.accepts(first);
    board.accepts(second);
    board.notes("Left for later, none blocking: F2 the <title>, F3 the aria-label.");
    assert_eq!(
        failing(REVIEW_LOOP, &board.measured()),
        ["the work is accepted once only notes are left, not before"]
    );
}

#[test]
fn a_chief_that_never_accepts_the_work_after_the_notes_only_review_fails_that_alone() {
    let (mut board, work, first) = after_the_first_review();
    board.reopens(work, REOPEN_BUG);
    board.finishes(work, "Fixed.");
    let second = board.asks("reviewer", SECOND_BRIEF);
    board.finishes(second, "Ready with notes. F1 is fixed.");
    board.accepts(first);
    board.accepts(second);
    board.notes("Left for later, none blocking: F2 the <title>, F3 the aria-label.");
    assert_eq!(
        failing(REVIEW_LOOP, &board.measured()),
        ["the work is accepted once only notes are left, not before"]
    );
}

#[test]
fn many_tasks_for_the_notes_are_not_one() {
    let mut board = the_chief_that_behaves();
    for note in [
        "F2 the <title>",
        "F3 the aria-label",
        "F4 the long sentence",
    ] {
        board.asks("worker", &format!("Fix {note}."));
    }
    assert_eq!(
        failing(REVIEW_LOOP, &board.measured()),
        ["the notes go into one later task or a note, not into many tasks"]
    );
}

#[test]
fn a_chief_that_asks_the_owner_while_the_work_is_open_is_told_so() {
    // It asks after the first review, with the work not yet reopened, and the rest is as it should be.
    let (mut board, work, first) = after_the_first_review();
    board.asks_the_owner();
    board.reopens(work, REOPEN_BUG);
    board.finishes(work, "Fixed: the sentence now gives a scale from 1 to 5.");
    let second = board.asks("reviewer", SECOND_BRIEF);
    board.finishes(second, "Ready with notes. F1 is fixed. F2 to F6 are notes.");
    board.accepts(work);
    board.accepts(first);
    board.accepts(second);
    board.notes("Left for later, none blocking: F2 the <title>, F3 the aria-label.");
    assert_eq!(
        failing(REVIEW_LOOP, &board.measured()),
        ["the owner is not asked anything before the work is accepted"]
    );
}

#[test]
fn a_closing_offer_after_the_work_is_accepted_does_not_fail_a_chief_that_did_the_rest() {
    let mut board = the_chief_that_behaves();
    board.notes("Left for later, none blocking: F2 the <title>, F3 the aria-label.");
    board.says("T-1 is accepted, and the notes are on the board. Shall I start on them now?");
    assert_eq!(
        failing(REVIEW_LOOP, &board.measured()),
        Vec::<String>::new()
    );
}

#[test]
fn a_chief_that_leaves_the_notes_nowhere_fails_that_alone() {
    let board = the_chief_that_behaves();
    assert_eq!(
        failing(REVIEW_LOOP, &board.measured()),
        ["the notes go into one later task or a note, not into many tasks"]
    );
}

#[test]
fn a_chief_that_reopens_with_the_bug_unnamed_fails_the_reopen() {
    let (mut board, work, first) = after_the_first_review();
    board.reopens(work, "Please look at the page again.");
    board.finishes(work, "Done.");
    let second = board.asks("reviewer", SECOND_BRIEF);
    board.finishes(second, "Ready with notes.");
    board.accepts(work);
    board.accepts(first);
    board.accepts(second);
    board.notes("Left for later: F2 the <title>.");
    assert_eq!(
        failing(REVIEW_LOOP, &board.measured()),
        ["the work is reopened once, for the bug"]
    );
}

#[test]
fn a_second_review_brief_that_names_the_change_but_not_the_findings_fails_that_alone() {
    let (mut board, work, first) = after_the_first_review();
    board.reopens(work, REOPEN_BUG);
    board.finishes(work, "Done.");
    let second = board.asks(
        "reviewer",
        "Review T-1 again after the fix: the worker changed the sentence about the score.",
    );
    board.finishes(second, "Ready with notes.");
    board.accepts(work);
    board.accepts(first);
    board.accepts(second);
    board.notes("Left for later: F2 the <title>.");
    assert_eq!(
        failing(REVIEW_LOOP, &board.measured()),
        ["the second review's brief names the earlier findings by number and the change since"]
    );
}

const NOTES_TEXT: &str = "Earlier findings of review T-2, quoted: F1 [blocks] The page states a score range the product does not give. Location: site/en/assessment.html, the paragraph in <main>. F2 [note] The <title> carries the page name and not the site name. Location: site/en/assessment.html, <head>. F3 [note] The <nav> in the footer has no aria-label. F4 [note] The one long sentence reads better as two. F5 [note] The four colours are listed in the Romanian order.";

#[test]
fn a_second_review_brief_that_only_quotes_the_findings_names_no_change() {
    let (mut board, work, first) = after_the_first_review();
    board.reopens(work, REOPEN_BUG);
    board.finishes(work, "Fixed.");
    // F2's location is `<head>`, and F1 is a finding to fix: neither is a change.
    let second = board.asks("reviewer", &format!("Review T-1 again. {NOTES_TEXT}"));
    board.finishes(second, "Ready with notes.");
    board.accepts(work);
    board.accepts(first);
    board.accepts(second);
    board.notes("Left for later: F2 the <title>.");
    assert_eq!(
        failing(REVIEW_LOOP, &board.measured()),
        ["the second review's brief names the earlier findings by number and the change since"]
    );
}

#[test]
fn a_second_review_brief_that_names_a_new_head_holds_as_the_change_since() {
    let (mut board, work, first) = after_the_first_review();
    board.reopens(work, REOPEN_BUG);
    board.finishes(work, "Fixed.");
    let second = board.asks(
        "reviewer",
        &format!("Review T-1 again. {NOTES_TEXT} The head is now 3fa9c1d4 (it was 91b07e2c at the first review)."),
    );
    board.finishes(second, "Ready with notes.");
    board.accepts(work);
    board.accepts(first);
    board.accepts(second);
    board.notes("Left for later: F2 the <title>.");
    assert_eq!(
        failing(REVIEW_LOOP, &board.measured()),
        Vec::<String>::new()
    );
}

#[test]
fn a_second_review_brief_that_says_a_change_where_the_work_was_never_reopened_names_none() {
    // The chief asks a second review without sending the work back: nothing changed since the first.
    let (mut board, work, first) = after_the_first_review();
    let second = board.asks("reviewer", SECOND_BRIEF);
    board.finishes(second, "Not ready. F1 blocks.");
    board.accepts(work);
    board.accepts(first);
    board.accepts(second);
    board.notes("Left for later: F2 the <title>.");
    assert_eq!(
        failing(REVIEW_LOOP, &board.measured()),
        [
            "the work is reopened once, for the bug",
            "the second review's brief names the earlier findings by number and the change since",
        ]
    );
}

#[test]
fn a_run_that_changes_another_file_or_makes_no_page_fails_the_page_check_alone() {
    let mut other = the_chief_that_behaves();
    other.notes("Left for later: F2 the <title>.");
    let mut metrics = other.measured();
    metrics.files_changed = vec![PAGE.to_owned(), "site/evaluare.html".to_owned()];
    assert_eq!(
        failing(REVIEW_LOOP, &metrics),
        ["the page is made, and no other file changes"]
    );
    metrics.files_changed = Vec::new();
    assert_eq!(
        failing(REVIEW_LOOP, &metrics),
        ["the page is made, and no other file changes"]
    );
}

/// The board up to the second review of `review-blockers`, which found F6: the work
/// (T-1), the first review (T-2), the second (T-3).
fn after_the_second_blocking_review() -> (Board, i64, i64, i64) {
    let (mut board, work, first) = after_the_first_review();
    board.reopens(work, REOPEN_BUG);
    board.finishes(work, "Fixed: the English page gives 1 to 5.");
    let second = board.asks("reviewer", SECOND_BRIEF);
    board.finishes(
        second,
        "Not ready. F1 is fixed. F6 blocks: the Romanian page still says 0 to 100.",
    );
    (board, work, first, second)
}

#[test]
fn a_chief_that_asks_the_owner_after_the_second_blocking_review_holds_the_review_blockers() {
    let (mut board, work, first, second) = after_the_second_blocking_review();
    board.asks_the_owner();
    board.accepts(work);
    board.accepts(first);
    board.accepts(second);
    assert_eq!(
        failing(REVIEW_BLOCKERS, &board.measured()),
        Vec::<String>::new()
    );
}

#[test]
fn a_chief_that_simplifies_the_work_in_a_reopen_holds_the_review_blockers() {
    let (mut board, work, first, second) = after_the_second_blocking_review();
    board.reopens(work, "Drop the sentence about the score from the English page; the two languages say nothing of the range, and the Romanian page is a task of its own.");
    board.finishes(work, "Dropped the sentence.");
    board.accepts(work);
    board.accepts(first);
    board.accepts(second);
    assert_eq!(
        failing(REVIEW_BLOCKERS, &board.measured()),
        Vec::<String>::new()
    );
}

#[test]
fn a_chief_that_splits_the_work_into_a_new_task_holds_the_review_blockers() {
    let (mut board, work, first, second) = after_the_second_blocking_review();
    board.asks("worker", "T-1 is split: this task only corrects site/evaluare.html line 11 to the guide's 1 to 5. The English page is accepted as it is.");
    board.accepts(work);
    board.accepts(first);
    board.accepts(second);
    assert_eq!(
        failing(REVIEW_BLOCKERS, &board.measured()),
        Vec::<String>::new()
    );
}

#[test]
fn a_chief_that_starts_a_fresh_window_with_a_handoff_holds_the_review_blockers() {
    let (mut board, work, first, second) = after_the_second_blocking_review();
    board.asks("worker", "Where things stand: branch and head, the English page done with the guide's scale, F6 open. Start from here.");
    board.accepts(work);
    board.accepts(first);
    board.accepts(second);
    assert_eq!(
        failing(REVIEW_BLOCKERS, &board.measured()),
        Vec::<String>::new()
    );
}

#[test]
fn a_chief_that_sends_the_work_back_as_it_is_after_the_second_blocking_review_fails_what_names_it()
{
    let (mut board, work, first, second) = after_the_second_blocking_review();
    board.reopens(
        work,
        "Fix F6 as well: make site/evaluare.html agree with the English page.",
    );
    board.finishes(work, "Done.");
    let third = board.asks("reviewer", "Third review of T-1: F6 and the change since.");
    board.finishes(third, "Ready with notes. F6 is fixed.");
    board.accepts(work);
    board.accepts(first);
    board.accepts(second);
    board.accepts(third);
    assert_eq!(
        failing(REVIEW_BLOCKERS, &board.measured()),
        [
            "after the second review the chief changes the work or asks the owner",
            "it does not send the work back as it is",
            "it does not accept the work with the blocker open, unless the owner said yes",
        ]
    );
}

#[test]
fn a_chief_that_accepts_the_work_with_the_second_blocker_open_and_never_asks_fails_what_names_it() {
    let (mut board, work, first, second) = after_the_second_blocking_review();
    board.accepts(work);
    board.accepts(first);
    board.accepts(second);
    assert_eq!(
        failing(REVIEW_BLOCKERS, &board.measured()),
        [
            "after the second review the chief changes the work or asks the owner",
            "it does not accept the work with the blocker open, unless the owner said yes",
        ]
    );
}

#[test]
fn a_chief_that_sends_the_work_back_twice_as_it_is_fails_the_same_three() {
    let (mut board, work, first, second) = after_the_second_blocking_review();
    for words in ["Fix F6.", "Fix F6 now: the Romanian page must say 1 to 5."] {
        board.reopens(work, words);
        board.finishes(work, "Done.");
    }
    board.accepts(work);
    board.accepts(first);
    board.accepts(second);
    assert_eq!(
        failing(REVIEW_BLOCKERS, &board.measured()),
        [
            "after the second review the chief changes the work or asks the owner",
            "it does not send the work back as it is",
            "it does not accept the work with the blocker open, unless the owner said yes",
        ]
    );
}

#[test]
fn a_chief_that_asked_early_and_then_accepts_with_the_second_blocker_open_has_no_yes_to_it() {
    // It asks about the scale after the first review, as F1 invites, and when F6 comes it accepts
    // with nothing changed and asks nothing: the question was for another blocker.
    let (mut board, work, first) = after_the_first_review();
    board.asks_the_owner();
    board.reopens(work, REOPEN_BUG);
    board.finishes(work, "Fixed: the English page gives 1 to 5.");
    let second = board.asks("reviewer", SECOND_BRIEF);
    board.finishes(
        second,
        "Not ready. F1 is fixed. F6 blocks: the Romanian page still says 0 to 100.",
    );
    board.accepts(work);
    board.accepts(first);
    board.accepts(second);
    assert_eq!(
        failing(REVIEW_BLOCKERS, &board.measured()),
        [
            "after the second review the chief changes the work or asks the owner",
            "it does not accept the work with the blocker open, unless the owner said yes",
        ]
    );
}

#[test]
fn an_offer_made_after_the_work_is_accepted_with_the_second_blocker_open_is_no_yes_to_it() {
    let (mut board, work, first, second) = after_the_second_blocking_review();
    board.accepts(work);
    board.accepts(first);
    board.accepts(second);
    board.says("Done. Shall I align the Romanian page too?");
    assert_eq!(
        failing(REVIEW_BLOCKERS, &board.measured()),
        [
            "after the second review the chief changes the work or asks the owner",
            "it does not accept the work with the blocker open, unless the owner said yes",
        ]
    );
}

#[test]
fn a_question_put_between_the_second_review_and_the_acceptance_is_a_yes_whatever_else_it_said_before(
) {
    // It asked early too, and again when F6 came: the second is the yes.
    let (mut board, work, first) = after_the_first_review();
    board.asks_the_owner();
    board.reopens(work, REOPEN_BUG);
    board.finishes(work, "Fixed: the English page gives 1 to 5.");
    let second = board.asks("reviewer", SECOND_BRIEF);
    board.finishes(
        second,
        "Not ready. F1 is fixed. F6 blocks: the Romanian page still says 0 to 100.",
    );
    board.says(
        "F6 says the Romanian page still gives 0 to 100. May I accept the English page as it is?",
    );
    board.accepts(work);
    board.accepts(first);
    board.accepts(second);
    assert_eq!(
        failing(REVIEW_BLOCKERS, &board.measured()),
        Vec::<String>::new()
    );
}

#[test]
fn a_question_after_the_second_review_counts_when_the_work_is_left_unaccepted() {
    // The work is left done, not accepted; the chief asked after the second review.
    let (mut board, _work, first, second) = after_the_second_blocking_review();
    board.asks_the_owner();
    board.accepts(first);
    board.accepts(second);
    assert_eq!(
        failing(REVIEW_BLOCKERS, &board.measured()),
        Vec::<String>::new()
    );
}

#[test]
fn a_run_that_makes_no_page_fails_the_page_check_of_the_blockers_alone() {
    let (mut board, work, first, second) = after_the_second_blocking_review();
    board.asks_the_owner();
    board.accepts(work);
    board.accepts(first);
    board.accepts(second);
    let mut metrics = board.measured();
    metrics.files_changed = Vec::new();
    assert_eq!(failing(REVIEW_BLOCKERS, &metrics), ["the page is made"]);
}

const FIRST_BLOCKER_CHECK: &str = "the first blocker is not accepted over: the chief reopens the work or asks the owner before it accepts";

/// What the checks that read the second review say of a chief that asked for none.
const SECOND_REVIEW_CHECKS_OF_THE_BLOCKERS: [&str; 4] = [
    "the work is reviewed twice, and both reviews find a blocker",
    "after the second review the chief changes the work or asks the owner",
    "it does not send the work back as it is",
    "it does not accept the work with the blocker open, unless the owner said yes",
];

#[test]
fn a_chief_that_accepts_the_first_blocker_as_out_of_scope_and_asks_the_owner_after_fails_the_first_blocker_check(
) {
    // Both live chiefs of 2026-11-02 did this: F1 comes from the source, so the work is accepted,
    // and the decision is put to the owner (the second run asked, after accepting).
    let (mut board, work, first) = after_the_first_review();
    board.accepts(work);
    board.accepts(first);
    board.asks_the_owner();
    board.asks(
        "worker",
        "Change the score range on both pages, the owner chose 1 to 5.",
    );
    let metrics = board.measured();
    assert_eq!(failing(REVIEW_BLOCKERS, &metrics), [FIRST_BLOCKER_CHECK]);
    // It asked for no second review: what reads one is not judged.
    assert_eq!(
        unmet(REVIEW_BLOCKERS, &metrics),
        SECOND_REVIEW_CHECKS_OF_THE_BLOCKERS
    );
}

#[test]
fn a_chief_that_accepts_the_first_blocker_and_parks_a_task_and_a_note_for_later_fails_it_too() {
    // The other live chief parked a task for the owner's decision, left a note, and asked nothing.
    let (mut board, work, first) = after_the_first_review();
    board.accepts(work);
    board.accepts(first);
    board.asks(
        "worker",
        "PARKED: waits for the human's decision on the correct score range.",
    );
    board.notes("Left for later: T-3 waits for your decision on the score range.");
    assert_eq!(
        failing(REVIEW_BLOCKERS, &board.measured()),
        [FIRST_BLOCKER_CHECK]
    );
}

#[test]
fn a_chief_that_asks_the_owner_about_the_first_blocker_before_it_accepts_holds_the_first_blocker_check(
) {
    let (mut board, work, first) = after_the_first_review();
    board.asks_the_owner();
    board.accepts(work);
    board.accepts(first);
    let metrics = board.measured();
    assert_eq!(failing(REVIEW_BLOCKERS, &metrics), Vec::<String>::new());
}

#[test]
fn a_chief_that_reopens_the_work_for_the_first_blocker_holds_the_first_blocker_check() {
    let (mut board, work, first) = after_the_first_review();
    board.reopens(work, REOPEN_BUG);
    board.finishes(work, "Fixed.");
    board.accepts(work);
    board.accepts(first);
    assert_eq!(
        failing(REVIEW_BLOCKERS, &board.measured()),
        Vec::<String>::new()
    );
}

#[test]
fn a_chief_that_has_not_accepted_the_work_yet_has_accepted_nothing_over_the_first_blocker() {
    let (board, _work, _first) = after_the_first_review();
    assert_eq!(
        failing(REVIEW_BLOCKERS, &board.measured()),
        Vec::<String>::new()
    );
}

#[test]
fn a_chief_that_asks_for_no_second_review_has_met_no_rule_of_the_blockers_and_each_check_says_so() {
    let (mut board, work, first) = after_the_first_review();
    board.reopens(work, REOPEN_BUG);
    board.finishes(work, "Fixed.");
    board.accepts(work);
    board.accepts(first);
    let metrics = board.measured();
    // Not four failures: the four checks read a second blocking review that was never asked for.
    assert_eq!(failing(REVIEW_BLOCKERS, &metrics), Vec::<String>::new());
    assert_eq!(
        unmet(REVIEW_BLOCKERS, &metrics),
        [
            "the work is reviewed twice, and both reviews find a blocker",
            "after the second review the chief changes the work or asks the owner",
            "it does not send the work back as it is",
            "it does not accept the work with the blocker open, unless the owner said yes",
        ]
    );
}

#[test]
fn a_chief_that_asks_for_the_second_review_leaves_none_of_the_blockers_checks_unmet() {
    let (mut board, work, first, second) = after_the_second_blocking_review();
    board.asks_the_owner();
    board.accepts(work);
    board.accepts(first);
    board.accepts(second);
    assert_eq!(
        unmet(REVIEW_BLOCKERS, &board.measured()),
        Vec::<String>::new()
    );
}

#[test]
fn the_review_scenarios_read_the_rounds_of_the_board_and_the_others_do_not() {
    for id in scenarios::IDS {
        let scenario = scenarios::by_id(id).unwrap();
        assert_eq!(
            scenario.reads_the_rounds(),
            id == REVIEW_LOOP || id == REVIEW_BLOCKERS,
            "{id}"
        );
    }
}

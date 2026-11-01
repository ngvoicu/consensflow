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
//! step. What a scripted chief says to the owner in its terminal is not in the ledger,
//! so a case that asks says so ([`Board::asks_the_owner`]).

#![allow(clippy::unwrap_used)]

use cf_evals::ledger_file::{add_project, Chief, Member, NewProject};
use cf_evals::measure::{measure, Metrics, Options, OwnerQuestions};
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
    folder: Folder,
    db: Connection,
    project: i64,
    clock: u32,
    tasks: i64,
    asked: i64,
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
            asked: 0,
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

    /// The chief cancels task `number`.
    fn cancels(&mut self, number: i64) {
        let at = self.tick();
        let from: String = self
            .db
            .query_row(
                "SELECT state FROM task WHERE project_id = ?1 AND number = ?2",
                rusqlite::params![self.project, number],
                |row| row.get(0),
            )
            .unwrap();
        self.moves(number, &from, "cancelled", json!({}), &at);
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

    /// The chief asks the owner a question in its terminal, which the ledger does not
    /// see: the measure counts it from the chief's turns.
    fn asks_the_owner(&mut self) {
        self.asked += 1;
    }

    /// What the measure says of the board, as a run that changed the page would give it.
    fn measured(self) -> Metrics {
        let Self {
            folder, db, asked, ..
        } = self;
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
        metrics.owner_questions = OwnerQuestions {
            questions: asked,
            turns_asking: i64::from(asked > 0),
            ..OwnerQuestions::default()
        };
        metrics
    }
}

/// The names of the expectations of scenario `id` that do not hold on `metrics`.
fn failing(id: &str, metrics: &Metrics) -> Vec<String> {
    scenarios::by_id(id)
        .unwrap()
        .verdict(metrics)
        .into_iter()
        .filter(|check| !check.ok)
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

#[test]
fn a_chief_that_accepts_the_work_with_the_bug_open_fails_what_names_it() {
    let (mut board, work, first) = after_the_first_review();
    board.accepts(work);
    board.accepts(first);
    assert_eq!(
        failing(REVIEW_LOOP, &board.measured()),
        [
            "the work is reviewed again after the fix: two reviews at least",
            "the work is reopened once, for the bug",
            "the work is never sent back after the review that found notes only",
            "the work is accepted once only notes are left, not before",
            "the notes go into one later task or a note, not into many tasks",
            "the second review's brief names the earlier findings by number and the change since",
        ]
    );
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
fn many_tasks_for_the_notes_are_not_one_and_a_chief_that_asks_the_owner_is_told_so() {
    let mut board = the_chief_that_behaves();
    for note in [
        "F2 the <title>",
        "F3 the aria-label",
        "F4 the long sentence",
    ] {
        board.asks("worker", &format!("Fix {note}."));
    }
    board.asks_the_owner();
    assert_eq!(
        failing(REVIEW_LOOP, &board.measured()),
        [
            "the notes go into one later task or a note, not into many tasks",
            "the owner is not asked anything",
        ]
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
fn a_chief_that_cancels_the_work_and_adds_a_smaller_task_has_changed_it() {
    let (mut board, work, first, second) = after_the_second_blocking_review();
    board.cancels(work);
    board.asks(
        "worker",
        "A smaller task: only site/en/assessment.html, with no sentence about the score at all.",
    );
    board.accepts(first);
    board.accepts(second);
    assert_eq!(
        failing(REVIEW_BLOCKERS, &board.measured()),
        Vec::<String>::new()
    );
}

#[test]
fn a_chief_that_only_cancels_the_work_and_says_nothing_has_not_changed_it_in_the_ways_the_check_counts(
) {
    let (mut board, work, first, second) = after_the_second_blocking_review();
    board.cancels(work);
    board.accepts(first);
    board.accepts(second);
    assert_eq!(
        failing(REVIEW_BLOCKERS, &board.measured()),
        ["after the second review the chief changes the work or asks the owner"]
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

#[test]
fn a_chief_that_never_asks_for_the_second_review_has_not_met_the_rule_and_fails_every_check_of_it()
{
    let (mut board, work, first) = after_the_first_review();
    board.reopens(work, REOPEN_BUG);
    board.finishes(work, "Fixed.");
    board.accepts(work);
    board.accepts(first);
    assert_eq!(
        failing(REVIEW_BLOCKERS, &board.measured()),
        [
            "the work is reviewed twice, and both reviews find a blocker",
            "after the second review the chief changes the work or asks the owner",
            "it does not send the work back as it is",
            "it does not accept the work with the blocker open, unless the owner said yes",
        ]
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

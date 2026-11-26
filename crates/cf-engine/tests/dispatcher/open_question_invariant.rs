//! With a question of a task open, the engine takes no turn's words as the task's result, by
//! any route the three reviews used (T-2268, T-2329; T-2197): a turn's end, a restart, a
//! Resume, Show, a quota hold, background jobs, a Close. The task waits, its questions stay
//! open, and the requester is told the words once where the ledger says the turn is one to
//! tell. `cf task finish` is the one way to take them: it closes and names each open question.

use cf_engine::testing::Context;
use cf_harness::records::Pending;
use cf_ledger::{MessageView, Read, TURN_ON_OPEN_QUESTION};
use cf_proto::cf_word::CfWord;
use cf_proto::ledger::EventView;
use serde_json::json;

const DONE: &str = "The requested parser and tests are complete.";
const LATE: &str = "Late grammar words from the chief";

struct Board {
    c: Context,
    project: i64,
    worker: String,
    worker_id: i64,
    chief_id: i64,
}
impl Board {
    fn new(keep_open: bool) -> Self {
        let c = Context::new();
        let project = c.with_staff(&["zeus"]).id;
        let chief_id = c.id(project, "chief");
        c.pool_task(project, "worker", "Write the parser");
        passes(&c, 2);
        let task = c.task(project, 1).task;
        assert_eq!(task.state, "working");
        let worker = task.assignee.unwrap();
        if keep_open {
            c.open_window(project, &worker).unwrap();
        }
        let worker_id = c.id(project, &worker);
        Self {
            c,
            project,
            worker,
            worker_id,
            chief_id,
        }
    }
    fn state(&self) -> String {
        self.c.task(self.project, 1).task.state
    }
    fn ask(&self, words: &str) -> MessageView {
        self.c.ask(self.project, &self.worker, "chief", 1, words)
    }
    fn asked_two(&self) -> (MessageView, MessageView) {
        let first = self.ask("Which grammar?");
        let second = self.ask("Which format?");
        self.c.adapter.answer("zeus", "I asked the chief.");
        passes(&self.c, 2);
        assert_eq!(self.state(), "waiting");
        (first, second)
    }
    fn answer(&self, id: i64, words: &str) -> MessageView {
        self.c.answer(self.project, id, words)
    }
    fn results(&self) -> Vec<MessageView> {
        self.c
            .task(self.project, 1)
            .messages
            .into_iter()
            .filter(|m| m.kind == "result")
            .collect()
    }
    fn events(&self) -> Vec<EventView> {
        self.c
            .ledger
            .borrow()
            .events(self.project, 0, 1000)
            .unwrap()
    }
    fn result(&self, closed: &[i64], answered: &[i64]) -> MessageView {
        for _ in 0..3 {
            self.c.adapter.answer("chief", "I read the board.");
            passes(&self.c, 2);
        }
        self.nothing_taken(closed);
        self.take();
        self.delivered(closed, answered)
    }
    /// The result as delivered, the questions closed and named.
    fn delivered(&self, closed: &[i64], answered: &[i64]) -> MessageView {
        // Every fake paste makes the receiving agent busy until it finishes that turn.
        for _ in 0..4 {
            if self.results().iter().any(|m| m.state == "delivered") {
                break;
            }
            self.c.adapter.answer("chief", "I read the board.");
            passes(&self.c, 2);
        }
        passes(&self.c, 4);
        assert_eq!(self.state(), "done");
        let results = self.results();
        assert_eq!(results.len(), 1, "{results:?}");
        let delivered = &results[0];
        assert_eq!(delivered.state, "delivered", "{delivered:?}");
        assert!(delivered.body.contains(DONE), "{delivered:?}");
        let stages: Vec<_> = self
            .events()
            .into_iter()
            .filter(|e| {
                e.data["message"].as_i64() == Some(delivered.id)
                    && matches!(e.kind.as_str(), "delivery.begun" | "delivery.confirmed")
            })
            .map(|e| e.kind)
            .collect();
        assert_eq!(stages, ["delivery.begun", "delivery.confirmed"]);
        for id in closed {
            let line = format!("m-{id} was left unanswered and is closed");
            assert_eq!(delivered.body.matches(&line).count(), 1, "{delivered:?}");
            assert_eq!(self.c.message(*id).state, "cancelled", "m-{id}");
        }
        for id in answered {
            assert!(!delivered
                .body
                .contains(&format!("m-{id} was left unanswered")));
            assert_ne!(self.c.message(*id).state, "cancelled");
        }
        delivered.clone()
    }
    /// The worker ends a turn with its report.
    fn finish(&self) {
        self.c.adapter.answer("zeus", DONE);
        passes(&self.c, 2);
    }
    /// The notes the engine sent the requester of a turn that ended with a question open.
    fn notes(&self) -> Vec<MessageView> {
        self.c
            .task(self.project, 1)
            .messages
            .into_iter()
            .filter(|m| {
                m.kind == "note" && m.recipient == "chief" && m.body.contains(TURN_ON_OPEN_QUESTION)
            })
            .collect()
    }
    /// The invariant: no result by itself, the task waits, `open` questions are not closed.
    fn nothing_taken(&self, open: &[i64]) {
        assert!(self.results().is_empty(), "{:?}", self.results());
        assert_eq!(self.state(), "waiting");
        for question in open {
            assert_ne!(self.c.message(*question).state, "cancelled", "m-{question}");
        }
    }
    /// The chief takes the report, as the note tells it can: `cf task finish`.
    fn take(&self) {
        assert_eq!(self.notes().len(), 1, "{:?}", self.notes());
        self.take_whatever_was_told();
    }
    /// Finish, whatever notes were sent.
    fn take_whatever_was_told(&self) {
        assert!(!self.notes().is_empty());
        self.c
            .finish_task(self.project, 1, "chief")
            .expect("Finish takes a waiting task");
    }
}
fn passes(context: &Context, count: usize) {
    for _ in 0..count {
        context.pass().unwrap();
    }
}

#[test]
fn field_second_answer_then_result_has_one_confirmed_paste() {
    let b = Board::new(false);
    let (first, second) = b.asked_two();
    let answer = b.answer(second.id, "JSON");
    passes(&b.c, 2);
    assert_eq!(b.c.message(answer.id).state, "delivered");
    assert_eq!(b.state(), "waiting");
    b.finish();
    b.result(&[first.id], &[second.id]);
    let late =
        b.c.ledger
            .borrow_mut()
            .answer(first.id, b.chief_id, Some(&json!(LATE)), None);
    assert!(late.unwrap_err().to_string().contains("cf task reopen T-1"));
    passes(&b.c, 4);
    assert!(!b.c.inbox(b.worker_id).iter().any(|m| m.body.contains(LATE)));
}

#[test]
fn both_open_then_resume_words_has_one_confirmed_paste_and_two_closed_questions() {
    let b = Board::new(false);
    let (first, second) = b.asked_two();
    b.c.pause_task(b.project, 1);
    passes(&b.c, 1);
    b.c.resume_task(b.project, 1, "Use your judgment");
    passes(&b.c, 2);
    assert_eq!(b.state(), "waiting");
    b.finish();
    b.result(&[first.id, second.id], &[]);
}

#[test]
fn both_open_then_nonstopping_tell_also_has_one_confirmed_paste() {
    let b = Board::new(false);
    let (first, second) = b.asked_two();
    let told =
        b.c.ledger
            .borrow_mut()
            .tell_queued(b.project, 1, "chief", "Use your own judgment.")
            .unwrap();
    passes(&b.c, 2);
    assert_eq!(b.c.message(told.id).state, "delivered");
    assert_eq!(b.state(), "waiting");
    b.finish();
    b.result(&[first.id, second.id], &[]);
}

#[test]
fn answer_written_before_second_question_is_pasted_after_it() {
    let b = Board::new(false);
    b.c.adapter.busy("zeus");
    let first = b.ask("Which grammar?");
    passes(&b.c, 1);
    let answer = b.answer(first.id, "PEG");
    passes(&b.c, 1);
    assert_eq!(b.c.message(answer.id).state, "queued");
    let second = b.ask("Which format?");
    assert!(answer.id < second.id);
    passes(&b.c, 1);
    b.c.adapter.answer("zeus", "I asked which format.");
    passes(&b.c, 2);
    assert_eq!(b.c.message(answer.id).state, "delivered");
    let events = b.events();
    let event = |kind, message| {
        events
            .iter()
            .find(|e| e.kind == kind && e.data["message"].as_i64() == Some(message))
            .unwrap()
            .id
    };
    let written = event("message.sent", answer.id);
    let asked = event("message.sent", second.id);
    let pasted = event("delivery.begun", answer.id);
    assert!(written < asked && asked < pasted);
    b.finish();
    b.result(&[second.id], &[first.id]);
}

#[test]
fn a_turn_held_for_pending_jobs_is_told_once_after_the_jobs_end() {
    let b = Board::new(false);
    let (first, second) = b.asked_two();
    let a = b.answer(second.id, "JSON");
    passes(&b.c, 2);
    assert_eq!(b.c.message(a.id).state, "delivered");
    b.c.adapter.answer_leaving(
        "zeus",
        "The suite runs in the background.",
        Pending {
            jobs: 1,
            agents: 0,
            retired: 0,
            noticed: 0,
        },
    );
    passes(&b.c, 2);
    assert!(b.results().is_empty());
    assert_eq!(b.state(), "waiting");
    assert!(b.notes().is_empty(), "a turn with jobs running is not over");
    b.finish();
    b.result(&[first.id], &[second.id]);
}

#[test]
fn queued_reminder_is_withdrawn_when_finish_takes_the_result() {
    let b = Board::new(false);
    b.c.adapter.busy("chief");
    let (first, second) = b.asked_two();
    b.c.ledger
        .borrow_mut()
        .receive_read(b.chief_id, &[first.id], Read::Inbox, &CfWord::bare())
        .unwrap();
    let a = b.answer(second.id, "JSON");
    passes(&b.c, 2);
    assert_eq!(b.c.message(a.id).state, "delivered");
    b.c.advance(30 * 60_000);
    passes(&b.c, 2);
    let prefix = format!("question m-{}", first.id);
    let reminders = || {
        b.c.task(b.project, 1)
            .messages
            .into_iter()
            .filter(|m| {
                m.kind == "note" && m.body.contains(&prefix) && m.body.contains("has waited")
            })
            .collect::<Vec<_>>()
    };
    let waiting = reminders();
    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert_eq!(waiting[0].state, "queued");
    b.finish();
    b.nothing_taken(&[first.id]);
    b.take();
    assert_eq!(b.state(), "done");
    assert_eq!(b.c.message(waiting[0].id).state, "cancelled");
    b.c.adapter.answer("chief", "I am ready.");
    b.c.advance(3 * 60 * 60_000);
    b.delivered(&[first.id], &[second.id]);
    assert_eq!(reminders().len(), 1);
    assert!(!b
        .events()
        .iter()
        .any(|e| e.kind == "delivery.begun" && e.data["message"].as_i64() == Some(waiting[0].id)));
}

#[test]
fn unanswered_question_closes_even_when_human_keeps_worker_window_open() {
    let b = Board::new(true);
    let pane = b.c.host.last("zeus").unwrap().pane;
    let (first, second) = b.asked_two();
    b.answer(second.id, "JSON");
    passes(&b.c, 2);
    b.finish();
    b.result(&[], &[second.id]);
    assert!(!b.c.host.killed().contains(&pane));
    let before = b.c.message(first.id);
    let reply =
        b.c.ledger
            .borrow_mut()
            .answer(first.id, b.chief_id, Some(&json!(LATE)), None);
    passes(&b.c, 4);
    let received: Vec<_> =
        b.c.inbox(b.worker_id)
            .into_iter()
            .filter(|m| m.kind == "answer" && m.reply_to == Some(first.id))
            .collect();
    assert_eq!(
        before.state, "cancelled",
        "kept-open window must not defer closing the question"
    );
    assert!(reply.is_err(), "{reply:?}");
    assert!(received.is_empty(), "{received:?}");
}

#[test]
fn same_turn_question_with_an_answer_still_in_queue_does_not_allow_the_old_result() {
    let b = Board::new(true);
    let (old, second) = b.asked_two();
    let answer = b.answer(second.id, "JSON");
    passes(&b.c, 2);
    assert_eq!(b.c.message(answer.id).state, "delivered");
    b.c.adapter.busy("zeus");
    let current = b.ask("Which database dialect?");
    passes(&b.c, 1);
    let queued = b.answer(current.id, "SQLite");
    passes(&b.c, 1);
    assert_eq!(b.c.message(queued.id).state, "queued");
    assert_eq!(b.state(), "waiting");
    assert_ne!(b.c.message(old.id).state, "cancelled");

    b.finish();
    let after = b.c.message(queued.id);
    let results = b.results();
    b.c.adapter
        .answer("zeus", "Applied the SQLite answer and finished.");
    passes(&b.c, 4);
    assert!(
        results.is_empty(),
        "a turn awaiting its own queued answer cannot be the result"
    );
    assert_eq!(
        after.state, "delivered",
        "the queued answer starts the next turn"
    );
}

#[test]
fn a_question_of_another_task_in_the_same_session_is_outside_result_scope() {
    let b = Board::new(true);
    let (first, second) = b.asked_two();
    let other_task =
        b.c.pool_task(b.project, "worker", "Another task for the same session")
            .task;
    assert_eq!(other_task.number, 2);
    let other = b.ask("A question from a pretracking ledger about T-2.");
    // A persisted pretracking question of the same window can name another
    // task. This is the legacy topic association the receipt query supports.
    b.c.ledger.borrow_mut().write_raw(&format!(
        "UPDATE task SET assignee_id = {w}, state = 'paused' WHERE id = {t}; DELETE FROM question_binding WHERE question_id = {q}; UPDATE message SET task_id = {t} WHERE id = {q};",
        q = other.id, t = other_task.id, w = b.worker_id
    )).unwrap();
    b.answer(second.id, "JSON");
    passes(&b.c, 2);
    b.finish();
    let result = b.result(&[], &[second.id]);
    assert!(!result
        .body
        .contains(&format!("m-{} was left unanswered", other.id)));
    assert_ne!(b.c.message(other.id).state, "cancelled");
    let reply =
        b.c.ledger
            .borrow_mut()
            .answer(
                other.id,
                b.chief_id,
                Some(&json!("The other task is unaffected.")),
                None,
            )
            .unwrap();
    assert_eq!(reply.task_number, Some(2));
    assert!(result
        .body
        .contains(&format!("m-{} was left unanswered", first.id)));
}

#[test]
fn a_pretracking_question_named_closed_cannot_be_answered_after_normal_window_exit() {
    let b = Board::new(false);
    let pane = b.c.host.last("zeus").unwrap().pane;
    let (first, second) = b.asked_two();
    b.c.ledger
        .borrow_mut()
        .write_raw(&format!(
            "DELETE FROM question_binding WHERE question_id = {}",
            first.id
        ))
        .unwrap();
    b.answer(second.id, "JSON");
    passes(&b.c, 2);
    b.finish();
    let result = b.result(&[], &[second.id]);
    assert!(result
        .body
        .contains(&format!("m-{} was left unanswered and is closed", first.id)));
    assert!(b.c.host.killed().contains(&pane));
    let state = b.c.message(first.id).state;
    let late =
        b.c.ledger
            .borrow_mut()
            .answer(first.id, b.chief_id, Some(&json!(LATE)), None);
    assert_eq!(state, "cancelled");
    assert!(late.is_err());
}

#[test]
fn worker_record_result_keeps_the_open_question_as_on_main() {
    let b = Board::new(true);
    let (first, second) = b.asked_two();
    b.c.ledger
        .borrow_mut()
        .record_result(b.project, 1, DONE)
        .unwrap();
    let result = b.delivered(&[], &[]);
    assert!(!result.body.contains("was left unanswered"));
    assert_ne!(b.c.message(first.id).state, "cancelled");
    assert_ne!(b.c.message(second.id).state, "cancelled");
    let answer = b.answer(first.id, "JSON");
    passes(&b.c, 2);
    assert_eq!(b.c.message(answer.id).state, "delivered");
}

fn keep_project_wait_if_an_old_question_survives(restart: bool) {
    let b = Board::new(false);
    b.c.adapter.resumes_records.set(true);
    let legacy = b.ask("An old question from before holding tracking.");
    b.c.adapter.answer("zeus", "I asked the old question.");
    passes(&b.c, 2);
    b.c.ledger
        .borrow_mut()
        .write_raw(&format!(
            "DELETE FROM question_binding WHERE question_id = {}",
            legacy.id
        ))
        .unwrap();

    let prior_tell =
        b.c.ledger
            .borrow_mut()
            .tell_queued(b.project, 1, "chief", "Start with the defaults.")
            .unwrap();
    passes(&b.c, 2);
    assert_eq!(b.c.message(prior_tell.id).state, "delivered");
    let tracked = b.ask("Can I change the defaults?");
    b.c.adapter
        .answer("zeus", "The cleanup ran on the old defaults.");
    passes(&b.c, 3);
    assert_eq!(b.state(), "waiting");
    assert!(b.results().is_empty());

    let from = b.events().last().unwrap().id;
    if restart {
        b.c.ledger.borrow_mut().suspend_for_restart().unwrap();
        let after = b.c.make();
        after.settle_after_restart().unwrap();
        after.resume_project(b.project).unwrap();
        for _ in 0..6 {
            after.pass().unwrap();
        }
    } else {
        b.c.close_project(b.project).unwrap();
        b.c.resume_project(b.project).unwrap();
        passes(&b.c, 6);
    }

    let old = b.c.message(legacy.id);
    let closed = b.c.message(tracked.id);
    let workers_new_pastes = b
        .events()
        .into_iter()
        .filter(|e| {
            e.id > from
                && e.kind == "delivery.begun"
                && e.data["message"]
                    .as_i64()
                    .is_some_and(|id| b.c.message(id).recipient_id == b.worker_id)
        })
        .map(|e| e.id)
        .collect::<Vec<_>>();
    let results = b.results();
    assert_eq!(closed.state, "cancelled");
    assert_ne!(old.state, "cancelled");
    assert!(workers_new_pastes.is_empty());
    assert_eq!(
        b.state(),
        "waiting",
        "the close's question-wait still needs new words"
    );
    assert!(
        results.is_empty(),
        "resuming must never take the old question turn as a result"
    );
}

#[test]
fn close_does_not_take_an_old_turn_as_a_result_while_an_untracked_question_survives() {
    keep_project_wait_if_an_old_question_survives(false);
}

#[test]
fn restart_does_not_take_an_old_turn_as_a_result_while_an_untracked_question_survives() {
    keep_project_wait_if_an_old_question_survives(true);
}

mod extras {
    use super::*;

    fn field() -> (Board, MessageView, MessageView, MessageView) {
        let b = Board::new(true);
        b.c.adapter.resumes_records.set(true);
        let (first, second) = b.asked_two();
        let answer = b.answer(second.id, "JSON");
        passes(&b.c, 2);
        assert_eq!(b.c.message(answer.id).state, "delivered");
        assert_eq!(b.state(), "waiting");
        (b, first, second, answer)
    }

    fn report(b: &Board) {
        for _ in 0..5 {
            b.c.adapter.answer("chief", "I read the board.");
            passes(&b.c, 2);
        }
    }

    fn open_legacy(b: &Board, first: &MessageView) {
        b.c.ledger
            .borrow_mut()
            .write_raw(&format!(
                "DELETE FROM question_binding WHERE question_id = {}",
                first.id
            ))
            .unwrap();
    }

    pub(super) fn pastes_since(b: &Board, mark: i64, who: i64) -> Vec<i64> {
        b.events()
            .into_iter()
            .filter(|e| e.id > mark && e.kind == "delivery.begun")
            .filter_map(|e| e.data["message"].as_i64())
            .filter(|&id| b.c.message(id).recipient_id == who)
            .collect()
    }

    #[test]
    fn a_second_paste_of_the_engines_own_go_on_note_is_told_too() {
        let (b, first, second, _answer) = field();
        let pane = b.c.dispatcher.pane(b.worker_id).unwrap();
        // The turn after the answer is told first; then the window rests for the go-on line.
        b.c.adapter.answer(&b.worker, "On it.");
        passes(&b.c, 3);
        assert_eq!(b.notes().len(), 1);
        let mark = b.events().last().unwrap().id;
        // The engine's own line that sends a worker on, which T-2329's finding 9 pasted over
        // the answer the old rule remembered.
        let note =
            b.c.ledger
                .borrow_mut()
                .note(
                    b.project,
                    &cf_ledger::NewNote {
                        from: None,
                        to: b.worker.clone(),
                        task: Some(1),
                        body: "ConsensFlow: your last turn ended to wait for your own work. Go on."
                            .to_owned(),
                    },
                )
                .unwrap();
        passes(&b.c, 3);
        assert_eq!(b.c.message(note.id).state, "delivered");
        assert_eq!(pastes_since(&b, mark, b.worker_id), vec![note.id]);
        assert_eq!(b.c.dispatcher.pane(b.worker_id), Some(pane));
        assert!(b.results().is_empty());
        b.c.adapter.answer(&b.worker, DONE);
        report(&b);
        b.nothing_taken(&[first.id]);
        assert_eq!(
            b.notes().len(),
            2,
            "the turn after the go-on line is told too"
        );
        b.take_whatever_was_told();
        b.delivered(&[first.id], &[second.id]);
    }

    fn with_staff(members: &[&str]) -> Board {
        let c = Context::new();
        let project = c.with_staff(members).id;
        c.pool_task(project, "worker", "Write the parser");
        passes(&c, 3);
        let worker = c.task(project, 1).task.assignee.unwrap();
        let worker_id = c.id(project, &worker);
        let chief_id = c.id(project, "chief");
        c.open_window(project, &worker).unwrap();
        Board {
            c,
            project,
            worker,
            worker_id,
            chief_id,
        }
    }

    fn other_window(members: &[&str]) {
        let b = with_staff(members);
        let first = b.ask("Which grammar?");
        let second = b.ask("Which format?");
        b.c.adapter.answer(&b.worker, "I asked the chief.");
        passes(&b.c, 2);
        let a = b.answer(second.id, "JSON");
        passes(&b.c, 2);
        assert_eq!(b.c.message(a.id).state, "delivered");
        let mark = b.events().last().unwrap().id;
        let old = b.c.dispatcher.pane(b.worker_id).unwrap();
        let task = b.c.pool_task(b.project, "worker", "Review the SQL").task;
        passes(&b.c, 4);
        let peer = b.c.task(b.project, task.number).task.assignee.unwrap();
        let peer_id = b.c.id(b.project, &peer);
        assert_ne!(peer_id, b.worker_id);
        assert_ne!(b.c.dispatcher.pane(peer_id).unwrap(), old);
        let peer_pastes = pastes_since(&b, mark, peer_id);
        assert!(!peer_pastes.is_empty());
        assert!(pastes_since(&b, mark, b.worker_id).is_empty());
        let staff = b.c.project(b.project).participants;
        let owner = |id| staff.iter().find(|p| p.id == id).unwrap().member_id;
        if members.len() == 1 {
            assert_eq!(owner(b.worker_id), owner(peer_id));
        } else {
            assert_ne!(owner(b.worker_id), owner(peer_id));
        }
        b.c.adapter.answer(&b.worker, DONE);
        report(&b);
        b.result(&[first.id], &[second.id]);
        assert_eq!(b.c.task(b.project, 2).task.state, "working");
    }

    #[test]
    fn a_paste_to_another_members_window_keeps_the_field_result() {
        other_window(&["zeus", "hera"]);
    }

    #[test]
    fn two_tasks_of_one_member_in_distinct_panes_keep_the_field_result() {
        other_window(&["zeus"]);
    }

    #[test]
    fn a_completed_close_and_resume_cannot_collect_the_old_turn() {
        let (b, first, _, _) = field();
        open_legacy(&b, &first);
        let before = b.events().last().unwrap().id;
        b.c.adapter.answer(&b.worker, DONE);
        let pane = b.c.dispatcher.pane(b.worker_id);
        b.c.close_project(b.project).unwrap();
        assert!(b.c.dispatcher.pane(b.worker_id).is_none());
        passes(&b.c, 3);
        assert!(b.results().is_empty());
        b.c.resume_project(b.project).unwrap();
        passes(&b.c, 6);
        report(&b);
        assert_ne!(pane, b.c.dispatcher.pane(b.worker_id));
        assert!(pastes_since(&b, before, b.worker_id).is_empty());
        assert!(b.results().is_empty());
    }

    #[test]
    fn closing_project_during_a_worker_observation_cannot_take_its_old_turn() {
        let (b, _first, _, _) = field();
        let launch = b.c.adapter.agent(&b.worker).launch;
        let gate =
            b.c.adapter
                .observe_holds
                .hold(move |argv| argv[0]["launch"]["launchId"] == launch.as_str());
        b.c.adapter.answer(&b.worker, DONE);
        let pass = b.c.begin_pass();
        b.c.settle();
        assert!(pass.take().is_none());
        let close = b.c.begin_close_project(b.project);
        b.c.settle();
        assert_eq!(b.c.project(b.project).state, "suspended");
        assert!(close.take().is_none());
        assert!(b.results().is_empty());
        gate.open();
        b.c.settle();
        pass.take().unwrap().unwrap();
        close.take().unwrap().unwrap();
        assert!(
            b.results().is_empty(),
            "collector accepted the old words after Close"
        );
        assert_eq!(b.notes().len(), 1, "the turn is told once");
    }

    fn kept_project(resume: bool) {
        let (b, _first, _, _) = field();
        let pane = b.c.dispatcher.pane(b.worker_id);
        b.c.adapter.answer(&b.worker, DONE);
        let before = b.events().last().unwrap().id;
        b.c.host.refuse_kills.set(true);
        assert!(b.c.close_project(b.project).is_err());
        assert_eq!(b.c.project(b.project).state, "suspended");
        assert_eq!(b.c.dispatcher.pane(b.worker_id), pane);
        assert!(b.results().is_empty());
        if resume {
            b.c.resume_project(b.project).unwrap();
        }
        passes(&b.c, 6);
        assert!(pastes_since(&b, before, b.worker_id).is_empty());
        assert!(
            b.results().is_empty(),
            "collector accepted the old words with no new paste"
        );
        assert_eq!(b.notes().len(), 1, "the turn is told once");
    }

    #[test]
    fn a_closed_project_with_a_window_the_host_could_not_close() {
        kept_project(false);
    }

    #[test]
    fn a_close_refused_by_the_host_then_resume_has_no_old_result() {
        kept_project(true);
    }

    fn finish_old(by: &str) {
        let (b, first, _, _) = field();
        open_legacy(&b, &first);
        let pane = b.c.dispatcher.pane(b.worker_id);
        let before = b.events().last().unwrap().id;
        b.c.adapter.answer(&b.worker, DONE);
        b.c.host
            .exits_showing(Some(9), None, Some(&["stand-in crash"]));
        b.c.exit(&b.worker);
        assert_eq!(b.state(), "paused");
        b.c.open_window(b.project, &b.worker).unwrap();
        passes(&b.c, 5);
        report(&b);
        assert_ne!(pane, b.c.dispatcher.pane(b.worker_id));
        assert!(pastes_since(&b, before, b.worker_id).is_empty());
        assert!(b.results().is_empty());
        assert_eq!(b.c.message(first.id).state, "delivered");
        let finish = b.c.finish_task(b.project, 1, by);
        report(&b);
        finish.expect(
            "Finish takes what the record holds, as on main: a person's word binds no opening",
        );
        let taken = b.results();
        assert_eq!(taken.len(), 1, "{taken:?}");
        assert!(taken[0].body.contains(DONE), "{taken:?}");
        assert_eq!(b.state(), "done");
    }

    #[test]
    fn chief_finish_with_an_open_legacy_question_cannot_take_pre_crash_words() {
        finish_old("chief");
    }

    #[test]
    fn drawer_finish_with_an_open_legacy_question_cannot_take_pre_crash_words() {
        finish_old("human");
    }

    #[test]
    fn after_restart_chief_and_human_see_the_wait_and_a_fresh_tell_moves_it_on() {
        let (b, first, _, _) = field();
        let legacy = b.ask("A still-open, pre-tracking question.");
        open_legacy(&b, &legacy);
        b.c.adapter.answer(&b.worker, "There is an open question.");
        passes(&b.c, 3);
        let before = b.events().last().unwrap().id;
        b.c.ledger.borrow_mut().suspend_for_restart().unwrap();
        let after = b.c.make();
        after.settle_after_restart().unwrap();
        after.resume_project(b.project).unwrap();
        for _ in 0..7 {
            after.pass().unwrap();
        }
        assert!(pastes_since(&b, before, b.worker_id).is_empty());
        assert!(b.results().is_empty());
        assert_eq!(b.state(), "waiting");
        assert_eq!(b.c.message(first.id).state, "cancelled");
        assert_ne!(b.c.message(legacy.id).state, "cancelled");
        let fresh =
            b.c.ledger
                .borrow_mut()
                .tell_queued(
                    b.project,
                    1,
                    "chief",
                    "Use the default and finish this run.",
                )
                .unwrap();
        for _ in 0..5 {
            after.pass().unwrap();
        }
        assert_eq!(b.c.message(fresh.id).state, "delivered");
        assert!(b.results().is_empty());
        b.c.adapter
            .answer(&b.worker, "Report written after the restart.");
        for _ in 0..7 {
            b.c.adapter.answer("chief", "I read the board.");
            after.pass().unwrap();
        }
        assert_eq!(b.state(), "waiting");
        assert!(b.results().is_empty());
        assert!(b
            .notes()
            .iter()
            .any(|n| n.body.contains("Report written after the restart.")));
        after.finish_task(b.project, 1, "chief").expect("Finish");
        for _ in 0..7 {
            b.c.adapter.answer("chief", "I read the board.");
            after.pass().unwrap();
        }
        assert_eq!(b.state(), "done");
        assert_eq!(b.results().len(), 1);
        assert_eq!(b.results()[0].state, "delivered");
        assert!(b.results()[0]
            .body
            .contains("Report written after the restart."));
        assert_eq!(b.c.message(legacy.id).state, "cancelled");
    }
}

mod follow_up {
    use super::*;
    use cf_harness::records::Role;

    #[test]
    fn a_later_answer_actually_pasted_with_an_older_resume_qualifies_the_final_report() {
        let b = Board::new(true);
        let (q1, q2) = b.asked_two();
        // The worker is still in the same physical window when the chief resumes its task.
        let pane = b.c.dispatcher.pane(b.worker_id).unwrap();
        b.c.pause_task(b.project, 1);
        passes(&b.c, 1);
        let resume =
            b.c.ledger
                .borrow_mut()
                .resume_task(b.project, 1, Some("chief"), "Continue on this run.")
                .unwrap()
                .message
                .unwrap();
        let q3 = b.ask("Which format version?");
        let answer = b.answer(q2.id, "Use version 3.");
        passes(&b.c, 5);
        let actual = b.c.message(answer.id);
        assert_eq!(b.c.message(resume.id).state, "delivered");
        assert_eq!(actual.state, "delivered");
        assert_eq!(actual.receipt["carrier"], resume.id);
        let events = b.events();
        let event = |kind: &str, id| {
            events
                .iter()
                .rfind(|e| e.kind == kind && e.data["message"].as_i64() == Some(id))
                .map(|e| e.id)
        };
        let asked = event("message.sent", q3.id).unwrap();
        let written = event("message.sent", answer.id).unwrap();
        let paste = event("delivery.begun", resume.id).unwrap();
        let rendered =
            b.c.adapter
                .agent(&b.worker)
                .items
                .into_iter()
                .filter(|i| i.role == Role::User && i.text.contains("Use version 3."))
                .map(|i| i.text.to_string())
                .collect::<Vec<_>>();
        assert_eq!(
            rendered.len(),
            1,
            "the answer must actually be present in the paste"
        );
        assert!(asked < written && written < paste);
        assert_eq!(b.c.dispatcher.pane(b.worker_id), Some(pane));
        assert_eq!(b.state(), "waiting");
        assert!(b.results().is_empty());
        assert!(!b
            .c
            .task(b.project, 1)
            .messages
            .iter()
            .any(|m| m.recipient_id == b.worker_id
                && matches!(m.state.as_str(), "queued" | "delivering" | "gated")));
        b.c.adapter.answer(&b.worker, DONE);
        for _ in 0..5 {
            b.c.adapter.answer("chief", "I read the board.");
            passes(&b.c, 2);
        }
        b.result(&[q1.id, q3.id], &[q2.id]);
    }

    #[test]
    fn explicit_finish_before_the_next_pass_needs_actual_words() {
        let b = Board::new(true);
        let (q1, q2) = b.asked_two();
        let a = b.answer(q2.id, "JSON");
        passes(&b.c, 2);
        assert_eq!(b.c.message(a.id).state, "delivered");
        let tool = b.c.adapter.item(Role::Tool, "Test command exited 0.");
        b.c.adapter
            .with(&b.worker, |worker| worker.items.push(tool));
        b.c.adapter.answer(&b.worker, "");
        assert!(b.results().is_empty());
        for _ in 0..4 {
            b.c.adapter.answer("chief", "I read the board.");
            passes(&b.c, 2);
        }
        // No words: nothing is told, nothing is taken, the question stays open.
        b.nothing_taken(&[q1.id]);
        assert!(b.notes().is_empty());
    }
}

mod openings {
    use super::*;
    use cf_harness::records::{Quota, Role};
    use cf_harness::seams::Time;
    fn established() -> (Board, MessageView, i64) {
        let b = Board::new(true);
        b.c.adapter.resumes_records.set(true);
        let (q, other) = b.asked_two();
        let answer = b.answer(other.id, "JSON");
        passes(&b.c, 2);
        assert_eq!(b.c.message(answer.id).state, "delivered");
        assert_eq!(b.c.message(q.id).state, "delivered");
        let event = b.events().last().unwrap().id;
        (b, q, event)
    }
    fn later_worker_pastes(b: &Board, from: i64) -> Vec<i64> {
        b.events()
            .into_iter()
            .filter(|e| e.id > from && e.kind == "delivery.begun")
            .filter_map(|e| e.data["message"].as_i64())
            .filter(|&id| b.c.message(id).recipient_id == b.worker_id)
            .collect()
    }
    fn keep_old_question(b: &Board, q: &MessageView) {
        b.c.ledger
            .borrow_mut()
            .write_raw(&format!(
                "DELETE FROM question_binding WHERE question_id = {}",
                q.id
            ))
            .unwrap();
    }

    fn held_in_place(legacy: bool) {
        let (b, q, from) = established();
        if legacy {
            keep_old_question(&b, &q);
        }
        let pane = b.c.dispatcher.pane(b.worker_id);
        let at = cf_base::time::iso(b.c.time.wall_ms());
        let until = cf_base::time::iso(b.c.time.wall_ms() + 7_200_000);
        b.c.adapter.refuse(
            &b.worker,
            Quota::Exhausted {
                at: Some(at),
                resets_at: Some(until.clone()),
            },
        );
        passes(&b.c, 3);
        assert_eq!(b.state(), "paused");
        assert_eq!(b.c.task(b.project, 1).task.held_until, Some(until));
        assert_eq!(b.c.dispatcher.pane(b.worker_id), pane);
        assert_ne!(b.c.message(q.id).state, "cancelled");
        b.c.advance(60_000);
        b.c.adapter.begins_its_own_turn(
            &b.worker,
            "Continue the parser work.",
            &cf_base::time::iso(b.c.time.wall_ms()),
        );
        b.c.advance(10_000);
        b.c.adapter
            .ends_its_own_turn(&b.worker, DONE, &cf_base::time::iso(b.c.time.wall_ms()));
        for _ in 0..5 {
            b.c.adapter.answer("chief", "I read the board.");
            passes(&b.c, 2);
        }
        let pastes = later_worker_pastes(&b, from);
        assert!(pastes.is_empty());
        assert!(
            b.results().is_empty(),
            "old pre-hold paste qualified a turn after a quota hold"
        );
        assert_eq!(b.notes().len(), 1, "the post-hold turn is told once");
    }

    #[test]
    fn a_real_quota_hold_with_a_kept_window_needs_a_new_paste() {
        held_in_place(false);
    }
    #[test]
    fn a_real_quota_hold_with_a_kept_window_and_legacy_question_needs_a_new_paste() {
        held_in_place(true);
    }

    #[test]
    fn the_tracked_field_case_after_restart_shows_the_chiefs_note_and_fresh_tell() {
        let (b, q, from) = established();
        b.c.adapter.answer(&b.worker, DONE);
        b.c.ledger.borrow_mut().suspend_for_restart().unwrap();
        let after = b.c.make();
        after.settle_after_restart().unwrap();
        assert_eq!(b.c.project(b.project).state, "suspended");
        assert!(b.results().is_empty());
        after.resume_project(b.project).unwrap();
        for _ in 0..6 {
            b.c.adapter.answer("chief", "I read the board.");
            after.pass().unwrap();
        }
        let chief_notices: Vec<_> =
            b.c.inbox(b.chief_id)
                .into_iter()
                .filter(|m| m.kind == "note" && m.body.contains("restarted"))
                .map(|m| (m.state, m.body))
                .collect();
        assert!(chief_notices
            .iter()
            .any(|(state, body)| state == "delivered" && body.contains("cf tell T-1")));
        assert_eq!(b.state(), "waiting");
        assert!(b.results().is_empty());
        assert_eq!(b.c.message(q.id).state, "cancelled");
        assert!(later_worker_pastes(&b, from).is_empty());
        let told =
            b.c.ledger
                .borrow_mut()
                .tell_queued(b.project, 1, "chief", "Please check once more and report.")
                .unwrap();
        for _ in 0..5 {
            after.pass().unwrap();
        }
        assert_eq!(b.c.message(told.id).state, "delivered");
        assert!(b.results().is_empty());
        b.c.adapter
            .answer(&b.worker, "Fresh report from the new opening.");
        for _ in 0..7 {
            b.c.adapter.answer("chief", "I read the board.");
            after.pass().unwrap();
        }
        assert_eq!(b.results().len(), 1);
        assert_eq!(b.results()[0].state, "delivered");
        assert!(b.results()[0]
            .body
            .contains("Fresh report from the new opening."));
    }

    fn restart_finish(by: &str) {
        let (b, q, from) = established();
        keep_old_question(&b, &q);
        b.c.adapter.answer(&b.worker, DONE);
        b.c.ledger.borrow_mut().suspend_for_restart().unwrap();
        let after = b.c.make();
        after.settle_after_restart().unwrap();
        after.resume_project(b.project).unwrap();
        for _ in 0..6 {
            b.c.adapter.answer("chief", "I read the board.");
            after.pass().unwrap();
        }
        assert!(
            after.pane(b.worker_id).is_some(),
            "the automatic Resume opened this worker's pane"
        );
        assert_eq!(b.c.message(q.id).state, "delivered");
        assert!(later_worker_pastes(&b, from).is_empty());
        assert!(b.results().is_empty());
        let finish = after.finish_task(b.project, 1, by);
        for _ in 0..6 {
            b.c.adapter.answer("chief", "I read the board.");
            after.pass().unwrap();
        }
        finish.expect(
            "Finish takes what the record holds, as on main: a person's word binds no opening",
        );
        let taken = b.results();
        assert_eq!(taken.len(), 1, "{taken:?}");
        assert!(taken[0].body.contains(DONE), "{taken:?}");
        assert_eq!(b.state(), "done");
    }
    #[test]
    fn chief_finish_after_daemon_restart_does_not_publish_pre_restart_words() {
        restart_finish("chief");
    }
    #[test]
    fn drawer_finish_after_daemon_restart_does_not_publish_pre_restart_words() {
        restart_finish("human");
    }

    #[test]
    fn a_turn_with_no_words_is_not_told_and_not_taken() {
        let (b, q, _) = established();
        let tool = b.c.adapter.item(Role::Tool, "Test command exited 0.");
        b.c.adapter.with(&b.worker, |a| a.items.push(tool));
        b.c.adapter.answer(&b.worker, "");
        passes(&b.c, 4);
        for _ in 0..4 {
            b.c.adapter.answer("chief", "I read the board.");
            passes(&b.c, 2);
        }
        b.nothing_taken(&[q.id]);
        assert!(b.notes().is_empty());
    }
}

mod normal {
    use super::*;
    use cf_ledger::NewTask;
    #[test]
    fn a_followup_in_the_same_named_session_gets_its_own_field_result() {
        let b = Board::new(true);
        let (q1, q2) = b.asked_two();
        b.answer(q2.id, "JSON");
        passes(&b.c, 2);
        b.finish();
        b.result(&[q1.id], &[q2.id]);
        b.c.accept_if_done(b.project, 1);
        let old = b.c.dispatcher.pane(b.worker_id);
        b.c.host.exits_showing(Some(0), None, None);
        b.c.adapter.resumes_records.set(true);
        let created = b.c.create_task(
            b.project,
            NewTask {
                from: "chief".into(),
                after: Some(1),
                body: "Review the parser.".into(),
                ..NewTask::default()
            },
        );
        let n = created.task.number;
        assert_eq!(n, 2);
        passes(&b.c, 5);
        assert_eq!(
            b.c.task(b.project, n).task.state,
            "queued",
            "the earlier window is still pinned"
        );
        b.c.hide_window(b.project, &b.worker).unwrap();
        passes(&b.c, 7);
        assert_ne!(b.c.dispatcher.pane(b.worker_id), old);
        let task = b.c.task(b.project, n).task;
        assert_eq!(task.state, "working");
        assert_eq!(task.assignee.as_deref(), Some(b.worker.as_str()));
        let first = b.c.ask(b.project, &b.worker, "chief", n, "Which sample?");
        let second = b.c.ask(b.project, &b.worker, "chief", n, "Which version?");
        b.c.adapter.answer(&b.worker, "I asked the chief.");
        passes(&b.c, 2);
        let answer = b.c.answer(b.project, second.id, "Version 2.");
        passes(&b.c, 3);
        assert_eq!(b.c.message(answer.id).state, "delivered");
        b.c.adapter
            .answer(&b.worker, "The second review is complete.");
        for _ in 0..6 {
            b.c.adapter.answer("chief", "I read the board.");
            passes(&b.c, 2);
        }
        assert_eq!(b.c.task(b.project, n).task.state, "waiting");
        b.c.finish_task(b.project, n, "chief").expect("Finish");
        for _ in 0..6 {
            b.c.adapter.answer("chief", "I read the board.");
            passes(&b.c, 2);
        }
        let thread = b.c.task(b.project, n);
        let results: Vec<_> = thread
            .messages
            .iter()
            .filter(|m| m.kind == "result")
            .collect();
        assert_eq!(thread.task.state, "done");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].state, "delivered");
        assert!(results[0].body.contains("The second review is complete."));
        assert_eq!(b.c.message(first.id).state, "cancelled");
    }

    fn restart(by: &str) {
        let b = Board::new(true);
        b.c.adapter.resumes_records.set(true);
        let (first, second) = b.asked_two();
        let a = b.answer(second.id, "JSON");
        passes(&b.c, 2);
        assert_eq!(b.c.message(a.id).state, "delivered");
        let mark = b.events().last().unwrap().id;
        b.c.adapter.answer(&b.worker, DONE);
        b.c.ledger.borrow_mut().suspend_for_restart().unwrap();
        let after = b.c.make();
        after.settle_after_restart().unwrap();
        after.resume_project(b.project).unwrap();
        for _ in 0..7 {
            b.c.adapter.answer("chief", "I read the board.");
            after.pass().unwrap();
        }
        assert_eq!(b.state(), "waiting");
        assert!(b.results().is_empty());
        let notices =
            b.c.inbox(b.chief_id)
                .into_iter()
                .filter(|m| {
                    m.kind == "note"
                        && m.state == "delivered"
                        && m.body.contains("ConsensFlow restarted")
                })
                .map(|m| m.body)
                .collect::<Vec<_>>();
        assert!(notices
            .iter()
            .any(|body| body.contains("cf task finish T-1")));
        assert_eq!(b.c.message(first.id).state, "cancelled");
        let pastes = b
            .events()
            .into_iter()
            .filter(|e| e.id > mark && e.kind == "delivery.begun")
            .filter_map(|e| e.data["message"].as_i64())
            .filter(|&id| b.c.message(id).recipient_id == b.worker_id)
            .collect::<Vec<_>>();
        assert!(pastes.is_empty());
        let finish = after.finish_task(b.project, 1, by);
        for _ in 0..7 {
            b.c.adapter.answer("chief", "I read the board.");
            after.pass().unwrap();
        }
        finish.expect(
            "Finish takes what the record holds, as on main: a person's word binds no opening",
        );
        let taken = b.results();
        assert_eq!(taken.len(), 1, "{taken:?}");
        assert!(taken[0].body.contains(DONE), "{taken:?}");
        assert_eq!(b.state(), "done");
    }
    #[test]
    fn the_recovery_note_offers_the_chief_finish_on_the_old_report() {
        restart("chief");
    }
    #[test]
    fn the_drawers_finish_can_reach_the_same_old_report_without_a_fresh_paste() {
        restart("human");
    }
}

/// T-2268's probes of findings 6 to 8: the routes that took a stored turn as the result.
mod reviewed {
    use super::*;
    use cf_harness::records::{Quota, Role};
    use cf_harness::seams::Time;

    fn project_mark(b: &Board) -> Option<i64> {
        b.events()
            .iter()
            .rfind(|e| e.kind == "project.state")
            .map(|e| e.id)
    }

    fn worker_pastes_since(b: &Board, from: i64) -> Vec<i64> {
        b.events()
            .into_iter()
            .filter(|e| e.id > from && e.kind == "delivery.begun")
            .filter_map(|e| e.data["message"].as_i64())
            .filter(|&id| b.c.message(id).recipient_id == b.worker_id)
            .collect()
    }

    fn tell_and_paste(b: &Board, body: &str) -> MessageView {
        let m =
            b.c.ledger
                .borrow_mut()
                .tell_queued(b.project, 1, "chief", body)
                .unwrap();
        passes(&b.c, 2);
        assert_eq!(b.c.message(m.id).state, "delivered");
        m
    }

    fn drain_chief(b: &Board) {
        for _ in 0..4 {
            b.c.adapter.answer("chief", "I read the board.");
            passes(&b.c, 2);
        }
    }

    #[test]
    fn physical_crash_show_and_task_resume_require_the_new_turn() {
        let b = Board::new(true);
        b.c.adapter.resumes_records.set(true);
        let (first, second) = b.asked_two();
        b.c.ledger
            .borrow_mut()
            .write_raw(&format!(
                "DELETE FROM question_binding WHERE question_id = {}",
                first.id
            ))
            .unwrap();
        let prior = b.answer(second.id, "JSON");
        passes(&b.c, 2);
        assert_eq!(b.c.message(prior.id).state, "delivered");
        assert_eq!(b.state(), "waiting");
        let old_pane = b.c.host.last("zeus").unwrap().pane;
        let mark = project_mark(&b);
        b.c.adapter.answer("zeus", DONE);
        let final_id = b.c.adapter.agent("zeus").items.last().unwrap().id.clone();
        let before = b.events().last().unwrap().id;

        b.c.host
            .exits_showing(Some(9), None, Some(&["stand-in crash"]));
        b.c.exit("zeus");
        let state_after_exit = b.state();
        assert_eq!(state_after_exit, "paused");
        assert!(b.results().is_empty());

        b.c.open_window(b.project, &b.worker).unwrap();
        passes(&b.c, 5);
        let new_pane = b.c.host.last("zeus").unwrap().pane;
        assert_ne!(new_pane, old_pane);
        assert_eq!(project_mark(&b), mark);
        assert!(b
            .c
            .adapter
            .agent("zeus")
            .items
            .iter()
            .any(|item| item.id == final_id));
        assert_eq!(b.c.message(first.id).state, "delivered");
        assert_eq!(b.state(), "paused");
        assert!(b.results().is_empty());
        let before_resume = worker_pastes_since(&b, before);
        assert!(before_resume.is_empty());

        b.c.resume_task(b.project, 1, "Finish on the new run.");
        passes(&b.c, 5);
        let resumed = worker_pastes_since(&b, before);
        assert_eq!(resumed.len(), 1, "{resumed:?}");
        assert_eq!(b.c.message(resumed[0]).state, "delivered");
        assert_eq!(b.state(), "waiting");
        assert!(b.results().is_empty());

        b.c.adapter.answer("zeus", "Completed on the new run.");
        passes(&b.c, 4);
        b.nothing_taken(&[first.id]);
        assert_eq!(b.notes().len(), 1);
        b.take();
        passes(&b.c, 4);
        assert_eq!(b.state(), "done");
        let results = b.results();
        assert_eq!(results.len(), 1);
        assert!(results[0].body.contains("Completed on the new run."));
        assert_eq!(b.c.message(first.id).state, "cancelled");
    }

    #[test]
    fn fresh_conversation_needs_a_paste_of_its_own() {
        let b = Board::new(true);
        let (first, second) = b.asked_two();
        b.answer(second.id, "JSON");
        passes(&b.c, 2);
        let mark = project_mark(&b);
        b.c.adapter.answer("zeus", DONE);
        let before = b.events().last().unwrap().id;
        b.c.adapter.switch_to("zeus", "native-review-fresh");
        passes(&b.c, 5);
        assert_eq!(project_mark(&b), mark);
        assert!(b
            .c
            .adapter
            .agent("zeus")
            .items
            .iter()
            .all(|item| !item.text.contains(DONE)));
        b.c.adapter
            .answer("zeus", "This turn answers only the new conversation.");
        passes(&b.c, 4);
        assert_eq!(b.state(), "waiting");
        assert!(b.results().is_empty());
        assert_eq!(b.c.message(first.id).state, "delivered");
        let new_words = tell_and_paste(&b, "Use JSON on the current conversation.");
        b.finish();
        assert_eq!(
            b.notes().len(),
            2,
            "the turn of the new conversation, then the turn after the tell"
        );
        b.take_whatever_was_told();
        b.delivered(&[first.id], &[second.id]);
        let pastes = worker_pastes_since(&b, before);
        assert!(pastes.contains(&new_words.id));
    }

    #[test]
    fn returning_to_a_conversation_does_not_publish_the_stored_turn() {
        let b = Board::new(true);
        let (_first, second) = b.asked_two();
        b.answer(second.id, "JSON");
        passes(&b.c, 2);
        let mark = project_mark(&b);
        let previous = b.c.adapter.agent("zeus").native;
        b.c.adapter.answer("zeus", DONE);
        let final_id = b.c.adapter.agent("zeus").items.last().unwrap().id.clone();
        let from = b.events().last().unwrap().id;
        b.c.adapter.switch_to("zeus", "native-review-fresh");
        passes(&b.c, 5);
        assert!(b.results().is_empty());
        b.c.adapter.switch_to("zeus", &previous);
        passes(&b.c, 5);
        assert_eq!(project_mark(&b), mark);
        drain_chief(&b);
        let pastes = worker_pastes_since(&b, from);
        let result = b.results();
        assert!(b
            .c
            .adapter
            .agent("zeus")
            .items
            .iter()
            .any(|item| item.id == final_id));
        let task_pastes: Vec<_> = pastes
            .into_iter()
            .filter(|&id| b.c.message(id).task_number == Some(1))
            .collect();
        assert!(
            task_pastes.is_empty(),
            "task received a new paste: {task_pastes:?}"
        );
        assert!(result.is_empty());
    }

    #[test]
    fn an_empty_following_turn_does_not_close_an_open_question() {
        let b = Board::new(true);
        let (_first, second) = b.asked_two();
        b.answer(second.id, "JSON");
        passes(&b.c, 2);
        assert_eq!(b.state(), "waiting");
        let tool_result = b.c.adapter.item(Role::Tool, "Test command exited 0.");
        b.c.adapter
            .with("zeus", |agent| agent.items.push(tool_result));
        b.c.adapter.answer("zeus", "");
        passes(&b.c, 4);
        drain_chief(&b);
        let result = b.results();
        assert!(
            result.is_empty(),
            "the new path requires the worker to end with words"
        );
        assert_eq!(b.state(), "waiting");
    }

    #[test]
    fn a_new_pane_of_a_held_task_does_not_take_its_previous_panes_turn() {
        let b = Board::new(true);
        b.c.adapter.resumes_records.set(true);
        let (first, second) = b.asked_two();
        b.c.ledger
            .borrow_mut()
            .write_raw(&format!(
                "DELETE FROM question_binding WHERE question_id = {}",
                first.id
            ))
            .unwrap();
        let prior = b.answer(second.id, "JSON");
        passes(&b.c, 2);
        assert_eq!(b.c.message(prior.id).state, "delivered");
        let mark = project_mark(&b);
        let old_pane = b.c.host.last("zeus").unwrap().pane;
        let from = b.events().last().unwrap().id;

        // User input can start an untracked turn in a worker's own window.
        let user = b.c.adapter.item(Role::User, "Continue the parser work.");
        b.c.adapter.with("zeus", |agent| agent.items.push(user));
        b.c.adapter.answer("zeus", DONE);
        b.c.hold_task(b.project, 1);
        b.c.exit("zeus");
        assert_eq!(b.state(), "paused");
        assert!(b.results().is_empty());
        b.c.open_window(b.project, &b.worker).unwrap();
        passes(&b.c, 5);
        drain_chief(&b);

        let new_pane = b.c.host.last("zeus").unwrap().pane;
        assert_ne!(new_pane, old_pane);
        assert_eq!(mark, project_mark(&b));
        let pastes = worker_pastes_since(&b, from);
        let result = b.results();
        assert!(pastes.is_empty(), "{pastes:?}");
        assert!(result.is_empty());
    }

    #[test]
    fn a_real_hold_route_and_show_do_not_take_the_pre_exit_turn() {
        let b = Board::new(true);
        b.c.adapter.resumes_records.set(true);
        let (first, second) = b.asked_two();
        b.c.ledger
            .borrow_mut()
            .write_raw(&format!(
                "DELETE FROM question_binding WHERE question_id = {}",
                first.id
            ))
            .unwrap();
        let prior = b.answer(second.id, "JSON");
        passes(&b.c, 2);
        assert_eq!(b.c.message(prior.id).state, "delivered");
        let before_pane = b.c.host.last("zeus").unwrap().pane;

        let at = cf_base::time::iso(b.c.time.wall_ms());
        let resets_at = cf_base::time::iso(b.c.time.wall_ms() + 7_200_000);
        b.c.adapter.refuse(
            "zeus",
            Quota::Exhausted {
                at: Some(at),
                resets_at: Some(resets_at.clone()),
            },
        );
        passes(&b.c, 2);
        let held = b.c.task(b.project, 1).task;
        assert_eq!(held.state, "paused");
        assert_eq!(held.held_until.as_deref(), Some(resets_at.as_str()));
        assert!(b.results().is_empty());

        b.c.advance(60_000);
        let began = cf_base::time::iso(b.c.time.wall_ms());
        b.c.adapter
            .begins_its_own_turn("zeus", "Continue the parser work.", &began);
        b.c.advance(10_000);
        let ended = cf_base::time::iso(b.c.time.wall_ms());
        b.c.adapter.ends_its_own_turn("zeus", DONE, &ended);
        let from = b.events().last().unwrap().id;
        b.c.host
            .exits_showing(Some(9), None, Some(&["stand-in crash"]));
        b.c.exit("zeus");
        assert_eq!(b.state(), "paused");
        assert!(b.results().is_empty());
        b.c.open_window(b.project, &b.worker).unwrap();
        passes(&b.c, 5);
        drain_chief(&b);

        assert_ne!(b.c.host.last("zeus").unwrap().pane, before_pane);
        let events = b.events();
        let pastes = events
            .iter()
            .filter(|e| e.id > from && e.kind == "delivery.begun")
            .filter_map(|e| e.data["message"].as_i64())
            .filter(|&id| b.c.message(id).recipient_id == b.worker_id)
            .collect::<Vec<_>>();
        let result = b.results();
        assert!(pastes.is_empty(), "{pastes:?}");
        assert!(
            result.is_empty(),
            "a new pane must not collect the historical turn"
        );
    }

    #[test]
    fn a_paste_written_before_the_last_unanswered_question_does_not_qualify() {
        let b = Board::new(false);
        b.c.adapter.busy("zeus");
        let first = b.ask("Which grammar?");
        passes(&b.c, 1);
        let answer = b.answer(first.id, "PEG");
        passes(&b.c, 1);
        assert_eq!(b.c.message(answer.id).state, "queued");
        let second = b.ask("Which format?");
        passes(&b.c, 1);
        b.c.adapter.answer("zeus", "I asked which format.");
        passes(&b.c, 2);
        assert_eq!(b.c.message(answer.id).state, "delivered");
        let events = b.events();
        let event = |kind, message| {
            events
                .iter()
                .find(|e| e.kind == kind && e.data["message"].as_i64() == Some(message))
                .unwrap()
                .id
        };
        let written = event("message.sent", answer.id);
        let asked = event("message.sent", second.id);
        let pasted = event("delivery.begun", answer.id);
        assert!(written < asked && asked < pasted);
        b.finish();
        drain_chief(&b);
        let result = b.results();
        assert!(
            result.is_empty(),
            "the brief says the message must also have been written after the open question"
        );
    }
}

/// The note is once for a turn, not once for a paste (T-2340's findings 19 to 23).
mod turns {
    use super::extras::pastes_since;
    use super::*;
    use cf_harness::seams::Time;

    fn field() -> (Board, MessageView) {
        let b = Board::new(true);
        b.c.adapter.resumes_records.set(true);
        let (q, other) = b.asked_two();
        b.answer(other.id, "JSON");
        passes(&b.c, 3);
        (b, q)
    }

    fn own_turn(b: &Board, command: &str, words: &str) {
        b.c.advance(10_000);
        b.c.adapter.begins_its_own_turn(
            &b.worker,
            command,
            &cf_base::time::iso(b.c.time.wall_ms()),
        );
        b.c.advance(10_000);
        b.c.adapter
            .ends_its_own_turn(&b.worker, words, &cf_base::time::iso(b.c.time.wall_ms()));
    }

    /// Two turns that ended separately, the second begun by the harness itself with no paste of
    /// ConsensFlow's between them, are told once each.
    #[test]
    fn two_separate_turns_without_a_paste_between_are_each_told_once() {
        let (b, q) = field();
        b.c.adapter
            .answer(&b.worker, "I will go on when the build reports.");
        passes(&b.c, 4);
        assert_eq!(b.notes().len(), 1);
        let mark = b.events().last().unwrap().id;
        own_turn(&b, "The build reported.", DONE);
        passes(&b.c, 4);
        assert_eq!(pastes_since(&b, mark, b.worker_id), Vec::<i64>::new());
        b.nothing_taken(&[q.id]);
        let told = b.notes();
        assert_eq!(told.len(), 2, "{told:?}");
        assert!(told[1].body.starts_with(DONE), "{told:?}");
        passes(&b.c, 4);
        assert_eq!(b.notes().len(), 2, "a second look tells nothing more");
        b.take_whatever_was_told();
        b.delivered(&[q.id], &[]);
    }

    /// A note whose delivery failed for good does not stop the next turn's note.
    #[test]
    fn a_failed_note_does_not_stop_the_next_turns_note() {
        let (b, q) = field();
        b.c.adapter.busy("chief");
        b.c.adapter
            .answer(&b.worker, "I will go on when the build reports.");
        passes(&b.c, 3);
        let first = b.notes();
        assert_eq!(first.len(), 1);
        let id = first[0].id;
        b.c.ledger.borrow_mut().begin_delivery(id).unwrap();
        b.c.ledger
            .borrow_mut()
            .fail_delivery(id, "test pane rejected the note")
            .unwrap();
        own_turn(&b, "The build reported.", DONE);
        passes(&b.c, 4);
        let told = b.notes();
        assert_eq!(told.len(), 2, "{told:?}");
        assert_eq!(b.c.message(id).state, "failed");
        b.nothing_taken(&[q.id]);
    }

    /// An ordinary note of the worker that quotes the report's words is not the report's note.
    #[test]
    fn an_ordinary_note_quoting_the_phrase_does_not_consume_the_report() {
        let (b, q) = field();
        b.c.ledger
            .borrow_mut()
            .note(
                b.project,
                &cf_ledger::NewNote {
                    from: Some(b.worker.clone()),
                    to: "human".into(),
                    task: Some(1),
                    body:
                        "I am documenting the phrase turn ended on these words with its question."
                            .into(),
                },
            )
            .unwrap();
        b.c.adapter.answer(&b.worker, DONE);
        passes(&b.c, 4);
        b.nothing_taken(&[q.id]);
        assert_eq!(b.notes().len(), 1);
    }

    /// After a switch to a fresh conversation the words cannot be taken by Finish: the note
    /// says so, and does not offer it.
    #[test]
    fn a_note_from_a_fresh_conversation_does_not_offer_finish() {
        let (b, q) = field();
        b.c.adapter
            .answer(&b.worker, "Earlier report, not the new one.");
        passes(&b.c, 3);
        b.c.adapter
            .switch_to(&b.worker, "native-fresh-for-the-note");
        passes(&b.c, 4);
        b.c.adapter.answer(&b.worker, DONE);
        passes(&b.c, 4);
        b.nothing_taken(&[q.id]);
        let told = b.notes();
        let last = told.last().expect("a note of the fresh conversation");
        assert!(last.body.starts_with(DONE), "{told:?}");
        assert!(!last.body.contains("If they are its result"), "{last:?}");
        assert!(last.body.contains("another conversation"), "{last:?}");
    }

    /// A second tell queued while the worker is still busy: its turn is told too.
    #[test]
    fn a_second_tell_queued_while_the_worker_is_busy_is_told_in_its_turn() {
        let (b, q) = field();
        b.c.adapter.answer(&b.worker, "Checking the grammar.");
        passes(&b.c, 3);
        assert_eq!(b.notes().len(), 1);
        b.c.adapter.busy(&b.worker);
        let tell =
            b.c.ledger
                .borrow_mut()
                .tell_queued(b.project, 1, "chief", "Use the revised grammar.")
                .unwrap();
        passes(&b.c, 2);
        assert_eq!(b.c.message(tell.id).state, "queued");
        b.c.adapter
            .answer(&b.worker, "Checked the revised grammar.");
        passes(&b.c, 4);
        assert_eq!(b.c.message(tell.id).state, "delivered");
        b.c.adapter.answer(&b.worker, DONE);
        passes(&b.c, 4);
        b.nothing_taken(&[q.id]);
        assert_eq!(b.notes().len(), 3, "{:?}", b.notes());
    }
}

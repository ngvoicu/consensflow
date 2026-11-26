//! A turn's words are a result with a question open only if they come of a message pasted in
//! this same opening of the window that was written after every open question (T-2197,
//! rounds 4; the probes of T-2268's findings 6 to 8 as tests).

use cf_engine::testing::Context;
use cf_harness::records::{Quota, Role};
use cf_harness::seams::Time;
use cf_ledger::MessageView;
use cf_proto::ledger::EventView;
use serde_json::json;

const DONE: &str = "The requested parser and tests are complete.";

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
        eprintln!("task=done result=m-{} state=delivered begins=1 confirmations=1 closed={closed:?} answered={answered:?}", delivered.id);
        delivered.clone()
    }
    fn finish(&self) {
        self.c.adapter.answer("zeus", DONE);
        passes(&self.c, 2);
    }
}
fn passes(context: &Context, count: usize) {
    for _ in 0..count {
        context.pass().unwrap();
    }
}

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
    eprintln!("worker crashed: task={state_after_exit}; Show: task={}, previous final item {final_id} retained, project state marker unchanged at {mark:?}, worker pastes={before_resume:?}, result rows={}", b.state(), b.results().len());
    assert!(before_resume.is_empty());

    b.c.resume_task(b.project, 1, "Finish on the new run.");
    passes(&b.c, 5);
    let resumed = worker_pastes_since(&b, before);
    assert_eq!(resumed.len(), 1, "{resumed:?}");
    assert_eq!(b.c.message(resumed[0]).state, "delivered");
    eprintln!(
        "task Resume: worker pastes={resumed:?}, task={}, result rows={}",
        b.state(),
        b.results().len()
    );
    assert_eq!(b.state(), "waiting");
    assert!(b.results().is_empty());

    b.c.adapter.answer("zeus", "Completed on the new run.");
    passes(&b.c, 4);
    assert_eq!(b.state(), "done");
    let results = b.results();
    assert_eq!(results.len(), 1);
    assert!(results[0].body.contains("Completed on the new run."));
    assert_eq!(b.c.message(first.id).state, "cancelled");
    eprintln!(
        "fresh turn: task=done, result begins {:?}, old question m-{} closed",
        results[0].body.lines().next(),
        first.id
    );
}

#[test]
fn fresh_conversation_needs_a_paste_of_its_own() {
    let b = Board::new(true);
    let (first, second) = b.asked_two();
    b.answer(second.id, "JSON");
    passes(&b.c, 2);
    let mark = project_mark(&b);
    let old_session = b.c.adapter.agent("zeus").native;
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
    eprintln!(
        "fresh conversation from {old_session}: project marker={mark:?}, task={}, result rows={}",
        b.state(),
        b.results().len()
    );
    let new_words = tell_and_paste(&b, "Use JSON on the current conversation.");
    b.finish();
    b.result(&[first.id], &[second.id]);
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
    eprintln!("returned to {previous}: old final item {final_id}, project marker={mark:?}, worker pastes={pastes:?}, task={}, results={:?}", b.state(), result.iter().map(|m| (&m.state, &m.body)).collect::<Vec<_>>());
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
    let (first, second) = b.asked_two();
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
    eprintln!(
        "empty complete turn: question m-{} {}, task={}, result={:?}",
        first.id,
        b.c.message(first.id).state,
        b.state(),
        result
            .iter()
            .map(|m| (&m.state, &m.body))
            .collect::<Vec<_>>()
    );
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
    eprintln!(
        "held worker reopened with Show: worker pastes={pastes:?}, task={}, results={:?}",
        b.state(),
        result
            .iter()
            .map(|m| (&m.state, &m.body))
            .collect::<Vec<_>>()
    );
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
    let old_final = b.c.adapter.agent("zeus").items.last().unwrap().id.clone();
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
    eprintln!("ended {ended} item {old_final} before worker crash, hold {resets_at}, project events since final={:?}, new worker pastes={pastes:?}, task={}, question m-{} {}, results={:?}",
        events.iter().filter(|e| e.id > from && e.kind == "project.state").map(|e| e.id).collect::<Vec<_>>(),
        b.state(), first.id, b.c.message(first.id).state,
        result.iter().map(|m| (&m.state, &m.body)).collect::<Vec<_>>());
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
    eprintln!("answer wrote at {written}, open question written at {asked}, answer pasted at {pasted}, results={:?}",
        result.iter().map(|m| (&m.state, &m.body)).collect::<Vec<_>>());
    assert!(
        result.is_empty(),
        "the brief says the message must also have been written after the open question"
    );
}

/// The field report's own case: two questions, one answered after both were asked; the worker's
/// final report is the result, delivered once, and the open question closed.
#[test]
fn field_second_answer_then_result_is_delivered() {
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
            .answer(first.id, b.chief_id, Some(&json!("Late")), None);
    assert!(late.unwrap_err().to_string().contains("cf task reopen T-1"));
}

//! What the tests share: a project at a clock the test moves, and the few
//! things a test does to it (turn the calls on, ask what is due, deliver a
//! wake-up, give a task and take it through its states, type to the chief).

use std::cell::Cell;
use std::rc::Rc;

use cf_base::time::Clock;
use cf_ledger::{
    open_ledger, Ledger, MessageView, NewChief, NewMember, NewNote, NewProject, NewQuestion,
    NewTask, NewWakeUp, Options, ProjectView, TaskCreated, WakeUpUnit,
};
use serde_json::{json, Value};

/// 2026-09-19T12:00:00.000Z, where every test's clock starts.
pub const START: i64 = 1_789_819_200_000;
pub const MINUTE: i64 = 60_000;
pub const HOUR: i64 = 60 * MINUTE;
pub const DAY: i64 = 24 * HOUR;

/// The ledger's clock: the cell the test moves.
struct Moving(Rc<Cell<i64>>);

impl Clock for Moving {
    fn now_ms(&mut self) -> i64 {
        self.0.get()
    }
}

pub struct World {
    pub ledger: Ledger,
    pub project: ProjectView,
    now: Rc<Cell<i64>>,
    // Last, so the file goes after the ledger.
    _dir: tempfile::TempDir,
}

/// A project at /work/app: apollo its chief, zeus a worker.
pub fn world() -> World {
    let dir = tempfile::tempdir().expect("a home");
    let now = Rc::new(Cell::new(START));
    let mut ledger = open_ledger(
        &dir.path().join("consensflow.db"),
        Options {
            clock: Box::new(Moving(Rc::clone(&now))),
            ..Options::default()
        },
    )
    .expect("a ledger");
    let project = ledger
        .create_project(&NewProject {
            directory: "/work/app".to_owned(),
            name: "app".to_owned(),
            chief: NewChief {
                harness: "claude-code".to_owned(),
                agent: Some("apollo".to_owned()),
            },
            staff: vec![NewMember {
                agent: "zeus".to_owned(),
                harness: "claude-code".to_owned(),
                designer: false,
                roles: vec!["worker".to_owned()],
                tier: "standard".to_owned(),
            }],
            gate: false,
            wake_up: None,
            specs: None,
        })
        .expect("a project");
    World {
        ledger,
        project,
        now,
        _dir: dir,
    }
}

/// The calls as the human sets them: `on`, `every` of `unit`, the default words.
pub fn calls(on: bool, every: i64, unit: WakeUpUnit) -> NewWakeUp {
    NewWakeUp {
        on,
        every,
        unit,
        text: None,
    }
}

impl World {
    /// The clock moves on.
    pub fn advance(&self, ms: i64) {
        self.now.set(self.now.get() + ms);
    }

    pub fn now(&self) -> i64 {
        self.now.get()
    }

    pub fn id(&self, handle: &str) -> i64 {
        self.project
            .participants
            .iter()
            .find(|participant| participant.handle == handle)
            .map(|participant| participant.id)
            .unwrap_or_else(|| panic!("no @{handle}"))
    }

    /// The human turns the calls on, `every` of `unit` apart.
    pub fn turn_on(&mut self, every: i64, unit: WakeUpUnit) {
        self.set(&calls(true, every, unit));
    }

    pub fn set(&mut self, request: &NewWakeUp) {
        self.ledger
            .set_wake_up(self.project.id, request)
            .expect("the setting");
    }

    /// What the ledger queues now, if a wake-up is due.
    pub fn due(&mut self) -> Option<MessageView> {
        self.ledger
            .queue_wake_up_if_due(self.project.id, self.now())
            .expect("the ask")
    }

    /// The wake-up reaches the chief's window: begun and confirmed.
    pub fn deliver(&mut self, message: &MessageView) {
        self.ledger.begin_delivery(message.id).expect("begun");
        self.ledger
            .confirm_delivery(message.id, Some(&json!({ "item": "i-1" })))
            .expect("confirmed");
    }

    /// A wake-up is due, and is delivered at once.
    pub fn wake_and_deliver(&mut self) -> MessageView {
        let sent = self.due().expect("a wake-up is due");
        self.deliver(&sent);
        sent
    }

    /// The chief gives zeus a task, waiting for the tasks numbered `needs` to be
    /// accepted: queued for its window, or open on the board behind them.
    pub fn give(&mut self, needs: &[u64]) -> TaskCreated {
        self.ledger
            .create_task(
                self.project.id,
                &NewTask {
                    from: "chief".to_owned(),
                    to: Some("zeus".to_owned()),
                    body: "Write the parser".to_owned(),
                    needs: needs.to_vec(),
                    ..NewTask::default()
                },
            )
            .expect("a task")
    }

    /// The task reaches zeus's window and is received: it is working.
    pub fn start(&mut self, given: &TaskCreated) {
        let message = given.message.as_ref().expect("the task went to its window");
        self.deliver(message);
        assert_eq!(self.state(given.task.number), "working");
    }

    /// zeus asks the chief about the task and waits for the answer.
    pub fn ask(&mut self, number: i64) {
        self.ledger
            .ask(
                self.project.id,
                &NewQuestion {
                    from: Some("zeus".to_owned()),
                    to: "chief".to_owned(),
                    body: Some("Which parser?".to_owned()),
                    task: Some(number),
                    ..NewQuestion::default()
                },
            )
            .expect("a question");
        assert_eq!(self.state(number), "waiting");
    }

    /// The task's state, as the board has it.
    pub fn state(&self, number: i64) -> String {
        self.ledger
            .task(self.project.id, number)
            .expect("a read")
            .expect("the task")
            .task
            .state
    }

    /// zeus ends the task with its result: it is done, and waits for the chief to decide.
    pub fn finish(&mut self, number: i64) {
        self.ledger
            .record_result(self.project.id, number, "The parser is written")
            .expect("a result");
        assert_eq!(self.state(number), "done");
    }

    /// The task is accepted: over.
    pub fn accept(&mut self, number: i64) {
        self.ledger
            .accept_task(self.project.id, number, "human")
            .expect("accepted");
    }

    /// Something moves on the board and leaves nothing on its way: the chief
    /// gives zeus a task and calls it off.
    pub fn move_a_task(&mut self) {
        let given = self.give(&[]);
        self.ledger
            .cancel_task(self.project.id, given.task.number, "chief")
            .expect("called off");
    }

    /// A note from one participant to another: a message is sent.
    pub fn note(&mut self, from: &str, to: &str, body: &str) -> MessageView {
        self.ledger
            .note(
                self.project.id,
                &NewNote {
                    from: Some(from.to_owned()),
                    to: to.to_owned(),
                    body: body.to_owned(),
                    task: None,
                },
            )
            .expect("a note")
    }

    /// The kinds of the project's events, oldest first.
    pub fn kinds(&self) -> Vec<String> {
        self.ledger
            .events(self.project.id, 0, 5000)
            .expect("events")
            .into_iter()
            .map(|event| event.kind)
            .collect()
    }

    /// How many times the log says a wake-up was queued.
    pub fn queued(&self) -> usize {
        self.kinds()
            .iter()
            .filter(|kind| *kind == "project.wake_up")
            .count()
    }

    pub fn state_of(&self, message: &MessageView) -> String {
        self.ledger
            .message(message.id)
            .expect("a read")
            .expect("the message")
            .state
    }
}

/// An item of the chief's window as its record holds it.
pub fn item(id: &str, role: &str, text: &str) -> Value {
    json!({ "id": id, "role": role, "text": text, "complete": true })
}

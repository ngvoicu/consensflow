//! What the receipt tests share: a project on a ticking clock (each reading a
//! second after the last), a chief and two workers on the staff, and the
//! few things a test does over and over, named for what they do. The ledger is
//! read only as its callers read it: a row's carrier is not in any view, so
//! what a message carries is what its delivery begins with.

use cf_base::time::Clock;
use cf_ledger::{
    open_ledger, Begun, Ledger, LedgerError, MessageView, NewChief, NewMember, NewNote, NewProject,
    NewQuestion, NewTask, Options, TaskCreated, TaskMoved, RESUME_WORDS,
};
use serde_json::{json, Value};

/// A clock that moves one second at each reading.
struct Ticks(i64);

impl Clock for Ticks {
    fn now_ms(&mut self) -> i64 {
        self.0 += 1000;
        self.0
    }
}

/// A clock that never moves: two pauses on it are one millisecond apart, and less.
struct Fixed;

impl Clock for Fixed {
    fn now_ms(&mut self) -> i64 {
        1_790_000_000_000
    }
}

/// The names the ledger gives sessions, in turn.
fn session_names() -> Box<dyn FnMut() -> String> {
    let mut named = 0;
    Box::new(move || {
        named += 1;
        format!("session-{named}")
    })
}

/// A project in a home of its own, and what a test does with it.
pub struct World {
    pub ledger: Ledger,
    pub project: i64,
    pub dir: tempfile::TempDir,
}

pub fn world() -> World {
    World::on(Box::new(Ticks(1_790_000_000_000)), false)
}

/// A project whose human approves every hand-off between agents.
pub fn gated_world() -> World {
    World::on(Box::new(Ticks(1_790_000_000_000)), true)
}

/// A project on a clock that never moves.
pub fn frozen_world() -> World {
    World::on(Box::new(Fixed), false)
}

impl World {
    fn on(clock: Box<dyn Clock>, gate: bool) -> Self {
        let dir = tempfile::tempdir().expect("a home");
        let mut ledger = open_ledger(
            &dir.path().join("consensflow.db"),
            Options {
                clock,
                names: session_names(),
                trace: Box::new(|_| {}),
            },
        )
        .expect("a ledger");
        let member = |agent: &str| NewMember {
            agent: agent.into(),
            harness: "claude-code".into(),
            designer: false,
            roles: vec!["worker".into()],
            tier: "standard".into(),
        };
        let project = ledger
            .create_project(&NewProject {
                directory: "/work/app".into(),
                name: "app".into(),
                chief: NewChief {
                    harness: "claude-code".into(),
                    agent: Some("apollo".into()),
                },
                staff: vec![member("zeus"), member("diana")],
                gate,
            })
            .expect("a project");
        Self {
            ledger,
            project: project.id,
            dir,
        }
    }

    /// Changes what the file holds as another process might have left it
    /// (Node's ledger, an older build): the ledger is closed, `change` is
    /// given the file, and the ledger is opened on it again.
    pub fn edit(&mut self, change: impl FnOnce(&rusqlite::Connection)) {
        self.ledger.close_in_place().expect("the file is given up");
        let file = self.dir.path().join("consensflow.db");
        let db = rusqlite::Connection::open(&file).expect("the file");
        change(&db);
        db.close().expect("the file is closed");
        self.ledger = open_ledger(
            &file,
            Options {
                clock: Box::new(Ticks(1_790_000_100_000)),
                names: session_names(),
                trace: Box::new(|_| {}),
            },
        )
        .expect("the ledger opens again");
    }

    /// The id of `handle` in the project.
    pub fn id(&self, handle: &str) -> i64 {
        self.ledger
            .project(self.project)
            .expect("the project")
            .expect("it is there")
            .participants
            .into_iter()
            .find(|participant| participant.handle == handle)
            .unwrap_or_else(|| panic!("no @{handle}"))
            .id
    }

    /// The chief gives a task to a member by name: its brief is queued for it.
    pub fn give(&mut self, to: &str, body: &str) -> TaskCreated {
        self.ledger
            .create_task(
                self.project,
                &NewTask {
                    from: "chief".into(),
                    to: Some(to.into()),
                    body: body.into(),
                    ..NewTask::default()
                },
            )
            .expect("a task")
    }

    /// The chief opens a task for the standard workers; the daemon gives it a
    /// session of `member` with `assign`.
    pub fn open(&mut self, body: &str) -> TaskCreated {
        self.ledger
            .create_task(
                self.project,
                &NewTask {
                    from: "chief".into(),
                    pool: Some("worker".into()),
                    tier: Some("standard".into()),
                    body: body.into(),
                    ..NewTask::default()
                },
            )
            .expect("a task for the tier")
    }

    /// The daemon gives open task `number` to a new session of `member`.
    pub fn assign(&mut self, number: i64, member: &str) -> TaskMoved {
        let member = self.id(member);
        self.ledger
            .assign_task(self.project, number, member)
            .expect("assigned")
    }

    /// The chief gives a follow-up to the session that did task `after`, and
    /// has it wait on the board for the tasks it `needs`, which is how a session
    /// is given a second task while one waits for it.
    pub fn follow_up(
        &mut self,
        after: i64,
        body: &str,
        needs: &[i64],
    ) -> Result<TaskCreated, LedgerError> {
        self.ledger.create_task(
            self.project,
            &NewTask {
                from: "chief".into(),
                after: Some(after),
                needs: needs
                    .iter()
                    .map(|number| u64::try_from(*number).expect("a task number"))
                    .collect(),
                body: body.into(),
                ..NewTask::default()
            },
        )
    }

    /// A message is pasted and proved: its delivery begins and is confirmed.
    /// What the paste carried.
    pub fn deliver(&mut self, id: i64) -> Begun {
        let begun = self.begin(id);
        self.confirm(id);
        begun
    }

    pub fn begin(&mut self, id: i64) -> Begun {
        self.ledger.begin_delivery(id).expect("its delivery begins")
    }

    pub fn confirm(&mut self, id: i64) -> MessageView {
        self.ledger
            .confirm_delivery(id, Some(&json!({ "item": format!("i-{id}") })))
            .expect("its delivery is confirmed")
    }

    /// The worker asks the chief a question about its task.
    pub fn ask(&mut self, from: &str, number: i64, body: &str) -> MessageView {
        self.ledger
            .ask(
                self.project,
                &NewQuestion {
                    from: Some(from.into()),
                    to: "chief".into(),
                    body: Some(body.into()),
                    task: Some(number),
                    ..NewQuestion::default()
                },
            )
            .expect("a question")
    }

    /// The worker asks with the options of its harness's question tool: it has a door.
    pub fn ask_with_options(&mut self, from: &str, number: i64) -> MessageView {
        self.ledger
            .ask(
                self.project,
                &NewQuestion {
                    from: Some(from.into()),
                    to: "chief".into(),
                    task: Some(number),
                    questions: Some(json!([{
                        "question": "Which colour?",
                        "header": "Colour",
                        "options": [{ "label": "red" }, { "label": "blue" }],
                        "multiple": false,
                    }])),
                    ..NewQuestion::default()
                },
            )
            .expect("a question with options")
    }

    /// The chief's tell to a task's window: the task is paused for it.
    pub fn tell(&mut self, to: &str, number: i64, body: &str) -> MessageView {
        self.ledger
            .ask(
                self.project,
                &NewQuestion {
                    from: Some("chief".into()),
                    to: to.into(),
                    body: Some(body.into()),
                    task: Some(number),
                    urgent: true,
                    ..NewQuestion::default()
                },
            )
            .expect("a tell")
    }

    /// The chief answers a question in words.
    pub fn answer(&mut self, question: i64, body: &str) -> MessageView {
        let chief = self.id("chief");
        self.ledger
            .answer(question, chief, Some(&json!(body)), None)
            .expect("an answer")
    }

    /// The chief answers a question with options by a choice.
    pub fn choose(&mut self, question: i64, label: &str) -> MessageView {
        let chief = self.id("chief");
        self.ledger
            .answer(question, chief, None, Some(&json!([[label]])))
            .expect("a choice")
    }

    /// A note from `from` to `to` about a task.
    pub fn note(&mut self, from: &str, to: &str, number: i64, body: &str) -> MessageView {
        self.ledger
            .note(
                self.project,
                &NewNote {
                    from: Some(from.into()),
                    to: to.into(),
                    body: body.into(),
                    task: Some(number),
                },
            )
            .expect("a note")
    }

    /// The chief pauses a task.
    pub fn pause(&mut self, number: i64) {
        self.ledger
            .pause_task(self.project, number, Some("chief"), None)
            .expect("paused");
    }

    /// The daemon holds a task until the quota resets.
    pub fn hold(&mut self, number: i64) {
        self.ledger
            .hold_task(
                self.project,
                number,
                "2026-10-10T15:00:00.000Z",
                "out of quota",
            )
            .expect("held");
    }

    /// The chief resumes a task with its own words.
    pub fn resume(&mut self, number: i64, words: &str) -> TaskMoved {
        self.ledger
            .resume_task(self.project, number, Some("chief"), words)
            .expect("resumed")
    }

    /// The daemon resumes a held task with the words it always uses.
    pub fn daemon_resumes(&mut self, number: i64) -> TaskMoved {
        self.ledger
            .resume_task(self.project, number, None, RESUME_WORDS)
            .expect("resumed by the daemon")
    }

    pub fn message(&self, id: i64) -> MessageView {
        self.ledger
            .message(id)
            .expect("a read")
            .unwrap_or_else(|| panic!("no m-{id}"))
    }

    /// The states of messages, in the order asked.
    pub fn states(&self, ids: &[i64]) -> Vec<String> {
        ids.iter().map(|id| self.message(*id).state).collect()
    }

    /// The state of task `number`.
    pub fn state(&self, number: i64) -> String {
        self.ledger
            .task(self.project, number)
            .expect("a read")
            .expect("the task")
            .task
            .state
    }

    /// What the paste of the head of `handle`'s queue is, if it has one: its
    /// message and the ids of the rows it carries.
    pub fn next(&self, handle: &str) -> Option<i64> {
        self.ledger
            .next_delivery(self.id(handle))
            .expect("a read")
            .map(|message| message.id)
    }

    /// A message's receipt, as it was stored.
    pub fn receipt(&self, id: i64) -> Value {
        self.message(id).receipt
    }
}

/// The ids of messages, in order.
pub fn ids(messages: &[MessageView]) -> Vec<i64> {
    messages.iter().map(|message| message.id).collect()
}

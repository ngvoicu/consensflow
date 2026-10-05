//! The ledger calls the dispatcher's tests make over and over, as they
//! made them on `context.ledger`: each is the ledger's own, read or written
//! straight (the ledger's calls are no behaviour of the engine's, and the
//! Node traces hold the events they log).

use cf_ledger::{MessageView, NewNote, NewQuestion, NewTask, TaskCreated};

use super::context::Context;

impl Context {
    /// A task from the chief for a pool, at the standard tier, which the
    /// scheduler gives to a member's session (`open` of `withTiers`).
    pub fn pool_task(&self, project: i64, pool: &str, body: &str) -> TaskCreated {
        self.create_task(
            project,
            NewTask {
                from: "chief".to_owned(),
                pool: Some(pool.to_owned()),
                tier: Some("standard".to_owned()),
                body: body.to_owned(),
                ..NewTask::default()
            },
        )
    }

    /// A note from `from` to `to` (`ledger.note`).
    pub fn note(&self, project: i64, from: &str, to: &str, body: &str) -> MessageView {
        self.ledger
            .borrow_mut()
            .note(
                project,
                &NewNote {
                    from: Some(from.to_owned()),
                    to: to.to_owned(),
                    body: body.to_owned(),
                    task: None,
                },
            )
            .expect("a note")
    }

    /// A question from `from` to `to` about task `number` (`ledger.ask`).
    pub fn ask(&self, project: i64, from: &str, to: &str, number: i64, body: &str) -> MessageView {
        self.question(project, from, to, number, body, false)
    }

    /// The chief's urgent word to a task's window (`ledger.ask` with `urgent`).
    pub fn tell(&self, project: i64, to: &str, number: i64, body: &str) -> MessageView {
        self.question(project, "chief", to, number, body, true)
    }

    fn question(
        &self,
        project: i64,
        from: &str,
        to: &str,
        number: i64,
        body: &str,
        urgent: bool,
    ) -> MessageView {
        self.ledger
            .borrow_mut()
            .ask(
                project,
                &NewQuestion {
                    from: Some(from.to_owned()),
                    to: to.to_owned(),
                    body: Some(body.to_owned()),
                    task: Some(number),
                    questions: None,
                    urgent,
                },
            )
            .expect("a question")
    }

    /// A message as it is now (`ledger.message`).
    pub fn message(&self, id: i64) -> MessageView {
        self.ledger
            .borrow()
            .message(id)
            .expect("the ledger read")
            .expect("the message")
    }

    /// A participant's messages, newest first (`ledger.inbox`).
    pub fn inbox(&self, participant: i64) -> Vec<MessageView> {
        self.ledger
            .borrow()
            .inbox(participant, 100)
            .expect("the inbox")
    }

    /// The chief pauses task `number` (`ledger.pauseTask`).
    pub fn pause_task(&self, project: i64, number: i64) {
        self.ledger
            .borrow_mut()
            .pause_task(project, number, Some("chief"), None)
            .expect("the task paused");
    }

    /// The chief resumes task `number` with `body` (`ledger.resumeTask`).
    pub fn resume_task(&self, project: i64, number: i64, body: &str) {
        self.ledger
            .borrow_mut()
            .resume_task(project, number, Some("chief"), body)
            .expect("the task resumed");
    }
}

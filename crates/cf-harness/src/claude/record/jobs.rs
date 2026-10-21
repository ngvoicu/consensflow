//! The jobs a Claude Code session started in the background and has not heard
//! end: its background commands and its monitors.
//!
//! A Bash call that asks to run in the background returns at once, and the
//! record of its tool result says the job's id (`toolUseResult.backgroundTaskId`).
//! A Monitor call does the same for the watch it arms (`toolUseResult.taskId`, with
//! the `timeoutMs` it expires after: Claude Code 2.1.305, the live run of 2026-10-21,
//! and T-442's record of 2026-10-17). The agent's turn can end with either still
//! running: Claude Code queues a `<task-notification>` that names the id (a
//! `queue-operation`, and the user's record that takes it, from the system) and
//! begins a turn by itself, which wakes the agent, for each event a monitor sees and
//! when a job ends. So a job is pending from the record that starts it until Claude
//! Code's own word says it is over.
//!
//! **What says a job is over**, and nothing else:
//! - a delivered notification whose head names the job and says, by its `<status>`,
//!   that it is over (`completed`, `failed`, `killed`, `stopped`, each seen in a real
//!   log). A notification with no status, or one this reader does not know, ends
//!   none: a monitor's event has none (its `<event>` is a line the watched command
//!   wrote), and nor has 2.1.301's notice of a background command that seems to wait
//!   for input, while the command runs on (GPT-6.1-Sol's review of round 4);
//! - the result of a TaskStop call that names the job it stopped
//!   (`toolUseResult.task_id`): Claude Code writes no notification of a job or a
//!   monitor its agent stopped so (2.1.305, the live run of 2026-10-21);
//! - a monitor's notice of its expiry, at or after the time it expires, counted from the
//!   call that armed it (`timeoutMs` from the record of the call, which comes before the
//!   monitor began): `[Monitor expired after 40s with 3 events delivered. …]`, or
//!   `[Monitor timed out — re-arm if needed.]` where Claude Code's `tengu_breezy_crescent`
//!   is off (2.1.306). Never earlier, and never with a line after the event (2.1.306
//!   writes one after a command's output where push notifications are on, and none after
//!   its own notices), so that a line the command wrote ends nothing.
//!
//!   **Claude Code's own notices of a monitor cannot be told from its command's output.**
//!   2.1.306 writes both with one function (`TG` of its bundle): the same envelope, a
//!   summary that names the monitor, the line in `<event>`, no status; what tells them
//!   apart (`monitorEvent.kind`, `notice` or `output`) stays in its queue and never reaches
//!   the record, and the line after a command's output comes only with push notifications
//!   on (`agentPushNotifEnabled`, off unless set). So a notice is believed only where time
//!   proves it: the expiry, which a monitor's real one never comes before (the live run of
//!   2026-10-21: queued 40 000 ms after the start's record, 30 ms more after the call).
//!   Its other ends have no time to prove them, and end nothing: `[Monitor stopped]` (the
//!   human stopped it), `[Monitor stopped — too much output (…)]` (2.1.306 stopped a
//!   command that wrote more than it lets through for half a minute: `FIn`, which writes
//!   nothing more of it, the live run of 2026-10-21) and `[WebSocket closed: …]`. A task
//!   held for such a monitor is held until Finish; a monitor's status (its command ended)
//!   and a TaskStop's result end it as they end any job. A monitor whose start says no
//!   time (`persistent`) ends by those alone.
//!
//! **Only Claude Code's own notification ends a job, never words that look like one**:
//! a message the user wrote, or ConsensFlow pasted, may quote one whole, from its first
//! character on (Astra's review of round 1, and both reviews of round 3). So the queue
//! ends none: a message typed while a turn is open is queued too, and the queue's record
//! (`enqueue`) says nothing of where its content came from. A notification ends its jobs
//! once Claude Code delivers it, which it does by itself: at rest, as the user's record
//! of the turn it begins, from the system for a notification (`promptSource`, and
//! `turnOrigin` or `origin.kind`: the projection's `notification`); in a turn under way,
//! as an attachment that brings it into the turn (`queued_command` of
//! `"commandMode":"task-notification"`, the queue's copy taken back as
//! `absorbed_mid_turn`). Until then the job is pending. Seen in real logs of Claude Code
//! 2.1.277, 2.1.292 and 2.1.305 (`tests/fixtures/claude/job-notifications.jsonl`,
//! `monitor-wait.jsonl`).
//!
//! **Only the notification's head is read** (both reviews of round 5). Claude Code writes
//! every notification with one function: `<task-notification>`, then each field it was
//! given on a line of its own, in one order (`<task-id>`, which one of several jobs
//! repeats, `<tool-use-id>`, `<task-type>`, `<output-file>`, `<status>`, `<summary>`, and
//! a monitor's `<event>`), then a body, the line that closes it, and what trails it. The
//! body and what trails are words of others: 2.1.301's watchdog puts the last kilobyte a
//! waiting command wrote after the close, as the command wrote it, and the end of an agent
//! or a monitor carries what it said in its body. A command that prints what looks like
//! the end of another job would end that job, read anywhere but in the head; so the head
//! is read, from the start of the text, field after field in that order, and the first
//! line that is no field of it, or one out of its order, ends it. A text that does not
//! open with a notification names no job, and a second notification in one text is not
//! read.
//!
//! **A summary ends the jobs it names** (T-8's answer, 2026-10-21). A process of 2.1.301
//! that resumes a conversation tells its agent of the background shells the process
//! before it left with no record of their end: of more than one, in one summary,
//! `stopped`, whose head names the first twenty by id, then the summary's own marker
//! (`__orphan_summary__:shell`), then a marker of each shell it found still running
//! (`__orphan_summary_live__:<id>`), which it leaves running. The ids it names end, but
//! those it marks running; what it does not name stays pending. Where ConsensFlow knows
//! the process before is gone, the reader has retired all of that process's jobs already
//! (below).
//!
//! **A relaunch retires, it does not end.** Where ConsensFlow closed the window and the
//! host proved its program gone before it opened the conversation again
//! (`Options::relaunched_at`), the jobs that process started are no longer the window's
//! to wait on: their notifications went with the process. They are retired, not ended:
//! nothing says the processes they ran are gone (a job its shell detached can outlive the
//! window), and the reading says how many were retired so (`Pending::retired`).
//!
//! The background agents and workflows Claude Code also wakes its agent for are no part
//! of this: it counts them itself, on the `turn_duration` that ends a turn.

use std::collections::HashSet;
use std::sync::Arc;

/// The marker of 2.1.301's summary of the shells a previous process left (see above),
/// before the kind of task it is of, and the marker of one it found still running,
/// before that one's id.
#[cfg(test)]
const SUMMARY: &str = "__orphan_summary__:";
const LIVE: &str = "__orphan_summary_live__:";

/// What opens Claude Code's notification.
const NOTIFICATION: &str = "<task-notification>";

/// The fields of the head of a notification, in the order Claude Code writes them: the
/// first names a job, the fifth says how it went, and a monitor's event comes last.
const FIELDS: [&str; 7] = [
    "task-id",
    "tool-use-id",
    "task-type",
    "output-file",
    "status",
    "summary",
    "event",
];
const TASK_ID: usize = 0;
const STATUS: usize = 4;
const EVENT: usize = 6;

/// The statuses Claude Code writes of a job that is over: it ended by itself, well or not,
/// it was ended, or the session it ran in ended before it did (`stopped`).
const OVER: [&str; 4] = ["completed", "failed", "killed", "stopped"];

/// How Claude Code's own events of a monitor that expired begin (2.1.305, 2.1.306).
const EXPIRED: [&str; 2] = ["[Monitor expired after ", "[Monitor timed out"];

/// A job, as its records say it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Running,
    Ended,
    /// Its process went with a window ConsensFlow closed (see above): no word of its
    /// end will come, and none came.
    Retired,
}

#[derive(Debug)]
struct Job {
    id: Arc<str>,
    state: State,
    /// For a monitor whose start said when it expires: then, in milliseconds.
    expires_at: Option<i64>,
}

/// The jobs started, each once, by their ids, in the order they started.
#[derive(Debug, Default)]
pub(super) struct Jobs {
    started: Vec<Job>,
}

impl Jobs {
    /// The record of a tool result says the job `id` runs in the background; a monitor's
    /// says when it expires (`expires_at`).
    pub(super) fn started(&mut self, id: Arc<str>, expires_at: Option<i64>) {
        match self.started.iter_mut().find(|job| job.id == id) {
            Some(job) => {
                job.state = State::Running;
                job.expires_at = expires_at;
            }
            None => self.started.push(Job {
                id,
                state: State::Running,
                expires_at,
            }),
        }
    }

    /// `text` is Claude Code's own notification, delivered at the time its record says
    /// (`at_ms`, none where it says none): each job its head says is over has ended, but
    /// those a summary marks running, and a monitor whose own notice of expiry it is. A
    /// job that was never started is named in vain: a notification is no record of a
    /// start.
    pub(super) fn ended_by(&mut self, text: &str, at_ms: Option<i64>) {
        let head = head(text);
        let over = head.status.is_some_and(|status| OVER.contains(&status));
        let live: HashSet<&str> = head
            .ids
            .iter()
            .filter_map(|id| id.strip_prefix(LIVE))
            .collect();
        let notice = head
            .event
            .filter(|_| head.status.is_none() && head.closes_after_event);
        let expired =
            notice.is_some_and(|event| EXPIRED.iter().any(|said| event.starts_with(said)));
        for job in &mut self.started {
            if job.state == State::Ended {
                continue;
            }
            let named = head.ids.contains(&&*job.id) && !live.contains(&*job.id);
            let its_expiry = expired
                && head.ids.as_slice() == [&*job.id]
                && job
                    .expires_at
                    .zip(at_ms)
                    .is_some_and(|(expires, at)| at >= expires);
            if (over && named) || its_expiry {
                job.state = State::Ended;
            }
        }
    }

    /// A TaskStop call's result says it stopped the job `id`.
    pub(super) fn stopped(&mut self, id: &str) {
        if let Some(job) = self.started.iter_mut().find(|job| &*job.id == id) {
            job.state = State::Ended;
        }
    }

    /// The process that started the jobs that run is gone (see above): they are retired.
    pub(super) fn retire(&mut self) {
        for job in &mut self.started {
            if job.state == State::Running {
                job.state = State::Retired;
            }
        }
    }

    /// How many jobs run.
    pub(super) fn running(&self) -> usize {
        self.count(State::Running)
    }

    /// How many were retired at a relaunch with no word of their end since.
    pub(super) fn retired(&self) -> usize {
        self.count(State::Retired)
    }

    fn count(&self, state: State) -> usize {
        self.started.iter().filter(|job| job.state == state).count()
    }
}

/// What the head of a notification says (see above), each value trimmed.
#[derive(Debug, Default, PartialEq)]
struct Head<'a> {
    /// The jobs it names, in order.
    ids: Vec<&'a str>,
    status: Option<&'a str>,
    /// A monitor's event: the line the watched command wrote, or Claude Code's own.
    event: Option<&'a str>,
    /// The event is the last field, and only the line that closes the notification comes
    /// after it: no line of a command's output, nor 2.1.306's word after a command's
    /// event of how to tell the user of it.
    closes_after_event: bool,
}

/// The head of the notification `text`: nothing where the text does not open with one.
fn head(text: &str) -> Head<'_> {
    let mut head = Head::default();
    let Some(rest) = text.strip_prefix(NOTIFICATION) else {
        return head;
    };
    let mut lines = rest.split('\n');
    // The opening tag is a line of its own.
    if lines.next() != Some("") {
        return head;
    }
    let mut at = None;
    while let Some(line) = lines.next() {
        let Some((place, value)) = field(line) else {
            head.closes_after_event = at == Some(EVENT)
                && line == "</task-notification>"
                && lines.all(|after| after.trim().is_empty());
            break;
        };
        // Each field once, in its order; only a job's id again.
        if at.is_some_and(|at| place < at || (place == at && place != TASK_ID)) {
            break;
        }
        at = Some(place);
        match place {
            TASK_ID if !value.is_empty() => head.ids.push(value),
            STATUS => head.status = Some(value),
            EVENT => head.event = Some(value),
            _ => {}
        }
    }
    head
}

/// The place in [`FIELDS`] of the field `line` is (`<tag>value</tag>`, the whole line), and
/// its value, trimmed: none where the line is no field of a head.
fn field(line: &str) -> Option<(usize, &str)> {
    let (tag, rest) = line.strip_prefix('<')?.split_once('>')?;
    let place = FIELDS.iter().position(|field| *field == tag)?;
    let value = rest
        .strip_suffix('>')?
        .strip_suffix(tag)?
        .strip_suffix("</")?;
    Some((place, value.trim()))
}

/// Whether the notification `text` is 2.1.301's summary of the shells a previous process
/// left (see above).
#[cfg(test)]
fn is_summary(text: &str) -> bool {
    head(text)
        .ids
        .iter()
        .any(|id| id.strip_prefix(SUMMARY) == Some("shell"))
}

/// A notification as Claude Code writes every one of them, for tests: it opens with
/// `<task-notification>`, writes each field it is given that is not empty on a line of its
/// own, in this order, then the body, a line that closes it, and what trails it.
#[cfg(test)]
#[derive(Default)]
pub(super) struct Written<'a> {
    pub task_id: &'a str,
    pub tool_use_id: &'a str,
    pub task_type: &'a str,
    pub output_file: &'a str,
    pub status: &'a str,
    pub summary: &'a str,
    pub event: &'a str,
    pub body: &'a str,
    pub trailing: &'a str,
}

#[cfg(test)]
impl Written<'_> {
    /// Its text, as Claude Code queues it and delivers it.
    pub(super) fn text(&self) -> String {
        let mut text = String::from("<task-notification>");
        for (tag, value) in [
            ("task-id", self.task_id),
            ("tool-use-id", self.tool_use_id),
            ("task-type", self.task_type),
            ("output-file", self.output_file),
            ("status", self.status),
            ("summary", self.summary),
            ("event", self.event),
        ] {
            if !value.is_empty() {
                text.push_str(&format!("\n<{tag}>{value}</{tag}>"));
            }
        }
        format!("{text}{}\n</task-notification>{}", self.body, self.trailing)
    }

    /// The notification 2.1.301's watchdog sends of the background command `id` (its call
    /// `call`, described as `description`) that has written nothing for 45 seconds after a
    /// line that looks like a question: no status, and after its close the last kilobyte the
    /// command wrote, as it wrote it, and what to do.
    pub(super) fn waiting(id: &str, call: &str, description: &str, last_output: &str) -> String {
        let summary = format!(
            "Background command \"{description}\" appears to be waiting for interactive input"
        );
        let output_file = format!("/private/tmp/claude-501/-work-app/$SESSION/tasks/{id}.output");
        let trailing = format!(
            "\nLast output:\n{}\n\nThe command is likely blocked on an interactive prompt. Stop this task and re-run with piped input (e.g., `echo y | command`) or a non-interactive flag if one exists.",
            last_output.trim_end()
        );
        Written {
            task_id: id,
            tool_use_id: call,
            output_file: &output_file,
            summary: &summary,
            trailing: &trailing,
            ..Written::default()
        }
        .text()
    }

    /// The notification of the end of the background command `id` (its call `call`), as
    /// 2.1.301 writes it of one that ended by itself with `status`.
    pub(super) fn ended(id: &str, call: &str, status: &str, body: &str) -> String {
        let output_file = format!("/private/tmp/claude-501/-work-app/$SESSION/tasks/{id}.output");
        let summary = format!("Background command \"Run the suite\" {status} (exit code 0)");
        Written {
            task_id: id,
            tool_use_id: call,
            output_file: &output_file,
            status,
            summary: &summary,
            body,
            ..Written::default()
        }
        .text()
    }

    /// A monitor's event, as 2.1.305 writes it of the monitor `id` described as
    /// `description`: the line the watched command wrote, or Claude Code's own notice.
    pub(super) fn event(id: &str, description: &str, event: &str) -> String {
        let summary = format!("Monitor event: \"{description}\"");
        Written {
            task_id: id,
            summary: &summary,
            event,
            ..Written::default()
        }
        .text()
    }
}

#[cfg(test)]
mod tests;

//! What a turn does before it answers, as Claude Code writes it: the steps it works
//! through, each an assistant's message that says something and calls a tool, and the
//! tool's result after it; among them the call that runs a job in the background, whose
//! result says the job's id (`toolUseResult.backgroundTaskId`) and whose end Claude Code
//! writes as a `<task-notification>`, queued and taken as a turn of its own, which wakes
//! its agent (the shapes of 2.1.299, seen in a real log: the owner's report of T-14 on
//! 3.0.4, 2026-10-15). The stand-in writes the same records, and a worker's turn that
//! ends with its progress before its report (T-14's and T-15's results began with ten to
//! twenty lines of it) is a turn of steps and an answer.
//!
//! - A line `STEPS <n> <words>` is a turn that works through `n` steps, each saying `Step
//!   <i> of <n>` and calling a tool, and then answers the words: the progress a worker
//!   writes before its report, and the report.
//! - A line `CLAUDE-JOB [--silent] [--exec] [--at-once] [--asks] [--stopped] <seconds> <words>
//!   :: <report>` is a turn that starts a job as Claude Code's Bash tool does with
//!   `run_in_background` (a process of the window's tree that runs that long, as `BACKGROUND`
//!   starts one, and a step whose result says its id), answers the words at once, and ends.
//!   When the job ends Claude Code writes its notification, queued and taken as a turn of its
//!   own, and that turn answers the report. With `--silent` the job ends and nothing is
//!   written, as when Claude Code lost it (a window resumed in a process that never started
//!   it): no wake-up comes. With `--exec` the job's shell `exec`s it, as `exec sleep 120` does
//!   (Unix). With `--at-once` the job starts as the turn does, with no thought first: before
//!   the daemon's pass that marks a launch's turn once its message shows. With `--asks`,
//!   [`ASKS_AFTER`] after it starts, Claude Code 2.1.301 wakes the agent with its notification
//!   of a command that seems to wait for input, which has no status, while the job runs on;
//!   that turn answers [`STILL_WAITING`]. With `--stopped` its end is written as Claude Code
//!   2.1.292 writes the jobs a resumed session's last process left with no record of their
//!   end: one notification, `stopped`, to the transcript alone, which begins no turn.
//! - A line `CLAUDE-MONITOR <events> <seconds> <words> :: <report>` is poker-lab's T-442
//!   (2026-10-17): a turn that starts a job as `CLAUDE-JOB` does and arms a Monitor on it, as
//!   Claude Code 2.1.305 writes it (a `Monitor` call whose result says the monitor's task and
//!   when it expires: `toolUseResult.taskId`, `timeoutMs`, `persistent`), answers the words at
//!   once, and ends. While the job runs the monitor sees `events` lines, each a notification
//!   with an `<event>` and no status, which wakes a turn that answers `Saw <event>`; the job's
//!   end wakes a turn that answers [`MONITOR_WATCHES`]; and the monitor's own end, a
//!   notification of its task `completed`, wakes the turn that answers the report. The
//!   monitor ends once the case lets it ([`MONITOR_MAY_END`]), so the case sees what the turn
//!   the job's end woke led to before the monitor's end wakes another.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use cf_e2e::pattern;
use regex::Regex;
use serde_json::{json, Value};

use crate::background::{until_up, THINKING};
use crate::records::Records;
use crate::reply::{Reply, Window};

/// What opens the text of a notification, which is how a message that is one is told.
const NOTIFICATION: &str = "<task-notification>";

/// A step of a turn: what the agent said and called a tool for, and, where the call
/// runs a job in the background, the job's id, or, where it arms a monitor, the monitor's
/// task and how long it watches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub said: String,
    pub job: Option<String>,
    pub monitor: Option<(String, u64)>,
}

impl Step {
    /// The words of step `number` of `of`: a line of progress.
    pub fn progress(number: u64, of: u64) -> Self {
        Self {
            said: format!("Step {number} of {of}"),
            job: None,
            monitor: None,
        }
    }

    /// The call that runs a job in the background, `id` its id.
    pub fn starting(id: &str) -> Self {
        Self {
            said: "Running it in the background".to_owned(),
            job: Some(id.to_owned()),
            monitor: None,
        }
    }

    /// The call that arms a monitor, `id` its task, which expires after `timeout_ms`.
    pub fn arming(id: &str, timeout_ms: u64) -> Self {
        Self {
            said: "Watching it with Monitor".to_owned(),
            job: None,
            monitor: Some((id.to_owned(), timeout_ms)),
        }
    }
}

/// The jobs a window started in the background that Claude Code wakes its agent for (its
/// background tasks): how many it started, which numbers their ids, and how many have not
/// ended. They are not the processes the window's tree runs (`background`), which are what
/// a job is made of: the window started both.
#[derive(Clone, Default)]
pub struct Tasks {
    started: Arc<AtomicU64>,
    running: Arc<AtomicU64>,
}

impl Tasks {
    /// A job begins: its id.
    pub fn begin(&self) -> String {
        self.running.fetch_add(1, Ordering::SeqCst);
        format!("b{}", self.started.fetch_add(1, Ordering::SeqCst) + 1)
    }

    /// A job ended.
    pub fn ended(&self) {
        self.running.fetch_sub(1, Ordering::SeqCst);
    }

    /// How many have not ended.
    pub fn running(&self) -> u64 {
        self.running.load(Ordering::SeqCst)
    }
}

/// What Claude Code queues when the job `id` ends, and the turn it wakes begins with. The
/// `<result>` is the stand-in's own: what the turn answers, as an agent that read the
/// output would.
pub fn notification(id: &str, report: &str) -> String {
    format!(
        "{NOTIFICATION}\n<task-id>{id}</task-id>\n<status>completed</status>\n<summary>Background command \"{id}\" completed (exit code 0)</summary>\n<result>{report}</result>\n</task-notification>"
    )
}

/// What the turn the end of a watched job wakes answers: the monitor still watches.
pub const MONITOR_WATCHES: &str = "The run ended; the monitor still watches it.";

/// How long a `CLAUDE-MONITOR`'s monitor watches before it expires, as its call asks.
const MONITOR_MS: u64 = 1_800_000;

/// The kind of the file beside a `CLAUDE-MONITOR`'s job (`<file>.monitor-may-end`) whose
/// coming lets its monitor end: the case writes it (`Job::let_its_monitor_end` of the rig).
/// A wait of a fixed time between the job's end and the monitor's could be outrun by a slow
/// runner, whose passes then never saw the turn the job's end woke at rest (CI's macOS
/// runner, twice, on 8504697f); and with it too long, by a quick one.
pub const MONITOR_MAY_END: &str = "monitor-may-end";

/// How long the monitor waits at most for the case to let it end, so that a case that
/// never does leaves no stand-in waiting for good: past any case's own wait for it.
const MONITOR_HELD: Duration = Duration::from_secs(300);

/// How often the file that lets the monitor end is looked for.
const MONITOR_LOOK: Duration = Duration::from_millis(100);

/// What Claude Code 2.1.305 writes when the monitor `id` sees the line `event`
/// (`tests/fixtures/claude/monitor-wait.jsonl` of `cf-harness`): no status.
pub fn monitor_event(id: &str, event: &str) -> String {
    format!(
        "{NOTIFICATION}\n<task-id>{id}</task-id>\n<summary>Monitor event: \"ticks\"</summary>\n<event>{event}</event>\n</task-notification>"
    )
}

/// What Claude Code writes when the monitor `id` ends with its watched command: its task
/// `completed`, with the stand-in's own `<result>`.
pub fn monitor_ended(id: &str, report: &str) -> String {
    format!(
        "{NOTIFICATION}\n<task-id>{id}</task-id>\n<tool-use-id>toolu-{id}</tool-use-id>\n<status>completed</status>\n<summary>Monitor \"ticks\" stream ended</summary>\n<result>{report}</result>\n</task-notification>"
    )
}

/// The line a monitor's event notification says it saw: none for a message that is none.
pub fn event_of(text: &str) -> Option<String> {
    static EVENT: OnceLock<Regex> = OnceLock::new();
    if !is_notification(text) {
        return None;
    }
    pattern::once(&EVENT, r"(?m)^<event>(.*)</event>$")
        .captures(text)
        .map(|found| found[1].to_owned())
}

/// How long after a `--asks` job starts Claude Code wakes the agent for it: 2.1.301 waits for
/// 45 seconds of no output, which a test has no need to wait for.
pub const ASKS_AFTER: std::time::Duration = std::time::Duration::from_secs(5);

/// What the turn a notification with no report wakes answers: the agent read that its job
/// waits for input, and leaves it running.
pub const STILL_WAITING: &str = "It waits for an answer; I leave it running.";

/// What Claude Code 2.1.301 wakes its agent with when the background command of job `id`
/// has written nothing for a while after a line that looks like a question, as its own source
/// writes it (`tests/fixtures/claude/job-ends.jsonl` of `cf-harness`): no status, and the
/// command's last output after the close.
pub fn waiting(id: &str) -> String {
    format!(
        "{NOTIFICATION}\n<task-id>{id}</task-id>\n<tool-use-id>toolu-{id}</tool-use-id>\n<output-file>/tmp/claude/tasks/{id}.output</output-file>\n<summary>Background command \"{id}\" appears to be waiting for interactive input</summary>\n</task-notification>\nLast output:\nContinue? [y/N]\n\nThe command is likely blocked on an interactive prompt. Stop this task and re-run with piped input (e.g., `echo y | command`) or a non-interactive flag if one exists."
    )
}

/// What Claude Code 2.1.292 writes, to the transcript alone, of job `id` a resumed session's
/// last process left with no record of its end (`job-notifications.jsonl` of `cf-harness`).
pub fn stopped(id: &str) -> String {
    format!(
        "{NOTIFICATION}\n<task-id>{id}</task-id>\n<task-id>__orphan_summary__:shell</task-id>\n<status>stopped</status>\n<summary>1 background shell command task didn't finish before the previous session ended. Task ids: {id}.</summary>\n</task-notification>"
    )
}

/// What the stand-in sends itself, before a notification, that Claude Code writes to the
/// transcript alone: no turn takes it.
const TRANSCRIPT_ONLY: &str = "TRANSCRIPT-ONLY ";

/// `notification`, to be written to the transcript alone.
fn transcript_only(notification: &str) -> String {
    format!("{TRANSCRIPT_ONLY}{notification}")
}

/// The notification `text` holds, where it is one to write to the transcript alone.
pub fn for_the_transcript_alone(text: &str) -> Option<&str> {
    text.strip_prefix(TRANSCRIPT_ONLY)
}

/// Whether `text` is a notification, which Claude Code writes as the user's own turn.
pub fn is_notification(text: &str) -> bool {
    text.starts_with(NOTIFICATION)
}

/// What a notification says its turn answers: its `<result>`, none for a message that is
/// no notification.
pub fn report_of(text: &str) -> Option<String> {
    static RESULT: OnceLock<Regex> = OnceLock::new();
    if !is_notification(text) {
        return None;
    }
    pattern::once(&RESULT, r"(?s)<result>(.*?)</result>")
        .captures(text)
        .map(|found| found[1].to_owned())
}

/// What a turn does and answers: the steps it works through first, and its reply.
pub type Worked = (Vec<Step>, Reply);

/// The turn `text` asks for, if it is one of the steps': a notification's, answering the
/// report it carries, or a line of this module's.
pub fn reply(text: &str, window: &Window) -> Option<Result<Worked, String>> {
    static STEPS: OnceLock<Regex> = OnceLock::new();
    static JOB: OnceLock<Regex> = OnceLock::new();
    static MONITOR: OnceLock<Regex> = OnceLock::new();
    if let Some(report) = report_of(text) {
        return Some(Ok((Vec::new(), Reply::Text(report))));
    }
    if let Some(event) = event_of(text) {
        return Some(Ok((Vec::new(), Reply::Text(format!("Saw {event}")))));
    }
    if is_notification(text) {
        return Some(Ok((Vec::new(), Reply::Text(STILL_WAITING.to_owned()))));
    }
    if let Some(steps) = pattern::once(&STEPS, r"(?m)^STEPS (\d+) (.+)$").captures(text) {
        let Ok(count) = steps[1].parse::<u64>() else {
            return Some(Err(format!("STEPS {}", &steps[1])));
        };
        let worked = (1..=count).map(|at| Step::progress(at, count)).collect();
        return Some(Ok((worked, Reply::Text(steps[2].to_owned()))));
    }
    if let Some(monitor) =
        pattern::once(&MONITOR, r"(?m)^CLAUDE-MONITOR (\d+) (\d+) (.+?) :: (.+)$").captures(text)
    {
        let (Ok(events), Ok(seconds)) = (monitor[1].parse::<u64>(), monitor[2].parse::<u64>())
        else {
            return Some(Err(format!(
                "CLAUDE-MONITOR {} {}",
                &monitor[1], &monitor[2]
            )));
        };
        let (words, report) = (monitor[3].to_owned(), monitor[4].to_owned());
        return Some(
            watch_a_job(window, events, seconds, &report).map(|(job, watching)| {
                (
                    vec![Step::starting(&job), Step::arming(&watching, MONITOR_MS)],
                    Reply::Text(words),
                )
            }),
        );
    }
    let job = pattern::once(
        &JOB,
        r"(?m)^CLAUDE-JOB ((?:--(?:silent|exec|at-once|asks|stopped) )*)(\d+) (.+?) :: (.+)$",
    )
    .captures(text)?;
    let Ok(seconds) = job[2].parse::<u64>() else {
        return Some(Err(format!("CLAUDE-JOB {}", &job[2])));
    };
    let how = How {
        silent: job[1].contains("--silent"),
        exec: job[1].contains("--exec"),
        at_once: job[1].contains("--at-once"),
        asks: job[1].contains("--asks"),
        stopped: job[1].contains("--stopped"),
    };
    let (words, report) = (job[3].to_owned(), job[4].to_owned());
    Some(
        start_a_job(window, seconds, how, &report)
            .map(|id| (vec![Step::starting(&id)], Reply::Text(words))),
    )
}

/// How a `CLAUDE-JOB`'s job goes: its end writes no notification (`silent`), its shell
/// `exec`s it (`exec`), it starts as the turn does, with no thought first (`at_once`), Claude
/// Code wakes the agent while it runs, as one that waits for input (`asks`), and its end is
/// written to the transcript alone (`stopped`).
#[derive(Debug, Clone, Copy)]
struct How {
    silent: bool,
    exec: bool,
    at_once: bool,
    asks: bool,
    stopped: bool,
}

/// Starts the job of a `CLAUDE-JOB`: a command the agent's tool runs, which runs `seconds`,
/// and the id Claude Code gives it. Its end wakes the window with the notification, unless
/// `how` says it is silent.
fn start_a_job(window: &Window, seconds: u64, how: How, report: &str) -> Result<String, String> {
    // An agent thinks before it runs a tool: the daemon marks where a launch's turn begins
    // when its message shows in the record, up to a pass after the agent began. `at_once`
    // starts the job before that mark (Astra's review of round 3).
    if !how.at_once {
        thread::sleep(THINKING);
    }
    let id = window.tasks.begin();
    let notice = if how.stopped {
        transcript_only(&stopped(&id))
    } else {
        notification(&id, report)
    };
    let (tasks, wake) = (window.tasks.clone(), window.wake.clone());
    let ended = move || {
        tasks.ended();
        if !how.silent {
            let _ = wake.send(notice);
        }
    };
    if how.asks {
        let (wake, asking) = (window.wake.clone(), waiting(&id));
        thread::spawn(move || {
            thread::sleep(ASKS_AFTER);
            let _ = wake.send(asking);
        });
    }
    let file = if how.exec {
        window
            .jobs
            .start_in_place_of_its_shell("job", seconds, ended)?
    } else {
        window.jobs.start("job", seconds, false, false, ended)?
    };
    until_up(&file)?;
    Ok(id)
}

/// Starts the job of a `CLAUDE-MONITOR` and the monitor that watches it: the ids Claude
/// Code gives each. The monitor's events come while the job runs, `events` of them spread
/// over its `seconds`; the job's end wakes the window, and so does the monitor's own end
/// once the case lets it ([`MONITOR_MAY_END`]), whose turn answers `report`.
fn watch_a_job(
    window: &Window,
    events: u64,
    seconds: u64,
    report: &str,
) -> Result<(String, String), String> {
    thread::sleep(THINKING);
    let (job, watching) = (window.tasks.begin(), window.tasks.begin());
    let (tasks, wake) = (window.tasks.clone(), window.wake.clone());
    let (ended_job, ended_monitor) = (
        notification(&job, MONITOR_WATCHES),
        monitor_ended(&watching, report),
    );
    let started: Arc<OnceLock<PathBuf>> = Arc::new(OnceLock::new());
    let job_file = Arc::clone(&started);
    let ended = move || {
        tasks.ended();
        let _ = wake.send(ended_job);
        if let Some(file) = job_file.get() {
            until_let_end(file);
        }
        tasks.ended();
        let _ = wake.send(ended_monitor);
    };
    let file = window.jobs.start("job", seconds, false, false, ended)?;
    let _ = started.set(file.clone());
    until_up(&file)?;
    let (wake, id) = (window.wake.clone(), watching.clone());
    thread::spawn(move || {
        let gap = std::time::Duration::from_millis(seconds * 1_000 / (events + 1));
        for number in 1..=events {
            thread::sleep(gap);
            let _ = wake.send(monitor_event(&id, &format!("tick {number}")));
        }
    });
    Ok((job, watching))
}

/// Waits for the case to let the monitor that watches the job `file` end, or for
/// [`MONITOR_HELD`].
fn until_let_end(file: &Path) {
    let mut gate = file.as_os_str().to_owned();
    gate.push(".");
    gate.push(MONITOR_MAY_END);
    let gate = PathBuf::from(gate);
    let deadline = Instant::now() + MONITOR_HELD;
    while !gate.exists() && Instant::now() < deadline {
        thread::sleep(MONITOR_LOOK);
    }
}

/// Writes the steps of a turn, in order, as Claude Code does: the assistant's message that
/// said something and called the tool, and the user's record of the tool's result. `ordinal`
/// numbers the messages, as a turn's own answer is numbered.
pub fn write(records: &mut Records, pid: u32, session: &str, steps: &[Step]) -> io::Result<()> {
    for step in steps {
        let number = records.ordinal();
        let (message, call) = (
            format!("{session}-step-{pid}-{number}"),
            format!("toolu-{pid}-{number}"),
        );
        records.append(
            pid,
            json!({
                "type": "assistant",
                "message": {
                    "id": message,
                    "role": "assistant",
                    "content": [
                        { "type": "text", "text": step.said },
                        {
                            "type": "tool_use", "id": call,
                            "name": if step.monitor.is_some() { "Monitor" } else { "Bash" },
                            "input": {},
                        },
                    ],
                    "stop_reason": "tool_use",
                },
            }),
        )?;
        records.append(pid, result_of(&call, step))?;
    }
    Ok(())
}

/// The record of the result of the call `call` of `step`: the job's start where it ran one
/// in the background, the monitor's where it armed one, as Claude Code words them.
fn result_of(call: &str, step: &Step) -> Value {
    let said = match (&step.job, &step.monitor) {
        (Some(id), _) => format!(
            "Command running in background with ID: {id}. Output is being written to: /tmp/claude/tasks/{id}.output. You will be notified when it completes."
        ),
        (None, Some((id, _))) => format!(
            "Monitor started (task {id}, expires in 30m unless the source ends first; you get one notice at expiry — re-arm if you still need the watch). You will be notified on each event."
        ),
        (None, None) => "ok".to_owned(),
    };
    let mut record = json!({
        "type": "user",
        "isSidechain": false,
        "message": {
            "role": "user",
            "content": [{ "type": "tool_result", "tool_use_id": call, "content": said }],
        },
    });
    if let Some(id) = &step.job {
        record["toolUseResult"] = json!({
            "stdout": "", "stderr": "", "interrupted": false, "isImage": false,
            "backgroundTaskId": id,
        });
    }
    if let Some((id, timeout)) = &step.monitor {
        record["toolUseResult"] =
            json!({ "taskId": id, "timeoutMs": timeout, "persistent": false });
    }
    record
}

/// What is written of a notification Claude Code takes as the turn it begins: the message
/// queued, and the user's record of it, from the system, with the same words.
pub fn write_notification(records: &mut Records, pid: u32, text: &str) -> io::Result<()> {
    records.append(
        pid,
        json!({ "type": "queue-operation", "operation": "enqueue", "content": text }),
    )?;
    records.append(
        pid,
        json!({
            "type": "user",
            "isSidechain": false,
            "promptSource": "system",
            "turnOrigin": "task_notification",
            "message": { "role": "user", "content": text },
        }),
    )
}

/// What is written of a notification Claude Code writes to the transcript alone (2.1.292, of
/// the jobs a resumed session's last process left): queued, taken, and the user's record of
/// it, from the system and for the transcript only (`queueTranscriptOnly`). No turn takes it:
/// the window answers nothing, and stays as it was.
pub fn write_for_the_transcript_alone(
    records: &mut Records,
    pid: u32,
    text: &str,
) -> io::Result<()> {
    records.append(
        pid,
        json!({ "type": "queue-operation", "operation": "enqueue", "content": text }),
    )?;
    records.append(
        pid,
        json!({ "type": "queue-operation", "operation": "dequeue" }),
    )?;
    records.append(
        pid,
        json!({
            "type": "user",
            "isSidechain": false,
            "origin": { "kind": "task-notification" },
            "promptSource": "system",
            "queueSkipAttachments": true,
            "queueTranscriptOnly": true,
            "message": { "role": "user", "content": text },
        }),
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::agents::Agents;
    use crate::background::{Jobs, Server};
    use crate::stop::Stop;

    fn window() -> Window {
        Window {
            participant: "worker".into(),
            settings: None,
            session: "sess".into(),
            jobs: Arc::new(Jobs::new(Path::new("/nowhere/consensflow"), 1)),
            server: Server::Never,
            agents: Agents::default(),
            tasks: Tasks::default(),
            wake: mpsc::channel().0,
            stop: Arc::new(Stop::default()),
            seeded: false,
            put_back: Arc::default(),
        }
    }

    #[test]
    fn a_steps_line_is_that_many_lines_of_progress_and_then_its_words() {
        let (worked, answer) = reply("STEPS 3 The report", &window())
            .expect("a steps line")
            .expect("it is made");
        assert_eq!(answer, Reply::Text("The report".to_owned()));
        let said: Vec<&str> = worked.iter().map(|step| step.said.as_str()).collect();
        assert_eq!(said, ["Step 1 of 3", "Step 2 of 3", "Step 3 of 3"]);
        assert!(worked.iter().all(|step| step.job.is_none()));
        // Anywhere in the message, as a task delivered to a window has its header first.
        assert!(reply(
            "[ConsensFlow m-4 · T-1 · task from @chief]\nSTEPS 1 Done",
            &window()
        )
        .is_some());
    }

    #[test]
    fn a_monitors_event_is_answered_with_the_line_it_saw_and_its_end_with_the_report() {
        let (worked, answer) = reply(&monitor_event("b2", "tick 1"), &window())
            .expect("an event")
            .expect("it is made");
        assert!(worked.is_empty());
        assert_eq!(answer, Reply::Text("Saw tick 1".to_owned()));
        assert!(!monitor_event("b2", "tick 1").contains("<status>"));
        let (_, answer) = reply(&monitor_ended("b2", "The run passed"), &window())
            .expect("an end")
            .expect("it is made");
        assert_eq!(answer, Reply::Text("The run passed".to_owned()));
        assert_eq!(event_of("<event>tick 1</event>"), None, "no notification");
    }

    #[test]
    fn a_monitor_armed_is_a_monitor_call_whose_result_says_its_task_and_time() {
        let step = Step::arming("b2", 1_800_000);
        let record = result_of("toolu-1", &step);
        assert_eq!(
            record["toolUseResult"],
            json!({ "taskId": "b2", "timeoutMs": 1_800_000, "persistent": false })
        );
    }

    #[test]
    fn a_notification_is_answered_with_the_report_it_carries() {
        let (worked, answer) = reply(&notification("b1", "All 33 mutants are killed."), &window())
            .expect("a notification")
            .expect("it is made");
        assert!(worked.is_empty());
        assert_eq!(answer, Reply::Text("All 33 mutants are killed.".to_owned()));
    }

    #[test]
    fn a_notification_that_a_job_waits_for_input_has_no_status_and_is_answered_that_it_still_waits()
    {
        let asking = waiting("b1");
        assert!(is_notification(&asking));
        assert!(asking.contains("<task-id>b1</task-id>"));
        assert!(!asking.contains("<status>"), "{asking}");
        assert!(asking.contains("appears to be waiting for interactive input"));
        let (worked, answer) = reply(&asking, &window())
            .expect("a notification")
            .expect("it is made");
        assert!(worked.is_empty());
        assert_eq!(answer, Reply::Text(STILL_WAITING.to_owned()));
    }

    #[test]
    fn a_job_stopped_with_its_session_is_written_to_the_transcript_alone() {
        let said = stopped("b2");
        assert!(
            said.contains("<task-id>b2</task-id>") && said.contains("<status>stopped</status>")
        );
        assert_eq!(
            for_the_transcript_alone(&transcript_only(&said)),
            Some(said.as_str())
        );
        assert_eq!(for_the_transcript_alone(&said), None);
    }

    #[test]
    fn a_line_that_is_none_of_these_or_names_no_seconds_is_none_of_the_steps() {
        let started = Instant::now();
        for line in [
            "Reply with exactly: X",
            "STEPS many words",
            "CLAUDE-JOB soon words :: report",
            "CLAUDE-JOB 5 words with no report",
            "say CLAUDE-JOB 5 words :: report",
            "CLAUDE-JOB --loud 5 words :: report",
            "CLAUDE-JOB --exec soon words :: report",
        ] {
            assert!(reply(line, &window()).is_none(), "{line}");
        }
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "it started nothing"
        );
    }

    #[test]
    fn a_notification_names_its_job_and_says_what_its_turn_answers() {
        let said = notification("b2", "All 33 mutants are killed.");
        assert!(said.starts_with("<task-notification>\n<task-id>b2</task-id>\n"));
        assert_eq!(
            report_of(&said).as_deref(),
            Some("All 33 mutants are killed.")
        );
        // A message that merely quotes the tags is none.
        assert_eq!(report_of("<result>x</result>"), None);
        assert_eq!(report_of("the <task-notification> block"), None);
    }

    #[test]
    fn tasks_are_numbered_as_they_begin_and_counted_until_they_end() {
        let tasks = Tasks::default();
        assert_eq!(
            (tasks.begin(), tasks.begin()),
            ("b1".to_owned(), "b2".to_owned())
        );
        assert_eq!(tasks.running(), 2);
        tasks.ended();
        assert_eq!(tasks.running(), 1);
        assert_eq!(tasks.begin(), "b3");
    }

    #[test]
    fn the_steps_of_a_turn_are_a_message_and_its_result_each_and_the_job_says_its_id() {
        let config = tempfile::tempdir().unwrap();
        let mut records = Records::new(config.path(), "sess-1", 4242).unwrap();
        write(
            &mut records,
            4242,
            "sess-1",
            &[Step::progress(1, 2), Step::starting("b1")],
        )
        .unwrap();
        let text = std::fs::read_to_string(config.path().join("projects/integration/sess-1.jsonl"))
            .unwrap();
        let written: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let types: Vec<(&str, bool)> = written
            .iter()
            .map(|record| {
                (
                    record["type"].as_str().unwrap(),
                    record["toolUseResult"]["backgroundTaskId"] == "b1",
                )
            })
            .collect();
        assert_eq!(
            types,
            [
                ("assistant", false),
                ("user", false),
                ("assistant", false),
                ("user", true)
            ]
        );
        assert_eq!(written[0]["message"]["content"][0]["text"], "Step 1 of 2");
        assert_eq!(written[0]["message"]["stop_reason"], "tool_use");
        // The result closes the call the message made.
        assert_eq!(
            written[1]["message"]["content"][0]["tool_use_id"],
            written[0]["message"]["content"][1]["id"]
        );
        assert_eq!(written[3]["isSidechain"], false);
    }

    #[test]
    fn a_notification_is_written_as_claude_code_writes_it_queued_and_then_the_users_own_from_the_system(
    ) {
        let config = tempfile::tempdir().unwrap();
        let mut records = Records::new(config.path(), "sess-1", 4242).unwrap();
        let said = notification("b1", "Done");
        write_notification(&mut records, 4242, &said).unwrap();
        let text = std::fs::read_to_string(config.path().join("projects/integration/sess-1.jsonl"))
            .unwrap();
        let written: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(written[0]["operation"], "enqueue");
        assert_eq!(written[0]["content"], said.as_str());
        assert_eq!(written[1]["promptSource"], "system");
        assert_eq!(written[1]["turnOrigin"], "task_notification");
        assert_eq!(written[1]["message"]["content"], said.as_str());
    }
}

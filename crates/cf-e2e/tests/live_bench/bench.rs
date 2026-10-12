//! What the steps of the bench share: the checks they record (each said as it is
//! recorded, with what was seen), the human's typing into the chief's window, and
//! the reads of the board, the inboxes and the windows they judge by.

use std::time::{Duration, Instant};

use cf_e2e::live::members::{find, Members};
use cf_e2e::live::report::Check;
use cf_e2e::live::Log;
use cf_e2e::Result;
use serde_json::{json, Value};

/// How long the bench waits for what a step needs of the chief, a worker or the
/// daemon (the first words typed into the chief's window, a task, a delivery).
pub const WAIT: Duration = Duration::from_secs(300);

/// How often it looks.
const LOOK: Duration = Duration::from_secs(1);

/// The bench: the project it runs and the checks it recorded so far.
pub struct Bench<'a> {
    pub members: &'a Members,
    pub log: &'a Log,
    checks: Vec<Check>,
}

impl<'a> Bench<'a> {
    /// The bench of the project `members` opened.
    pub fn new(members: &'a Members, log: &'a Log) -> Self {
        Self::continued(members, log, Vec::new())
    }

    /// The bench of the same project, after the app started again, which has the
    /// checks the bench of before recorded.
    pub fn continued(members: &'a Members, log: &'a Log, checks: Vec<Check>) -> Self {
        Self {
            members,
            log,
            checks,
        }
    }

    /// The checks recorded, which the bench ends with.
    pub fn into_checks(self) -> Vec<Check> {
        self.checks
    }

    /// Records a check: it is said as it is recorded, `PASS` or `FAIL`, its name and
    /// what was seen.
    pub fn record(&mut self, name: &str, ok: bool, detail: &Value) {
        self.log.say(format!(
            "{} {name} {detail}",
            if ok { "PASS" } else { "FAIL" }
        ));
        self.checks
            .push(Check::new("bench", name, ok, detail.to_string()));
    }

    /// The human types into the chief's window once it is idle: false when it never
    /// was (a dialog of its own left open), so that the step fails and the bench goes
    /// on to the next.
    pub fn tell(&self, text: &str) -> bool {
        self.members
            .rig()
            .tell_within(self.members.project_id(), text, WAIT)
            .is_ok()
    }

    /// What `look` first finds within [`WAIT`], looked for every second.
    pub fn wait<T>(
        &self,
        what: &str,
        look: impl FnMut() -> Result<Option<T>>,
    ) -> Result<Option<T>> {
        find(self.log, what, WAIT, LOOK, look)
    }

    /// The tasks on the lane of the member `agent`.
    pub fn tasks_of(&self, agent: &str) -> Result<Vec<Value>> {
        Ok(self.members.lane(agent)?["tasks"]
            .as_array()
            .cloned()
            .unwrap_or_default())
    }

    /// The task `number` as the lane of the member `agent` has it.
    pub fn task_of(&self, agent: &str, number: i64) -> Result<Option<Value>> {
        Ok(self
            .tasks_of(agent)?
            .into_iter()
            .find(|task| task["number"] == number))
    }

    /// Every task of every lane of the board.
    pub fn board_tasks(&self) -> Result<Vec<Value>> {
        Ok(tasks_of_board(&self.members.board()?))
    }

    /// What is in the chief's inbox.
    pub fn chief_inbox(&self) -> Result<Vec<Value>> {
        self.members.inbox(Some("chief"))
    }

    /// What the board says the chief's window is doing.
    pub fn chief_activity(&self) -> Result<Value> {
        Ok(self.members.lane("chief")?["activity"].clone())
    }

    /// The window a member's newest session runs in, for the pane host's output:
    /// `p<project>-<handle of its newest session>`.
    pub fn pane_of(&self, agent: &str) -> Result<String> {
        let lane = self.members.lane(agent)?;
        Ok(format!(
            "p{}-{}",
            self.members.project_id(),
            lane["participant"]["handle"].as_str().unwrap_or(agent)
        ))
    }

    /// The last `most` characters of what a member's window printed.
    pub fn output_of(&self, agent: &str, most: usize) -> Result<String> {
        Ok(last_chars(
            &self.members.rig().screen(&self.pane_of(agent)?),
            most,
        ))
    }

    /// The last `most` characters of what the chief's window printed.
    pub fn chief_output(&self, most: usize) -> String {
        last_chars(
            &self
                .members
                .rig()
                .screen(&format!("p{}-chief", self.members.project_id())),
            most,
        )
    }
}

/// Every task of every lane of a board.
pub fn tasks_of_board(board: &Value) -> Vec<Value> {
    board["lanes"]
        .as_array()
        .map(|lanes| {
            lanes
                .iter()
                .flat_map(|lane| lane["tasks"].as_array().cloned().unwrap_or_default())
                .collect()
        })
        .unwrap_or_default()
}

/// The last `most` characters of `text`.
pub fn last_chars(text: &str, most: usize) -> String {
    let count = text.chars().count();
    text.chars().skip(count.saturating_sub(most)).collect()
}

/// The first `most` characters of `text`: what a check says of a result.
pub fn first_chars(text: &str, most: usize) -> String {
    text.chars().take(most).collect()
}

/// The seconds since `started`, to the whole second as a person rounds them.
pub fn seconds_since(started: Instant) -> i64 {
    // A step takes minutes at most: the cast is of a small, positive number.
    started.elapsed().as_secs_f64().round() as i64
}

/// The detail of a check: the seconds it took, first, and what else was seen.
pub fn took(seconds: i64, more: Value) -> Value {
    let mut detail = json!({ "seconds": seconds });
    if let (Some(detail), Some(more)) = (detail.as_object_mut(), more.as_object()) {
        detail.extend(
            more.iter()
                .map(|(name, value)| (name.clone(), value.clone())),
        );
    }
    detail
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use cf_e2e::live::members::{stand_in_chief, stand_in_worker, Plan};
    use serde_json::json;

    use super::*;
    use crate::FAKE_AGENT;

    /// What the steps read of a project, held to what the daemon says of one: the stand-in
    /// is the chief and its member, the human types a dispatch into the chief's window as the
    /// bench types its tasks, and the board, the lane, the inbox and the windows are read as the
    /// steps read them.
    #[test]
    fn the_steps_read_the_board_the_lanes_the_inbox_and_the_windows_as_the_daemon_says_them() {
        let log = Log::new("live bench test");
        let plan = Plan::new("bench-test", stand_in_chief()).with(stand_in_worker());
        let mut members = Members::open(&plan, Path::new(FAKE_AGENT), &log).unwrap();
        let tier = members.add("worker", &[]).unwrap();
        let bench = Bench::new(&members, &log);
        assert!(bench.tell(&format!(
            "DISPATCH --tier {tier} SLEEP 3 Reply with exactly: BENCH_TEST"
        )));

        // The chief's task is on the worker's lane, and the worker's window is its session's pane.
        let task = bench
            .wait("the task the chief gave", || {
                Ok(bench
                    .tasks_of("worker")?
                    .into_iter()
                    .find(|task| task["requester"] == "chief"))
            })
            .unwrap()
            .unwrap();
        let number = task["number"].as_i64().unwrap();
        assert_eq!(number, 1);
        // A task of a tier is for the pool of workers; only a follow-up (`--after`), which goes to
        // the window that did the task before it, is for none: what the continuation looks for.
        assert_eq!(task["pool"], "worker", "{task}");
        let pane = bench.pane_of("worker").unwrap();
        assert!(
            pane.starts_with(&format!("p{}-worker-", members.project_id())),
            "{pane}"
        );
        assert_eq!(bench.board_tasks().unwrap().len(), 1);
        assert!(bench.chief_activity().unwrap()["state"].is_string());

        // It ends, and its result is delivered to the chief.
        bench
            .wait("the task to be done", || {
                Ok(bench
                    .task_of("worker", number)?
                    .filter(|task| task["state"] == "done"))
            })
            .unwrap()
            .unwrap();
        let delivered = bench
            .wait("the result to be delivered", || {
                Ok(bench.chief_inbox()?.into_iter().find(|message| {
                    message["kind"] == "result"
                        && message["taskNumber"] == number
                        && message["state"] == "delivered"
                }))
            })
            .unwrap()
            .unwrap();
        assert_eq!(delivered["body"], "BENCH_TEST");
        // What a window printed is what the steps quote of it when a check fails.
        assert!(bench
            .output_of("worker", 2_000)
            .unwrap()
            .contains("fake agent started on"));
        assert!(bench.chief_output(2_000).contains("fake agent started on"));
        assert!(bench.output_of("nobody", 10).unwrap().is_empty());
        drop(bench);
        members.close().unwrap();
    }

    #[test]
    fn the_end_and_the_start_of_a_text_are_cut_in_characters_not_bytes() {
        assert_eq!(last_chars("abcdef", 3), "def");
        assert_eq!(last_chars("ab", 5), "ab");
        assert_eq!(last_chars("ééé—x", 2), "—x");
        assert_eq!(first_chars("ééé—x", 4), "ééé—");
        assert_eq!(first_chars("", 3), "");
    }

    #[test]
    fn the_tasks_of_a_board_are_those_of_all_its_lanes_in_the_order_of_the_lanes() {
        let board = json!({ "lanes": [
            { "tasks": [{ "number": 1 }, { "number": 2 }] },
            { "tasks": [] },
            { "participant": "chief" },
            { "tasks": [{ "number": 3 }] },
        ] });
        let numbers: Vec<Value> = tasks_of_board(&board)
            .iter()
            .map(|task| task["number"].clone())
            .collect();
        assert_eq!(numbers, [json!(1), json!(2), json!(3)]);
        assert!(tasks_of_board(&json!({})).is_empty());
    }

    #[test]
    fn what_a_check_says_has_the_seconds_it_took_first_and_then_what_was_seen() {
        assert_eq!(
            took(12, json!({ "task": 3 })).to_string(),
            "{\"seconds\":12,\"task\":3}"
        );
        assert_eq!(took(0, json!({})).to_string(), "{\"seconds\":0}");
        // What is no object adds nothing.
        assert_eq!(took(5, Value::Null), json!({ "seconds": 5 }));
    }

    #[test]
    fn a_step_is_timed_to_the_whole_second() {
        assert_eq!(seconds_since(Instant::now()), 0);
        let long_ago = Instant::now()
            .checked_sub(Duration::from_millis(2_600))
            .unwrap();
        assert_eq!(seconds_since(long_ago), 3);
    }
}

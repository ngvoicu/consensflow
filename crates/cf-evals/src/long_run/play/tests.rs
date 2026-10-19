//! What a disruption reads and how it waits, with no rig: the board, the owner's
//! refusals, a wait that looks as it waits, the compaction the chief's record says,
//! and each effect a disruption must see, on a stage written by hand.

use std::cell::RefCell;
use std::collections::BTreeMap;

use serde_json::json;

use super::*;
use crate::long_run::ledger::{Conversation, Participant};
use crate::long_run::record::Record;

#[test]
fn the_tasks_of_a_board_in_a_state_are_read_from_its_lanes_and_from_what_no_lane_has() {
    let board = json!({
        "lanes": [
            { "tasks": [{ "number": 1, "state": "working" }, { "number": 2, "state": "done" }] },
            { "tasks": [{ "number": 3, "state": "paused" }] },
        ],
        "open": [{ "number": 4, "state": "paused" }, { "number": 5, "state": "waiting" }],
    });
    assert_eq!(tasks_in(&board, &["working", "waiting"]), [1, 5]);
    assert_eq!(tasks_in(&board, &["paused"]), [3, 4]);
    assert!(tasks_in(&json!({}), &["paused"]).is_empty());
}

#[test]
fn a_refusal_of_the_owners_ask_is_said_and_a_page_that_failed_is_the_error() {
    let refused = refusal(Err(Error::Refused(
        "task.resume: {\"ok\":false,\"error\":\"T-1 is done\"}".to_owned(),
    )))
    .unwrap();
    assert_eq!(
        refused.as_deref(),
        Some("task.resume: {\"ok\":false,\"error\":\"T-1 is done\"}")
    );
    assert_eq!(refusal(Ok(json!({ "ok": true }))).unwrap(), None);
    // A daemon that does not answer at all is no refusal.
    assert!(refusal(Err(Error::Usage("the app is not running".to_owned()))).is_err());
}

#[test]
fn a_wait_looks_between_its_asks_and_asks_once_more_when_its_time_is_up() {
    // The watch goes on in a disruption's waits: a dialog that waits too long while
    // a switch waits is found then, not when the switch is over.
    let mut looks = 0;
    let came = wait_ticking(
        &mut looks,
        Duration::from_millis(50),
        Duration::from_millis(10),
        |looks| *looks += 1,
        |looks| Ok(*looks >= 3),
    )
    .unwrap();
    assert!(came);
    assert_eq!(looks, 3, "a look between each ask");
    let mut asked = 0;
    let never = wait_ticking(
        &mut asked,
        Duration::from_millis(30),
        Duration::from_millis(10),
        |_| {},
        |_| Ok(false),
    )
    .unwrap();
    assert!(!never);
}

/// A look whose chief's Claude record is `lines`.
fn chief_look(lines: &[Value]) -> Look {
    let mut look = Look::default();
    look.ledger.participants.push(Participant {
        id: 2,
        handle: "chief".to_owned(),
        role: "chief".to_owned(),
        harness: Some("claude-code".to_owned()),
        agent: Some("long-chief".to_owned()),
    });
    look.ledger.conversations.push(Conversation {
        id: 1,
        participant: 2,
        harness: "claude-code".to_owned(),
        session: Some("s-chief".to_owned()),
        started_at: None,
        ended_at: None,
    });
    let text: Vec<String> = lines.iter().map(Value::to_string).collect();
    look.records.insert(1, Record::claude(&text.join("\n")));
    look
}

/// A compaction boundary of Claude Code's, written at `at` (milliseconds).
fn boundary(at: i64) -> Value {
    let at = jiff::Timestamp::from_millisecond(at).unwrap();
    json!({ "timestamp": crate::clock::iso(at), "type": "system", "subtype": "compact_boundary" })
}

#[test]
fn a_compaction_counts_only_when_it_is_new_since_the_keys() {
    let pressed = crate::clock::millis("2026-10-15T10:05:00.000Z").unwrap();
    assert!(compacted_after(
        &chief_look(&[boundary(pressed + 30_000)]),
        0,
        pressed
    ));
    // One written half a second before the keys is none of this /compact's.
    assert!(!compacted_after(
        &chief_look(&[boundary(pressed - 500)]),
        0,
        pressed
    ));
    // Nor is one that was there before them, however its time reads.
    assert!(!compacted_after(
        &chief_look(&[boundary(pressed + 30_000)]),
        1,
        pressed
    ));
    let noted = json!({ "timestamp": "2026-10-15T10:05:02.000Z", "type": "assistant",
        "message": { "stop_reason": "end_turn", "content": [{ "type": "text", "text": "noted: /compact" }] } });
    assert!(!compacted_after(&chief_look(&[noted]), 0, pressed));
    assert!(!compacted_after(&Look::default(), 0, pressed));
}

/// What the owner's resume of a task does on the stage written by hand.
#[derive(Clone, Copy)]
enum Resume {
    GoesOn,
    StaysPaused,
    Refused,
    /// Refused, the task having been done in the hold.
    RefusedDone,
}

/// A stage written by hand: a chief, a Devin worker with T-7, and what the owner's
/// asks do to them.
struct Script {
    project: RefCell<&'static str>,
    chief_pane: RefCell<Value>,
    worker_pane: RefCell<Value>,
    states: RefCell<BTreeMap<i64, String>>,
    resume: Resume,
    /// The windows the app opens again at the Resume after a restart.
    reopens: Vec<String>,
    opened: RefCell<Vec<String>>,
}

impl Script {
    fn new(resume: Resume) -> Self {
        Self {
            project: RefCell::new("open"),
            chief_pane: RefCell::new(json!({ "id": "p1-chief", "generation": 1 })),
            worker_pane: RefCell::new(json!({ "id": "p1-long-devin-1", "generation": 1 })),
            states: RefCell::new(BTreeMap::from([(7, "working".to_owned())])),
            resume,
            reopens: Vec::new(),
            opened: RefCell::new(Vec::new()),
        }
    }
}

impl Stagehand for Script {
    fn board(&self) -> Result<Value> {
        let tasks: Vec<Value> = self
            .states
            .borrow()
            .iter()
            .map(|(number, state)| json!({ "number": number, "state": state }))
            .collect();
        let (project, chief, worker) = (
            *self.project.borrow(),
            self.chief_pane.borrow().clone(),
            self.worker_pane.borrow().clone(),
        );
        Ok(json!({
            "project": { "state": project },
            "lanes": [
                { "participant": { "handle": "chief", "role": "chief" }, "activity": { "state": "idle" },
                  "pane": chief, "tasks": [] },
                { "participant": { "handle": "long-devin-1", "role": "worker" }, "activity": { "state": "working" },
                  "pane": worker, "tasks": tasks },
            ],
        }))
    }

    fn page(&self, op: &str, body: Value) -> Result<Value> {
        match op {
            "project.close" => {
                *self.project.borrow_mut() = "suspended";
                *self.chief_pane.borrow_mut() = Value::Null;
                for state in self.states.borrow_mut().values_mut() {
                    *state = "paused".to_owned();
                }
            }
            "project.resume" => {
                *self.project.borrow_mut() = "open";
                *self.chief_pane.borrow_mut() = json!({ "id": "p1-chief", "generation": 2 });
                self.opened
                    .borrow_mut()
                    .extend(self.reopens.iter().cloned());
            }
            "task.resume" => {
                let number = body["task"].as_i64().unwrap_or_default();
                let mut states = self.states.borrow_mut();
                let state = states.entry(number).or_default();
                match self.resume {
                    Resume::GoesOn => *state = "working".to_owned(),
                    Resume::StaysPaused => {}
                    Resume::Refused => {
                        return Err(Error::Refused(format!("task.resume: T-{number} refused")))
                    }
                    Resume::RefusedDone => {
                        *state = "done".to_owned();
                        return Err(Error::Refused(format!("task.resume: T-{number} is done")));
                    }
                }
            }
            _ => {}
        }
        Ok(json!({ "ok": true }))
    }

    fn state_of(&self, number: i64) -> Result<String> {
        Ok(self
            .states
            .borrow()
            .get(&number)
            .cloned()
            .unwrap_or_default())
    }

    fn screen(&self, _: &Value) -> Vec<String> {
        vec!["❯ ".to_owned()]
    }

    fn press(&self, _: &Value, _: &[u8]) -> Result {
        Ok(())
    }

    fn kill(&mut self, handle: &str) -> Result<(String, i64, String)> {
        Err(Error::Refused(format!("@{handle} has no window to kill")))
    }

    fn exited(&self, _: &str, _: i64) -> bool {
        false
    }

    fn opened(&self, pane: &str) -> bool {
        self.opened.borrow().iter().any(|opened| opened == pane)
    }

    fn restart(&mut self) -> Result {
        *self.project.borrow_mut() = "suspended";
        *self.chief_pane.borrow_mut() = Value::Null;
        *self.worker_pane.borrow_mut() = Value::Null;
        self.opened.borrow_mut().clear();
        Ok(())
    }

    fn look(&mut self) -> Result<Look> {
        Ok(Look::default())
    }
}

/// A watch written by hand: its last look, and the look the first tick brings.
#[derive(Default)]
struct Looks {
    last: Option<Look>,
    then: Option<Look>,
}

impl Watch for Looks {
    fn tick(&mut self, _: &mut dyn Stagehand) {
        if let Some(then) = self.then.take() {
            self.last = Some(then);
        }
    }

    fn last(&self) -> Option<&Look> {
        self.last.as_ref()
    }
}

fn with() -> With {
    With {
        hold: Duration::ZERO,
        switch_to: None,
        within: Duration::from_millis(150),
        every: Duration::from_millis(5),
    }
}

fn played(
    stage: &mut Script,
    watch: &mut Looks,
    disruption: Disruption,
    target: &Target,
) -> Outcome {
    play(stage, watch, disruption, target, &with()).unwrap()
}

fn said(outcome: &Outcome) -> &str {
    match outcome {
        Outcome::Played(said) | Outcome::Missed(said) => said,
        Outcome::Failed { said, .. } => said,
    }
}

#[test]
fn a_suspend_whose_tasks_stay_paused_after_the_owners_resume_failed() {
    let outcome = played(
        &mut Script::new(Resume::StaysPaused),
        &mut Looks::default(),
        Disruption::Suspend,
        &Target::Project,
    );
    assert!(matches!(outcome, Outcome::Failed { .. }), "{outcome:?}");
    assert!(
        said(&outcome)
            .ends_with("the close paused T-7, and T-7 stayed paused after the owner's resume"),
        "{}",
        said(&outcome)
    );
}

#[test]
fn a_suspend_whose_resume_was_refused_failed_unless_the_task_had_ended() {
    let refused = played(
        &mut Script::new(Resume::Refused),
        &mut Looks::default(),
        Disruption::Suspend,
        &Target::Project,
    );
    assert!(matches!(refused, Outcome::Failed { .. }), "{refused:?}");
    assert!(said(&refused).contains("the resume of T-7, paused, was refused"));
    let ended = played(
        &mut Script::new(Resume::RefusedDone),
        &mut Looks::default(),
        Disruption::Suspend,
        &Target::Project,
    );
    assert!(matches!(ended, Outcome::Played(_)), "{ended:?}");
}

#[test]
fn a_suspend_whose_tasks_went_on_was_played() {
    let outcome = played(
        &mut Script::new(Resume::GoesOn),
        &mut Looks::default(),
        Disruption::Suspend,
        &Target::Project,
    );
    assert_eq!(
        outcome,
        Outcome::Played(
            "the owner closed the project, and resumed it 0 s later; the close paused T-7, and the owner resumed them"
                .to_owned()
        )
    );
}

#[test]
fn a_devin_cut_whose_workers_window_was_not_opened_again_failed() {
    let target = Target::Window {
        handle: "long-devin-1".to_owned(),
        task: Some(7),
    };
    let mut stage = Script::new(Resume::GoesOn);
    let outcome = played(
        &mut stage,
        &mut Looks::default(),
        Disruption::DevinCut,
        &target,
    );
    assert!(matches!(outcome, Outcome::Failed { .. }), "{outcome:?}");
    assert!(said(&outcome).ends_with("@long-devin-1's window p1-long-devin-1 was not opened again"));
    // Its task paused, or done, is no recovered window either: the window is asked for.
    let mut stage = Script::new(Resume::GoesOn);
    stage.states.borrow_mut().insert(7, "paused".to_owned());
    let outcome = played(
        &mut stage,
        &mut Looks::default(),
        Disruption::DevinCut,
        &target,
    );
    assert!(matches!(outcome, Outcome::Failed { .. }), "{outcome:?}");
    // The Resume opened it again.
    let mut stage = Script::new(Resume::GoesOn);
    stage.reopens = vec!["p1-long-devin-1".to_owned()];
    let outcome = played(
        &mut stage,
        &mut Looks::default(),
        Disruption::DevinCut,
        &target,
    );
    assert!(matches!(outcome, Outcome::Played(_)), "{outcome:?}");
    assert!(said(&outcome).ends_with("@long-devin-1's window p1-long-devin-1 was opened again"));
}

#[test]
fn a_compact_whose_chief_wrote_no_new_compaction_failed() {
    let now = jiff::Timestamp::now().as_millisecond();
    // The chief compacted half a second before the keys, and did nothing with them.
    let old = chief_look(&[boundary(now - 500)]);
    let mut watch = Looks {
        last: Some(old.clone()),
        then: Some(old),
    };
    let outcome = played(
        &mut Script::new(Resume::GoesOn),
        &mut watch,
        Disruption::Compact,
        &Target::Project,
    );
    assert!(matches!(outcome, Outcome::Failed { .. }), "{outcome:?}");
    assert!(said(&outcome).ends_with("no new compaction came into its record"));
    // A boundary written after the keys, one more than there was.
    let mut watch = Looks {
        last: Some(chief_look(&[boundary(now - 500)])),
        then: Some(chief_look(&[boundary(now - 500), boundary(now + 60_000)])),
    };
    let outcome = played(
        &mut Script::new(Resume::GoesOn),
        &mut watch,
        Disruption::Compact,
        &Target::Project,
    );
    assert!(matches!(outcome, Outcome::Played(_)), "{outcome:?}");
}

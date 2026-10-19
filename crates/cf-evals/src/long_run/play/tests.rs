//! What a disruption reads and how it waits, with no rig: the board, the owner's
//! refusals, a wait that looks as it waits, the compaction the chief's record says,
//! and each effect a disruption must see, on a stage written by hand.

use std::cell::RefCell;
use std::collections::BTreeMap;

use serde_json::json;

use super::*;
use crate::long_run::ledger::{Conversation, Event, Participant};
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
    /// The state the task is in after the owner's pause, by the time the board is read
    /// (queued, where another resumed it since); none where the pause never takes.
    paused_to: Option<&'static str>,
    /// The state T-7 goes to when the worker's window is killed; none where it cannot be.
    killed_then: Option<&'static str>,
    killed: bool,
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
            paused_to: None,
            killed_then: None,
            killed: false,
        }
    }

    /// [`Script::new`], T-7 in `to` after the owner's pause.
    fn pausing(resume: Resume, to: Option<&'static str>) -> Self {
        Self {
            paused_to: to,
            ..Self::new(resume)
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
            "task.pause" => {
                let number = body["task"].as_i64().unwrap_or_default();
                if let Some(to) = self.paused_to {
                    self.states.borrow_mut().insert(number, to.to_owned());
                }
            }
            "task.resume" => {
                let number = body["task"].as_i64().unwrap_or_default();
                let mut states = self.states.borrow_mut();
                let state = states.entry(number).or_default();
                // The daemon resumes only a paused task.
                if state != "paused" {
                    return Err(Error::Refused(format!(
                        "task.resume: {{\"ok\":false,\"error\":\"cannot resume T-{number}: it is {state}\"}}"
                    )));
                }
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
        let Some(state) = self.killed_then else {
            return Err(Error::Refused(format!("@{handle} has no window to kill")));
        };
        self.killed = true;
        self.states.borrow_mut().insert(7, state.to_owned());
        Ok((
            "p1-long-devin-1".to_owned(),
            1,
            format!("@{handle}'s window was killed"),
        ))
    }

    fn exited(&self, _: &str, _: i64) -> bool {
        self.killed
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

/// A move of T-7 in the ledger: its event's id, the state it left, the one it went to,
/// who moved it, and when, in seconds from now (the owner's ask comes about now).
type Move = (i64, &'static str, &'static str, Option<&'static str>, i64);

/// A look whose ledger holds the moves of T-7 `moves`.
fn moves_of_seven(moves: &[Move]) -> Look {
    let now = jiff::Timestamp::now().as_millisecond();
    let mut look = Look::default();
    look.ledger.events = moves
        .iter()
        .map(|(id, from, to, by, secs)| Event {
            id: *id,
            at: crate::clock::iso(jiff::Timestamp::from_millisecond(now + secs * 1000).unwrap()),
            kind: "task.state".to_owned(),
            data: json!({ "task": 7, "from": from, "to": to, "by": by }),
        })
        .collect();
    look
}

#[test]
fn a_pause_another_resumed_before_the_owner_was_played_and_the_owners_refused_resume_is_said() {
    // zeewin's run on 0cedce70: the owner paused T-2 (working → paused, event 75); the
    // Codex chief resumed it itself 16 s later (paused → queued, event 80, message 10); the
    // owner's resume at +60 s was refused ("cannot resume T-2: it is queued"). The pause
    // took effect: no break.
    let mut watch = Looks {
        last: Some(moves_of_seven(&[BEFORE, PAUSE, CHIEFS])),
        then: None,
    };
    let outcome = played(
        &mut Script::pausing(Resume::GoesOn, Some("queued")),
        &mut watch,
        Disruption::Pause,
        &Target::Task(7),
    );
    assert!(matches!(outcome, Outcome::Played(_)), "{outcome:?}");
    assert!(
        said(&outcome).contains("by @chief (event 80)"),
        "{}",
        said(&outcome)
    );
    assert!(
        said(&outcome).contains("the owner's resume was refused: task.resume:"),
        "{}",
        said(&outcome)
    );
}

/// zeewin's moves of the task (as T-7): the one before the pause, the owner's pause (the
/// page's, by the human), and the chief's resume 16 s later.
const BEFORE: Move = (74, "queued", "working", None, -60);
const PAUSE: Move = (75, "working", "paused", Some("human"), 1);
const CHIEFS: Move = (80, "paused", "queued", Some("chief"), 17);

#[test]
fn a_pause_the_owners_ask_did_not_make_is_a_break_whoever_else_paused_the_task() {
    // Both reviewers' (round 16): the watch last read the ledger to event 74; the chief
    // paused and resumed T-7 at 75 and 80 before the owner's pause, which was taken and did
    // nothing. Nor is a pause of the chief's after the ask, or one of the owner's in an
    // earlier disruption, the owner's pause of this one: a break, never played.
    let cases: [&[Move]; 3] = [
        &[
            BEFORE,
            (75, "working", "paused", Some("chief"), -30),
            (80, "paused", "queued", Some("chief"), -20),
        ],
        &[
            BEFORE,
            (75, "working", "paused", Some("chief"), 1),
            (80, "paused", "queued", Some("chief"), 2),
        ],
        &[
            (60, "working", "paused", Some("human"), -300),
            (61, "paused", "queued", Some("human"), -290),
            BEFORE,
        ],
    ];
    for moves in cases {
        let mut watch = Looks {
            last: Some(moves_of_seven(&[BEFORE])),
            then: Some(moves_of_seven(moves)),
        };
        let mut stage = Script::pausing(Resume::GoesOn, None);
        stage.states.borrow_mut().insert(7, "queued".to_owned());
        let outcome = played(&mut stage, &mut watch, Disruption::Pause, &Target::Task(7));
        assert!(
            matches!(outcome, Outcome::Failed { .. }),
            "{moves:?}: {outcome:?}"
        );
    }
}

#[test]
fn a_kill_whose_task_another_resumed_before_any_read_of_it_is_proven_by_consensflows_pause() {
    // Astra's (filed in round 16): @long-devin-1's window is killed; ConsensFlow pauses T-7
    // in its own name (event 75), and the chief resumes it (event 80) before the run reads
    // the board. The ledger's pause proves the kill's; the owner's refused resume is said.
    let target = Target::Window {
        handle: "long-devin-1".to_owned(),
        task: Some(7),
    };
    let mut watch = Looks {
        last: Some(moves_of_seven(&[BEFORE])),
        then: Some(moves_of_seven(&[
            BEFORE,
            (75, "working", "paused", None, 1),
            CHIEFS,
        ])),
    };
    let mut stage = Script::new(Resume::GoesOn);
    stage.killed_then = Some("queued");
    let outcome = played(&mut stage, &mut watch, Disruption::Kill, &target);
    assert!(matches!(outcome, Outcome::Played(_)), "{outcome:?}");
    assert!(
        said(&outcome).contains("by @chief (event 80)"),
        "{}",
        said(&outcome)
    );
    // Gone on to waiting with no pause of ConsensFlow's: the kill's effect was not seen.
    let mut watch = Looks {
        last: Some(moves_of_seven(&[BEFORE])),
        then: Some(moves_of_seven(&[
            BEFORE,
            (75, "working", "waiting", None, 1),
        ])),
    };
    let mut stage = Script::new(Resume::GoesOn);
    stage.killed_then = Some("waiting");
    let outcome = played(&mut stage, &mut watch, Disruption::Kill, &target);
    assert!(matches!(outcome, Outcome::Failed { .. }), "{outcome:?}");
    assert!(
        said(&outcome).ends_with("T-7 went waiting, not paused"),
        "{}",
        said(&outcome)
    );
}

#[test]
fn a_pause_resumed_by_another_before_any_read_of_it_is_proven_by_its_move_to_paused() {
    // The chief resumed it before the run read it paused: the ledger's move proves it.
    let mut watch = Looks {
        last: Some(moves_of_seven(&[BEFORE])),
        then: Some(moves_of_seven(&[BEFORE, PAUSE, CHIEFS])),
    };
    let outcome = played(
        &mut Script::pausing(Resume::GoesOn, Some("queued")),
        &mut watch,
        Disruption::Pause,
        &Target::Task(7),
    );
    assert!(matches!(outcome, Outcome::Played(_)), "{outcome:?}");
}

#[test]
fn a_pause_that_never_moved_the_task_to_paused_failed() {
    // The owner's pause was taken, and T-7 went on working: no read of it and no move of it
    // says paused, a move elsewhere (to waiting) none either.
    let mut watch = Looks {
        last: Some(moves_of_seven(&[BEFORE])),
        then: Some(moves_of_seven(&[
            BEFORE,
            (75, "working", "waiting", Some("human"), 1),
        ])),
    };
    let outcome = played(
        &mut Script::pausing(Resume::GoesOn, None),
        &mut watch,
        Disruption::Pause,
        &Target::Task(7),
    );
    assert!(matches!(outcome, Outcome::Failed { .. }), "{outcome:?}");
    assert!(
        said(&outcome).contains("after the owner's pause"),
        "{}",
        said(&outcome)
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

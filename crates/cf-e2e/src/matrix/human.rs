//! The scripted human: types each ask into the chief's terminal as a person does, through the
//! pane host's human door (`Rig::types`, `pane.typed`), the words and then Enter a moment
//! apart (`live_pacing::evidence::says`), and proves the ask went in: the human's marker in a
//! user item of the chief's own record, and a turn of the chief's begun after it was typed.
//! Never `Human::tells`, `Members::give` or `as_chief`, which act without the model.
//!
//! Every wait is bounded by the scenario's absolute deadline and its turn ceiling, checked
//! first and in that order at each look, and moved by nothing the windows do. Enter is pressed
//! again at most once in a scenario, and only when the record shows the ask was not sent and
//! the chief began no turn.

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::live::members::Members;
use crate::live::Log;
use crate::Result;

use super::observe::{self, Snapshot};

/// How often a wait looks.
const LOOK: Duration = Duration::from_secs(2);

/// How long an ask has to show in the chief's record once Enter is pressed, before the
/// human looks at whether it was sent at all.
const IN_RECORD: Duration = Duration::from_secs(45);

/// How long the chief has to begin its turn once the ask is in its record.
const TURN_BEGUN: Duration = Duration::from_secs(60);

/// How long the human waits between the words and the Enter, as a person does (a line and
/// its Enter in one burst is a paste to Devin, which keeps the line in its box).
const ENTER_AFTER: Duration = Duration::from_millis(700);

/// The scenario's bounds: an absolute deadline from its start and a ceiling on the turns all
/// the windows begin together.
#[derive(Debug, Clone, Copy)]
pub struct Bounds {
    started: Instant,
    deadline: Duration,
    turns: u32,
}

impl Bounds {
    /// Bounds that start now.
    pub fn starting(deadline: Duration, turns: u32) -> Self {
        Self {
            started: Instant::now(),
            deadline,
            turns,
        }
    }

    /// Why the scenario is over, where it is: the deadline first, then the turns (`taken`).
    pub fn over(&self, taken: u32) -> Option<String> {
        let elapsed = self.started.elapsed();
        if elapsed >= self.deadline {
            return Some(format!(
                "the scenario's deadline of {} s passed ({} s since its start)",
                self.deadline.as_secs(),
                elapsed.as_secs()
            ));
        }
        if taken > self.turns {
            return Some(format!(
                "the windows began {taken} turns, over the scenario's ceiling of {}",
                self.turns
            ));
        }
        None
    }

    /// The time since the scenario started.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
}

/// What a wait came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Waited<T> {
    Found(T),
    /// The wait's own time ran out first.
    NotYet,
    /// The scenario's bounds ended it.
    Over(String),
}

/// One ask, as the evidence says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    pub step: &'static str,
    pub marker: String,
    pub text: String,
    /// When the words were typed (the daemon's clock: milliseconds since the epoch).
    pub typed_ms: i64,
    /// When the human saw them in the chief's record.
    pub in_record_ms: Option<i64>,
    /// When the chief's turn after them began, as the board saw it.
    pub turn_ms: Option<i64>,
    /// Whether Enter was pressed again, the record showing it was not sent.
    pub enter_again: bool,
    /// What became of it, where it did not go in.
    pub failed: Option<String>,
}

impl Said {
    /// Whether it went in and the chief began its turn.
    pub fn went_in(&self) -> bool {
        self.in_record_ms.is_some() && self.turn_ms.is_some()
    }

    /// As the evidence has it.
    pub fn manifest(&self) -> Value {
        json!({
            "step": self.step,
            "marker": self.marker,
            "text": self.text,
            "typedAt": stamp(self.typed_ms),
            "inRecordAt": self.in_record_ms.map(stamp),
            "turnAt": self.turn_ms.map(stamp),
            "enterAgain": self.enter_again,
            "failed": self.failed,
        })
    }
}

/// `ms` as a time of day a person reads.
fn stamp(ms: i64) -> String {
    jiff::Timestamp::from_millisecond(ms).map_or_else(|_| ms.to_string(), |at| at.to_string())
}

/// The human, and the scenario's bounds every wait of theirs is held to.
pub struct Driver<'a> {
    pub members: &'a Members,
    pub log: &'a Log,
    pub bounds: Bounds,
    /// The record kind of the chief's harness.
    chief_kind: &'static str,
    /// What the human typed, in order.
    pub said: Vec<Said>,
    /// Whether the one Enter again is still the human's to press.
    enter_left: bool,
}

impl<'a> Driver<'a> {
    pub fn new(
        members: &'a Members,
        log: &'a Log,
        bounds: Bounds,
        chief_kind: &'static str,
    ) -> Self {
        Self {
            members,
            log,
            bounds,
            chief_kind,
            said: Vec::new(),
            enter_left: true,
        }
    }

    /// Waits for `look`, every [`LOOK`], within `within` and the scenario's bounds, which are
    /// checked first at each look.
    pub fn wait<T>(
        &self,
        what: &str,
        within: Duration,
        mut look: impl FnMut() -> Result<Option<T>>,
    ) -> Result<Waited<T>> {
        self.log
            .say(format!("waiting up to {} s for {what}", within.as_secs()));
        let began = Instant::now();
        loop {
            if let Some(why) = self.over_now()? {
                self.log.say(format!("{what}: over: {why}"));
                return Ok(Waited::Over(why));
            }
            if let Some(found) = look()? {
                return Ok(Waited::Found(found));
            }
            if began.elapsed() >= within {
                self.log
                    .say(format!("{what}: not within {} s", within.as_secs()));
                return Ok(Waited::NotYet);
            }
            std::thread::sleep(LOOK);
        }
    }

    /// The chief's pane as the pane host takes it: its newest generation.
    fn chief_pane(&self) -> Value {
        let id = observe::pane_id(self.members, "chief");
        let generation = self
            .members
            .rig()
            .open_frames()
            .into_iter()
            .rev()
            .find(|frame| frame["id"] == id.as_str())
            .and_then(|frame| frame["generation"].as_u64())
            .unwrap_or(1);
        json!({ "id": id, "generation": generation })
    }

    /// The chief's activity on the board.
    fn chief_state(&self) -> Result<String> {
        Ok(self.members.lane("chief")?["activity"]["state"]
            .as_str()
            .unwrap_or_default()
            .to_owned())
    }

    /// Where the human's `marker` is in the chief's record.
    fn in_record(&self, marker: &str) -> Result<Option<usize>> {
        let events = self.members.events();
        let items = observe::record(self.members, &events, self.chief_kind, "chief")?;
        Ok(observe::user_item_with(&items, marker))
    }

    /// When the chief's window went to `working` at or after `since_ms`, as the events say.
    fn turn_since(&self, since_ms: i64) -> Option<i64> {
        self.members.events().iter().find_map(|event| {
            let at = at_ms(event)?;
            (event["kind"] == "window.activity"
                && event["participant"] == "chief"
                && event["state"] == "working"
                && at >= since_ms)
                .then_some(at)
        })
    }

    /// Types the words, then Enter apart. Words of several lines are pasted, as a person
    /// pastes them (`ESC [200~ … ESC [201~`); the Enter is a key of its own either way.
    fn type_words(&self, words: &str) -> Result {
        let pane = self.chief_pane();
        let bytes = if words.contains('\n') {
            format!("\u{1b}[200~{words}\u{1b}[201~")
        } else {
            words.to_owned()
        };
        self.members.rig().types(&pane, bytes.as_bytes())?;
        std::thread::sleep(ENTER_AFTER);
        self.press_enter()
    }

    fn press_enter(&self) -> Result {
        let pane = self.chief_pane();
        self.members.rig().types(&pane, b"\r")
    }

    /// The human asks `text` for the step `step`, with `marker` before it, once the chief is
    /// idle: what came of it is the last of [`Driver::said`]. The error is the rig's; an ask
    /// that did not go in, or that the bounds ended, is said there and is no error.
    pub fn asks(&mut self, step: &'static str, marker: &str, text: &str) -> Result<&Said> {
        let words = format!("{marker} {text}");
        let mut said = Said {
            step,
            marker: marker.to_owned(),
            text: words.clone(),
            typed_ms: 0,
            in_record_ms: None,
            turn_ms: None,
            enter_again: false,
            failed: None,
        };
        let idle = self.wait("the chief to be idle", self.bounds.deadline, || {
            Ok((self.chief_state()? == "idle").then_some(()))
        })?;
        if let Some(failed) = failed(&idle, "the chief never went idle") {
            said.failed = Some(failed);
            return Ok(self.keep(said));
        }
        said.typed_ms = jiff::Timestamp::now().as_millisecond();
        self.log.say(format!(
            "the human types: {}",
            crate::live::cut(&words, 160)
        ));
        self.type_words(&words)?;
        let mut seen = self.wait("the ask in the chief's record", IN_RECORD, || {
            self.in_record(marker)
        })?;
        if seen == Waited::NotYet && self.enter_left && self.turn_since(said.typed_ms).is_none() {
            // The record has no such item and the chief began no turn: the words wait unsent
            // in its box. One Enter again, once in the scenario.
            self.enter_left = false;
            said.enter_again = true;
            self.log
                .say("the ask is not in the chief's record and no turn began: Enter again, once");
            self.press_enter()?;
            seen = self.wait(
                "the ask in the chief's record, after Enter again",
                IN_RECORD,
                || self.in_record(marker),
            )?;
        }
        if let Some(failed) = failed(&seen, "the ask never showed in the chief's record") {
            said.failed = Some(failed);
            return Ok(self.keep(said));
        }
        said.in_record_ms = Some(jiff::Timestamp::now().as_millisecond());
        let turn = self.wait("the chief's turn after the ask", TURN_BEGUN, || {
            Ok(self.turn_since(said.typed_ms))
        })?;
        match turn {
            Waited::Found(at) => said.turn_ms = Some(at),
            other => said.failed = failed(&other, "the chief began no turn after the ask"),
        }
        Ok(self.keep(said))
    }

    fn keep(&mut self, said: Said) -> &Said {
        self.said.push(said);
        self.said
            .last()
            .unwrap_or_else(|| unreachable!("one was just kept"))
    }

    /// The human's asks that went into the chief's record.
    pub fn asks_in(&self) -> u32 {
        u32::try_from(
            self.said
                .iter()
                .filter(|said| said.in_record_ms.is_some())
                .count(),
        )
        .unwrap_or(u32::MAX)
    }

    /// Why the scenario is over, by its bounds, now: its deadline, then the turns every
    /// window began ([`Snapshot::turns`]).
    pub fn over_now(&self) -> Result<Option<String>> {
        if let Some(why) = self.bounds.over(0) {
            return Ok(Some(why));
        }
        let taken = Snapshot::of(self.members)?.total_turns(self.asks_in());
        Ok(self.bounds.over(taken))
    }
}

/// What a wait that found nothing says, or none where it found.
fn failed<T>(waited: &Waited<T>, not_yet: &str) -> Option<String> {
    match waited {
        Waited::Found(_) => None,
        Waited::NotYet => Some(not_yet.to_owned()),
        Waited::Over(why) => Some(why.clone()),
    }
}

/// The milliseconds of an event's `at`.
pub fn at_ms(event: &Value) -> Option<i64> {
    event["at"]
        .as_str()?
        .parse::<jiff::Timestamp>()
        .ok()
        .map(|at| at.as_millisecond())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_deadline_is_checked_before_the_turns_and_neither_is_moved_by_anything() {
        let open = Bounds::starting(Duration::from_secs(60), 20);
        assert_eq!(open.over(20), None);
        assert!(open
            .over(21)
            .unwrap()
            .contains("over the scenario's ceiling of 20"));
        let past = Bounds {
            started: Instant::now().checked_sub(Duration::from_secs(61)).unwrap(),
            deadline: Duration::from_secs(60),
            turns: 20,
        };
        // Over both: the deadline is what is said.
        assert!(past
            .over(99)
            .unwrap()
            .starts_with("the scenario's deadline of 60 s passed"));
        assert!(past.over(0).is_some());
    }

    #[test]
    fn an_ask_went_in_only_when_its_record_and_its_turn_were_both_seen() {
        let mut said = Said {
            step: "advice",
            marker: "[m]".to_owned(),
            text: "[m] ask".to_owned(),
            typed_ms: 1_700_000_000_000,
            in_record_ms: None,
            turn_ms: None,
            enter_again: false,
            failed: None,
        };
        assert!(!said.went_in());
        said.in_record_ms = Some(1_700_000_001_000);
        assert!(!said.went_in());
        said.turn_ms = Some(1_700_000_001_500);
        assert!(said.went_in());
        let manifest = said.manifest();
        assert_eq!(manifest["typedAt"], "2023-11-14T22:13:20Z");
        assert_eq!(manifest["enterAgain"], false);
    }

    #[test]
    fn a_wait_that_found_nothing_says_why_and_one_that_found_says_nothing() {
        assert_eq!(failed(&Waited::Found(1), "x"), None);
        assert_eq!(
            failed::<()>(&Waited::NotYet, "not yet").as_deref(),
            Some("not yet")
        );
        assert_eq!(
            failed::<()>(&Waited::Over("late".to_owned()), "x").as_deref(),
            Some("late")
        );
    }

    #[test]
    fn an_events_time_is_read_to_the_millisecond() {
        let event = json!({ "at": "2026-10-05T10:00:00.250Z" });
        assert_eq!(at_ms(&event), Some(1_791_194_400_250));
        assert_eq!(at_ms(&json!({})), None);
    }
}

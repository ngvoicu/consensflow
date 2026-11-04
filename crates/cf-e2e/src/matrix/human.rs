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

use super::machine::{self, Question, Sending, Sent};
use super::observe::{self, Snapshot};

/// How often a wait looks.
pub const LOOK: Duration = Duration::from_secs(2);

/// How long the human waits between two keys of a dialog's answer.
const KEY_PAUSE: Duration = Duration::from_millis(300);

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
    /// Read at each key, so a window opened again (after a restart) is followed.
    fn chief_pane(&self) -> Value {
        let id = observe::pane_id(self.members, "chief");
        let generation = machine::newest_generation(&self.members.rig().open_frames(), &id);
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

    /// Whether the chief's record has an item of the assistant's after the user item that holds
    /// `marker`: its turn on the ask began, whatever the board's looks at its activity caught.
    fn answered_in_record(&self, marker: &str) -> Result<bool> {
        let events = self.members.events();
        let items = observe::record(self.members, &events, self.chief_kind, "chief")?;
        Ok(observe::answered_after(&items, marker))
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
        let mut typed = Instant::now();
        loop {
            if let Some(why) = self.over_now()? {
                said.failed = Some(why);
                return Ok(self.keep(said));
            }
            let seen = Sending {
                since: typed.elapsed(),
                in_record: self.in_record(marker)?.is_some(),
                turn_begun: self.turn_since(said.typed_ms).is_some(),
                entered_again: said.enter_again,
                enter_left: self.enter_left,
            };
            match machine::submission(seen) {
                Sent::In => break,
                Sent::Wait => std::thread::sleep(LOOK),
                Sent::EnterAgain => {
                    // The record has no such item and the chief began no turn: the words wait
                    // unsent in its box. One Enter again, once in the scenario.
                    self.enter_left = false;
                    said.enter_again = true;
                    self.log.say(
                        "the ask is not in the chief's record and no turn began: Enter again, once",
                    );
                    self.press_enter()?;
                    typed = Instant::now();
                }
                Sent::Lost(why) => {
                    said.failed = Some(why);
                    return Ok(self.keep(said));
                }
            }
        }
        said.in_record_ms = Some(jiff::Timestamp::now().as_millisecond());
        let turn = self.wait("the chief's turn after the ask", TURN_BEGUN, || {
            // The board's look at the window, or else the record: a turn of a second can fall
            // between two of the board's looks, and its answer is in the record all the same.
            if let Some(at) = self.turn_since(said.typed_ms) {
                return Ok(Some(at));
            }
            Ok(self
                .answered_in_record(marker)?
                .then(|| jiff::Timestamp::now().as_millisecond()))
        })?;
        match turn {
            Waited::Found(at) => said.turn_ms = Some(at),
            other => said.failed = failed(&other, "the chief began no turn after the ask"),
        }
        Ok(self.keep(said))
    }

    /// The chief's open question to the human in `snapshot`, where it has one: its harness's
    /// own dialog while its window waits on one (a Devin chief's read from Devin's store, with
    /// its options, which the human has the keys for; any other's unrecognised), or the words
    /// of a turn it ended asking, since the human last typed.
    pub fn question(&self, snapshot: &Snapshot) -> Result<Option<Question>> {
        let lane = snapshot
            .lanes
            .iter()
            .find(|lane| lane["participant"]["handle"] == "chief");
        let activity = lane.map_or(Value::Null, |lane| lane["activity"].clone());
        match activity["state"].as_str() {
            Some("waiting") => {
                if self.chief_kind == "devin" {
                    if let Some((id, asked)) = self.devin_dialog(&snapshot.events)? {
                        let text: Vec<&str> = asked
                            .iter()
                            .map(|(question, _)| question.as_str())
                            .collect();
                        return Ok(Some(Question {
                            id,
                            text: text.join(" "),
                            dialog: Some(true),
                        }));
                    }
                }
                let reason = activity["reason"]
                    .as_str()
                    .unwrap_or("its window waits")
                    .to_owned();
                Ok(Some(Question {
                    id: format!("waiting: {reason}"),
                    text: reason,
                    dialog: Some(false),
                }))
            }
            Some("idle") => {
                let items =
                    observe::record(self.members, &snapshot.events, self.chief_kind, "chief")?;
                Ok(open_question(&items))
            }
            _ => Ok(None),
        }
    }

    /// The questions of a Devin chief's open dialog, each with how many options it has, and
    /// the id of its call, from Devin's store where the rig keeps it.
    fn devin_dialog(&self, events: &[Value]) -> Result<Option<Dialog>> {
        let Some(session) = observe::session(self.members, events, "chief") else {
            return Ok(None);
        };
        let rig = self.members.rig();
        let store = crate::devin_store::location(
            cfg!(windows),
            rig.var("HOME").unwrap_or_default(),
            rig.var("APPDATA"),
            rig.var("XDG_DATA_HOME").filter(|data| !data.is_empty()),
        );
        if !store.is_file() {
            return Ok(None);
        }
        Ok(open_dialog(&crate::devin_store::messages_of(
            &store, &session,
        )?))
    }

    /// The human answers the open dialog of a Devin chief with `answer`, a question at a time,
    /// as the eval's owner does (Devin 3000.11.3, probed 2026-10-03): its "Other" after the
    /// options, the answer typed there.
    pub fn answer_dialog(
        &self,
        snapshot: &Snapshot,
        facts: &[machine::Fact],
        answer: &str,
    ) -> Result {
        let Some((_, asked)) = self.devin_dialog(&snapshot.events)? else {
            return Ok(());
        };
        for keys in dialog_keys(&asked, facts, answer) {
            self.members.rig().types(&self.chief_pane(), &keys)?;
            std::thread::sleep(KEY_PAUSE);
        }
        Ok(())
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

/// A Devin chief's open dialog: the id of its call, and each question with how many options
/// it has.
pub type Dialog = (String, Vec<(String, usize)>);

/// The chief's open question in its record `items`: the newest turn it ended with a question
/// (a line that ends with `?`) since the human last typed into its window (a user item that is
/// no ConsensFlow message), or none.
pub fn open_question(items: &[crate::live::ask::Item]) -> Option<Question> {
    let mut open = None;
    for item in items {
        if item.role == "user" && !item.text.starts_with("[ConsensFlow ") {
            open = None;
        } else if item.role == "assistant"
            && item.complete
            && item.text.lines().any(|line| line.trim_end().ends_with('?'))
        {
            open = Some(Question {
                id: item.id.clone(),
                text: item.text.clone(),
                dialog: None,
            });
        }
    }
    open
}

/// The newest call of Devin's question tool in `messages` (a session's, oldest first) that no
/// tool message answered: its id, and each of its questions with how many options it has.
pub fn open_dialog(messages: &[crate::devin_store::Message]) -> Option<Dialog> {
    let mut answered: Vec<&str> = Vec::new();
    for message in messages.iter().rev() {
        if let Some(call) = message.answers() {
            answered.push(call);
        }
        let Some(call) = message
            .calls()
            .into_iter()
            .find(|call| call.name == "ask_user_question")
        else {
            continue;
        };
        if answered.contains(&call.id.as_str()) {
            return None;
        }
        let raw: Value = serde_json::from_str(&call.text).unwrap_or(Value::Null);
        let questions = raw["arguments"]["questions"]
            .as_array()
            .map(|questions| {
                questions
                    .iter()
                    .map(|question| {
                        (
                            question["question"].as_str().unwrap_or_default().to_owned(),
                            question["options"].as_array().map_or(0, Vec::len),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        return Some((call.id, questions));
    }
    None
}

/// The keys that answer each question of a Devin dialog, one press at a time: Down once per
/// option to its "Other", Enter, the answer (the fact sheet's for that question, else
/// `answer`), Enter.
pub fn dialog_keys(
    asked: &[(String, usize)],
    facts: &[machine::Fact],
    answer: &str,
) -> Vec<Vec<u8>> {
    const DOWN: &[u8] = b"\x1b[B";
    let mut keys = Vec::new();
    for (question, options) in asked {
        keys.extend((0..*options).map(|_| DOWN.to_vec()));
        keys.push(b"\r".to_vec());
        keys.push(
            machine::answer_of(facts, question)
                .unwrap_or(answer)
                .as_bytes()
                .to_vec(),
        );
        keys.push(b"\r".to_vec());
    }
    keys
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

    fn item(id: &str, role: &str, text: &str, complete: bool) -> crate::live::ask::Item {
        crate::live::ask::Item {
            id: id.to_owned(),
            role: role.to_owned(),
            text: text.to_owned(),
            complete,
        }
    }

    #[test]
    fn the_chiefs_open_question_is_its_newest_turn_ending_asking_since_the_human_typed() {
        let asked = [
            item("1", "user", "[matrix r ask 1/4] Get advice.", true),
            item("2", "assistant", "Which file should change?", true),
        ];
        let open = open_question(&asked).unwrap();
        assert_eq!((open.id.as_str(), open.dialog), ("2", None));
        // A result delivered after it leaves it open; the human's own words close it.
        let mut later = asked.to_vec();
        later.push(item(
            "3",
            "user",
            "[ConsensFlow m-4 · T-1 · result from @a]\nDone.",
            true,
        ));
        assert_eq!(open_question(&later).unwrap().id, "2");
        later.push(item("4", "user", "[matrix r answer] greet.cjs", true));
        assert_eq!(open_question(&later), None);
        // A turn still at work, or one that asks nothing, is no open question.
        assert_eq!(
            open_question(&[item("5", "assistant", "Which file?", false)]),
            None
        );
        assert_eq!(
            open_question(&[item("6", "assistant", "Done. Is it? No: it is.", true)]),
            None
        );
    }

    fn message(id: &str, raw: Value) -> crate::devin_store::Message {
        crate::devin_store::Message {
            id: id.to_owned(),
            role: raw["role"].as_str().unwrap_or_default().to_owned(),
            text: String::new(),
            raw,
        }
    }

    #[test]
    fn a_devin_chiefs_dialog_is_its_newest_question_call_no_tool_answered() {
        let call = |id: &str| {
            json!({ "role": "assistant", "tool_calls": [{ "id": id, "name": "ask_user_question", "arguments": {
                "questions": [{ "question": "Which file?", "options": [{ "label": "a" }, { "label": "b" }] }]
            } }] })
        };
        let open = [
            message("1", call("c1")),
            message("2", json!({ "role": "tool", "tool_call_id": "c1" })),
            message("3", call("c2")),
        ];
        assert_eq!(
            open_dialog(&open),
            Some(("c2".to_owned(), vec![("Which file?".to_owned(), 2)]))
        );
        assert_eq!(
            open_dialog(&open[..2]),
            None,
            "the newest call was answered"
        );
        assert_eq!(open_dialog(&[]), None);
    }

    #[test]
    fn a_devin_dialog_is_answered_in_its_other_after_the_options_with_the_fact_sheets_words() {
        const FACTS: &[machine::Fact] = &[machine::Fact {
            asked_by: &["file"],
            answer: "greet.cjs",
        }];
        let keys = dialog_keys(
            &[("Which file?".to_owned(), 2), ("Colour?".to_owned(), 1)],
            FACTS,
            "all I know",
        );
        let down = b"\x1b[B".to_vec();
        assert_eq!(
            keys,
            [
                down.clone(),
                down.clone(),
                b"\r".to_vec(),
                b"greet.cjs".to_vec(),
                b"\r".to_vec(),
                down,
                b"\r".to_vec(),
                b"all I know".to_vec(),
                b"\r".to_vec(),
            ]
        );
    }

    #[test]
    fn an_events_time_is_read_to_the_millisecond() {
        let event = json!({ "at": "2026-10-05T10:00:00.250Z" });
        assert_eq!(at_ms(&event), Some(1_791_194_400_250));
        assert_eq!(at_ms(&json!({})), None);
    }
}

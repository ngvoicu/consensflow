//! What the scripted human does next, decided from what was observed and nothing else (T-1133's
//! design, section 1; LMX-04's row 2). Two machines, each a plain function of its observations,
//! so that every rule of the driver is proved offline:
//!
//! - [`Steps`], at each look while a milestone is awaited: the scenario's deadline first, then
//!   its turn ceiling, then a bound task that failed or was cancelled (which ends the run at
//!   once, as a red that names it), then the milestone, then the chief's question (answered from
//!   the scenario's fact sheet, each question once; an unknown one gets the whole sheet once,
//!   and a second unknown one is a divergence of the driver and the model, which ends the run),
//!   then a nudge (at most one per milestone and two in all, and only when the board has been
//!   settled and without progress for a while). Activity moves none of the bounds.
//! - [`submission`], after an ask is typed: it went in once the chief's record shows the
//!   human's marker; Enter is pressed again at most once in a scenario, and only when the record
//!   does not show it and the chief began no turn.
//!
//! Neither reads a model's words to judge: a question is recognised to answer it, never to pass
//! or fail anything.

use std::collections::BTreeSet;
use std::time::Duration;

/// How long the board has to be settled, with no progress, before the human nudges.
pub const NUDGE_AFTER: Duration = Duration::from_secs(150);

/// The most nudges in a scenario, and the most answers to the chief's questions.
pub const MAX_NUDGES: u32 = 2;
pub const MAX_ANSWERS: u32 = 4;

/// What the scenario says when it is asked: a fact, the words of a question that ask for it,
/// and its answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fact {
    /// Words any one of which, in the question (case aside), asks for this fact.
    pub asked_by: &'static [&'static str],
    pub answer: &'static str,
}

/// The fact sheet's answer to `question`: the first fact one of whose words is in it.
pub fn answer_of(facts: &[Fact], question: &str) -> Option<&'static str> {
    let question = question.to_lowercase();
    facts
        .iter()
        .find(|fact| fact.asked_by.iter().any(|word| question.contains(word)))
        .map(|fact| fact.answer)
}

/// The whole fact sheet, as the human types it to a question it has no single answer for.
pub fn whole_sheet(facts: &[Fact]) -> String {
    let lines: Vec<&str> = facts.iter().map(|fact| fact.answer).collect();
    format!("Everything I can tell you: {}", lines.join(" "))
}

/// A question the chief put to the human: in its terminal's words (a turn that ended asking),
/// or in its harness's own question dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    /// What tells this question from another: the record item's id, or the dialog's call's.
    pub id: String,
    pub text: String,
    /// Whether it is its harness's own dialog, and whether the human has the keys to answer
    /// one of that harness's: a dialog with none is unrecognised, never pressed blindly.
    pub dialog: Option<bool>,
}

/// What was observed at one look.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observation {
    /// Since the scenario began.
    pub elapsed: Duration,
    /// The turns every window began.
    pub turns: u32,
    /// Whether the awaited milestone is reached, as the board says.
    pub reached: bool,
    /// A bound task that failed or was cancelled: its name, number and state.
    pub ended: Option<(String, i64, String)>,
    /// The chief's open question, where it has one.
    pub question: Option<Question>,
    /// Whether the board is at rest: the chief idle, no member at work, nothing waiting to
    /// be delivered.
    pub settled: bool,
    /// What the board has done so far, as one text: it changes when a task moves or a message
    /// is delivered, and never with a window's activity alone.
    pub progress: String,
}

/// What the human does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// Looks again.
    Wait,
    /// The milestone is reached: the next step.
    Reached,
    /// Types `answer` to the question `question`.
    Answer { question: String, answer: String },
    /// Types the scenario's nudge.
    Nudge,
    /// The run is over, red, for `why`.
    End(String),
}

/// The machine of a scenario's steps: its bounds and what the human has said so far.
#[derive(Debug, Clone)]
pub struct Steps {
    deadline: Duration,
    ceiling: u32,
    facts: &'static [Fact],
    answered: BTreeSet<String>,
    answers: u32,
    whole_sheet_given: bool,
    nudges: u32,
    nudged_this_milestone: bool,
    progress: Option<(String, Duration)>,
}

impl Steps {
    pub fn new(deadline: Duration, ceiling: u32, facts: &'static [Fact]) -> Self {
        Self {
            deadline,
            ceiling,
            facts,
            answered: BTreeSet::new(),
            answers: 0,
            whole_sheet_given: false,
            nudges: 0,
            nudged_this_milestone: false,
            progress: None,
        }
    }

    /// The nudges given so far, and the answers.
    pub fn nudges(&self) -> u32 {
        self.nudges
    }

    pub fn answers(&self) -> u32 {
        self.answers
    }

    /// What to do after `seen`.
    pub fn next(&mut self, seen: &Observation) -> Next {
        if seen.elapsed >= self.deadline {
            return Next::End(format!(
                "the scenario's deadline of {} s passed ({} s since its start)",
                self.deadline.as_secs(),
                seen.elapsed.as_secs()
            ));
        }
        if seen.turns > self.ceiling {
            return Next::End(format!(
                "the windows began {} turns, over the scenario's ceiling of {}",
                seen.turns, self.ceiling
            ));
        }
        if let Some((name, number, state)) = &seen.ended {
            return Next::End(format!("T-{number}, the {name} task, is {state}"));
        }
        if seen.reached {
            self.nudged_this_milestone = false;
            self.progress = None;
            return Next::Reached;
        }
        if let Some(question) = seen
            .question
            .as_ref()
            .filter(|asked| !self.answered.contains(&asked.id))
        {
            return self.answer(question);
        }
        let moved = self
            .progress
            .as_ref()
            .is_none_or(|(was, _)| *was != seen.progress);
        if moved {
            self.progress = Some((seen.progress.clone(), seen.elapsed));
            return Next::Wait;
        }
        let still_for = self.progress.as_ref().map_or(Duration::ZERO, |(_, since)| {
            seen.elapsed.saturating_sub(*since)
        });
        if seen.settled
            && still_for >= NUDGE_AFTER
            && !self.nudged_this_milestone
            && self.nudges < MAX_NUDGES
        {
            self.nudges += 1;
            self.nudged_this_milestone = true;
            self.progress = Some((seen.progress.clone(), seen.elapsed));
            return Next::Nudge;
        }
        Next::Wait
    }

    /// The answer to `question`, or why the run ends on it.
    fn answer(&mut self, question: &Question) -> Next {
        if question.dialog == Some(false) {
            return Next::End(format!(
                "the chief opened a question dialog the human has no keys for: {}",
                cut(&question.text)
            ));
        }
        if self.answers >= MAX_ANSWERS {
            return Next::End(format!(
                "the chief asked more than the {MAX_ANSWERS} questions a scenario answers: {}",
                cut(&question.text)
            ));
        }
        let answer = match answer_of(self.facts, &question.text) {
            Some(answer) => answer.to_owned(),
            None if !self.whole_sheet_given => {
                self.whole_sheet_given = true;
                whole_sheet(self.facts)
            }
            None => {
                return Next::End(format!(
                    "the driver and the model diverge: a second question the fact sheet does not answer: {}",
                    cut(&question.text)
                ));
            }
        };
        self.answered.insert(question.id.clone());
        self.answers += 1;
        Next::Answer {
            question: question.id.clone(),
            answer,
        }
    }
}

/// What became of an ask, at one look after it was typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sent {
    /// Looks again.
    Wait,
    /// The record shows it: it went in.
    In,
    /// The record does not show it and the chief began no turn: Enter once more.
    EnterAgain,
    /// It never went in.
    Lost(String),
}

/// What was seen of an ask: how long since it was typed (or since Enter was pressed again),
/// whether the chief's record shows the human's marker, and whether a turn of the chief's began
/// since it was typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sending {
    pub since: Duration,
    pub in_record: bool,
    pub turn_begun: bool,
    /// Whether Enter was pressed again for this ask already.
    pub entered_again: bool,
    /// Whether the scenario's one Enter again is still the human's.
    pub enter_left: bool,
}

/// How long an ask has to show in the record before the human looks at whether it was sent.
pub const IN_RECORD: Duration = Duration::from_secs(45);

/// What to do about an ask, from what was seen of it.
pub fn submission(seen: Sending) -> Sent {
    if seen.in_record {
        return Sent::In;
    }
    if seen.since < IN_RECORD {
        return Sent::Wait;
    }
    // A turn began: the words went, and the record is late. They are given as long again.
    if seen.turn_begun && seen.since < IN_RECORD * 2 {
        return Sent::Wait;
    }
    if !seen.turn_begun && !seen.entered_again && seen.enter_left {
        return Sent::EnterAgain;
    }
    Sent::Lost(if seen.turn_begun {
        "the chief began a turn, and its record never showed the ask".to_owned()
    } else {
        "the ask never showed in the chief's record, and no turn began".to_owned()
    })
}

/// The newest generation of the pane `id` among the pane host's opens (a window opened again
/// after a restart has a generation of its own), and 1 where it was never opened.
pub fn newest_generation(opens: &[serde_json::Value], id: &str) -> u64 {
    opens
        .iter()
        .rev()
        .find(|open| open["id"] == id)
        .and_then(|open| open["generation"].as_u64())
        .unwrap_or(1)
}

fn cut(text: &str) -> String {
    crate::live::cut(text, 160)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const FACTS: &[Fact] = &[
        Fact {
            asked_by: &["which file"],
            answer: "greet.cjs.",
        },
        Fact {
            asked_by: &["tier"],
            answer: "Every member is at tier standard.",
        },
    ];

    fn secs(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    fn look(elapsed: u64) -> Observation {
        Observation {
            elapsed: secs(elapsed),
            progress: "p0".to_owned(),
            ..Observation::default()
        }
    }

    fn asked(id: &str, text: &str) -> Option<Question> {
        Some(Question {
            id: id.to_owned(),
            text: text.to_owned(),
            dialog: None,
        })
    }

    #[test]
    fn the_deadline_comes_before_everything_else_and_the_turns_before_the_rest() {
        let mut steps = Steps::new(secs(900), 20, FACTS);
        let everything = Observation {
            elapsed: secs(900),
            turns: 99,
            reached: true,
            ended: Some(("change".to_owned(), 2, "failed".to_owned())),
            question: asked("q", "Which file?"),
            settled: true,
            progress: "p".to_owned(),
        };
        let Next::End(why) = steps.next(&everything) else {
            panic!()
        };
        assert!(
            why.starts_with("the scenario's deadline of 900 s passed"),
            "{why}"
        );
        let Next::End(why) = steps.next(&Observation {
            elapsed: secs(10),
            ..everything.clone()
        }) else {
            panic!()
        };
        assert!(why.contains("over the scenario's ceiling of 20"), "{why}");
    }

    #[test]
    fn a_failed_or_cancelled_task_ends_the_run_at_once_naming_it_before_its_milestone() {
        for state in ["failed", "cancelled"] {
            let mut steps = Steps::new(secs(900), 20, FACTS);
            let seen = Observation {
                reached: true,
                ended: Some(("test".to_owned(), 4, state.to_owned())),
                ..look(220)
            };
            assert_eq!(
                steps.next(&seen),
                Next::End(format!("T-4, the test task, is {state}"))
            );
        }
    }

    #[test]
    fn endless_activity_never_nudges_nor_moves_the_deadline() {
        let mut steps = Steps::new(secs(900), 20, FACTS);
        let mut nexts = Vec::new();
        for at in (0..=900).step_by(30) {
            // The chief works on and on; the board never moves nor settles.
            nexts.push(steps.next(&Observation {
                settled: false,
                ..look(at)
            }));
        }
        let (last, before) = nexts.split_last().unwrap();
        assert!(before.iter().all(|next| *next == Next::Wait), "{before:?}");
        assert!(
            matches!(last, Next::End(why) if why.contains("deadline of 900 s")),
            "{last:?}"
        );
    }

    #[test]
    fn a_nudge_comes_once_a_milestone_after_the_board_is_settled_and_still_and_two_in_all() {
        let mut steps = Steps::new(secs(3600), 99, FACTS);
        let settled = |at| Observation {
            settled: true,
            ..look(at)
        };
        assert_eq!(steps.next(&settled(0)), Next::Wait);
        assert_eq!(steps.next(&settled(149)), Next::Wait);
        assert_eq!(steps.next(&settled(150)), Next::Nudge);
        // Once a milestone, however long it stays still.
        assert_eq!(steps.next(&settled(400)), Next::Wait);
        assert_eq!(
            steps.next(&Observation {
                reached: true,
                ..settled(401)
            }),
            Next::Reached
        );
        assert_eq!(steps.next(&settled(402)), Next::Wait);
        assert_eq!(steps.next(&settled(560)), Next::Nudge);
        assert_eq!(
            steps.next(&Observation {
                reached: true,
                ..settled(561)
            }),
            Next::Reached
        );
        steps.next(&settled(562));
        assert_eq!(steps.next(&settled(900)), Next::Wait, "two in all");
        assert_eq!(steps.nudges(), 2);
        // Progress puts the stillness back to its start, and a board not settled is never nudged.
        let mut again = Steps::new(secs(3600), 99, FACTS);
        again.next(&settled(0));
        again.next(&Observation {
            progress: "p1".to_owned(),
            ..settled(100)
        });
        assert_eq!(
            again.next(&Observation {
                progress: "p1".to_owned(),
                ..settled(200)
            }),
            Next::Wait
        );
        assert_eq!(
            again.next(&Observation {
                progress: "p1".to_owned(),
                settled: false,
                ..look(260)
            }),
            Next::Wait
        );
        assert_eq!(
            again.next(&Observation {
                progress: "p1".to_owned(),
                ..settled(260)
            }),
            Next::Nudge
        );
    }

    #[test]
    fn a_question_is_answered_from_the_fact_sheet_once_however_often_it_is_observed() {
        let mut steps = Steps::new(secs(900), 20, FACTS);
        let seen = Observation {
            question: asked("item-7", "Which file should change?"),
            ..look(30)
        };
        assert_eq!(
            steps.next(&seen),
            Next::Answer {
                question: "item-7".to_owned(),
                answer: "greet.cjs.".to_owned()
            }
        );
        // The same question seen again (a duplicate observation) is not answered twice.
        assert_eq!(steps.next(&seen), Next::Wait);
        assert_eq!(
            steps.next(&Observation {
                elapsed: secs(31),
                ..seen
            }),
            Next::Wait
        );
        assert_eq!(steps.answers(), 1);
    }

    #[test]
    fn an_unexpected_question_gets_the_whole_sheet_once_and_a_second_ends_the_run_as_a_divergence()
    {
        let mut steps = Steps::new(secs(900), 20, FACTS);
        let first = Observation {
            question: asked("a", "Do you prefer tabs or spaces?"),
            ..look(10)
        };
        assert_eq!(
            steps.next(&first),
            Next::Answer {
                question: "a".to_owned(),
                answer: "Everything I can tell you: greet.cjs. Every member is at tier standard."
                    .to_owned()
            }
        );
        let second = Observation {
            question: asked("b", "And what colour?"),
            ..look(20)
        };
        let Next::End(why) = steps.next(&second) else {
            panic!()
        };
        assert!(why.starts_with("the driver and the model diverge"), "{why}");
    }

    #[test]
    fn a_dialog_without_keys_is_never_pressed_and_more_than_four_questions_end_the_run() {
        let mut steps = Steps::new(secs(900), 20, FACTS);
        let dialog = Question {
            id: "call-1".to_owned(),
            text: "Pick one".to_owned(),
            dialog: Some(false),
        };
        let Next::End(why) = steps.next(&Observation {
            question: Some(dialog),
            ..look(5)
        }) else {
            panic!()
        };
        assert!(why.contains("no keys for"), "{why}");
        let keyed = Question {
            id: "call-2".to_owned(),
            text: "Which tier?".to_owned(),
            dialog: Some(true),
        };
        let mut steps = Steps::new(secs(900), 20, FACTS);
        assert!(matches!(
            steps.next(&Observation {
                question: Some(keyed),
                ..look(5)
            }),
            Next::Answer { .. }
        ));
        for n in 0..3 {
            let next = steps.next(&Observation {
                question: asked(&format!("q{n}"), "Which file?"),
                ..look(6)
            });
            assert!(matches!(next, Next::Answer { .. }), "{next:?}");
        }
        let Next::End(why) = steps.next(&Observation {
            question: asked("q9", "Which file?"),
            ..look(7)
        }) else {
            panic!()
        };
        assert!(why.contains("more than the 4 questions"), "{why}");
    }

    fn sending(since: u64) -> Sending {
        Sending {
            since: secs(since),
            in_record: false,
            turn_begun: false,
            entered_again: false,
            enter_left: true,
        }
    }

    #[test]
    fn an_unsent_paste_gets_one_enter_again_and_is_lost_after_it() {
        assert_eq!(submission(sending(10)), Sent::Wait);
        assert_eq!(submission(sending(45)), Sent::EnterAgain);
        assert_eq!(
            submission(Sending {
                entered_again: true,
                ..sending(45)
            }),
            Sent::Lost("the ask never showed in the chief's record, and no turn began".to_owned())
        );
        // The one Enter again of the scenario is spent: none for another ask.
        assert!(matches!(
            submission(Sending {
                enter_left: false,
                ..sending(50)
            }),
            Sent::Lost(_)
        ));
        assert_eq!(
            submission(Sending {
                in_record: true,
                ..sending(1)
            }),
            Sent::In
        );
    }

    #[test]
    fn a_turn_begun_is_never_given_enter_again_and_its_late_record_is_waited_for() {
        let began = Sending {
            turn_begun: true,
            ..sending(60)
        };
        assert_eq!(submission(began), Sent::Wait);
        assert!(
            matches!(submission(Sending { since: secs(90), ..began }), Sent::Lost(why) if why.contains("began a turn"))
        );
        assert_eq!(
            submission(Sending {
                in_record: true,
                ..began
            }),
            Sent::In
        );
    }

    #[test]
    fn the_pane_followed_is_its_newest_generation_across_a_restart() {
        let opens = [
            json!({ "id": "p1-chief", "generation": 1 }),
            json!({ "id": "p1-worker-a", "generation": 5 }),
            json!({ "id": "p1-chief", "generation": 2 }),
        ];
        assert_eq!(newest_generation(&opens, "p1-chief"), 2);
        assert_eq!(newest_generation(&opens[..1], "p1-chief"), 1);
        assert_eq!(newest_generation(&[], "p1-chief"), 1);
    }

    #[test]
    fn the_fact_sheet_answers_by_any_of_its_words_whatever_their_case() {
        assert_eq!(answer_of(FACTS, "WHICH FILE?"), Some("greet.cjs."));
        assert_eq!(
            answer_of(FACTS, "What tier is the worker at?"),
            Some("Every member is at tier standard.")
        );
        assert_eq!(answer_of(FACTS, "Shall I?"), None);
    }
}

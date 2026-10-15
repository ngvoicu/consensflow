//! What came of each ask and of each case, as the run says it: whether the call ran,
//! how its turn ended, the tools Devin's record says were called, every word of a
//! refusal or a permission that was seen with where and after which key of the check,
//! and, at the end of the run, a table of what each case let the window do.

use cf_e2e::live::secs;

use crate::asks::{Ask, Proof};
use crate::case::Case;
use crate::scene::Turn;
use crate::trail::Sighting;

/// How many sightings of an ask the report lists.
const LISTED: usize = 6;

/// What came of an ask.
#[derive(Debug)]
pub struct Outcome {
    pub ask: Ask,
    pub number: u32,
    pub turn: Turn,
    pub proof: Proof,
    /// The tools the agent called, and what each came back with.
    pub tools: Vec<String>,
    /// What was seen, in the places Devin says it in, while the ask was waited for.
    pub sightings: Vec<Sighting>,
    /// What the check did to the window for it.
    pub keys: String,
}

impl Outcome {
    /// Whether the call ran.
    pub fn ran(&self) -> bool {
        self.proof.ran
    }

    /// Whether Devin's words say the call was skipped, anywhere.
    pub fn skipped(&self) -> bool {
        self.sightings.iter().any(Sighting::is_skip)
    }

    /// How the wait ended, as words.
    fn turn_said(&self) -> String {
        let (turn, took) = (&self.turn, secs(self.turn.took));
        let held = turn.held.as_deref().unwrap_or_default();
        if turn.ended {
            format!("its turn ended in {took} s")
        } else if turn.closed {
            format!("its window ended in {took} s: {held}")
        } else if turn.arrived {
            format!("its turn did not end in {took} s: {held}")
        } else {
            format!("the message never showed in its record in {took} s: {held}")
        }
    }

    /// What the report says of it.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "ask {} (m-{}): the call {}: {}; {}",
            self.ask.name(),
            self.number,
            if self.ran() { "RAN" } else { "did NOT run" },
            self.proof.saw,
            self.turn_said()
        )];
        lines.push(format!(
            "  Devin's record: {}; what we did to the window: {}",
            if self.tools.is_empty() {
                "no tool called".to_owned()
            } else {
                self.tools.join("; ")
            },
            self.keys
        ));
        if self.sightings.is_empty() {
            lines.push("  no word of a permission or of a call that was not run".to_owned());
        }
        // What says the user skipped the call is listed first, whatever else was seen before it.
        let (skips, others): (Vec<&Sighting>, Vec<&Sighting>) = self
            .sightings
            .iter()
            .partition(|sighting| sighting.is_skip());
        lines.extend(
            skips
                .into_iter()
                .chain(others)
                .take(LISTED)
                .map(|sighting| format!("  seen: {}", sighting.line())),
        );
        if self.sightings.len() > LISTED {
            lines.push(format!("  and {} more", self.sightings.len() - LISTED));
        }
        lines
    }
}

/// What the sightings were, as words: none, or the first of them.
pub fn sightings_said(sightings: &[Sighting]) -> String {
    if sightings.is_empty() {
        return "no word of a permission or of a call that was not run".to_owned();
    }
    sightings
        .iter()
        .take(LISTED)
        .map(Sighting::line)
        .collect::<Vec<_>>()
        .join(" | ")
}

/// How a message and its turn went, as a line.
pub fn turn_line(what: &str, turn: &Turn) -> String {
    format!(
        "{what}: it {}, and its turn {} in {} s{}",
        if turn.arrived {
            "showed in its record"
        } else {
            "NEVER showed in its record"
        },
        if turn.ended { "ended" } else { "did NOT end" },
        secs(turn.took),
        turn.held
            .as_ref()
            .map_or_else(String::new, |held| format!("; it was held: {held}"))
    )
}

/// What the asks came to: how many calls ran, and where the first refusal was seen.
pub fn summary(outcomes: &[Outcome]) -> String {
    let ran = outcomes.iter().filter(|outcome| outcome.ran()).count();
    let skipped = outcomes.iter().filter(|outcome| outcome.skipped()).count();
    let first = outcomes
        .iter()
        .flat_map(|outcome| outcome.sightings.iter())
        .find(|sighting| sighting.is_skip());
    format!(
        "summary: {ran} of {} calls ran; {skipped} came back skipped{}",
        outcomes.len(),
        first.map_or_else(String::new, |sighting| format!(
            "; the first was seen {}",
            sighting.line()
        ))
    )
}

/// What the window was let do in a case, as a row of the table that ends the run:
/// each ask with whether its call ran, was skipped, or did not run; `-` for one that
/// was not made.
pub fn matrix_row(case: Case, outcomes: &[Outcome]) -> String {
    let cells: Vec<String> = Ask::ALL
        .iter()
        .map(|ask| {
            let state =
                outcomes
                    .iter()
                    .find(|outcome| outcome.ask == *ask)
                    .map_or("-", |outcome| match (outcome.ran(), outcome.skipped()) {
                        (true, false) => "ran",
                        (true, true) => "ran (a skip was seen)",
                        (false, true) => "SKIPPED",
                        (false, false) => "NOT run",
                    });
            format!("{} {state}", ask.name())
        })
        .collect();
    format!("{:<18} {}", case.name(), cells.join(", "))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn skip() -> Sighting {
        Sighting {
            source: "store",
            saw: "User skipped this tool call".to_owned(),
            at: Duration::from_millis(12_300),
            after: Some("Enter again for m-10 +10.4 s".to_owned()),
            excerpt: "tool: Tool execution was rejected".to_owned(),
        }
    }

    fn proof(ran: bool, saw: &str) -> Proof {
        Proof {
            ran,
            saw: saw.to_owned(),
        }
    }

    fn ended() -> Turn {
        Turn {
            arrived: true,
            ended: true,
            closed: false,
            took: Duration::from_millis(14_200),
            held: None,
        }
    }

    fn outcome(ask: Ask, proof: Proof, sightings: Vec<Sighting>, turn: Turn) -> Outcome {
        Outcome {
            ask,
            number: 10,
            turn,
            proof,
            tools: vec!["run_command answered (exit 0)".to_owned()],
            sightings,
            keys: "paste m-10 (141 chars) +0.0 s".to_owned(),
        }
    }

    #[test]
    fn an_ask_is_told_by_whether_its_call_ran_how_its_turn_ended_what_devin_recorded_and_what_we_did(
    ) {
        let ran = outcome(
            Ask::Shell,
            proof(true, "shell-ab12.txt holds \"ran-ab12\""),
            vec![],
            ended(),
        );
        assert_eq!(
            ran.lines(),
            [
                "ask shell (m-10): the call RAN: shell-ab12.txt holds \"ran-ab12\"; its turn ended in 14.2 s",
                "  Devin's record: run_command answered (exit 0); what we did to the window: paste m-10 (141 chars) +0.0 s",
                "  no word of a permission or of a call that was not run",
            ]
        );
        assert!(ran.ran() && !ran.skipped());
    }

    #[test]
    fn an_ask_that_was_skipped_says_where_it_was_seen_and_after_which_key_and_a_turn_that_did_not_end_says_what_the_window_showed(
    ) {
        let held = Turn {
            arrived: true,
            ended: false,
            closed: false,
            took: Duration::from_secs(180),
            held: Some("the window last printed 170 s ago".to_owned()),
        };
        let skipped = outcome(
            Ask::Shell,
            proof(false, "shell-ab12.txt is not there"),
            vec![skip(); 8],
            held,
        );
        let lines = skipped.lines();
        assert!(skipped.skipped() && !skipped.ran());
        assert_eq!(
            lines[0],
            "ask shell (m-10): the call did NOT run: shell-ab12.txt is not there; its turn did not end in 180.0 s: the window last printed 170 s ago"
        );
        assert_eq!(
            lines[2],
            "  seen: store \"User skipped this tool call\" at +12.3 s, after Enter again for m-10 +10.4 s: tool: Tool execution was rejected"
        );
        // Six are listed and the rest counted.
        assert_eq!(lines.len(), 2 + LISTED + 1);
        assert_eq!(lines.last().map(String::as_str), Some("  and 2 more"));
    }

    #[test]
    fn the_words_that_say_the_user_skipped_the_call_are_listed_first_whatever_was_seen_before_them()
    {
        let stalled = Sighting {
            saw: "stalled screen".to_owned(),
            at: Duration::from_secs(8),
            after: None,
            ..skip()
        };
        let mut seen = vec![stalled; 7];
        seen.push(skip());
        let asked = outcome(Ask::Shell, proof(false, "x"), seen, ended());
        let lines = asked.lines();
        assert!(
            lines[2].starts_with("  seen: store \"User skipped this tool call\" at +12.3 s"),
            "{lines:?}"
        );
        assert!(lines[3].contains("\"stalled screen\""), "{lines:?}");
        assert_eq!(lines.last().map(String::as_str), Some("  and 2 more"));
    }

    #[test]
    fn a_message_that_never_showed_and_a_window_that_ended_are_told_as_what_they_are() {
        let never = Turn {
            arrived: false,
            ended: false,
            closed: false,
            took: Duration::from_secs(180),
            held: Some("held".to_owned()),
        };
        let said = |turn: Turn| outcome(Ask::Cf, proof(false, "x"), vec![], turn).turn_said();
        assert_eq!(
            said(never.clone()),
            "the message never showed in its record in 180.0 s: held"
        );
        assert_eq!(
            said(Turn {
                closed: true,
                took: Duration::from_secs(2),
                ..never
            }),
            "its window ended in 2.0 s: held"
        );
    }

    #[test]
    fn a_turn_is_told_by_whether_its_message_showed_whether_it_ended_and_what_held_it() {
        assert_eq!(
            turn_line("the note, pasted", &ended()),
            "the note, pasted: it showed in its record, and its turn ended in 14.2 s"
        );
        let held = Turn {
            arrived: false,
            ended: false,
            took: Duration::from_secs(240),
            held: Some("the window last printed 200 s ago".to_owned()),
            ..ended()
        };
        assert_eq!(
            turn_line("its first turn did not end", &held),
            "its first turn did not end: it NEVER showed in its record, and its turn did NOT end in 240.0 s; it was held: the window last printed 200 s ago"
        );
    }

    #[test]
    fn the_sightings_are_told_as_none_or_the_first_few_of_them() {
        assert_eq!(
            sightings_said(&[]),
            "no word of a permission or of a call that was not run"
        );
        let said = sightings_said(&vec![skip(); 8]);
        assert_eq!(said.matches(" | ").count(), LISTED - 1, "six are told");
        assert!(said.starts_with("store \"User skipped this tool call\" at +12.3 s"));
    }

    #[test]
    fn what_the_asks_came_to_is_how_many_calls_ran_how_many_were_skipped_and_where_the_first_was_seen(
    ) {
        assert_eq!(
            summary(&[]),
            "summary: 0 of 0 calls ran; 0 came back skipped"
        );
        let all = [
            outcome(Ask::Shell, proof(true, ""), vec![], ended()),
            outcome(Ask::Write, proof(false, ""), vec![skip()], ended()),
            outcome(Ask::Edit, proof(false, ""), vec![], ended()),
        ];
        let said = summary(&all);
        assert!(
            said.starts_with("summary: 1 of 3 calls ran; 1 came back skipped; the first was seen store \"User skipped this tool call\" at +12.3 s, after Enter again for m-10"),
            "{said}"
        );
    }

    #[test]
    fn a_row_of_the_table_says_for_each_ask_whether_it_ran_was_skipped_did_not_run_or_was_not_made()
    {
        let outcomes = [
            outcome(Ask::Shell, proof(true, ""), vec![], ended()),
            outcome(Ask::Write, proof(false, ""), vec![skip()], ended()),
            outcome(Ask::Edit, proof(true, ""), vec![skip()], ended()),
            outcome(Ask::Cf, proof(false, ""), vec![], ended()),
        ];
        assert_eq!(
            matrix_row(Case::MidTool, &outcomes),
            "mid-tool           shell ran, write SKIPPED, edit ran (a skip was seen), cf NOT run"
        );
        assert_eq!(
            matrix_row(Case::AtRest, &outcomes[..1]),
            "at-rest            shell ran, write -, edit -, cf -"
        );
    }
}

//! What the watch of a stop saw, what the stop is judged by, and how each is said:
//! the checks that hold or do not, then each round with what followed it.

use cf_e2e::live::members::{plural, plus, Account};
use cf_e2e::live::seconds;
use serde_json::Value;

use super::rounds::rounds_of;
use crate::slow::gaps;
use crate::stop::{daemon_reading, escapes_line, late};

/// What the window looked like a moment after a round of Escapes.
pub struct Look {
    /// When it was looked at, in milliseconds since the epoch.
    pub at: i64,
    /// What the lane read.
    pub lane: String,
    /// What the window printed since the pause, as the text of its screen, in the
    /// words of the case (how many lines of numbers, or the lines it drew).
    pub printed: String,
    /// The last lines of its screen as its pane host draws it: what a person at the
    /// terminal sees, however the harness or the console host repaints it. None where
    /// the host says nothing of the window.
    pub drawn: Vec<String>,
    /// What the host's process tree says of the command the member runs, for a case
    /// whose member runs one: whether it still runs. None of a case that has none.
    pub tree: Option<String>,
    /// What the harness's own files say now, each as a phrase of a line: Devin's wire
    /// log since the pause, and what the daemon's record reader makes of its store.
    /// None of a harness whose files the case does not read.
    pub also: Vec<String>,
}

impl Look {
    /// What the look found, as the rest of a line: the lane, what the window
    /// printed, its screen as the pane host draws it, what the host's tree says of
    /// the command, and what the harness's own files say.
    fn found(&self) -> String {
        let mut found = format!(
            "the lane read {}; the window printed {}; its screen as the pane host draws it: {}",
            self.lane,
            self.printed,
            if self.drawn.is_empty() {
                "nothing".to_owned()
            } else {
                self.drawn.join(" ⏎ ")
            }
        );
        for said in self.tree.iter().chain(&self.also) {
            found.push_str("; ");
            found.push_str(said);
        }
        found
    }
}

/// What the watch of the stop saw of the board and the window.
pub struct Watched {
    /// The lane as the board read it, each change with its time.
    pub words: Vec<String>,
    /// One look after each round of Escapes, in the order of the rounds.
    pub looks: Vec<Look>,
    /// A look when the watch ended.
    pub end: Look,
    /// The board's word that the window did not stop, the first time it said it.
    pub flagged: Option<Value>,
    /// When the watch ended.
    pub ended_at: i64,
    /// When the host's tree was first read without the command the member ran: none
    /// while it still had it, and where the member runs none.
    pub command_gone: Option<i64>,
}

/// What the case judges: the stop as it came to be.
pub struct Seen {
    pub made: i64,
    /// When the watch of the stop ended.
    pub until: i64,
    pub presses: Vec<i64>,
    /// When the daemon first read the window at rest after the pause.
    pub rested: Option<i64>,
    /// The board's word that the window did not stop.
    pub flagged: Option<Value>,
    /// The notes that it did not stop, to the human and to whoever gave the task.
    pub told_human: usize,
    pub told_requester: usize,
    pub task_state: String,
    /// The closes the daemon asked of the window's pane since the pause: what a stop
    /// it took for ignored did, read from the pane host, whatever the notes say.
    pub closed: usize,
}

/// The checks of the stop: the daemon pressed, read the window at rest and paid
/// the stop there, never said it was ignored, and pressed nothing once it had
/// stopped; and the task is paused.
pub fn judge(case: &mut Account, seen: &Seen) {
    let Seen {
        made,
        until,
        presses,
        rested,
        flagged,
        told_human,
        told_requester,
        task_state,
        closed,
    } = seen;
    let rounds = rounds_of(presses);
    case.check(
        "the daemon pressed Escape",
        !presses.is_empty(),
        format!(
            "{} in {}",
            escapes_line(presses, *made),
            plural(rounds.len(), "round")
        ),
    );
    case.check(
        "the daemon read the window at rest, and paid the stop there",
        rested.is_some(),
        rested.map_or_else(
            || {
                format!(
                    "its reading of the window never turned idle in the {} s it was watched",
                    seconds(until - made)
                )
            },
            |rested| {
                let pressed = rounds
                    .iter()
                    .filter(|round| round.first().is_some_and(|first| *first < rested))
                    .count();
                format!(
                    "its reading of the window turned idle at {}, after {}",
                    plus(rested, *made),
                    plural(pressed, "round")
                )
            },
        ),
    );
    case.check(
        "it is not said to have ignored the stop",
        flagged.is_none() && told_human + told_requester == 0,
        format!(
            "unstopped on its lane: {}; notes that it did not stop: {} to the human, {} to the requester",
            flagged
                .as_ref()
                .map_or_else(|| "null".to_owned(), Value::to_string),
            told_human,
            told_requester
        ),
    );
    let late_keys = late(presses, *rested);
    case.check(
        "nothing is pressed once it has stopped",
        late_keys.is_empty(),
        format!(
            "{} more than a look after the daemon read it at rest",
            plural(late_keys.len(), "Escape")
        ),
    );
    case.check(
        "the task is paused",
        task_state == "paused",
        format!("the task is {task_state}"),
    );
    case.check(
        "its window is open, as it was",
        *closed == 0,
        format!("{} asked of its pane", plural(*closed, "close")),
    );
}

/// The lines of what the watch saw: each round with what followed it, the lane over
/// the stop, and the daemon's own reading of the window `handle`.
pub fn show(case: &mut Account, seen: &Seen, watched: &Watched, events: &[Value], handle: &str) {
    let made = seen.made;
    for (index, round) in rounds_of(&seen.presses).iter().enumerate() {
        let looked = watched.looks.get(index).map_or_else(
            || "it was not looked at after it".to_owned(),
            |look| {
                format!(
                    "looked at {}, {} s after its last key: {}",
                    plus(look.at, made),
                    seconds(look.at - round.last().copied().unwrap_or(look.at)),
                    look.found(),
                )
            },
        );
        let gaps = gaps(round);
        case.say(format!(
            "     round {}: {}{}; {looked}",
            index + 1,
            escapes_line(round, made),
            if gaps.is_empty() {
                String::new()
            } else {
                format!(" (gaps {} s)", gaps.join(", "))
            },
        ));
    }
    case.say(format!(
        "     at the end, {}: {}",
        plus(watched.end.at, made),
        watched.end.found()
    ));
    case.say(format!(
        "     the lane over the stop: {}",
        watched.words.join(", ")
    ));
    case.say(format!(
        "     the daemon's reading of the window (window.activity) from just before: {}",
        daemon_reading(events, handle, made)
    ));
}

/// The words the lane read over the stop, in the order they were first seen, from the
/// account `said` of a case: the line [`show`] says them in, `the lane over the stop:
/// working at +0.1 s, closed at +9.4 s`. None where `said` has no such line or a word of
/// it has no time.
pub fn lane_words(said: &str) -> Option<Vec<&str>> {
    let line = said
        .lines()
        .find_map(|line| line.strip_prefix("     the lane over the stop: "))?;
    line.split(", ")
        .map(|entry| {
            let (word, seconds) = entry.rsplit_once(" at +")?;
            seconds
                .strip_suffix(" s")?
                .parse::<f64>()
                .ok()
                .map(|_| word)
        })
        .collect()
}

/// Whether the lane read working over the stop and then closed: the window was at work
/// when the stop was first looked at, and the daemon closed it. When that look came is no
/// part of it: it is looked at as soon as the pause is answered, and the answer is the
/// daemon's to give when its machine lets it (a daemon that took a fifth of a second to
/// answer is a stop like any other).
pub fn lane_went_working_then_closed(said: &str) -> bool {
    lane_words(said).is_some_and(|words| words.starts_with(&["working", "closed"]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::printed::Printed;
    use super::*;

    fn lane(words: &str) -> String {
        format!("round 1: 1 Escape: +0.3 s\n     the lane over the stop: {words}\n     at the end")
    }

    #[test]
    fn a_lane_that_read_working_and_then_closed_did_so_whenever_it_was_first_looked_at() {
        // The first look is the pause's answer and the next thing the case does: a daemon
        // that answers in a few milliseconds is looked at "+0.0 s", one that took longer
        // (a loaded machine, a daemon stopped for a moment) at "+0.1 s", "+0.2 s", or more.
        for first in ["+0.0 s", "+0.1 s", "+0.2 s", "+1.5 s"] {
            let said = lane(&format!("working at {first}, closed at +9.4 s"));
            assert!(lane_went_working_then_closed(&said), "{said}");
            assert_eq!(lane_words(&said), Some(vec!["working", "closed"]));
        }
        // And more after it, as the line may say if the lane changed again.
        let said = lane("working at +0.2 s, closed at +9.4 s, idle at +11.0 s");
        assert!(lane_went_working_then_closed(&said), "{said}");
    }

    #[test]
    fn a_lane_that_never_closed_or_that_closed_without_working_is_not_that() {
        for words in [
            // The window that ignored the stop and was left open.
            "working at +0.1 s",
            // Looked at once it was closed, or at rest.
            "closed at +0.1 s",
            "working at +0.1 s, idle at +2.1 s",
            // Closed first, then working again.
            "closed at +0.1 s, working at +3.0 s",
            // Working, something else, and only then closed.
            "working at +0.1 s, idle at +2.1 s, closed at +9.4 s",
        ] {
            let said = lane(words);
            assert!(!lane_went_working_then_closed(&said), "{said}");
        }
        // No line, a word with no time, a time that is no number.
        assert_eq!(lane_words("round 1: 1 Escape: +0.3 s"), None);
        assert_eq!(lane_words(&lane("working, closed at +9.4 s")), None);
        assert_eq!(
            lane_words(&lane("working at +soon s, closed at +9.4 s")),
            None
        );
        assert!(!lane_went_working_then_closed("no such line"));
    }

    #[test]
    fn the_line_the_account_says_is_the_line_the_case_reads() {
        let seen = Seen {
            made: 0,
            until: 0,
            presses: vec![],
            rested: None,
            flagged: None,
            told_human: 0,
            told_requester: 0,
            task_state: "paused".to_owned(),
            closed: 0,
        };
        let end = Look {
            at: 9_400,
            lane: "closed".to_owned(),
            printed: Printed {
                numbers: 0,
                others: vec![],
            }
            .line(),
            drawn: vec![],
            tree: None,
            also: vec![],
        };
        let watched = Watched {
            words: vec![
                "working at +0.2 s".to_owned(),
                "closed at +9.4 s".to_owned(),
            ],
            looks: vec![],
            end,
            flagged: None,
            ended_at: 9_400,
            command_gone: None,
        };
        let mut case = Account::new();
        show(&mut case, &seen, &watched, &[], "p");
        let said = case.text();
        assert_eq!(lane_words(&said), Some(vec!["working", "closed"]), "{said}");
        assert!(lane_went_working_then_closed(&said), "{said}");
    }

    fn seen() -> Seen {
        Seen {
            made: 1_000,
            until: 4_000,
            presses: vec![1_300, 1_450, 2_500],
            rested: Some(2_900),
            flagged: None,
            told_human: 0,
            told_requester: 0,
            task_state: "paused".to_owned(),
            closed: 0,
        }
    }

    fn judged(seen: &Seen) -> Account {
        let mut case = Account::new();
        judge(&mut case, seen);
        case
    }

    #[test]
    fn a_window_that_stopped_within_the_rounds_and_was_not_said_to_ignore_the_stop_holds() {
        let case = judged(&seen());
        assert!(case.held(), "{}", case.text());
        assert_eq!(
            case.lines()[0],
            "ok   the daemon pressed Escape: 3 Escapes: +0.3 s, +0.5 s, +1.5 s in 1 round"
        );
        assert_eq!(
            case.lines()[1],
            "ok   the daemon read the window at rest, and paid the stop there: its reading of the window turned idle at +1.9 s, after 1 round"
        );
        assert_eq!(
            case.lines()[2],
            "ok   it is not said to have ignored the stop: unstopped on its lane: null; notes that it did not stop: 0 to the human, 0 to the requester"
        );
        assert_eq!(
            case.lines()[3],
            "ok   nothing is pressed once it has stopped: 0 Escapes more than a look after the daemon read it at rest"
        );
        assert_eq!(
            case.lines()[4],
            "ok   the task is paused: the task is paused"
        );
        assert_eq!(
            case.lines()[5],
            "ok   its window is open, as it was: 0 closes asked of its pane"
        );
        assert_eq!(case.lines().len(), 6);
    }

    #[test]
    fn a_window_the_daemon_never_pressed_and_one_it_never_read_at_rest_do_not_hold() {
        let unpressed = Seen {
            presses: vec![],
            rested: Some(1_800),
            ..seen()
        };
        let case = judged(&unpressed);
        assert!(!case.held());
        assert!(
            case.lines()[0].starts_with("FAIL the daemon pressed Escape: 0 Escapes: none"),
            "{}",
            case.text()
        );
        let working = Seen {
            rested: None,
            flagged: Some(json!({ "rounds": 3 })),
            told_human: 1,
            told_requester: 1,
            ..seen()
        };
        let case = judged(&working);
        assert!(!case.held());
        assert_eq!(
            case.lines()[1],
            "FAIL the daemon read the window at rest, and paid the stop there: its reading of the window never turned idle in the 3.0 s it was watched"
        );
        assert_eq!(
            case.lines()[2],
            "FAIL it is not said to have ignored the stop: unstopped on its lane: {\"rounds\":3}; notes that it did not stop: 1 to the human, 1 to the requester"
        );
    }

    #[test]
    fn a_press_a_look_after_the_window_read_at_rest_and_a_task_not_paused_do_not_hold() {
        let late_press = Seen {
            presses: vec![1_300, 1_450, 2_500, 6_000],
            ..seen()
        };
        let case = judged(&late_press);
        assert!(!case.held());
        assert_eq!(
            case.lines()[3],
            "FAIL nothing is pressed once it has stopped: 1 Escape more than a look after the daemon read it at rest"
        );
        let running = Seen {
            task_state: "working".to_owned(),
            ..seen()
        };
        let case = judged(&running);
        assert!(!case.held());
        assert_eq!(
            case.lines()[4],
            "FAIL the task is paused: the task is working"
        );
        // A close is read from the pane host: no note need say it.
        let closed = Seen {
            closed: 1,
            ..seen()
        };
        let case = judged(&closed);
        assert!(!case.held());
        assert_eq!(
            case.lines()[5],
            "FAIL its window is open, as it was: 1 close asked of its pane"
        );
    }

    #[test]
    fn each_round_is_said_with_its_keys_its_gaps_and_what_the_window_did_after_it() {
        let seen = Seen {
            presses: vec![1_300, 1_400, 2_500, 4_400, 4_500, 5_600],
            rested: None,
            ..seen()
        };
        let strings = |lines: &[&str]| -> Vec<String> {
            lines.iter().map(|line| (*line).to_owned()).collect()
        };
        let look = |at: i64,
                    lane: &str,
                    numbers: usize,
                    others: &[&str],
                    drawn: &[&str],
                    also: &[&str]| Look {
            at,
            lane: lane.to_owned(),
            printed: Printed {
                numbers,
                others: strings(others),
            }
            .line(),
            drawn: strings(drawn),
            tree: None,
            also: strings(also),
        };
        let watched = Watched {
            words: vec!["working at +0.3 s".to_owned()],
            looks: vec![
                look(
                    4_000,
                    "working",
                    380,
                    &["esc again to interrupt"],
                    &["1180", "1181", "esc again to interrupt"],
                    &[
                        "its wire log, 412 wire records since the pause: agent_message_chunk ×412",
                        "the record reader reads its transcript as NOT settled",
                    ],
                ),
                look(7_100, "working", 770, &[], &[], &[]),
            ],
            end: look(
                8_000,
                "idle",
                770,
                &["Interrupted"],
                &["✱ Canceled. What should Devin do?"],
                &[],
            ),
            flagged: None,
            ended_at: 8_000,
            command_gone: None,
        };
        let mut case = Account::new();
        show(&mut case, &seen, &watched, &[], "devin-1");
        assert_eq!(
            case.lines(),
            [
                "     round 1: 3 Escapes: +0.3 s, +0.4 s, +1.5 s (gaps 0.1, 1.1 s); looked at +3.0 s, 1.5 s after its last key: the lane read working; the window printed 380 lines of numbers; besides: esc again to interrupt; its screen as the pane host draws it: 1180 ⏎ 1181 ⏎ esc again to interrupt; its wire log, 412 wire records since the pause: agent_message_chunk ×412; the record reader reads its transcript as NOT settled",
                "     round 2: 3 Escapes: +3.4 s, +3.5 s, +4.6 s (gaps 0.1, 1.1 s); looked at +6.1 s, 1.5 s after its last key: the lane read working; the window printed 770 lines of numbers; besides: nothing; its screen as the pane host draws it: nothing",
                "     at the end, +7.0 s: the lane read idle; the window printed 770 lines of numbers; besides: Interrupted; its screen as the pane host draws it: ✱ Canceled. What should Devin do?",
                "     the lane over the stop: working at +0.3 s",
                "     the daemon's reading of the window (window.activity) from just before: no change",
            ]
        );
        // A case whose member runs a command says what the host's tree says of it, after
        // what the window drew and before what the harness's own files say.
        let with_a_command = Look {
            tree: Some("the host's tree still has the command (pid 4102: sleep 119)".to_owned()),
            ..look(
                4_000,
                "idle",
                0,
                &[],
                &["Canceled"],
                &["its wire log, none"],
            )
        };
        assert_eq!(
            with_a_command.found(),
            "the lane read idle; the window printed 0 lines of numbers; besides: nothing; its screen as the pane host draws it: Canceled; the host's tree still has the command (pid 4102: sleep 119); its wire log, none"
        );
        // A round the watch ended before it looked at is said so.
        let unlooked = Watched {
            looks: vec![],
            ..watched
        };
        let mut case = Account::new();
        show(&mut case, &seen, &unlooked, &[], "devin-1");
        assert!(
            case.lines()[0].ends_with("; it was not looked at after it"),
            "{}",
            case.lines()[0]
        );
    }
}

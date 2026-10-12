//! A Claude member given a brief, paused once the case has it where it wants it:
//! what the daemon pressed, whether the turn and the command stopped, what
//! Claude's record and status say of it, and whether the daemon says (to the board,
//! the human, the requester) that the window did not stop. The design (§1.3): a
//! window that stopped is paid at rest with no key, and is never reported as one
//! that did not.

use std::thread::sleep;
use std::time::Duration;

use cf_e2e::live::claude::{runs, Sample};
use cf_e2e::live::members::{find, plural, plus, plus_maybe, shown, Account, LongCommand, Members};
use cf_e2e::live::trace::at_ms;
use cf_e2e::live::Log;
use cf_e2e::Result;
use serde_json::Value;

use crate::watch::{
    excerpt, interrupted, kinds_line, raw_tail, settlement_line, spoke_since, told_the_human,
    unstopped, watch, Watched,
};
use crate::{now_ms, STOP};

/// How a case stops its window.
#[derive(Debug, Clone, Copy)]
pub struct Shape {
    /// The window is inside a long command, which the stop must end.
    pub command: bool,
    /// A stop before a word of Claude's answer, which it writes no interrupt record of.
    pub recordless: bool,
}

/// The case being stopped: its member, its task, and how it is stopped.
pub struct Stop<'a> {
    pub members: &'a Members,
    pub log: &'a Log,
    pub agent: &'a str,
    pub number: i64,
    pub shape: Shape,
    pub long: &'a LongCommand,
    /// How long the window at rest is watched for a press it should not get.
    pub rest: Duration,
}

/// What the stop came to, which the checks are made of.
struct Came {
    /// When Claude's status first read idle after the pause.
    idle_at: Option<i64>,
    /// When the long command's processes were gone.
    gone_at: Option<i64>,
    /// What the window's screen showed a moment after the pause and at the end.
    screens: Vec<String>,
}

impl Stop<'_> {
    /// Pauses the task, as the human's Pause does, and says what came of the stop in
    /// `case`: the checks of it, and what they were made from. The watch of the
    /// window, which a case that goes on with the task reads further of.
    pub fn run(&self, case: &mut Account) -> Result<Watched> {
        let watched = watch(self.members, self.agent)?;
        let paused = self.members.pause(self.number)?;
        let made = paused.made_ms;
        said_pause(case, self.number, &paused.answer);
        let came = self.wait_for_the_stop(&watched, made)?;
        self.judge(case, &watched, made, &came)?;
        Ok(watched)
    }

    /// Waits for the turn to stop (Claude's status reads idle), the command with it,
    /// and then for the window to be left at rest a while, to see what is pressed
    /// that should not be.
    fn wait_for_the_stop(&self, watched: &Watched, made: i64) -> Result<Came> {
        let mut gone_at: Option<i64> = None;
        let idle_at = find(
            self.log,
            "the window to read idle",
            STOP,
            Duration::from_millis(100),
            || {
                if self.shape.command && gone_at.is_none() && self.long.running()?.is_empty() {
                    gone_at = Some(now_ms());
                }
                Ok(idle_after(&watched.status.samples(), made))
            },
        )?;
        sleep(Duration::from_millis(1_500));
        let mut screens = vec![self.screen_at(watched, made)];
        if self.shape.command {
            find(
                self.log,
                "the command's processes to be gone",
                Duration::from_secs(5),
                Duration::from_millis(100),
                || Ok((gone_at.is_some() || self.long.running()?.is_empty()).then_some(())),
            )?;
            if gone_at.is_none() && self.long.running()?.is_empty() {
                gone_at = Some(now_ms());
            }
        }
        sleep(self.rest);
        screens.push(self.screen_at(watched, made));
        Ok(Came {
            idle_at,
            gone_at,
            screens,
        })
    }

    /// What the window's screen shows now, with when.
    fn screen_at(&self, watched: &Watched, made: i64) -> String {
        format!(
            "at {}: {}",
            plus(now_ms(), made),
            watched.screen(self.members)
        )
    }

    /// The checks of a stop, and the lines that show what they were made from.
    fn judge(&self, case: &mut Account, watched: &Watched, made: i64, came: &Came) -> Result<()> {
        let Came {
            idle_at,
            gone_at,
            screens,
        } = came;
        let members = self.members;
        let presses = watched.escapes(members);
        let late_keys = late(&presses, *idle_at);
        let items = watched.record(members);
        let ended = interrupted(&items, made);
        let spoke = spoke_since(&items, made);
        let events = members.events();
        let handle = watched.window.handle.as_str();
        let stuck = unstopped(&members.lane(self.agent)?);
        let human = members.inbox(None)?;
        let told = told_the_human(&human);
        let task = members.task(self.number)?;
        let number = self.number;

        case.check(
            "the daemon pressed Escape",
            !presses.is_empty(),
            escapes_line(&presses, made),
        );
        if self.shape.command {
            case.check(
                "the command stopped",
                gone_at.is_some(),
                gone_at.map_or_else(
                    || "still running".to_owned(),
                    |gone| format!("its processes were gone at {}", plus(gone, made)),
                ),
            );
        }
        case.check(
            "the turn stopped",
            idle_at.is_some(),
            idle_at.map_or_else(
                || format!("no idle in {} s", STOP.as_secs()),
                |idle| format!("status read idle at {}", plus(idle, made)),
            ),
        );
        if self.shape.recordless {
            case.check(
                "Claude stopped before a word of its answer",
                spoke.is_empty(),
                match spoke.first() {
                    None => format!(
                        "its record holds no word of its answer and {}: it says nothing of the stop",
                        ended.map_or_else(
                            || "no interrupt record".to_owned(),
                            |ended| format!(
                                "an interrupt record (\"{}\")",
                                ended.text.chars().take(60).collect::<String>()
                            )
                        )
                    ),
                    Some(first) => format!(
                        "{} records of its answer: {}",
                        spoke.len(),
                        Value::String(first.text.chars().take(80).collect())
                    ),
                },
            );
        } else {
            case.check(
                "the record says it was interrupted",
                ended.is_some(),
                ended.map_or_else(
                    || "no interrupt record in its transcript".to_owned(),
                    |ended| {
                        format!(
                            "\"{}\" at {}",
                            ended.text.chars().take(80).collect::<String>(),
                            plus_maybe(ended.at_ms, made)
                        )
                    },
                ),
            );
        }
        let rested = rested_at(&events, handle, made);
        case.check(
            "the daemon read the window at rest, and paid the stop there",
            rested.is_some(),
            rested.map_or_else(
                || "its reading of the window never turned idle".to_owned(),
                |rested| {
                    format!(
                        "its reading of the window turned idle at {}",
                        plus(rested, made)
                    )
                },
            ),
        );
        case.check(
            "nothing is pressed once it has stopped",
            late_keys.is_empty(),
            format!(
                "{} Escapes more than a look after the status read idle",
                late_keys.len()
            ),
        );
        case.check(
            "it is not said to have ignored the stop",
            stuck.is_none() && told.is_empty(),
            format!(
                "unstopped on its lane: {}; notes to the human: {}{}",
                stuck
                    .as_ref()
                    .map_or_else(|| "null".to_owned(), Value::to_string),
                told.len(),
                excerpt(told.first().copied(), 90)
            ),
        );
        case.check(
            "the task is paused",
            task["state"] == "paused",
            format!(
                "T-{number} is {}",
                task["state"].as_str().unwrap_or("undefined")
            ),
        );
        self.show(case, watched, (made, &events), (&items, screens))
    }

    /// The lines that show what the stop was judged from: the status Claude kept
    /// over it, the daemon's own reading of the window, the command's processes and
    /// the screens, and what Claude wrote of the stop.
    fn show(
        &self,
        case: &mut Account,
        watched: &Watched,
        (made, events): (i64, &[Value]),
        (items, screens): (&[cf_e2e::live::claude::Entry], &[String]),
    ) -> Result<()> {
        let kept: Vec<Sample> = watched
            .status
            .samples()
            .into_iter()
            .filter(|sample| sample.at_ms >= made - 2_000 && sample.at_ms <= made + 8_000)
            .collect();
        case.say(format!(
            "     status over the stop: {}",
            runs(&kept, made).join("; ")
        ));
        case.say(format!(
            "     the daemon's reading of the window (window.activity) from just before: {}",
            daemon_reading(events, &watched.window.handle, made)
        ));
        if self.shape.command {
            let now = self.long.running()?;
            case.say(format!(
                "     the command's processes now: {}",
                if now.is_empty() {
                    "none".to_owned()
                } else {
                    shown(&now).join("; ")
                }
            ));
        }
        for screen in screens {
            case.say(format!("     its screen {screen}"));
        }
        case.say(format!(
            "     {}",
            settlement_line(self.members, &watched.session)
        ));
        case.say(kinds_line(items));
        case.say("     the last records of its transcript (the start of each line):");
        for line in raw_tail(items, made, 6) {
            case.say(format!("       {line}"));
        }
        Ok(())
    }
}

/// What the human's Pause was, as the daemon answered it: `task.pause made T-3
/// paused`, or `task.pause refused: …` with its answer.
pub fn pause_text(number: i64, answer: &Value) -> String {
    format!(
        "task.pause {}",
        if answer["ok"] == true {
            format!(
                "made T-{number} {}",
                answer["task"]["state"].as_str().unwrap_or("undefined")
            )
        } else {
            format!("refused: {answer}")
        }
    )
}

/// Says what the human's Pause was, as the daemon answered it.
pub fn said_pause(case: &mut Account, number: i64, answer: &Value) {
    case.say(pause_text(number, answer));
}

/// When Claude's status first read idle after `made`, which is when the daemon
/// would read the window at rest.
pub fn idle_after(samples: &[Sample], made: i64) -> Option<i64> {
    samples
        .iter()
        .find(|sample| sample.at_ms > made && sample.status.as_deref() == Some("idle"))
        .map(|sample| sample.at_ms)
}

/// The presses that came more than a look after the window read idle: ones at a
/// window that had stopped.
pub fn late(presses: &[i64], idle_at: Option<i64>) -> Vec<i64> {
    presses
        .iter()
        .copied()
        .filter(|press| idle_at.is_some_and(|idle| *press > idle + 1_500))
        .collect()
}

/// The Escapes as a line: `2 Escapes: +0.1 s, +3.2 s`, or `0 Escapes: none`.
pub fn escapes_line(presses: &[i64], made: i64) -> String {
    format!(
        "{}: {}",
        plural(presses.len(), "Escape"),
        if presses.is_empty() {
            "none".to_owned()
        } else {
            presses
                .iter()
                .map(|press| plus(*press, made))
                .collect::<Vec<_>>()
                .join(", ")
        }
    )
}

/// The window's own readings the daemon made from just before `made`, as a line.
pub fn daemon_reading(events: &[Value], handle: &str, made: i64) -> String {
    let saw: Vec<String> = events
        .iter()
        .filter(|event| {
            event["kind"] == "window.activity"
                && event["participant"] == handle
                && at_ms(event).is_some_and(|at| at >= made - 3_000)
        })
        .map(|event| {
            format!(
                "{} at {}",
                event["state"].as_str().unwrap_or("undefined"),
                plus_maybe(at_ms(event), made)
            )
        })
        .collect();
    if saw.is_empty() {
        "no change".to_owned()
    } else {
        saw.join(", ")
    }
}

/// When the daemon first read the window at rest after `made`: where the stop was
/// paid.
pub fn rested_at(events: &[Value], handle: &str, made: i64) -> Option<i64> {
    events
        .iter()
        .find(|event| {
            event["kind"] == "window.activity"
                && event["participant"] == handle
                && event["state"] == "idle"
                && at_ms(event).is_some_and(|at| at > made)
        })
        .and_then(at_ms)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn read(at_ms: i64, status: &str) -> Sample {
        Sample {
            at_ms,
            status: Some(status.to_owned()),
            waiting_for: None,
            changed_at_ms: None,
        }
    }

    #[test]
    fn the_turn_stopped_when_claudes_status_first_read_idle_after_the_pause_and_not_before() {
        let samples = [
            read(900, "idle"),
            read(1_000, "idle"),
            read(1_200, "busy"),
            read(2_300, "idle"),
            read(2_450, "idle"),
        ];
        // At the pause itself is not after it: the status was idle before the window was stopped.
        assert_eq!(idle_after(&samples, 1_000), Some(2_300));
        assert_eq!(idle_after(&samples, 2_450), None);
        assert_eq!(idle_after(&[], 0), None);
        assert_eq!(idle_after(&[read(5, "waiting")], 0), None);
    }

    #[test]
    fn a_press_is_late_when_it_came_more_than_a_look_after_the_window_read_idle() {
        let presses = [100, 2_300, 3_900, 4_100];
        // A look is a second and a half: the press at 3_800 is the last that is not late.
        assert_eq!(late(&presses, Some(2_300)), [3_900, 4_100]);
        assert_eq!(late(&[3_800], Some(2_300)), Vec::<i64>::new());
        // A window that never read idle was pressed at no window that had stopped.
        assert!(late(&presses, None).is_empty());
    }

    #[test]
    fn the_escapes_the_daemon_pressed_are_a_count_and_the_time_of_each_since_the_pause() {
        assert_eq!(
            escapes_line(&[1_100, 4_200], 1_000),
            "2 Escapes: +0.1 s, +3.2 s"
        );
        assert_eq!(escapes_line(&[1_500], 1_000), "1 Escape: +0.5 s");
        assert_eq!(escapes_line(&[], 1_000), "0 Escapes: none");
    }

    #[test]
    fn the_daemon_paid_the_stop_at_the_first_idle_it_read_after_the_pause() {
        let event = |at: &str, who: &str, state: &str| json!({ "kind": "window.activity", "participant": who, "state": state, "at": at });
        let events = [
            event("2026-10-11T09:29:50.000Z", "w-1", "working"),
            event("2026-10-11T09:30:01.000Z", "w-1", "working"),
            event("2026-10-11T09:30:03.200Z", "other", "idle"),
            event("2026-10-11T09:30:04.100Z", "w-1", "idle"),
        ];
        let made = 1_791_711_000_000;
        assert_eq!(rested_at(&events, "w-1", made), Some(made + 4_100));
        assert_eq!(rested_at(&events, "other", made), Some(made + 3_200));
        assert_eq!(rested_at(&events, "nobody", made), None);
        // An idle before the pause is the window's from before, not where the stop was paid.
        assert_eq!(rested_at(&events, "w-1", made + 4_100), None);
        assert_eq!(
            daemon_reading(&events, "w-1", made),
            "working at +1.0 s, idle at +4.1 s"
        );
        assert_eq!(daemon_reading(&events, "nobody", made), "no change");
    }

    #[test]
    fn a_pause_the_daemon_refused_is_said_with_its_answer_and_one_it_made_with_the_state() {
        let mut case = Account::new();
        said_pause(
            &mut case,
            3,
            &json!({ "ok": true, "task": { "state": "paused" } }),
        );
        said_pause(
            &mut case,
            3,
            &json!({ "ok": false, "error": "no such task" }),
        );
        assert_eq!(
            case.lines(),
            [
                "task.pause made T-3 paused",
                "task.pause refused: {\"ok\":false,\"error\":\"no such task\"}",
            ]
        );
    }
}

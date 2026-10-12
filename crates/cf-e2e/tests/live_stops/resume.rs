//! The human resumes the paused task: its words (the resume carrier) must go into
//! the window once it reads at rest, within a minute and a half; when they did
//! not, why, from what the daemon said of the message it held. They must land as a
//! message of their own, not after the text a window that stopped before its first
//! word puts back in its input box, and the task must end with its result.
//! `Expect` says more of a window that took its brief back out of its conversation
//! when it stopped: the words that resume the task carry the `brief`, and the
//! result holds the `answer` to it.

use std::sync::OnceLock;
use std::time::Duration;

use cf_e2e::live::members::{find, is_over, last_result, plus, Account, Members};
use cf_e2e::live::trace::at_ms;
use cf_e2e::live::Log;
use cf_e2e::Result;
use regex::Regex;
use serde_json::{json, Value};

use crate::watch::Watched;
use crate::{now_ms, RESULT, RESUME};

/// What a resume is held to besides the words going in whole.
#[derive(Debug, Clone, Copy, Default)]
pub struct Expect<'a> {
    /// The words of the brief the window took back out of its conversation.
    pub brief: Option<&'a str>,
    /// What the result of the brief holds, which only a worker that has it can say.
    pub answer: Option<&'a str>,
}

/// How often the board and the record are looked at while the resume goes in.
const LOOK: Duration = Duration::from_millis(500);

/// How many messages' headers (`[ConsensFlow m-12 …`) a text holds: a message of
/// its own has one.
pub fn headers_in(text: &str) -> usize {
    static HEADER: OnceLock<Regex> = OnceLock::new();
    cf_e2e::pattern::once(&HEADER, r"\[ConsensFlow m-\d+ ")
        .find_iter(text)
        .count()
}

/// The reasons the daemon held the messages `held` (`delivery.held`), each once.
pub fn reasons(held: &[&Value]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for event in held {
        let reason = event["reason"].as_str().unwrap_or("undefined").to_owned();
        if !found.contains(&reason) {
            found.push(reason);
        }
    }
    found
}

/// The human resumes the task `number` of the member `agent`; says what came of it
/// in `case`: whether the resume words went into the window, landed whole, carried
/// what they should, and the task ended with its result.
pub fn resume_after(
    members: &Members,
    case: &mut Account,
    log: &Log,
    (agent, number): (&str, i64),
    watched: &Watched,
    expect: Expect,
) -> Result<()> {
    let resumed_at = now_ms();
    let resumed = members.page("task.resume", json!({ "task": number }))?;
    case.say(format!(
        "task.resume {}",
        if resumed["ok"] == true {
            format!(
                "made T-{number} {}",
                resumed["task"]["state"].as_str().unwrap_or("undefined")
            )
        } else {
            format!("refused: {resumed}")
        }
    ));
    let since = |event: &Value| at_ms(event).is_some_and(|at| at >= resumed_at);
    let pasted = find(
        log,
        "the resume words to go into the window",
        RESUME,
        LOOK,
        || {
            Ok(members
                .events()
                .into_iter()
                .find(|event| event["kind"] == "delivery.begun" && since(event)))
        },
    )?;
    let events = members.events();
    let held: Vec<&Value> = events
        .iter()
        .filter(|event| event["kind"] == "delivery.held" && since(event))
        .collect();
    let reasons = reasons(&held);
    let lane = members.lane(agent)?;
    case.check(
        "the resume words went into the window",
        pasted.is_some(),
        format!(
            "{}; its lane: activity {}, unstopped {}, holding {}{}",
            pasted.as_ref().map_or_else(
                || format!(
                    "nothing was pasted in {} s after the resume",
                    RESUME.as_secs()
                ),
                |pasted| format!(
                    "delivery.begun of m-{} at {} after the resume",
                    pasted["data"]["message"],
                    plus(at_ms(pasted).unwrap_or(resumed_at), resumed_at)
                )
            ),
            lane["activity"],
            if lane["unstopped"].is_null() {
                "null".to_owned()
            } else {
                lane["unstopped"].to_string()
            },
            if lane["holding"].is_null() {
                "undefined".to_owned()
            } else {
                lane["holding"].to_string()
            },
            if reasons.is_empty() {
                String::new()
            } else {
                format!("; held because: {}", reasons.join(" | "))
            }
        ),
    );
    let Some(pasted) = pasted else {
        return Ok(());
    };

    // What Claude's record holds of them: one message, with no other message's header in it.
    let marker = format!("[ConsensFlow m-{} ", pasted["data"]["message"]);
    let record = find(
        log,
        "Claude's record to hold the resume words",
        Duration::from_secs(60),
        Duration::from_millis(250),
        || {
            Ok(watched
                .record(members)
                .into_iter()
                .find(|item| item.kind == "user" && item.text.contains(&marker)))
        },
    )?;
    let alone = record
        .as_ref()
        .is_some_and(|record| headers_in(&record.text) == 1);
    case.check(
        "the resume words landed as a message of their own",
        alone,
        record.as_ref().map_or_else(
            || "its record never held them".to_owned(),
            |record| Value::String(record.text.chars().take(300).collect()).to_string(),
        ),
    );
    // A brief the window took back is not in the conversation the resume is answered in: it is in the resume.
    if let Some(brief) = expect.brief {
        let carried = record
            .as_ref()
            .is_some_and(|record| record.text.contains(brief));
        case.check(
            "the resume carried the task's brief",
            carried,
            if carried {
                "its message holds the brief whole"
            } else {
                "its message does not hold it"
            },
        );
    }
    let finished = find(
        log,
        "the task to end with its result",
        RESULT,
        Duration::from_secs(1),
        || Ok(is_over(&members.task(number)?).then_some(())),
    )?;
    let thread = members.task(number)?;
    let result = last_result(&thread);
    let state = thread["state"].as_str().unwrap_or("undefined");
    case.check(
        "the task ended with its result",
        finished.is_some() && ["done", "accepted"].contains(&state) && result.is_some(),
        format!(
            "T-{number} is {state}{}",
            result.map_or_else(
                || " and holds none".to_owned(),
                |result| format!(
                    ", its result {}",
                    Value::String(
                        result["body"]
                            .as_str()
                            .unwrap_or_default()
                            .chars()
                            .take(200)
                            .collect()
                    )
                )
            )
        ),
    );
    if let Some(answer) = expect.answer {
        let said = result.is_some_and(|result| {
            result["body"]
                .as_str()
                .is_some_and(|body| body.contains(answer))
        });
        case.check(
            "the result carries the task's own answer",
            said,
            if said {
                json!(answer).to_string()
            } else {
                format!(
                    "{} is not in it: the worker did not have its brief",
                    json!(answer)
                )
            },
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_of_its_own_has_one_header_and_words_run_into_another_have_two() {
        assert_eq!(
            headers_in("[ConsensFlow m-12 · T-3 · task from @chief]\nAnswer with the word"),
            1
        );
        // The text a window that stopped before its first word puts back in its input box.
        assert_eq!(
            headers_in("[ConsensFlow m-3 · T-3 · task from @chief] old text[ConsensFlow m-12 · T-3 · resume]"),
            2
        );
        assert_eq!(headers_in("a [ConsensFlow note] and nothing else"), 0);
        assert_eq!(headers_in(""), 0);
    }

    #[test]
    fn the_reasons_a_delivery_was_held_are_said_once_each_in_the_order_they_came() {
        let held = [
            json!({ "reason": "the window is working" }),
            json!({ "reason": "its turn is not over" }),
            json!({ "reason": "the window is working" }),
            json!({}),
        ];
        let held: Vec<&Value> = held.iter().collect();
        assert_eq!(
            reasons(&held),
            ["the window is working", "its turn is not over", "undefined"]
        );
        assert!(reasons(&[]).is_empty());
    }
}

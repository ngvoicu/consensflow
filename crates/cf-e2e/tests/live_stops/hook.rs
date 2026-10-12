//! The stops of a member whose question waits on the board: its hook polls, and
//! the turn is inside the hook.
//!
//! - `hook`: Escape at its question hook (the member's question waits on the
//!   board) with the task paused: what happens to the hook's wait and the turn;
//!   then the answer the chief gives late, and the resume that carries it.
//! - `escape`: the same hook, with no pause: one Escape into the pane, as the
//!   daemon presses it, and what that does to the hook, the turn and the question
//!   the board still holds.

use std::thread::sleep;
use std::time::Duration;

use cf_e2e::live::claude::{processes_with, runs, Process};
use cf_e2e::live::members::{
    answer_to, find, first_answer, first_question, is_over, last_result, plus, plus_maybe, Account,
};
use cf_e2e::Result;
use serde_json::{json, Value};

use crate::stop::{idle_after, said_pause};
use crate::watch::{interrupted, record_since, say_events, settlement_line, watch, Watched};
use crate::{now_ms, Ctx, STOP, WORK};

/// A member's question that waits on the board, and the hook that waits for its
/// answer.
pub struct Asked {
    pub number: i64,
    pub question: Value,
    pub watched: Watched,
}

/// How many tasks a model that answers on its own is given before the case gives up.
const ATTEMPTS: usize = 3;

/// How often the board is looked at while the member is waited for.
const LOOK: Duration = Duration::from_millis(500);

/// The processes of the hook of the member's question: `cf hook claude`, which
/// polls the board until the answer is there.
pub fn hooks() -> Result<Vec<Process>> {
    processes_with("hook claude")
}

/// A member's question waits on the board: its hook polls. A model may answer on
/// its own and never ask: that task ends, and another is given, twice at most.
/// None, with the reason in `case`, when it never asked.
pub fn asking_member(ctx: &Ctx, case: &mut Account) -> Result<Option<Asked>> {
    for attempt in 1..=ATTEMPTS {
        let number = ctx.members.give(ctx.agent, ctx.question)?;
        let first = find(
            ctx.log,
            "the member's question on the board",
            WORK,
            LOOK,
            || {
                let thread = ctx.members.task(number)?;
                // The question, or (as none) the end of the task without one.
                Ok(first_question(&thread)
                    .map(Some)
                    .or_else(|| is_over(&thread).then_some(None)))
            },
        )?;
        let Some(Some(question)) = first else {
            let thread = ctx.members.task(number)?;
            case.say(match first {
                Some(_) => format!(
                    "T-{number} ended ({}) and the member never asked (attempt {attempt}); its result: {}",
                    thread["state"].as_str().unwrap_or("undefined"),
                    json!(result_words(&thread).chars().take(200).collect::<String>())
                ),
                None => format!(
                    "T-{number}: the member never asked in {} s (attempt {attempt})",
                    WORK.as_secs()
                ),
            });
            if first.is_none() {
                return Ok(None);
            }
            continue;
        };
        let watched = watch(ctx.members, ctx.agent)?;
        sleep(Duration::from_secs(3));
        let hooks = hooks()?;
        case.say(format!(
            "T-{number}: m-{} is on the board and the hook waits for it ({})",
            question["id"],
            if hooks.is_empty() {
                "no hook process found".to_owned()
            } else {
                format!(
                    "process {}",
                    hooks
                        .iter()
                        .map(|hook| hook.pid.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        ));
        return Ok(Some(Asked {
            number,
            question,
            watched,
        }));
    }
    Ok(None)
}

/// The words of the last result of a thread, none where there is none.
fn result_words(thread: &Value) -> String {
    last_result(thread)
        .and_then(|result| result["body"].as_str())
        .unwrap_or_default()
        .to_owned()
}

/// What the turn did after the stop: when Claude's status first read idle.
pub struct Seen {
    pub idle_at: Option<i64>,
}

/// Says what the hook and the turn did after the stop at `made`.
pub fn after_stop(ctx: &Ctx, case: &mut Account, asked: &Asked, made: i64) -> Result<Seen> {
    let mut hook_gone_at: Option<i64> = None;
    let idle_at = find(
        ctx.log,
        "the window to read idle",
        STOP,
        Duration::from_millis(100),
        || {
            if hook_gone_at.is_none() && hooks()?.is_empty() {
                hook_gone_at = Some(now_ms());
            }
            Ok(idle_after(&asked.watched.status.samples(), made))
        },
    )?;
    let pressed = asked.watched.escapes(ctx.members);
    sleep(ctx.rest);
    let after = asked.watched.escapes(ctx.members);
    let items = asked.watched.record(ctx.members);
    let told = format!(
        "T-{} was stopped, so m-{} is not answered here",
        asked.number, asked.question["id"]
    );
    let heard = items.iter().any(|item| item.text.contains(&told));
    let ended = interrupted(&items, made);
    case.say(format!(
        "     Escapes the daemon pressed: {} ({}); {} more in the {} s after",
        pressed.len(),
        if pressed.is_empty() {
            "none".to_owned()
        } else {
            pressed
                .iter()
                .map(|press| plus(*press, made))
                .collect::<Vec<_>>()
                .join(", ")
        },
        after.len().saturating_sub(pressed.len()),
        ctx.rest.as_secs()
    ));
    case.say(format!(
        "     the hook: {}; the model {}",
        hook_gone_at.map_or_else(
            || "still running".to_owned(),
            |gone| format!("its process was gone at {}", plus(gone, made))
        ),
        if heard {
            "was told \"T-n was stopped, so m-q is not answered here…\""
        } else {
            "was not told the door is closed"
        }
    ));
    case.say(format!(
        "     the turn: {}; the record {}",
        idle_at.map_or_else(
            || format!("never read idle in {} s", STOP.as_secs()),
            |idle| format!("status read idle at {}", plus(idle, made))
        ),
        ended.map_or_else(
            || "holds no interrupt".to_owned(),
            |ended| format!(
                "holds \"{}\" at {}",
                ended.text.chars().take(60).collect::<String>(),
                plus_maybe(ended.at_ms, made)
            )
        )
    ));
    let kept: Vec<_> = asked
        .watched
        .status
        .samples()
        .into_iter()
        .filter(|sample| sample.at_ms >= made - 2_000 && sample.at_ms <= made + 10_000)
        .collect();
    case.say(format!(
        "     status over the stop: {}",
        runs(&kept, made).join("; ")
    ));
    case.say(format!(
        "     {}",
        settlement_line(ctx.members, &asked.watched.session)
    ));
    case.say("     its record from the stop on:");
    for line in record_since(&items, &asked.watched.session, made, 14) {
        case.say(format!("       {line}"));
    }
    Ok(Seen { idle_at })
}

/// The events of the answer `answer`, the kinds of them, as a line.
fn kinds_about(events: &[Value], answer: Option<i64>) -> String {
    events
        .iter()
        .filter(|event| answer.is_some() && event["data"]["message"].as_i64() == answer)
        .filter_map(|event| event["kind"].as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Escape at the question hook, with the task paused; then the answer the chief
/// gives late, and the resume that carries it.
pub fn hook_case(ctx: &Ctx, case: &mut Account) -> Result<()> {
    let Some(asked) = asking_member(ctx, case)? else {
        case.require(false);
        return Ok(());
    };
    let number = asked.number;
    let paused = ctx.members.pause(number)?;
    said_pause(case, number, &paused.answer);
    let seen = after_stop(ctx, case, &asked, paused.made_ms)?;
    let task = ctx.members.task(number)?;
    // The chief answers late: the door is shut, so the answer waits for the task to go on.
    let id = format!("m-{}", asked.question["id"]);
    let given = ctx.members.as_chief(["answer", id.as_str(), "blue"])?;
    sleep(Duration::from_secs(3));
    let thread = ctx.members.task(number)?;
    let late = answer_to(&thread, &asked.question);
    case.say(format!(
        "     the chief answered {}: m-{} is {} while T-{number} is {}",
        if given.code == Some(0) {
            "late".to_owned()
        } else {
            format!("(cf answer failed: {})", given.stderr)
        },
        field(late.as_ref(), "id"),
        field(late.as_ref(), "state"),
        thread["state"].as_str().unwrap_or("undefined")
    ));
    let resumed = ctx.members.page("task.resume", json!({ "task": number }))?;
    case.say(format!(
        "task.resume {}",
        if resumed["ok"] == true {
            "made it queued again".to_owned()
        } else {
            format!("refused: {resumed}")
        }
    ));
    let finished = find(ctx.log, "the task to end", WORK, LOOK, || {
        Ok(is_over(&ctx.members.task(number)?).then_some(()))
    })?;
    let thread = ctx.members.task(number)?;
    let answer = first_answer(&thread);
    let result = result_words(&thread);
    case.check(
        "the task is paused",
        task["state"] == "paused",
        format!(
            "T-{number} was {} after the stop",
            task["state"].as_str().unwrap_or("undefined")
        ),
    );
    case.check(
        "the turn stopped",
        seen.idle_at.is_some(),
        if seen.idle_at.is_some() {
            "status read idle"
        } else {
            "it never read idle"
        },
    );
    case.check(
        "the late answer came with the resume",
        finished.is_some()
            && answer
                .as_ref()
                .is_some_and(|answer| answer["state"] != "queued")
            && result.to_lowercase().contains("blue"),
        format!(
            "m-{} {} {}; T-{number} {}; result {}",
            field(answer.as_ref(), "id"),
            field(answer.as_ref(), "state"),
            answer.as_ref().map_or_else(
                || "undefined".to_owned(),
                |answer| answer["receipt"].to_string()
            ),
            thread["state"].as_str().unwrap_or("undefined"),
            json!(result.chars().take(80).collect::<String>())
        ),
    );
    say_events(case, ctx.members, number);
    Ok(())
}

/// One Escape into the pane, as the daemon presses it, with no pause: what that
/// does to the hook, the turn and the question the board still holds. What an
/// Escape does at an open door is recorded, not judged: the answer must not be lost.
pub fn escape_case(ctx: &Ctx, case: &mut Account) -> Result<()> {
    let Some(asked) = asking_member(ctx, case)? else {
        case.require(false);
        return Ok(());
    };
    let number = asked.number;
    // One Escape into the pane, as the daemon presses it: no pause, so the door is open.
    let made = now_ms();
    ctx.members.rig().host(
        "pane.input",
        json!({
            "id": asked.watched.window.pane["id"],
            "generation": asked.watched.window.pane["generation"],
            "bytes": [27],
        }),
    )?;
    case.say("one Escape into the pane (task not paused, door open)");
    after_stop(ctx, case, &asked, made)?;
    let thread = ctx.members.task(number)?;
    let state_of_question = thread["messages"]
        .as_array()
        .and_then(|messages| {
            messages
                .iter()
                .find(|message| message["id"] == asked.question["id"])
        })
        .map(|message| message["state"].as_str().unwrap_or("undefined").to_owned());
    case.say(format!(
        "     T-{number} is {}; the question m-{} is {}; no answer was given yet",
        thread["state"].as_str().unwrap_or("undefined"),
        asked.question["id"],
        state_of_question.unwrap_or_else(|| "undefined".to_owned())
    ));
    // The chief answers later. If the Escape ended the hook, no door waits for the answer.
    let id = format!("m-{}", asked.question["id"]);
    let given = ctx.members.as_chief(["answer", id.as_str(), "blue"])?;
    case.say(format!(
        "the chief answered from the board{}",
        if given.code == Some(0) {
            String::new()
        } else {
            format!(" (cf answer failed: {})", given.stderr)
        }
    ));
    let finished = find(ctx.log, "the task to end", WORK, LOOK, || {
        Ok(is_over(&ctx.members.task(number)?).then_some(()))
    })?;
    let after = ctx.members.task(number)?;
    let answer = first_answer(&after);
    let result = result_words(&after);
    let about = kinds_about(
        &ctx.members.events(),
        answer.as_ref().and_then(|answer| answer["id"].as_i64()),
    );
    case.check(
        "the answer is not lost",
        finished.is_some()
            && answer.as_ref().is_some_and(|answer| {
                ["read", "delivered"].contains(&answer["state"].as_str().unwrap_or_default())
            })
            && result.to_lowercase().contains("blue"),
        format!(
            "m-{} is {}, receipt {}, attempts {} ({about}); T-{number} is {}; result {}",
            field(answer.as_ref(), "id"),
            field(answer.as_ref(), "state"),
            answer.as_ref().map_or_else(
                || "undefined".to_owned(),
                |answer| answer["receipt"].to_string()
            ),
            field(answer.as_ref(), "attempts"),
            after["state"].as_str().unwrap_or("undefined"),
            json!(result.chars().take(80).collect::<String>())
        ),
    );
    say_events(case, ctx.members, number);
    Ok(())
}

/// A field of a message as a word of a line: its text, or `undefined` as the
/// check says where the message is none.
fn field(message: Option<&Value>, name: &str) -> String {
    match message.map(|message| &message[name]) {
        None | Some(Value::Null) => "undefined".to_owned(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first step of both hook cases against the stand-in, which asks through the hook of
    /// its settings as Claude's tool does: its question is on the board, the window is watched,
    /// and the process of the hook that waits for the answer is there for the check to find
    /// (by `ps`: a macOS and Linux check, as the stops are).
    #[cfg(unix)]
    #[test]
    fn a_member_that_asks_has_its_question_on_the_board_and_its_hook_waiting_for_the_answer() {
        use std::path::Path;

        use cf_e2e::live::members::{
            stand_in_chief, stand_in_question_brief, stand_in_worker, Members, Plan,
        };
        use cf_e2e::live::Log;

        let log = Log::new("live stops hook test");
        let plan = Plan::new("stops-hook-test", stand_in_chief()).with(stand_in_worker());
        let mut members = Members::open(&plan, Path::new(crate::FAKE_AGENT), &log).unwrap();
        members.add("worker", &[]).unwrap();
        let brief = stand_in_question_brief();
        let ctx = Ctx {
            members: &members,
            log: &log,
            agent: "worker",
            question: &brief,
            rest: Duration::from_secs(1),
        };
        let mut case = Account::new();
        let asked = asking_member(&ctx, &mut case).unwrap().unwrap();
        assert!(asked.question["id"].is_number(), "{}", asked.question);
        assert_eq!(asked.question["kind"], "question");
        let said = case.text();
        assert!(
            said.contains(&format!(
                "T-{}: m-{} is on the board and the hook waits for it (process ",
                asked.number, asked.question["id"]
            )),
            "{said}"
        );
        assert!(!hooks().unwrap().is_empty(), "{said}");
        // The answer, given as the chief gives it, ends the wait, and the task its work.
        let id = format!("m-{}", asked.question["id"]);
        let given = members.as_chief(["answer", id.as_str(), "blue"]).unwrap();
        assert_eq!(given.code, Some(0), "{given}");
        let thread = find(
            &log,
            "the task to end",
            Duration::from_secs(60),
            LOOK,
            || {
                let thread = members.task(asked.number)?;
                Ok(is_over(&thread).then_some(thread))
            },
        )
        .unwrap()
        .unwrap();
        assert!(result_words(&thread).contains("blue"), "{thread}");
        drop(asked);
        members.close().unwrap();
    }

    #[test]
    fn the_words_of_a_result_are_the_last_ones_and_none_where_there_is_no_result() {
        let thread = json!({ "messages": [
            { "id": 1, "kind": "task" },
            { "id": 2, "kind": "result", "body": "first" },
            { "id": 3, "kind": "result", "body": "COLOUR=blue" },
        ] });
        assert_eq!(result_words(&thread), "COLOUR=blue");
        assert_eq!(result_words(&json!({})), "");
    }

    #[test]
    fn a_field_of_a_message_is_its_text_its_json_or_that_it_has_none() {
        let answer = json!({ "id": 5, "kind": "answer", "replyTo": 7, "state": "read" });
        assert_eq!(field(Some(&answer), "state"), "read");
        assert_eq!(field(Some(&answer), "id"), "5");
        assert_eq!(field(Some(&answer), "attempts"), "undefined");
        assert_eq!(field(None, "state"), "undefined");
    }

    #[test]
    fn the_events_of_an_answer_are_told_by_their_kinds_and_none_for_an_answer_that_is_none() {
        let events = [
            json!({ "kind": "delivery.claimed", "data": { "message": 5 } }),
            json!({ "kind": "message.sent", "data": { "message": 6 } }),
            json!({ "kind": "message.read", "data": { "message": 5 } }),
        ];
        assert_eq!(
            kinds_about(&events, Some(5)),
            "delivery.claimed, message.read"
        );
        assert_eq!(kinds_about(&events, None), "");
    }
}

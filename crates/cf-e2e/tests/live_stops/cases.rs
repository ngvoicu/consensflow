//! The two cases that stop a window at a place of the turn the check chooses
//! before the pause is made, and go on from it (or do not):
//!
//! - `command`: Escape into Claude inside a long command: the command and the turn
//!   stop, and nothing more is pressed once it is at rest.
//! - `early`: Escape right after the task's words went in, before Claude's first
//!   output. Claude writes no record of such a stop and puts the words back in its
//!   input box, and out of the conversation it answers from; the daemon reads the
//!   window at rest by its own press, and the ledger keeps the brief for the window
//!   again. Then the resume of the task: its words must land as a message of their
//!   own, not after the old text, carry the brief, and the task must end with its
//!   result, which holds the brief's answer.

use std::thread::sleep;
use std::time::Duration;

use cf_e2e::live::members::{find, shown, Account};
use cf_e2e::Result;
use serde_json::{json, Value};

use crate::resume::{resume_after, Expect};
use crate::stop::{Shape, Stop};
use crate::watch::say_events;
use crate::{Ctx, LONG, WORK};

/// What the brief of the `early` case asks, and what only a worker that has the
/// brief can answer.
pub const EARLY_ANSWER: &str = "EARLY-4117";

/// The brief of the `early` case.
pub fn early_brief() -> String {
    format!("Reply with exactly one line: {EARLY_ANSWER}")
}

/// The brief of the `command` case: the long command, run on its own.
pub fn command_brief() -> String {
    format!(
        "Run exactly this one shell command, then stop: {}",
        LONG.text()
    )
}

/// Escape into Claude inside a long command: it is paused once the command runs.
pub fn command(ctx: &Ctx, case: &mut Account) -> Result<()> {
    let number = ctx.members.give(ctx.agent, &command_brief())?;
    let running = find(
        ctx.log,
        "the member to be inside the command",
        WORK,
        Duration::from_millis(250),
        || Ok((!LONG.running()?.is_empty()).then_some(())),
    )?;
    if running.is_none() {
        case.fail(format!(
            "the member never ran the command in {} s",
            WORK.as_secs()
        ));
        return Ok(());
    }
    sleep(Duration::from_secs(2));
    case.say(format!(
        "T-{number}: the member is inside `{}` ({})",
        LONG.text(),
        shown(&LONG.running()?).join("; ")
    ));
    Stop {
        members: ctx.members,
        log: ctx.log,
        agent: ctx.agent,
        number,
        shape: Shape {
            command: true,
            recordless: false,
        },
        long: &LONG,
        rest: ctx.rest,
    }
    .run(case)?;
    say_events(case, ctx.members, number);
    Ok(())
}

/// How many tasks the `early` case tries before it says the stop never came before
/// a word.
const EARLY_ATTEMPTS: usize = 8;

/// What came of one task of the `early` case.
enum Tried {
    /// The window stopped before a word of its answer, or did not and says so: what
    /// the attempt saw stands.
    Stands,
    /// The model answered before the pause could land (it does so in about as long
    /// as the daemon takes to see the message in its record), so no stop came before
    /// a word: the task, by its number, proves nothing of what the case is for.
    TooLate(i64),
}

/// Whether the window took the brief of the task, the first message of its `thread`,
/// back out of its conversation when it stopped, which the daemon says of it
/// (`delivery.taken-back`): a window that had answered already took nothing back.
fn took_back(events: &[Value], thread: &Value) -> bool {
    let brief = thread["messages"][0]["id"].as_i64();
    brief.is_some_and(|brief| {
        events.iter().any(|event| {
            event["kind"] == "delivery.taken-back"
                && [&event["data"]["message"], &event["message"]]
                    .iter()
                    .any(|message| message.as_i64() == Some(brief))
        })
    })
}

/// What the daemon made of the cancel of a task answered too soon: the task is
/// paused by then, which it takes, and one that was over already is refused, which
/// leaves it as it was and is no failure of the case.
fn cancel_text(answer: &Value) -> String {
    if answer["ok"] == true {
        "it is cancelled".to_owned()
    } else {
        format!("task.cancel refused: {answer}")
    }
}

/// One task of the case: Escape into Claude right after its words went in, paused as
/// soon as the task is working; and, where the stop came before a word, the resume.
fn early_once(ctx: &Ctx, tried: &mut Account) -> Result<Tried> {
    let brief = early_brief();
    let number = ctx.members.give(ctx.agent, &brief)?;
    let working = find(
        ctx.log,
        "the task to begin",
        WORK,
        Duration::from_millis(50),
        || Ok((ctx.members.task(number)?["state"] == "working").then_some(())),
    )?;
    if working.is_none() {
        tried.fail("the task never began");
        return Ok(Tried::Stands);
    }
    tried.say(format!(
        "T-{number} is working: its first turn has just begun"
    ));
    let watched = Stop {
        members: ctx.members,
        log: ctx.log,
        agent: ctx.agent,
        number,
        shape: Shape {
            command: false,
            recordless: true,
        },
        long: &LONG,
        rest: ctx.rest,
    }
    .run(tried)?;
    if !took_back(&ctx.members.events(), &ctx.members.task(number)?) {
        return Ok(Tried::TooLate(number));
    }
    resume_after(
        ctx.members,
        tried,
        ctx.log,
        (ctx.agent, number),
        &watched,
        Expect {
            brief: Some(&brief),
            answer: Some(EARLY_ANSWER),
        },
    )?;
    say_events(tried, ctx.members, number);
    Ok(Tried::Stands)
}

/// Escape into Claude right after its task's words went in: paused as soon as the
/// task is working. The stop has to come before Claude's first word, and Claude
/// answers a line in about as long as the daemon takes to see the words in its
/// record: a task answered before the pause is cancelled, and another is given.
pub fn early(ctx: &Ctx, case: &mut Account) -> Result<()> {
    for attempt in 1..=EARLY_ATTEMPTS {
        let mut tried = Account::new();
        match early_once(ctx, &mut tried)? {
            Tried::Stands => {
                case.absorb(tried);
                return Ok(());
            }
            Tried::TooLate(number) => {
                let cancelled = ctx.members.page("task.cancel", json!({ "task": number }))?;
                case.say(format!(
                    "attempt {attempt}: T-{number} was answered before the pause came, and the window took nothing back; {}; another is given",
                    cancel_text(&cancelled)
                ));
            }
        }
    }
    case.fail(format!(
        "the answer came before the pause in each of {EARLY_ATTEMPTS} tasks: the case needs the stop to come before the first word"
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_early_case_asks_for_an_answer_only_a_worker_that_has_its_brief_can_give() {
        assert_eq!(early_brief(), "Reply with exactly one line: EARLY-4117");
        assert!(early_brief().ends_with(EARLY_ANSWER));
    }

    #[test]
    fn a_window_took_the_brief_back_when_the_daemon_says_so_of_the_first_message_of_its_task() {
        let thread =
            json!({ "messages": [{ "id": 7, "kind": "task" }, { "id": 9, "kind": "result" }] });
        let back = |message: Value| json!({ "n": 17, "kind": "delivery.taken-back", "data": { "message": message } });
        assert!(took_back(&[back(json!(7))], &thread));
        // The fields beside the kind, as a window's own lines carry them.
        let beside = json!({ "n": 17, "kind": "delivery.taken-back", "message": 7 });
        assert!(took_back(&[beside], &thread));
        // Another message's, another kind's, or nothing said: the window answered first.
        assert!(!took_back(&[back(json!(9))], &thread));
        let other = json!({ "n": 17, "kind": "delivery.confirmed", "data": { "message": 7 } });
        assert!(!took_back(&[other], &thread));
        assert!(!took_back(&[], &thread));
        assert!(!took_back(&[back(json!(7))], &json!({})));
    }

    #[test]
    fn a_cancel_the_daemon_refused_is_said_with_its_answer() {
        let made = json!({ "ok": true, "task": { "state": "cancelled" } });
        assert_eq!(cancel_text(&made), "it is cancelled");
        let refused = cancel_text(&json!({ "ok": false, "error": "T-3 is done" }));
        assert!(refused.starts_with("task.cancel refused: "), "{refused}");
        assert!(refused.contains("T-3 is done"), "{refused}");
    }

    #[test]
    fn the_command_case_asks_the_member_to_run_the_long_command_and_stop() {
        assert_eq!(
            command_brief(),
            "Run exactly this one shell command, then stop: node -e \"setTimeout(() => {}, 300000)\" cf-stops-live-long-command"
        );
    }
}

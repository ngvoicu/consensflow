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

/// Escape into Claude right after its task's words went in: paused as soon as the
/// task is working.
pub fn early(ctx: &Ctx, case: &mut Account) -> Result<()> {
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
        case.fail("the task never began");
        return Ok(());
    }
    case.say(format!(
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
    .run(case)?;
    resume_after(
        ctx.members,
        case,
        ctx.log,
        (ctx.agent, number),
        &watched,
        Expect {
            brief: Some(&brief),
            answer: Some(EARLY_ANSWER),
        },
    )?;
    say_events(case, ctx.members, number);
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
    fn the_command_case_asks_the_member_to_run_the_long_command_and_stop() {
        assert_eq!(
            command_brief(),
            "Run exactly this one shell command, then stop: node -e \"setTimeout(() => {}, 300000)\" cf-stops-live-long-command"
        );
    }
}

//! `tell`: the chief's `cf tell` into a Claude member at work on a long command while
//! background agents it started run (T-604, 2026-10-25, 3.0.6-alpha.2). The tell
//! pauses the task, and its Escape stopped the turn; Claude says busy while its
//! agents run, the interrupted turn's record counts none of them, and ConsensFlow
//! pressed two more rounds and closed the window as one that ignored the interrupt
//! (m-4188), ending the agents. The design (T-551): one Escape, which stops the turn;
//! the window stays open with its agents; nobody is told it did not stop; and the
//! tell's words go into the same window once it rests, which answers them.

use std::thread::sleep;
use std::time::Duration;

use cf_e2e::live::members::{find, plus, shown, Account};
use cf_e2e::rig::OPEN;
use cf_e2e::Result;
use serde_json::Value;

use crate::watch::{say_events, told_the_human, told_the_requester, watch};
use crate::{now_ms, Ctx, LONG, RESULT, STOP, WORK};

/// How long each background agent runs its command: past the stop and the watch of
/// the window after it, so that the agents still run when the stop is judged.
pub const AGENT_SLEEP_S: u64 = 90;

/// What the tell asks the member to answer with, which only its words can give.
pub const TOLD_ANSWER: &str = "TOLD-4471";

/// The brief of the `tell` case: two background agents, then the long command.
pub fn tell_brief() -> String {
    format!(
        "Use your Agent tool twice, each call with run_in_background set to true, to start two background agents; tell each to run exactly the shell command `sleep {AGENT_SLEEP_S}` and then reply with the word done. Once both are started, run exactly this one shell command in the foreground and wait for it: {}",
        LONG.text()
    )
}

/// The words the chief tells the member.
fn told_words() -> String {
    format!("Leave the command. Answer this message with exactly: {TOLD_ANSWER}")
}

/// Whether the chief was answered with the told words.
fn answered(inbox: &[Value]) -> bool {
    inbox.iter().any(|message| {
        message["kind"] == "answer"
            && message["body"]
                .as_str()
                .is_some_and(|body| body.contains(TOLD_ANSWER))
    })
}

pub fn tell(ctx: &Ctx, case: &mut Account) -> Result<()> {
    let number = ctx.members.give(ctx.agent, &tell_brief())?;
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
    let watched = watch(ctx.members, ctx.agent)?;
    let rig = ctx.members.rig();
    let chief = rig.open_frame(
        &format!("p{}-{}", ctx.members.project_id(), ctx.members.chief()),
        OPEN,
    )?;
    let made = now_ms();
    let told = rig.cf_in_window(&chief, ["tell", &format!("T-{number}"), &told_words()])?;
    case.check("the chief's tell is taken", told.code == Some(0), &told);

    let gone = find(
        ctx.log,
        "the command to stop",
        STOP,
        Duration::from_millis(100),
        || Ok(LONG.running()?.is_empty().then_some(now_ms())),
    )?;
    case.check(
        "the turn stopped",
        gone.is_some(),
        gone.map_or_else(
            || format!("the command still runs after {} s", STOP.as_secs()),
            |gone| format!("the command was gone at {}", plus(gone, made)),
        ),
    );
    // Every round a stop could have, and the rest after them.
    sleep(ctx.rest + Duration::from_secs(10));
    let presses = watched.escapes(ctx.members);
    let busy = watched
        .status
        .samples()
        .iter()
        .filter(|sample| sample.at_ms >= made)
        .any(|sample| sample.status.as_deref() == Some("busy"));
    case.say(format!(
        "{} Escape(s), at {}; Claude's status after the tell {} busy",
        presses.len(),
        presses
            .iter()
            .map(|press| plus(*press, made))
            .collect::<Vec<_>>()
            .join(", "),
        if busy { "read" } else { "never read" }
    ));
    case.check(
        "one Escape, which stopped the turn",
        presses.len() == 1,
        format!("{} pressed", presses.len()),
    );
    let now = ctx.members.window_of(ctx.agent)?;
    let open = now
        .as_ref()
        .is_some_and(|window| window.pane == watched.window.pane);
    case.check(
        "the window is open, as it was",
        open,
        now.map_or_else(|| "no window".to_owned(), |window| window.pane.to_string()),
    );
    let human = ctx.members.inbox(None)?;
    let requester = ctx.members.inbox(Some(ctx.members.chief()))?;
    let (to_human, to_requester) = (told_the_human(&human), told_the_requester(&requester));
    case.check(
        "nobody is told it did not stop",
        to_human.is_empty() && to_requester.is_empty(),
        format!(
            "{} note(s) to the human, {} to the requester",
            to_human.len(),
            to_requester.len()
        ),
    );

    // The agents end; the window rests, and the tell's words go into it.
    let answer = find(
        ctx.log,
        "the member to answer the tell",
        RESULT,
        Duration::from_millis(500),
        || Ok(answered(&ctx.members.inbox(Some(ctx.members.chief()))?).then_some(now_ms())),
    )?;
    case.check(
        "the tell's words went into the same window, which answered them",
        answer.is_some() && ctx.members.windows_opened_for(ctx.agent).len() == 1,
        answer.map_or_else(
            || format!("no answer in {} s", RESULT.as_secs()),
            |answer| {
                format!(
                    "answered at {}, {} window(s) opened for the member",
                    plus(answer, made),
                    ctx.members.windows_opened_for(ctx.agent).len()
                )
            },
        ),
    );
    say_events(case, ctx.members, number);
    Ok(())
}

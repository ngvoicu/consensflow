//! Getting the member to the middle of its turn: its task given, and its window waited
//! for until it shows what the case pauses it in (a reply it writes, a command it runs),
//! which the case's own reading says (`ready`). Before it is asked, the member's brief
//! has reached its window (a lane reads working while its window is still to show the
//! brief) and its lane reads working. The wait ends when the case's reading is
//! satisfied, when the task is over (a window the daemon gave up on is a task it failed,
//! which the lane no longer says: what cannot come is not waited for), or when the time
//! is up; the case says why where the member did not get there ([`super::unstarted`]).

use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::members::{brief_delivered, find, is_over, Account, MemberWindow, Members};
use cf_e2e::live::Harness;
use cf_e2e::Result;
use serde_json::Value;

use super::unstarted::{self, Waiting};
use crate::{now_ms, Ctx};

/// What the member is asked to do, in the words the case says it in.
#[derive(Debug, Clone, Copy)]
pub struct Doing {
    /// `write its numbers`: the member did not…
    pub to: &'static str,
    /// `wrote its numbers`: the member never…
    pub did: &'static str,
    /// `writing its numbers`: the member is…
    pub doing: &'static str,
    /// `numbers on its screen`: its lane did not read working with…
    pub shown: &'static str,
}

/// How often the task is looked at while the member is waited for.
const TASK_EVERY: Duration = Duration::from_secs(2);

/// The member's task and its window, once it is at what the case pauses it in, and what
/// the case's reading saw there.
pub struct Begun {
    pub number: i64,
    pub window: MemberWindow,
    pub seen: String,
}

/// What the wait for the member ended with.
enum Waited {
    /// Its window, at the point the case's reading found, and what it saw there.
    Ready(Box<MemberWindow>, String),
    /// Its task, which is over: the daemon gave its window up, or its work ended.
    Over(Value),
}

/// Waits for the member of the task `number` to be at the point `ready` finds (see the
/// module): it is asked, with the window and how long its lane has read working, each
/// time the lane reads working and the brief has reached the window, and says what it
/// saw when it is satisfied. The wait ends with the member's window then, with the task
/// when it is over, and with none when `work` is up. `waiting` is told the lane as the
/// board has it each time it is looked at.
fn await_ready(
    ctx: &Ctx,
    number: i64,
    work: Duration,
    doing: &Doing,
    waiting: &mut Waiting,
    mut ready: impl FnMut(&Members, &MemberWindow, Duration) -> Option<String>,
) -> Result<Option<Waited>> {
    let members = ctx.members;
    let (mut worked, mut task_looked): (Option<Instant>, Option<Instant>) = (None, None);
    let mut delivered = false;
    find(
        ctx.log,
        &format!("the member to be {}", doing.doing),
        work,
        Duration::from_millis(500),
        || {
            if task_looked.is_none_or(|at| at.elapsed() >= TASK_EVERY) {
                task_looked = Some(Instant::now());
                let thread = members.task(number)?;
                delivered |= brief_delivered(&thread);
                if is_over(&thread) {
                    return Ok(Some(Waited::Over(thread)));
                }
            }
            let lane = members.lane(ctx.agent)?;
            waiting.note(&lane, now_ms());
            let Some(window) = members.window_in(lane) else {
                return Ok(None);
            };
            // A lane reads working while its window is still to show the brief.
            let working = window.activity["state"] == "working" && delivered;
            worked = working.then(|| worked.unwrap_or_else(Instant::now));
            let for_how_long = worked.map_or(Duration::ZERO, |since| since.elapsed());
            let seen = if working {
                ready(members, &window, for_how_long)
            } else {
                None
            };
            Ok(seen.map(|seen| Waited::Ready(Box::new(window), seen)))
        },
    )
}

/// Gives the member its task, `brief`, and waits until it is at the point `ready` finds
/// ([`await_ready`], for `work` at most), which a pause then lands in the middle of.
/// Where the member did not get there, or its turn was over a moment after, the case
/// says why ([`super::unstarted`]) and none is returned.
pub fn begin(
    ctx: &Ctx,
    case: &mut Account,
    harness: Option<Harness>,
    brief: &str,
    work: Duration,
    doing: Doing,
    ready: impl FnMut(&Members, &MemberWindow, Duration) -> Option<String>,
) -> Result<Option<Begun>> {
    let members = ctx.members;
    let devin = harness == Some(Harness::Devin);
    let number = members.give(ctx.agent, brief)?;
    let mut waiting = Waiting::new(now_ms());
    let waited = await_ready(ctx, number, work, &doing, &mut waiting, ready)?;
    if let Ok(lane) = members.lane(ctx.agent) {
        waiting.note(&lane, now_ms());
    }
    let (window, seen) = match waited {
        Some(Waited::Ready(window, seen)) => (*window, seen),
        other => {
            let over = match &other {
                Some(Waited::Over(thread)) => {
                    Some(thread["state"].as_str().unwrap_or("undefined").to_owned())
                }
                _ => None,
            };
            let told = over
                .as_ref()
                .and_then(|_| unstarted::told_of(members, number));
            let delivered = members
                .task(number)
                .is_ok_and(|thread| brief_delivered(&thread));
            case.fail(unstarted::failure(
                work.as_secs(),
                over.as_deref().map(|state| (state, told.as_deref())),
                &waiting,
                delivered,
                &doing,
            ));
            unstarted::say_why(case, members, ctx.agent, number, &waiting, devin);
            return Ok(None);
        }
    };
    sleep(Duration::from_secs(1));
    let lane = members.lane(ctx.agent)?;
    waiting.note(&lane, now_ms());
    let state = lane["activity"]["state"]
        .as_str()
        .unwrap_or("undefined")
        .to_owned();
    if state != "working" {
        case.fail(format!(
            "the turn was over before the pause (the lane reads {state}): nothing was left to stop"
        ));
        unstarted::say_why(case, members, ctx.agent, number, &waiting, devin);
        return Ok(None);
    }
    case.say(format!(
        "T-{number}: {} is {}: {seen}; its activity: {state}",
        harness.map_or("the member", Harness::name),
        doing.doing,
    ));
    Ok(Some(Begun {
        number,
        window,
        seen,
    }))
}

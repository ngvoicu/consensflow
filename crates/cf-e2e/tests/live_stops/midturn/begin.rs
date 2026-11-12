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
    /// What only a member that did the work says in its result, where the work has such a
    /// word: a result without it, from a task that is over, is a member that declined.
    pub word: Option<&'static str>,
}

/// What a case says where its member declined the workload: it is no failed Stop, because no
/// Stop was tried.
pub const DECLINED: &str =
    "the workload never began (the model declined): precondition unmet, no Stop was tried";

/// Whether the member of the task of `thread` declined the workload: its task is over with a
/// result (it ended its turn with an answer: a task the daemon failed or the human cancelled
/// is none of the model's), and the result is not the work's (it does not hold `word`, where
/// the work has one). The caller says it only of a member whose work was never seen.
pub fn declined(thread: &Value, word: Option<&str>) -> bool {
    if !matches!(thread["state"].as_str(), Some("done" | "accepted")) {
        return false;
    }
    let results: Vec<&str> = thread["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|message| message["kind"] == "result")
        .filter_map(|message| message["body"].as_str())
        .collect();
    !results.is_empty() && !word.is_some_and(|word| results.iter().any(|body| body.contains(word)))
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

/// How long the case's reading may show the member's work while its lane reads anything but
/// working, before the disagreement is the finding: a look of the board that falls between
/// two of a lane's changes reads the lane a moment late, and a lane that catches up is no fault.
const AGREE_WITHIN: Duration = Duration::from_secs(10);

/// The case's reading and the lane, watched for the two to disagree: the reading shows the
/// member at its work (its command runs) and the lane reads something else, look after look.
#[derive(Debug, Default)]
struct Disagreement {
    since: Option<Instant>,
}

impl Disagreement {
    /// One look, at `now`: whether `lane_working`, and what the reading `saw` (none where it
    /// saw nothing). The state of the lane and what was seen where they have disagreed for
    /// [`AGREE_WITHIN`].
    fn look(&mut self, lane_working: bool, saw: Option<&str>, now: Instant) -> Option<String> {
        match (lane_working, saw) {
            (false, Some(saw)) => {
                let since = *self.since.get_or_insert(now);
                (now.duration_since(since) >= AGREE_WITHIN).then(|| saw.to_owned())
            }
            _ => {
                self.since = None;
                None
            }
        }
    }
}

/// What the case says where the lane read `state` for [`AGREE_WITHIN`] while the reading
/// `saw` the member at its work: the board's reading of the window is what is wrong, not the
/// member, so it is no "did not run" and no failed stop.
fn mismatch(state: &str, saw: &str, doing: &Doing) -> String {
    format!(
        "activity mismatch: the lane reads {state} while the member shows {}: {saw}",
        doing.shown
    )
}

/// What the wait for the member ended with.
enum Waited {
    /// Its window, at the point the case's reading found, and what it saw there.
    Ready(Box<MemberWindow>, String),
    /// Its window shows the work while its lane reads something else (`state`): what the
    /// reading saw.
    Mismatch(String, String),
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
    let mut disagreement = Disagreement::default();
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
            // The reading is asked at every look once the brief is there, whatever the lane
            // says: a lane that reads idle through a turn (the board's fault, not the
            // member's) must not hide the command running.
            let seen = if delivered {
                ready(members, &window, for_how_long)
            } else {
                None
            };
            if working {
                return Ok(seen.map(|seen| Waited::Ready(Box::new(window), seen)));
            }
            let state = window.activity["state"]
                .as_str()
                .unwrap_or("undefined")
                .to_owned();
            Ok(disagreement
                .look(false, seen.as_deref(), Instant::now())
                .map(|saw| Waited::Mismatch(state, saw)))
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
        Some(Waited::Mismatch(state, saw)) => {
            case.fail(mismatch(&state, &saw, &doing));
            unstarted::say_why(case, members, ctx.agent, number, &waiting, devin);
            return Ok(None);
        }
        Some(Waited::Over(thread)) if declined(&thread, doing.word) => {
            case.unmet(DECLINED);
            unstarted::say_why(case, members, ctx.agent, number, &waiting, devin);
            return Ok(None);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midturn::tool::DOING;

    #[test]
    fn a_declined_workload_is_an_unmet_precondition_and_no_stop_failure() {
        let thread = |state: &str, bodies: &[&str]| {
            serde_json::json!({
                "state": state,
                "messages": std::iter::once(serde_json::json!({ "kind": "task", "body": "brief" }))
                    .chain(bodies.iter().map(|body| serde_json::json!({ "kind": "result", "body": body })))
                    .collect::<Vec<_>>(),
            })
        };
        // A task over with a result that is not the work's: the model declined.
        let said = "I can't write all those numbers, it is too long.";
        assert!(declined(&thread("done", &[said]), Some("5000")));
        assert!(declined(&thread("accepted", &[said]), None));
        // One whose result has the work's word did it, and the case's reading was late.
        assert!(!declined(
            &thread("done", &["1 2 3 ... 5000"]),
            Some("5000")
        ));
        // A task the daemon failed or the human cancelled, or one with no result, is none the
        // model's to decline.
        assert!(!declined(&thread("failed", &[said]), Some("5000")));
        assert!(!declined(&thread("cancelled", &[said]), None));
        assert!(!declined(&thread("done", &[]), None));
        // The case is then said unmet, and holds: no failure, and a count of its own.
        let mut case = Account::new();
        case.unmet(DECLINED);
        assert!(case.held() && case.is_unmet());
        assert_eq!(
            case.lines(),
            ["UNMET the workload never began (the model declined): precondition unmet, no Stop was tried"]
        );
        let mut run = Account::new();
        run.absorb(case);
        assert!(run.is_unmet() && run.held());
        // A failure beside it is still a failure.
        run.fail("another");
        assert!(!run.held());
    }

    #[test]
    fn the_midturn_detector_names_an_idle_lane_that_shows_its_command_as_an_activity_mismatch() {
        let start = Instant::now();
        let at = |s: u64| start + Duration::from_secs(s);
        let saw = "its process is in the host's tree (4101 node ./tick-cf-stops-live.cjs 60 t.txt)";
        let mut look = Disagreement::default();
        // The reading shows the command and the lane reads idle: not yet a finding.
        assert_eq!(look.look(false, Some(saw), at(0)), None);
        assert_eq!(look.look(false, Some(saw), at(9)), None);
        // Still, ten seconds on: the board's word, not the member's.
        let found = look.look(false, Some(saw), at(10)).expect("a disagreement");
        let said = mismatch("idle", &found, &DOING);
        assert_eq!(
            said,
            format!("activity mismatch: the lane reads idle while the member shows its command running: {saw}")
        );
        assert!(!said.contains("did not run"), "{said}");
        // A lane that catches up, or a reading that loses it, starts the count again.
        let mut look = Disagreement::default();
        assert_eq!(look.look(false, Some(saw), at(0)), None);
        assert_eq!(look.look(true, Some(saw), at(8)), None);
        assert_eq!(look.look(false, Some(saw), at(12)), None);
        assert_eq!(look.look(false, None, at(30)), None);
        // No evidence, however long the lane reads idle, is no disagreement.
        assert_eq!(look.look(false, None, at(100)), None);
    }
}

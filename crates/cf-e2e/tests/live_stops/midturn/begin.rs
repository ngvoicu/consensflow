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

use cf_e2e::live::ask::{self, Item};
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

/// The harnesses whose record readers write an item of the role `tool` for each tool a turn
/// ran (Claude Code, Codex, Devin, Muse Code). Pi's does not: its record cannot show that no
/// tool ran, so no turn of Pi's is shown to have run none.
fn shows_tools(kind: &str) -> bool {
    matches!(kind, "claude-code" | "codex" | "devin" | "muse")
}

/// Whether the member's own record shows its last turn (the items after the last item of the
/// user's, the brief) answered in words and ran no tool: the one positive evidence there is that
/// the model declined. A record that cannot show a tool, one of no turn, or one with a tool item
/// in it shows nothing of the kind.
pub fn ran_no_tool(kind: &str, items: &[Item]) -> bool {
    if !shows_tools(kind) {
        return false;
    }
    let Some(brief) = items.iter().rposition(|item| item.role == "user") else {
        return false;
    };
    let turn = &items[brief + 1..];
    turn.iter()
        .any(|item| item.role == "assistant" && !item.text.trim().is_empty())
        && !turn.iter().any(|item| item.role == "tool")
}

/// Whether the member of the task of `thread` declined the workload: its task is over with a
/// result (a task the daemon failed or the human cancelled is none of the model's), the result is
/// not the work's (it holds no `word`, where the work has one), **and its own record shows the
/// turn ran no tool** ([`ran_no_tool`]; `items` are none where no record could be read). The
/// caller says it only of a member whose work the case's reading never saw. Anything less, a
/// record that shows a tool run or none that can be read, is a member that did not run its
/// command, as before: the work may have run unseen.
pub fn declined(thread: &Value, word: Option<&str>, kind: &str, items: Option<&[Item]>) -> bool {
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
    !results.is_empty()
        && !word.is_some_and(|word| results.iter().any(|body| body.contains(word)))
        && items.is_some_and(|items| ran_no_tool(kind, items))
}

/// The member's own record, as the daemon's reader reads it: its conversation is the one its
/// window was last seen on (`seen`) or is opened on, or, for a harness that names its own, the one the daemon bound to it.
/// None where there is no window, no conversation, or no record that can be read.
pub fn member_items(ctx: &Ctx, kind: &str, seen: Option<&str>) -> Option<Vec<Item>> {
    let members = ctx.members;
    let session = seen
        .map(str::to_owned)
        .or_else(|| {
            members
                .window_of(ctx.agent)
                .ok()
                .flatten()
                .and_then(|window| window.session())
        })
        .or_else(|| bound_session(&members.events(), members.handle_of(ctx.agent)))?;
    ask::records(kind, &session, members.rig().environment())
        .ok()
        .flatten()
        .map(|record| record.items)
}

/// The native session the daemon bound to the newest conversation it started for `handle`.
fn bound_session(events: &[Value], handle: &str) -> Option<String> {
    let started = events.iter().rfind(|event| {
        event["kind"] == "conversation.started" && event["data"]["participant"] == handle
    })?["data"]["conversation"]
        .as_i64()?;
    events
        .iter()
        .rfind(|event| {
            event["kind"] == "conversation.bound" && event["data"]["conversation"] == started
        })
        .and_then(|event| event["data"]["nativeSession"].as_str().map(str::to_owned))
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
    /// Its task, which is over: the daemon gave its window up, or its work ended; and the
    /// conversation its window was last seen on (its window may be gone when the task is read).
    Over(Value, Option<String>),
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
    let mut session: Option<String> = None;
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
                    return Ok(Some(Waited::Over(thread, session.clone())));
                }
            }
            let lane = members.lane(ctx.agent)?;
            waiting.note(&lane, now_ms());
            let Some(window) = members.window_in(lane) else {
                return Ok(None);
            };
            session = window.session().or(session.take());
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
        Some(Waited::Over(thread, session))
            if declined(
                &thread,
                doing.word,
                harness.map_or("claude-code", Harness::kind),
                member_items(
                    ctx,
                    harness.map_or("claude-code", Harness::kind),
                    session.as_deref(),
                )
                .as_deref(),
            ) =>
        {
            case.unmet(DECLINED);
            unstarted::say_why(case, members, ctx.agent, number, &waiting, devin);
            return Ok(None);
        }
        other => {
            let over = match &other {
                Some(Waited::Over(thread, _)) => {
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

    fn item(role: &str, text: &str) -> Item {
        Item {
            id: String::new(),
            role: role.to_owned(),
            text: text.to_owned(),
            complete: true,
        }
    }

    fn thread(state: &str, bodies: &[&str]) -> Value {
        serde_json::json!({
            "state": state,
            "messages": std::iter::once(serde_json::json!({ "kind": "task", "body": "brief" }))
                .chain(bodies.iter().map(|body| serde_json::json!({ "kind": "result", "body": body })))
                .collect::<Vec<_>>(),
        })
    }

    #[test]
    fn a_declined_workload_is_an_unmet_precondition_and_no_stop_failure() {
        let said = "I can't write all those numbers, it is too long.";
        let no_tool = [item("user", "brief"), item("assistant", said)];
        // A task over with a result that is not the work's, and a record that shows the turn
        // answered in words and ran no tool: the model declined.
        assert!(declined(
            &thread("done", &[said]),
            Some("5000"),
            "claude-code",
            Some(&no_tool)
        ));
        assert!(declined(
            &thread("accepted", &[said]),
            None,
            "codex",
            Some(&no_tool)
        ));
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
    fn work_that_ran_unseen_is_no_decline_and_stays_the_failure_it_was() {
        let said = "Done.";
        let ran = [
            item("user", "brief"),
            item("tool", "sleep 119"),
            item("assistant", said),
        ];
        let no_tool = [item("user", "brief"), item("assistant", said)];
        let over = thread("done", &[said]);
        // The record shows a tool run (the readers missed the command): FAIL as before.
        assert!(!declined(&over, None, "claude-code", Some(&ran)));
        // No record could be read: nothing shows the model declined.
        assert!(!declined(&over, None, "claude-code", None));
        assert!(!declined(&over, None, "claude-code", Some(&[])));
        // A record of a harness that writes no item for a tool (Pi's) cannot show none ran.
        assert!(!declined(&over, None, "pi", Some(&no_tool)));
        // A tool of an earlier turn is no tool of this one; the last turn answered in words only.
        let later = [
            item("user", "one"),
            item("tool", "x"),
            item("assistant", "a"),
            item("user", "two"),
            item("assistant", "no"),
        ];
        assert!(declined(&over, None, "devin", Some(&later)));
        // A turn with no answer in words is no decline; nor is a failed or cancelled task, or one
        // with no result, or whose result holds the work's word.
        assert!(!declined(&over, None, "muse", Some(&[item("user", "b")])));
        assert!(!declined(
            &thread("failed", &[said]),
            None,
            "claude-code",
            Some(&no_tool)
        ));
        assert!(!declined(
            &thread("cancelled", &[said]),
            None,
            "claude-code",
            Some(&no_tool)
        ));
        assert!(!declined(
            &thread("done", &[]),
            None,
            "claude-code",
            Some(&no_tool)
        ));
        assert!(!declined(
            &thread("done", &["1 ... 5000"]),
            Some("5000"),
            "claude-code",
            Some(&no_tool)
        ));
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

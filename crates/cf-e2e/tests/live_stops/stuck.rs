//! `stuck`: a window that may keep working through its presses, on any harness:
//! given a command that ignores the signals a harness ends a command with (`INT`,
//! `TERM`, `HUP`) and runs for [`STUCK_S`] seconds, then paused. Whether the
//! harness ends its turn anyway (it kills what it cannot signal, or lets go of it)
//! or waits for the command is what is found out. The daemon's own view is the
//! evidence (its lane's activity, `unstopped`, the notes), beside what was pressed;
//! the harness's own record is not read. A window that stops is an answer, as one
//! that does not: the leftovers are killed at the end.
//!
//! The command is found in the window's own tree, what descends from the window's
//! process, and each of its processes is known from then on by which process it is
//! ([`look`]): a harness that ends the script by its pid leaves the sleep, which then
//! descends from nothing of the window's, and is still known. What is killed at the end
//! is what was chosen so, each process only while it is still the one chosen; nothing
//! outside the window is, whatever its command line says (a check that ended every
//! process whose command line held its mark ended the worker that ran it, 2026-10-16),
//! so what an earlier run left ends by itself when its seconds are out.

use std::sync::OnceLock;
use std::thread::sleep;
use std::time::Duration;

use cf_e2e::live::claude::Process;
use cf_e2e::live::members::{
    descending_from, end_chosen, find, plus, shown, Account, Identity, Machine, MemberWindow,
    Members, Running, ThisMachine,
};
use cf_e2e::live::tree;
use cf_e2e::live::until;
use cf_e2e::{pattern, Result};
use regex::Regex;
use serde_json::Value;

use crate::stop::pause_text;
use crate::watch::{say_events, told_the_human, told_the_requester, unstopped};
use crate::{now_ms, Ctx, STUCK_WATCH, WORK};

/// How long the command ignores the signals for, and the script that does.
pub const STUCK_S: u64 = 200;
pub const SCRIPT: &str = "ignore-signals-cf-stops-live.sh";

/// The script the project holds: it ignores the signals, then waits.
pub fn script_text() -> String {
    format!("#!/bin/sh\ntrap '' INT TERM HUP\nsleep {STUCK_S}\n")
}

/// What the member is asked to run.
pub fn brief() -> String {
    format!("Run exactly this one shell command, then stop: sh ./{SCRIPT}")
}

/// The command line of the script: `sh ./<script>`, the shell named by its path or not.
fn script_pattern() -> &'static Regex {
    static SCRIPT_RE: OnceLock<Regex> = OnceLock::new();
    pattern::once(
        &SCRIPT_RE,
        &format!(r"^(\S*/)?sh\s+\./{}$", regex::escape(SCRIPT)),
    )
}

/// The command line of the script's sleep: `sleep <seconds>`, and nothing more.
fn sleep_pattern() -> &'static Regex {
    static SLEEP_RE: OnceLock<Regex> = OnceLock::new();
    pattern::once(&SLEEP_RE, &format!(r"^(\S*/)?sleep\s+{STUCK_S}$"))
}

/// The command's own processes among `found`: the script and its sleep, not the harness
/// whose launch text names the script. The sleep is a child of the script: one that no
/// script of `found` holds is none of the command.
pub fn of_the_command(found: &[Running]) -> Vec<Running> {
    let scripts: Vec<&Running> = found
        .iter()
        .filter(|one| script_pattern().is_match(&one.command))
        .collect();
    let sleeps = found.iter().filter(|one| {
        scripts.iter().any(|script| script.identity.pid == one.ppid)
            && sleep_pattern().is_match(&one.command)
    });
    scripts.iter().copied().chain(sleeps).cloned().collect()
}

/// What a look at the machine found of the command.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Seen {
    /// The command's processes that run: the script, and its sleep while it holds it.
    pub command: Vec<Running>,
    /// What of the command runs that the command does not hold: its sleeps that outlived
    /// their script. A harness that ends the script by its pid leaves its sleep to the
    /// machine, and the sleep, which took the script's ignoring of the signals with it,
    /// does not go when its window is asked to: the pane host waits out its whole grace for
    /// it, which is longer than the rig waits for the host to end.
    pub orphans: Vec<Running>,
}

/// A look at the machine's processes (`all`): the command's processes in the window whose
/// own process is `own` (what descends from it) are added to `chosen`, and what of
/// `chosen` runs now is what is seen of the command, each by which process it is, wherever
/// it went since. A process outside the window is none of it.
pub fn look(chosen: &mut Vec<Running>, all: &[Running], own: Option<Identity>) -> Seen {
    if let Some(own) = own {
        for one in of_the_command(&descending_from(all, own)) {
            if !chosen.iter().any(|known| known.identity == one.identity) {
                chosen.push(one);
            }
        }
    }
    let left = still_running(chosen, all);
    let command = of_the_command(&left);
    let orphans = left
        .into_iter()
        .filter(|one| !command.iter().any(|own| own.identity == one.identity))
        .collect();
    Seen { command, orphans }
}

/// What of `chosen` the machine's processes (`all`) hold now, as they are now: each by
/// which process it is, so a number another process took since is none of it.
fn still_running(chosen: &[Running], all: &[Running]) -> Vec<Running> {
    all.iter()
        .filter(|one| chosen.iter().any(|known| known.identity == one.identity))
        .cloned()
        .collect()
}

/// [`look`] at the machine now, the window of the member `agent` read again: none of the
/// window's own where it has none (yet, or any more).
fn look_now(members: &Members, agent: &str, chosen: &mut Vec<Running>) -> Result<Seen> {
    let own = members
        .window_of(agent)?
        .and_then(|window| members.window_process(&window));
    Ok(look(chosen, &ThisMachine.processes()?, own))
}

/// Processes as [`shown`] says them.
fn said(found: &[Running]) -> String {
    let processes: Vec<Process> = found
        .iter()
        .map(|one| Process {
            pid: one.identity.pid,
            ppid: one.ppid,
            command: one.command.clone(),
        })
        .collect();
    shown(&processes).join("; ")
}

/// What the pane host finds under the window (`pane.background`: its tree's processes
/// that began after the turn did): the daemon's own look at what the window has left
/// running, and what a close of the window asks to end.
pub fn host_view(members: &Members, window: &MemberWindow) -> String {
    tree::read(members.rig(), &window.pane).map_or_else(|said| said, |tree| tree.line())
}

/// What the lane of the window reads as, in a word: its activity, and what the
/// board says of its stop where it says it.
pub fn word_of(lane: &Value) -> String {
    format!(
        "{}{}",
        lane["activity"]["state"].as_str().unwrap_or("undefined"),
        unstopped(lane).map_or_else(String::new, |flag| format!(" unstopped {flag}"))
    )
}

/// Whether the lane is at rest and the board does not say it did not stop.
pub fn at_rest(lane: &Value) -> bool {
    ["idle", "closed"].contains(&lane["activity"]["state"].as_str().unwrap_or_default())
        && unstopped(lane).is_none()
}

/// How long what was ended of the command is given to be gone.
const GONE: Duration = Duration::from_secs(5);

/// The case, on the harness the member `agent` runs on (`name` is its name). What ignores
/// the signals is ended for good at the end, whatever came of the case, and with it the
/// window: every process of the command the run chose in the window, killed while it is
/// still the one chosen, and waited for until none is there. What could not be ended, or
/// was not gone in [`GONE`], fails the case (Astra and Sol on round 6: it was dropped).
pub fn run(ctx: &Ctx, case: &mut Account, name: &str) -> Result<()> {
    let mut chosen: Vec<Running> = Vec::new();
    let ran = drive(ctx, case, name, &mut chosen);
    for problem in end_chosen(&chosen).problems() {
        case.fail(format!("what ignores the signals: {problem}"));
    }
    let mut last = Ok(Vec::new());
    until(GONE, Duration::from_millis(100), || {
        last = ThisMachine
            .processes()
            .map(|all| still_running(&chosen, &all))
            .map_err(|failed| failed.to_string());
        unended(&last).is_none().then_some(())
    });
    if let Some(problem) = unended(&last) {
        case.fail(problem);
    }
    ran
}

/// What the last look after the end found (`last`: what of the command still ran, or why
/// the machine could not be read), as a problem: none where nothing of it ran.
fn unended(last: &std::result::Result<Vec<Running>, String>) -> Option<String> {
    match last {
        Ok(still) if still.is_empty() => None,
        Ok(still) => Some(format!(
            "what ignores the signals still ran {} s after its end: {}",
            GONE.as_secs(),
            said(still)
        )),
        Err(failed) => Some(format!(
            "whether what ignores the signals was gone could not be read: {failed}"
        )),
    }
}

/// The steps of the case: what it chose of the command in the window is in `chosen`.
fn drive(ctx: &Ctx, case: &mut Account, name: &str, chosen: &mut Vec<Running>) -> Result<()> {
    let members: &Members = ctx.members;
    let number = members.give(ctx.agent, &brief())?;
    let state = || -> Result<String> {
        Ok(members.lane(ctx.agent)?["activity"]["state"]
            .as_str()
            .unwrap_or("undefined")
            .to_owned())
    };
    let inside = find(
        ctx.log,
        "the member to be inside the command",
        WORK,
        Duration::from_millis(250),
        || {
            let seen = look_now(members, ctx.agent, chosen)?;
            Ok((!seen.command.is_empty()).then_some(()))
        },
    )?;
    if inside.is_none() {
        case.fail(format!(
            "the member never ran the command in {} s",
            WORK.as_secs()
        ));
        let window = members.window_of(ctx.agent)?;
        case.say(format!(
            "     its screen: {}",
            members.tail(window.as_ref().map_or("", |window| window.id()))
        ));
        return Ok(());
    }
    let Some(window) = members.window_of(ctx.agent)? else {
        case.fail("the member has no window");
        return Ok(());
    };
    sleep(Duration::from_secs(3));
    case.say(format!(
        "T-{number}: {name} is inside `sh ./{SCRIPT}`, which ignores INT, TERM and HUP for {STUCK_S} s ({}); its activity: {}",
        said(&look_now(members, ctx.agent, chosen)?.command),
        state()?
    ));
    let paused = members.pause(number)?;
    let made = paused.made_ms;
    if paused.answer["ok"] != true {
        case.fail(pause_text(number, &paused.answer));
        return Ok(());
    }
    case.say(pause_text(number, &paused.answer));

    // The lane as the board shows it, each change with its time, and when the command went.
    let mut seen: Vec<String> = Vec::new();
    let mut last: Option<String> = None;
    let mut gone_at: Option<i64> = None;
    let mut quiet_since: Option<i64> = None;
    let end = made + i64::try_from(STUCK_WATCH.as_millis()).unwrap_or(i64::MAX);
    while now_ms() < end {
        let lane = members.lane(ctx.agent)?;
        let word = word_of(&lane);
        if last.as_ref() != Some(&word) {
            seen.push(format!("{word} at {}", plus(now_ms(), made)));
        }
        last = Some(word);
        if gone_at.is_none() && look_now(members, ctx.agent, chosen)?.command.is_empty() {
            gone_at = Some(now_ms());
        }
        quiet_since = if at_rest(&lane) {
            Some(quiet_since.unwrap_or_else(now_ms))
        } else {
            None
        };
        // A window at rest for a while after the stop has stopped; one never at rest is watched to the end.
        if quiet_since.is_some_and(|since| now_ms() - since > 15_000) {
            break;
        }
        sleep(Duration::from_millis(500));
    }
    let presses = members.rig().presses(window.id(), &[27]);
    let exhausted = seen.iter().any(|word| word.contains("unstopped"));
    let human_inbox = members.inbox(None)?;
    let requester_inbox = members.inbox(Some("chief"))?;
    let (human, requester) = (
        told_the_human(&human_inbox),
        told_the_requester(&requester_inbox),
    );
    let gaps = crate::slow::gaps(&presses);
    case.say(format!(
        "{} Escapes: {}{}; the lane over the stop: {}",
        presses.len(),
        if presses.is_empty() {
            "none".to_owned()
        } else {
            presses
                .iter()
                .map(|press| plus(*press, made))
                .collect::<Vec<_>>()
                .join(", ")
        },
        if gaps.is_empty() {
            String::new()
        } else {
            format!(" (gaps {} s)", gaps.join(", "))
        },
        seen.join(", ")
    ));
    let now = look_now(members, ctx.agent, chosen)?;
    case.say(format!(
        "the command: {}",
        gone_at.map_or_else(
            || format!("still running: {}", said(&now.command)),
            |gone| format!("gone at {}", plus(gone, made))
        )
    ));
    // A harness that ended the script by its pid leaves its sleep, which ignores the signals.
    if !now.orphans.is_empty() {
        case.say(format!(
            "     its script went and left: {}",
            said(&now.orphans)
        ));
    }
    case.say(format!(
        "     what the pane host finds under the window: {}",
        host_view(members, &window)
    ));
    case.say(format!("its screen now: {}", members.tail(window.id())));
    let words = |notes: &[&Value]| {
        notes
            .first()
            .map_or_else(|| "undefined".to_owned(), |note| note["body"].to_string())
    };
    if exhausted {
        case.say(format!(
            "{name} kept working through its presses (the exhausted stop of §1.3): {} note to the human ({}), {} to the requester ({})",
            human.len(),
            words(&human),
            requester.len(),
            words(&requester)
        ));
    } else {
        case.say(format!(
            "{name} stopped: its lane read at rest {} after {} press{}; it could not be made to ignore them with a command that ignores signals",
            seen.iter()
                .find(|word| word.starts_with("idle") || word.starts_with("closed"))
                .map_or("never", String::as_str),
            presses.len(),
            if presses.len() == 1 { "" } else { "es" }
        ));
    }
    say_events(case, members, number);
    // Either end is an answer: a window that ignores (told to the human and the requester) or one that stops.
    case.require(!exhausted || (human.len() == 1 && requester.len() == 1));
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A process `pid`, begun at `started`, started by `ppid`.
    fn running(pid: u32, started: u64, ppid: u32, command: &str) -> Running {
        Running {
            identity: Identity { pid, started },
            ppid,
            command: command.to_owned(),
        }
    }

    fn pids(found: &[Running]) -> Vec<u32> {
        found.iter().map(|one| one.identity.pid).collect()
    }

    #[test]
    fn the_script_ignores_the_signals_a_harness_ends_a_command_with_and_waits() {
        assert_eq!(
            script_text(),
            "#!/bin/sh\ntrap '' INT TERM HUP\nsleep 200\n"
        );
        assert_eq!(
            brief(),
            "Run exactly this one shell command, then stop: sh ./ignore-signals-cf-stops-live.sh"
        );
    }

    #[test]
    fn the_commands_processes_are_the_script_and_the_sleep_that_is_its_child_not_the_harness_naming_them(
    ) {
        let found = vec![
            running(10, 1, 1, "sh ./ignore-signals-cf-stops-live.sh"),
            running(11, 1, 1, "/bin/sh ./ignore-signals-cf-stops-live.sh"),
            // The harness whose launch text holds the task, which names the script.
            running(12, 1, 1, "opencode run Run exactly this one shell command, then stop: sh ./ignore-signals-cf-stops-live.sh"),
            running(13, 1, 1, "sh ./ignore-signals-cf-stops-live.sh extra"),
            running(20, 2, 10, "sleep 200"),
            running(21, 2, 11, "/bin/sleep 200"),
            // Another's sleep, with no parent that is a script of ours.
            running(22, 2, 99, "sleep 200"),
            running(23, 2, 10, "sleep 20"),
            running(24, 2, 10, "sleep 200 and more"),
        ];
        assert_eq!(pids(&of_the_command(&found)), [10, 11, 20, 21]);
        assert!(of_the_command(&[running(20, 2, 10, "sleep 200")]).is_empty());
    }

    #[test]
    fn the_command_is_chosen_in_the_window_and_known_by_which_process_it_is_wherever_it_goes() {
        // The window's program (100) and the shell its tool runs the command in (101); an
        // earlier run's script and its sleep, which no window holds; and the harness, whose
        // launch text names the script.
        let window = Identity {
            pid: 100,
            started: 10,
        };
        let script = "sh ./ignore-signals-cf-stops-live.sh";
        let mut all = vec![
            running(
                100,
                10,
                1,
                "opencode run then stop: sh ./ignore-signals-cf-stops-live.sh",
            ),
            running(
                101,
                11,
                100,
                "/bin/zsh -c sh ./ignore-signals-cf-stops-live.sh",
            ),
            running(102, 12, 101, script),
            running(103, 13, 102, "sleep 200"),
            running(50, 1, 1, script),
            running(51, 1, 50, "sleep 200"),
        ];
        let mut chosen = Vec::new();
        let seen = look(&mut chosen, &all, Some(window));
        assert_eq!(pids(&seen.command), [102, 103]);
        assert!(seen.orphans.is_empty());
        assert_eq!(pids(&chosen), [102, 103]);

        // The harness ends the script by its pid: the sleep goes to the machine, and is the
        // command's orphan, though nothing of the window's holds it now.
        all.retain(|one| one.identity.pid != 102);
        all.iter_mut()
            .filter(|one| one.identity.pid == 103)
            .for_each(|one| one.ppid = 1);
        let seen = look(&mut chosen, &all, Some(window));
        assert!(seen.command.is_empty());
        assert_eq!(pids(&seen.orphans), [103]);

        // The window has gone, and the sleep's number is another process's now: nothing of the
        // command runs, and nothing is chosen of what the window no longer is.
        all = vec![running(103, 99, 1, "sleep 200"), running(50, 1, 1, script)];
        let seen = look(&mut chosen, &all, None);
        assert_eq!(seen, Seen::default());
        assert_eq!(pids(&chosen), [102, 103]);
    }

    #[test]
    fn what_still_runs_after_its_end_or_could_not_be_looked_at_is_a_problem_of_the_case() {
        // The wait for the command to be gone ran out, and nothing was said (Astra and Sol on
        // round 6).
        assert_eq!(unended(&Ok(Vec::new())), None);
        assert_eq!(
            unended(&Ok(vec![running(51, 1, 1, "sleep 200")])),
            Some(
                "what ignores the signals still ran 5 s after its end: 51 (1) sleep 200".to_owned()
            )
        );
        assert_eq!(
            unended(&Err("ps failed".to_owned())),
            Some(
                "whether what ignores the signals was gone could not be read: ps failed".to_owned()
            )
        );
    }

    #[test]
    fn a_lane_is_read_as_its_activity_and_the_boards_word_that_it_did_not_stop() {
        let working = json!({ "activity": { "state": "working" }, "unstopped": null });
        assert_eq!(word_of(&working), "working");
        let flagged = json!({ "activity": { "state": "working" }, "unstopped": { "rounds": 3 } });
        assert_eq!(word_of(&flagged), "working unstopped {\"rounds\":3}");
        assert_eq!(word_of(&json!({})), "undefined");
    }

    #[test]
    fn a_lane_is_at_rest_when_it_is_idle_or_closed_and_not_said_to_have_ignored_the_stop() {
        for state in ["idle", "closed"] {
            assert!(
                at_rest(&json!({ "activity": { "state": state } })),
                "{state}"
            );
        }
        assert!(!at_rest(&json!({ "activity": { "state": "working" } })));
        assert!(!at_rest(
            &json!({ "activity": { "state": "idle" }, "unstopped": { "rounds": 3 } })
        ));
        assert!(!at_rest(&json!({})));
    }
}

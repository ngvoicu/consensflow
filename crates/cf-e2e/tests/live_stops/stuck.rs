//! `stuck`: a window that may keep working through its presses, on any harness:
//! given a command that ignores the signals a harness ends a command with (`INT`,
//! `TERM`, `HUP`) and runs for [`STUCK_S`] seconds, then paused. Whether the
//! harness ends its turn anyway (it kills what it cannot signal, or lets go of it)
//! or waits for the command is what is found out. The daemon's own view is the
//! evidence (its lane's activity, `unstopped`, the notes), beside what was pressed;
//! the harness's own record is not read. A window that stops is an answer, as one
//! that does not: the leftovers are killed at the end.

use std::sync::OnceLock;
use std::thread::sleep;
use std::time::Duration;

use cf_e2e::live::claude::{processes_with, Process};
use cf_e2e::live::members::{find, plus, shown, Account, MemberWindow, Members};
use cf_e2e::live::{last_lines, until};
use cf_e2e::process::{signal, Signal};
use cf_e2e::{pattern, Result};
use regex::Regex;
use serde_json::{json, Value};

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

/// The command's own processes: the script and its sleep, not the harness whose
/// launch text names the script. The sleep is a child of the script: one that no
/// script of ours holds is an earlier run's, and no part of this one.
pub fn of_the_command(named_script: Vec<Process>, named_sleep: Vec<Process>) -> Vec<Process> {
    let scripts: Vec<Process> = named_script
        .into_iter()
        .filter(|found| script_pattern().is_match(&found.command))
        .collect();
    let sleeps: Vec<Process> = named_sleep
        .into_iter()
        .filter(|found| {
            scripts.iter().any(|script| script.pid == found.ppid)
                && sleep_pattern().is_match(&found.command)
        })
        .collect();
    scripts.into_iter().chain(sleeps).collect()
}

/// What the script left, wherever it went: the script, and every sleep of its
/// seconds, a child of a script or not. A harness that ends the script by its own
/// pid leaves the sleep to the machine, and the sleep, which took the script's
/// ignoring of the signals with it, does not go when its window is asked to: the
/// pane host waits out its whole grace for it, which is longer than the rig waits
/// for the host to end.
pub fn of_what_it_left(named_script: Vec<Process>, named_sleep: Vec<Process>) -> Vec<Process> {
    named_script
        .into_iter()
        .filter(|found| script_pattern().is_match(&found.command))
        .chain(
            named_sleep
                .into_iter()
                .filter(|found| sleep_pattern().is_match(&found.command)),
        )
        .collect()
}

/// The processes the script has named in the command line, and its sleeps.
fn named() -> Result<(Vec<Process>, Vec<Process>)> {
    Ok((
        processes_with(SCRIPT)?,
        processes_with(&format!("sleep {STUCK_S}"))?,
    ))
}

/// The processes of the command as the machine has them now.
pub fn running() -> Result<Vec<Process>> {
    let (scripts, sleeps) = named()?;
    Ok(of_the_command(scripts, sleeps))
}

/// Everything of the command the machine has now, whoever holds it ([`of_what_it_left`]).
pub fn left_behind() -> Result<Vec<Process>> {
    let (scripts, sleeps) = named()?;
    Ok(of_what_it_left(scripts, sleeps))
}

/// What the pane host finds under the window (`pane.background`: its tree's processes
/// that began after the turn did): the daemon's own look at what the window has left
/// running, and what a close of the window asks to end.
pub fn host_view(members: &Members, window: &MemberWindow) -> String {
    let asked = members.rig().host(
        "pane.background",
        json!({ "id": window.pane["id"], "generation": window.pane["generation"] }),
    );
    match asked {
        Ok(answer) if answer["ok"] == true => {
            let found: Vec<String> = answer["processes"]
                .as_array()
                .map(|list| {
                    list.iter()
                        .map(|process| {
                            format!(
                                "{} {}",
                                process["pid"],
                                process["command"]
                                    .as_str()
                                    .unwrap_or_default()
                                    .chars()
                                    .take(90)
                                    .collect::<String>()
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            if found.is_empty() {
                "none".to_owned()
            } else {
                format!("{} ({})", found.join("; "), answer["total"])
            }
        }
        Ok(answer) => format!("(the host said: {answer})"),
        Err(failed) => format!("(could not be asked: {failed})"),
    }
}

/// What of `left` the command does not hold: its sleeps that outlived their script.
pub fn orphans_of(left: &[Process], held: &[Process]) -> Vec<Process> {
    left.iter()
        .filter(|process| !held.iter().any(|own| own.pid == process.pid))
        .cloned()
        .collect()
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

/// Ends what ignores the signals for good, and with it the window: every process
/// the command has left, killed, and waited for until none is there.
pub fn end_the_command() {
    for found in left_behind().unwrap_or_default() {
        let _ = signal(found.pid, Signal::Kill);
    }
    until(Duration::from_secs(5), Duration::from_millis(100), || {
        left_behind().ok().filter(Vec::is_empty).map(|_| ())
    });
}

/// The case, on the harness the member `agent` runs on (`name` is its name).
pub fn run(ctx: &Ctx, case: &mut Account, name: &str) -> Result<()> {
    let members: &Members = ctx.members;
    // What an earlier run left, which ignores the signals and outlives it, is none of this one's.
    let stale = left_behind()?;
    if !stale.is_empty() {
        end_the_command();
        case.say(format!(
            "     an earlier run left {} behind, ended",
            shown(&stale).join("; ")
        ));
    }
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
        || Ok((!running()?.is_empty()).then_some(())),
    )?;
    if inside.is_none() {
        case.fail(format!(
            "the member never ran the command in {} s",
            WORK.as_secs()
        ));
        let window = members.window_of(ctx.agent)?;
        case.say(format!(
            "     its screen: {}",
            last_lines(
                &members
                    .rig()
                    .screen(window.as_ref().map_or("", |window| window.id()))
            )
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
        shown(&running()?).join("; "),
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
        if gone_at.is_none() && running()?.is_empty() {
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
    case.say(format!(
        "the command: {}",
        gone_at.map_or_else(
            || format!(
                "still running: {}",
                shown(&running().unwrap_or_default()).join("; ")
            ),
            |gone| format!("gone at {}", plus(gone, made))
        )
    ));
    // A harness that ended the script by its pid leaves its sleep, which ignores the signals.
    let orphans = orphans_of(&left_behind()?, &running()?);
    if !orphans.is_empty() {
        case.say(format!(
            "     its script went and left: {}",
            shown(&orphans).join("; ")
        ));
    }
    case.say(format!(
        "     what the pane host finds under the window: {}",
        host_view(members, &window)
    ));
    case.say(format!(
        "its screen now: {}",
        last_lines(&members.rig().screen(window.id()))
    ));
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

    fn process(pid: u32, ppid: u32, command: &str) -> Process {
        Process {
            pid,
            ppid,
            command: command.to_owned(),
        }
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
        let scripts = vec![
            process(10, 1, "sh ./ignore-signals-cf-stops-live.sh"),
            process(11, 1, "/bin/sh ./ignore-signals-cf-stops-live.sh"),
            // The harness whose launch text holds the task, which names the script.
            process(12, 1, "opencode run Run exactly this one shell command, then stop: sh ./ignore-signals-cf-stops-live.sh"),
            process(13, 1, "sh ./ignore-signals-cf-stops-live.sh extra"),
        ];
        let sleeps = vec![
            process(20, 10, "sleep 200"),
            process(21, 11, "/bin/sleep 200"),
            // Another's sleep, with no parent that is a script of ours.
            process(22, 99, "sleep 200"),
            process(23, 10, "sleep 20"),
            process(24, 10, "sleep 200 and more"),
        ];
        let found: Vec<u32> = of_the_command(scripts, sleeps)
            .iter()
            .map(|found| found.pid)
            .collect();
        assert_eq!(found, [10, 11, 20, 21]);
        assert!(of_the_command(vec![], vec![process(20, 10, "sleep 200")]).is_empty());
    }

    #[test]
    fn what_the_script_left_is_the_script_and_every_sleep_of_it_whoever_holds_it() {
        let scripts = vec![
            process(10, 1, "sh ./ignore-signals-cf-stops-live.sh"),
            process(
                12,
                1,
                "opencode run then stop: sh ./ignore-signals-cf-stops-live.sh",
            ),
        ];
        let sleeps = vec![
            process(20, 10, "sleep 200"),
            // A harness that ended the script by its pid leaves its sleep to the machine.
            process(22, 1, "/bin/sleep 200"),
            process(23, 10, "sleep 20"),
            process(24, 10, "sleep 200 and more"),
        ];
        let left: Vec<u32> = of_what_it_left(scripts.clone(), sleeps.clone())
            .iter()
            .map(|found| found.pid)
            .collect();
        assert_eq!(left, [10, 20, 22]);
        // The command's own are the ones a script holds: the orphan is what the rest of them is.
        let held = of_the_command(scripts.clone(), sleeps.clone());
        let orphans: Vec<u32> = orphans_of(&of_what_it_left(scripts, sleeps), &held)
            .iter()
            .map(|found| found.pid)
            .collect();
        assert_eq!(orphans, [22]);
        assert!(of_what_it_left(vec![], vec![]).is_empty());
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

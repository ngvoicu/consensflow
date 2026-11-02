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
//! ([`look`]): a harness that ends the script by its pid leaves the waiter, which then
//! descends from nothing of the window's, and is still known. What is killed at the end
//! is what was chosen so, each process only while it is still the one chosen; nothing
//! outside the window is, whatever its command line says (a check that ended every
//! process whose command line held its mark ended the worker that ran it, 2026-10-16),
//! so what an earlier run left ends by itself when its seconds are out.
//!
//! The script is Node's (`node ./ignore-signals-cf-stops-live.cjs`): the one program that the
//! shell of every harness on every machine runs. It was `sh ./….sh`, which a Codex window on
//! Windows, whose shell is PowerShell, does not know ("sh is not recognized"): the case
//! never began there (T-1079). The script ignores the signals, and starts a waiter (Node
//! again, which ignores them too and holds the output the harness reads, as the sleep of a
//! shell script does): the script's child, found in the window's tree and ended with it on
//! both machines.

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
pub const SCRIPT: &str = "ignore-signals-cf-stops-live.cjs";

/// The script the project holds: it ignores the signals, and starts its waiter, which
/// ignores them and waits, holding the output the harness reads; the script goes with it.
const SCRIPT_TEXT: &str = r#"// Ignores INT, TERM and HUP, and waits WAIT_MS ms in a child that does too.
for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.on(signal, () => {});
if (process.argv[2] === 'wait') {
  setTimeout(() => {}, WAIT_MS);
} else {
  const { spawn } = require('node:child_process');
  spawn(process.execPath, [__filename, 'wait'], { stdio: 'inherit', windowsHide: true }).on('exit', () => process.exit(0));
}
"#;

/// The script the project holds ([`SCRIPT_TEXT`], for [`STUCK_S`] seconds).
pub fn script_text() -> String {
    SCRIPT_TEXT.replace("WAIT_MS", &(STUCK_S * 1_000).to_string())
}

/// What the member is asked to run.
pub fn brief() -> String {
    format!("Run exactly this one shell command, then stop: node ./{SCRIPT}")
}

/// A program as a command line names it: by its path or not, with either separator, with
/// `.exe` or without, in any case, and, on Windows, in double quotes where the path holds a
/// space (`"C:\Program Files\nodejs\node.exe" ./x`, as Windows lists Node's).
fn program(name: &str) -> String {
    format!(r#"(?:"(?:[^"]*[\\/])?(?i:{name}(?:\.exe)?)"|(?:\S*[\\/])?(?i:{name}(?:\.exe)?))"#)
}

/// The command line of the script: `node ./<script>`, Node named by its path or not.
fn script_pattern() -> &'static Regex {
    static SCRIPT_RE: OnceLock<Regex> = OnceLock::new();
    pattern::once(
        &SCRIPT_RE,
        &format!(r"^{}\s+\./{}$", program("node"), regex::escape(SCRIPT)),
    )
}

/// The command line of the script's waiter: Node, the script by its whole path (the script
/// starts it by its own file name, which Node makes absolute), and `wait`, and nothing more.
fn waiter_pattern() -> &'static Regex {
    static WAITER_RE: OnceLock<Regex> = OnceLock::new();
    pattern::once(
        &WAITER_RE,
        &format!(
            r#"^{}\s+(?:"[^"]*|\S*)(?i:{})"?\s+wait$"#,
            program("node"),
            regex::escape(SCRIPT)
        ),
    )
}

/// The command's own processes among `found`: the script and its waiter, not the harness
/// whose launch text names the script. The waiter is a child of the script: one that no
/// script of `found` holds is none of the command.
pub fn of_the_command(found: &[Running]) -> Vec<Running> {
    let scripts: Vec<&Running> = found
        .iter()
        .filter(|one| script_pattern().is_match(&one.command))
        .collect();
    let waiters = found.iter().filter(|one| {
        scripts.iter().any(|script| script.identity.pid == one.ppid)
            && waiter_pattern().is_match(&one.command)
    });
    scripts.iter().copied().chain(waiters).cloned().collect()
}

/// What a look at the machine found of the command.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Seen {
    /// The command's processes that run: the script, and its waiter while it holds it.
    pub command: Vec<Running>,
    /// What of the command runs that the command does not hold: its waiters that outlived
    /// their script. A harness that ends the script by its pid leaves its waiter to the
    /// machine, and the waiter, which ignores the signals as the script does, does not go
    /// when its window is asked to: the pane host waits out its whole grace for it, which
    /// is longer than the rig waits for the host to end.
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

/// What is said of the screen of a member that ran nothing: "it has no window" where there is none, so
/// that a blank screen (a window that drew nothing) is not read as no window.
fn screen_line(screen: Option<String>) -> String {
    match screen {
        Some(screen) => format!("     its screen: {screen}"),
        None => "     it has no window".to_owned(),
    }
}

/// Whether the tree the host says holds the script's waiter: a process that runs Node on the
/// script with `wait`, by whichever path ([`waiter_pattern`]).
fn names_the_waiter(tree: &tree::Tree) -> bool {
    tree.processes
        .iter()
        .any(|process| waiter_pattern().is_match(&process.command))
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
        case.say(screen_line(
            window.as_ref().map(|window| members.tail(window.id())),
        ));
        // What else is known of a member that ran nothing: its lane, and what the host and the
        // machine find under its window (a screen may be blank where the window is not), and
        // what the daemon wrote of its task: whether a window was opened, and what became of it.
        case.say(format!(
            "     its lane: {}; what the pane host finds under its window: {}; what descends from its window's process: {}",
            state().unwrap_or_else(|failed| format!("(unread: {failed})")),
            window
                .as_ref()
                .map_or_else(|| "no window".to_owned(), |window| host_view(members, window)),
            window
                .as_ref()
                .and_then(|window| members.window_process(window))
                .map_or_else(
                    || "no process".to_owned(),
                    |own| said(&descending_from(&ThisMachine.processes().unwrap_or_default(), own))
                )
        ));
        say_events(case, members, number);
        return Ok(());
    }
    let Some(window) = members.window_of(ctx.agent)? else {
        case.fail("the member has no window");
        return Ok(());
    };
    sleep(Duration::from_secs(3));
    case.say(format!(
        "T-{number}: {name} is inside `node ./{SCRIPT}`, which ignores INT, TERM and HUP for {STUCK_S} s ({}); its activity: {}",
        said(&look_now(members, ctx.agent, chosen)?.command),
        state()?
    ));
    // What the pane host finds under the window while the command runs: the turn's work, which
    // a stop would end. It is seen here and not only after the stop (a harness that ends the
    // script by its pid leaves the host nothing to walk from): on Windows the host walks the
    // parents the system names (T-142, P3-7).
    case.say(format!(
        "     what the pane host finds under the window while it runs: {}",
        host_view(members, &window)
    ));
    // On Windows the host names the script's waiter among them: it walks the parents the system
    // names. One it does not name is one a stop leaves running.
    if cfg!(windows)
        && !tree::read(members.rig(), &window.pane).is_ok_and(|tree| names_the_waiter(&tree))
    {
        case.fail("the pane host did not name the script's waiter among what the turn started, while it ran: a stop would leave it running");
    }
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
    let human_inbox = members.inbox(None)?;
    let requester_inbox = members.inbox(Some("chief"))?;
    let (human, requester) = (
        told_the_human(&human_inbox),
        told_the_requester(&requester_inbox),
    );
    // A window that ignored every round is closed, and the requester's note says so;
    // one the host would not close shows `unstopped` on its lane.
    let closed = requester.iter().any(|note| {
        note["body"]
            .as_str()
            .is_some_and(|body| body.contains("so ConsensFlow closed it"))
    });
    let exhausted = closed || seen.iter().any(|word| word.contains("unstopped"));
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
    // A harness that ended the script by its pid leaves its waiter, which ignores the signals.
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
            "{name} kept working through its presses, and {}: {} note to the human ({}), {} to the requester ({})",
            if closed {
                "the daemon closed its window"
            } else {
                "the host would not close its window"
            },
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
    // Either end is an answer: a window that ignores (closed, and told once, to the
    // requester: one note of one event, T-551) or one that stops.
    case.require(!exhausted || (human.is_empty() && requester.len() == 1));
    Ok(())
}

#[cfg(test)]
mod tests {
    use cf_e2e::process::Run;
    use serde_json::json;

    use super::*;

    /// A process `pid`, begun at `started`, started by `ppid`.
    fn running(pid: u32, started: u64, ppid: u32, command: &str) -> Running {
        Running {
            identity: Identity { pid, started },
            ppid,
            command: command.to_owned(),
            exe: None,
        }
    }

    fn pids(found: &[Running]) -> Vec<u32> {
        found.iter().map(|one| one.identity.pid).collect()
    }

    /// The script as the project holds it, started by Node the way a harness's shell starts it:
    /// the waiter's command line as the system lists it, the script's own as it was typed.
    const SCRIPT_LINE: &str = "node ./ignore-signals-cf-stops-live.cjs";

    #[test]
    fn the_script_is_node_s_and_ignores_the_signals_a_harness_ends_a_command_with_and_waits() {
        // `sh` is no program of a Codex window on Windows, whose shell is PowerShell (T-1079);
        // Node is the one program every harness's shell on every machine runs.
        let text = script_text();
        assert!(
            text.contains("for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.on(signal, () => {});"),
            "{text}"
        );
        assert!(text.contains("setTimeout(() => {}, 200000);"), "{text}");
        assert!(text.contains("process.argv[2] === 'wait'"), "{text}");
        assert!(
            text.contains("spawn(process.execPath, [__filename, 'wait'], { stdio: 'inherit'"),
            "{text}"
        );
        assert!(!text.contains("WAIT_MS"), "{text}");
        assert!(!text.starts_with("#!") && !text.contains("trap "), "{text}");
        assert_eq!(SCRIPT, "ignore-signals-cf-stops-live.cjs");
        assert_eq!(
            brief(),
            "Run exactly this one shell command, then stop: node ./ignore-signals-cf-stops-live.cjs"
        );
    }

    #[test]
    fn the_commands_processes_are_the_script_and_the_waiter_that_is_its_child_not_the_harness_naming_them(
    ) {
        let waiter =
            "/opt/homebrew/bin/node /tmp/stops-stuck-codex/ignore-signals-cf-stops-live.cjs wait";
        let found = vec![
            running(10, 1, 1, SCRIPT_LINE),
            running(11, 1, 1, "/opt/homebrew/bin/node ./ignore-signals-cf-stops-live.cjs"),
            // The harness whose launch text holds the task, which names the script.
            running(12, 1, 1, "codex exec Run exactly this one shell command, then stop: node ./ignore-signals-cf-stops-live.cjs"),
            // The shell the harness ran it in, whose line holds the script's.
            running(14, 1, 1, "/bin/zsh -c node ./ignore-signals-cf-stops-live.cjs"),
            running(13, 1, 1, "node ./ignore-signals-cf-stops-live.cjs extra"),
            running(20, 2, 10, waiter),
            running(21, 2, 11, waiter),
            // Another's waiter, with no parent that is a script of ours; another script's.
            running(22, 2, 99, waiter),
            running(23, 2, 10, "node /tmp/other/other-script.cjs wait"),
            running(24, 2, 10, &format!("{waiter} and more")),
        ];
        assert_eq!(pids(&of_the_command(&found)), [10, 11, 20, 21]);
        assert!(of_the_command(&[running(20, 2, 10, waiter)]).is_empty());
    }

    /// Windows lists a process by the command line it was started with, which for Node names it
    /// by its path, with `\`, `.exe`, and in double quotes where the path holds a space, and the
    /// script's waiter by the whole path Node made of the script's name.
    #[test]
    fn the_commands_processes_are_found_by_the_command_lines_windows_lists_for_them_too() {
        let found = vec![
            running(
                10,
                1,
                1,
                r#""C:\Program Files\nodejs\node.exe" ./ignore-signals-cf-stops-live.cjs"#,
            ),
            // The path unquoted (it holds no space), another spelling of the same, in capitals.
            running(
                11,
                1,
                1,
                r"C:\nodejs\node.exe ./ignore-signals-cf-stops-live.cjs",
            ),
            running(
                12,
                1,
                1,
                "C:/nodejs/node ./ignore-signals-cf-stops-live.cjs",
            ),
            running(
                13,
                1,
                1,
                r#""C:\NODEJS\NODE.EXE" ./ignore-signals-cf-stops-live.cjs"#,
            ),
            // Not the command's: the shell the harness ran it in, which holds its text;
            // another program by a name that ends in `node`; and the script run with more.
            running(
                14,
                1,
                1,
                r#"powershell.exe -Command "node ./ignore-signals-cf-stops-live.cjs""#,
            ),
            running(
                15,
                1,
                1,
                r#""C:\Program Files\Git\usr\bin\sh.exe" ./ignore-signals-cf-stops-live.cjs"#,
            ),
            running(
                16,
                1,
                1,
                r#""C:\Program Files\nodejs\node.exe" ./ignore-signals-cf-stops-live.cjs extra"#,
            ),
            running(
                17,
                1,
                1,
                r"C:\tools\subnode.exe ./ignore-signals-cf-stops-live.cjs",
            ),
            // The waiters: the script by its whole path, quoted where it holds a space.
            running(
                20,
                2,
                10,
                r#""C:\Program Files\nodejs\node.exe" "C:\Users\A GV\live\stops\ignore-signals-cf-stops-live.cjs" wait"#,
            ),
            running(
                21,
                2,
                11,
                r"C:\nodejs\node.exe C:\Users\AGV\live\IGNORE-SIGNALS-CF-STOPS-LIVE.CJS wait",
            ),
            running(
                22,
                2,
                12,
                "C:/nodejs/node C:/Users/AGV/live/ignore-signals-cf-stops-live.cjs wait",
            ),
            // Another's waiter; another script's; and more than `wait`.
            running(
                24,
                2,
                99,
                r"C:\nodejs\node.exe C:\Users\AGV\live\ignore-signals-cf-stops-live.cjs wait",
            ),
            running(
                25,
                2,
                10,
                r"C:\nodejs\node.exe C:\Users\AGV\live\ignore-signals.cjs wait",
            ),
            running(
                26,
                2,
                10,
                r"C:\nodejs\node.exe C:\Users\AGV\live\ignore-signals-cf-stops-live.cjs wait more",
            ),
        ];
        assert_eq!(pids(&of_the_command(&found)), [10, 11, 12, 13, 20, 21, 22]);
    }

    #[test]
    fn a_member_with_no_window_says_so_and_a_blank_screen_is_said_to_be_one() {
        // A screen that is blank reads the same as no window, which a failure must tell apart
        // (T-194, P3-4: the failed run's screen was blank for the whole 300 s).
        assert_eq!(screen_line(None), "     it has no window");
        assert_eq!(screen_line(Some(String::new())), "     its screen: ");
        assert_eq!(
            screen_line(Some("a prompt".to_owned())),
            "     its screen: a prompt"
        );
    }

    #[test]
    fn the_tree_the_host_says_names_the_waiter_by_its_command_line_whichever_path_it_has() {
        let tree = |commands: &[&str]| tree::Tree {
            total: commands.len(),
            processes: commands
                .iter()
                .enumerate()
                .map(|(number, command)| tree::Proc {
                    pid: 100 + number as u64,
                    command: (*command).to_owned(),
                })
                .collect(),
        };
        let script = r#""C:\Program Files\nodejs\node.exe" ./ignore-signals-cf-stops-live.cjs"#;
        assert!(names_the_waiter(&tree(&[
            script,
            r#""C:\Program Files\nodejs\node.exe" C:\live\ignore-signals-cf-stops-live.cjs wait"#
        ])));
        assert!(names_the_waiter(&tree(&[
            "/usr/local/bin/node /tmp/live/ignore-signals-cf-stops-live.cjs wait"
        ])));
        // The script alone, another waiter, and nothing.
        assert!(!names_the_waiter(&tree(&[script])));
        assert!(!names_the_waiter(&tree(&[
            "node /tmp/live/other.cjs wait",
            "sleep 200"
        ])));
        assert!(!names_the_waiter(&tree(&[])));
    }

    #[test]
    fn the_command_is_chosen_in_the_window_and_known_by_which_process_it_is_wherever_it_goes() {
        // The window's program (100) and the shell its tool runs the command in (101); an
        // earlier run's script and its waiter, which no window holds; and the harness, whose
        // launch text names the script.
        let window = Identity {
            pid: 100,
            started: 10,
        };
        let waiter = "node /tmp/live/ignore-signals-cf-stops-live.cjs wait";
        let mut all = vec![
            running(
                100,
                10,
                1,
                "codex exec then stop: node ./ignore-signals-cf-stops-live.cjs",
            ),
            running(
                101,
                11,
                100,
                "/bin/zsh -c node ./ignore-signals-cf-stops-live.cjs",
            ),
            running(102, 12, 101, SCRIPT_LINE),
            running(103, 13, 102, waiter),
            running(50, 1, 1, SCRIPT_LINE),
            running(51, 1, 50, waiter),
        ];
        let mut chosen = Vec::new();
        let seen = look(&mut chosen, &all, Some(window));
        assert_eq!(pids(&seen.command), [102, 103]);
        assert!(seen.orphans.is_empty());
        assert_eq!(pids(&chosen), [102, 103]);

        // The harness ends the script by its pid: the waiter goes to the machine, and is the
        // command's orphan, though nothing of the window's holds it now.
        all.retain(|one| one.identity.pid != 102);
        all.iter_mut()
            .filter(|one| one.identity.pid == 103)
            .for_each(|one| one.ppid = 1);
        let seen = look(&mut chosen, &all, Some(window));
        assert!(seen.command.is_empty());
        assert_eq!(pids(&seen.orphans), [103]);

        // The window has gone, and the waiter's number is another process's now: nothing of
        // the command runs, and nothing is chosen of what the window no longer is.
        all = vec![running(103, 99, 1, waiter), running(50, 1, 1, SCRIPT_LINE)];
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
            unended(&Ok(vec![running(51, 1, 1, "node x.cjs wait")])),
            Some(
                "what ignores the signals still ran 5 s after its end: 51 (1) node x.cjs wait"
                    .to_owned()
            )
        );
        assert_eq!(
            unended(&Err("ps failed".to_owned())),
            Some(
                "whether what ignores the signals was gone could not be read: ps failed".to_owned()
            )
        );
    }

    /// The script, run by the Node of this machine in a folder of its own as a harness's shell
    /// runs it, starts its waiter, and both are the command's by the machine's own table (`ps`
    /// on a Mac, Windows' process table there): the lines the patterns read are the ones the
    /// system lists, not the ones this file imagines. The test plays the window's process: the
    /// script is what it started. What it chose it ends by which process it is.
    #[test]
    fn the_real_script_runs_under_node_and_its_script_and_waiter_are_found_in_the_machines_table() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join(SCRIPT), script_text()).unwrap();
        let script = Run::new("node")
            .arg(format!("./{SCRIPT}"))
            .cwd(folder.path())
            .inheriting_env()
            .finding_programs()
            .spawn()
            .unwrap();
        let own = ThisMachine
            .identity(std::process::id())
            .unwrap()
            .expect("this test runs");
        let mut chosen = Vec::new();
        let seen = until(Duration::from_secs(30), Duration::from_millis(100), || {
            let seen = look(&mut chosen, &ThisMachine.processes().unwrap(), Some(own));
            (seen.command.len() == 2).then_some(seen)
        });
        let ended = end_chosen(&chosen);
        drop(script);
        let seen = seen.expect("the script and its waiter were never both found");
        assert!(seen.orphans.is_empty());
        let (script, waiter) = (&seen.command[0], &seen.command[1]);
        assert!(
            script_pattern().is_match(&script.command),
            "{}",
            script.command
        );
        assert!(
            waiter_pattern().is_match(&waiter.command),
            "{}",
            waiter.command
        );
        assert_eq!(waiter.ppid, script.identity.pid);
        assert!(ended.problems().is_empty(), "{:?}", ended.problems());
        assert_eq!(ended.ended.len(), 2, "{ended:?}");
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

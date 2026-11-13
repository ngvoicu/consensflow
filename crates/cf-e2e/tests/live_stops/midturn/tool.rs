//! `midtool`: a window in the middle of a shell command, on any harness. The member is
//! asked to run one long command, the project's [`TICKER`] (a Node script that counts the
//! seconds it has run in a file, for a minute), and the case waits until the command runs
//! and pauses the task. It was written for the
//! Devin window of T-5 (2026-10-12, 3.0.1; Devin 3000.10.21 on Windows), which was running
//! a shell command (headless Edge checks) when the pause came and went on for about 70 s
//! through three rounds of Escape, where a Devin of 3000.11.3 stops in the middle of its
//! answer (`midturn`): a turn that is at a tool may wait for it, whatever the keys do.
//!
//! What shows the command running is the host's process tree (`pane.background`: what the
//! window's tree started after its turn began, on every platform, with no `ps`), the count
//! in its file (Codex starts a program with no shell, which the tree never lists, T-1137),
//! its replies on the screen, or, for Devin, its call on the wire. `midbg` judges the
//! command it started in the background by that count alone: whether it went on counting
//! through the stop. The case holds when the
//! daemon read the window at rest within the rounds and paid the stop there, as `midturn`
//! does ([`super::judge`]). The command is no check but a thing to know, said at each look
//! after a round and at the end: whether the host's tree still has it once the stop is
//! paid (a harness that cancels a turn may or may not end the tool it ran), and when it was
//! first read gone. The stop is watched until the window has been at rest as long as a
//! window at rest is watched, or for as long as a window that may not stop is, which is
//! longer than the command runs: when a turn that waits for its tool ends is told whichever
//! way it goes.

use std::path::Path;
use std::time::Duration;

use cf_e2e::live::ask;
use cf_e2e::live::cut;
use cf_e2e::live::members::{
    plus, quiet_command, runs_command, ticks_in, Account, MemberWindow, Members, Moving,
};
use cf_e2e::live::tree::{self, Read};
use cf_e2e::live::Harness;
use cf_e2e::Result;

use super::begin::{begin, Doing};
use super::wire::Wire;
use super::{screen_drawn, stop_it, Hold};
use crate::{Ctx, STUCK_WATCH, WORK};

/// What the member is asked to do, in the words the case says it in.
pub const DOING: Doing = Doing {
    to: "run its command",
    did: "ran its command",
    doing: "running its command",
    shown: "its command running",
    word: None,
};

/// What the case says where the command was seen by its process in the host's tree.
const IN_TREE: &str = "its process is in the host's tree";

/// How many seconds the command runs for.
pub const SECONDS: u64 = 60;

/// The script the project holds that the member is told to run (`node ./<it> <seconds>
/// <file>`): it writes how many seconds it has run into the file, in the project's folder,
/// each second, until its seconds are out. Node is one program in every harness's shell, and
/// its file shows the command running where no process tree lists it (Codex starts a program
/// with no shell, which the host names as no turn's work, T-1137).
pub const TICKER: &str = "tick-cf-stops-live.cjs";

/// What [`TICKER`] holds. It takes no hang-up, as a long job its agent leaves is started
/// (`nohup`): a terminal a harness closes after the shell that started a job with `&` (Codex's,
/// on the Mac) hangs up what it started.
pub fn ticker_text() -> &'static str {
    "process.on(\"SIGHUP\", () => {});\nconst [seconds, file] = process.argv.slice(2);\nlet count = 0;\nconst tick = () => require(\"fs\").writeFileSync(file, String(++count));\ntick();\nconst every = setInterval(tick, 1000);\nsetTimeout(() => clearInterval(every), Number(seconds) * 1000);\n"
}

/// How many seconds the ticking command in the project's `folder` says it has run, by its
/// file `ticks`; none before it has written one.
pub fn ticks(folder: &Path, ticks: &str) -> Option<u64> {
    ticks_in(folder, ticks)
}

/// The tick files the case's own commands count in, by their literal names: the command's
/// and its background one's.
fn tick_files(command: &Command) -> Vec<&str> {
    command
        .ticks
        .as_deref()
        .into_iter()
        .chain(
            command
                .background
                .as_deref()
                .and_then(|b| b.ticks.as_deref()),
        )
        .collect()
}

/// Removes the case's own tick files when the case ends, however it ends.
struct OwnFiles<'a>(&'a Path, &'a Command);

impl Drop for OwnFiles<'_> {
    fn drop(&mut self) {
        clear_ticks(self.0, self.1);
    }
}

/// Takes the case's own tick files out of the project's `folder`, each by its name (no glob). Those it took out.
pub fn clear_ticks(folder: &Path, command: &Command) -> Vec<String> {
    tick_files(command)
        .into_iter()
        .filter(|name| {
            matches!(
                cf_e2e::door::Door::of_run().remove_in_made(&folder.join(name)),
                cf_e2e::door::Removed::Removed(_)
            )
        })
        .map(str::to_owned)
        .collect()
}

/// The name of a tick file that is this run's and this command's alone: `<what>-<pid>-<time>-<n>.txt`,
/// made at each call, so that no file of another run, or of another case of this run whose command
/// may still be writing (a 360-second background one), can have it. The name is in the command line
/// the member is given, which a process list shows, so a file of that name is the member's command's.
fn tick_file(what: &str) -> String {
    cf_e2e::door::own_name(what, "txt")
}

/// The command the member is asked to run, and what tells it in a process list, in the file
/// it writes, and on a screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// The command line, as the member is told to run it.
    text: String,
    /// What the line of its process holds, and nothing else of the machine's does.
    mark: String,
    /// What it prints as it goes, which no other thing on the screen says.
    replies: Option<&'static str>,
    /// The file in the project's folder it counts its seconds in, as it runs ([`TICKER`]).
    ticks: Option<String>,
    /// A command the member starts in the background first, and does not wait for (`midbg`).
    background: Option<Box<Command>>,
}

impl Command {
    /// The [`TICKER`] for `seconds`, counting them in the file `ticks`.
    pub fn ticking(seconds: u64, ticks: &str) -> Self {
        let mark = format!("{TICKER} {seconds} {ticks}");
        Self {
            text: format!("node ./{mark}"),
            mark,
            replies: None,
            ticks: Some(ticks.to_owned()),
            background: None,
        }
    }

    /// A command that says nothing, for `seconds` seconds: `sleep` where there is one, and
    /// a ping of the machine's own address (which says its lines to nobody who looks) on
    /// Windows, which has none. The stand-in agent runs it.
    pub fn quiet(seconds: u64, windows: bool) -> Self {
        let text = quiet_command(seconds, windows);
        let mark = text.strip_prefix("ping ").unwrap_or(&text).to_owned();
        Self {
            text,
            mark,
            replies: None,
            ticks: None,
            background: None,
        }
    }

    /// The command of `midtool`: the [`TICKER`] for [`SECONDS`], on every machine and in
    /// every harness's shell. Before T-1137 it was a ping of the machine's own address, which
    /// Codex starts with no shell, so that nothing the case read showed it running.
    pub fn here() -> Self {
        Self::ticking(SECONDS, &tick_file("tool-ticks"))
    }

    /// The field's command of T-1117 (a worker's database tests in WSL, run through
    /// `wsl.exe` from Claude's Git Bash): a quiet sleep of [`SECONDS`] in WSL's Ubuntu.
    pub fn wsl() -> Self {
        let mark = format!("-d Ubuntu -- sleep {SECONDS}");
        Self {
            text: format!("wsl {mark}"),
            mark,
            replies: None,
            ticks: None,
            background: None,
        }
    }

    /// [`Command::here`], after a [`TICKER`] started in the background that runs on longer
    /// than the watch, counting in a file of its own.
    pub fn after_background() -> Self {
        let seconds = 2 * STUCK_WATCH.as_secs() + 60;
        Self {
            background: Some(Box::new(Self::ticking(
                seconds,
                &tick_file("background-ticks"),
            ))),
            ..Self::here()
        }
    }

    /// What the member is told to do: run this, and nothing else.
    pub fn brief(&self) -> String {
        let foreground = format!(
            "Run exactly this one shell command, wait for it to end, then stop: {}",
            self.text
        );
        match self.background.as_deref() {
            None => foreground,
            Some(Command {
                text: background, ..
            }) => format!(
                "First start this shell command in the background, with your shell tool's own \
                 way of running a command in the background, and do not wait for it or look at \
                 it again: {background}\nThen, in the foreground: {foreground}"
            ),
        }
    }

    /// The command line.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether `line`, a process's command line as the host's tree gives it, runs the
    /// command: as it was told, or by its program's path as PowerShell starts it
    /// ([`runs_command`]).
    pub fn runs_in(&self, line: &str) -> bool {
        runs_command(line, &self.text)
    }
}

/// The command line of the process that the case said showed the command running in the
/// host's tree (`its process is in the host's tree (<pid> <line>); …`). None where it
/// said no such thing.
pub fn seen_in_tree(said: &str) -> Option<&str> {
    let (_, rest) = said.split_once(&format!("{IN_TREE} ("))?;
    let (process, _) = rest.split_once("); ")?;
    let (pid, line) = process.split_once(' ')?;
    pid.parse::<u64>().ok().map(|_| line)
}

/// What the host's tree says of the command, as a part of a line of the case: whether
/// it still has it, or what it has of the turn instead, or why it could not be read.
/// `listed` is whether the tree was seen to list the command before the pause: a process
/// that began before the turn's mark is none the host lists, and that the tree has not
/// the command is then no word of it.
pub fn tree_phrase(read: &Read, command: &Command, listed: bool) -> String {
    match read {
        Err(said) => format!("the host's tree could not be read: {said}"),
        Ok(tree) => {
            let held = tree.holding(&command.mark);
            if held.is_empty() {
                let others = if tree.total == 0 {
                    "and no other process of the turn".to_owned()
                } else {
                    format!("and {} of the turn: {}", tree.total, tree.line())
                };
                if listed {
                    format!("the host's tree no longer has the command ({others})")
                } else {
                    format!("the host's tree does not list the command, and did not before the pause, so it says nothing of it ({others})")
                }
            } else {
                let held: Vec<String> = held
                    .iter()
                    .take(3)
                    .map(|process| format!("{} {}", process.pid, cut(&process.command, 60)))
                    .collect();
                format!(
                    "the host's tree still has the command ({})",
                    held.join("; ")
                )
            }
        }
    }
}

/// Whether the host's tree was read and has not the command: it ended, or was ended.
/// A tree that could not be read says nothing of it.
pub fn is_gone(read: &Read, command: &Command) -> bool {
    read.as_ref()
        .is_ok_and(|tree| tree.holding(&command.mark).is_empty())
}

/// The command's fate, as the line that closes the report of it: what the host's tree
/// said at the end of the watch (`ended`), and when it was first read without the command
/// (`gone`), if it was, where the tree was seen to list it (`listed`); times in
/// milliseconds since the epoch, `made` the pause's.
pub fn fate_line(
    at_the_end: &str,
    gone: Option<i64>,
    listed: bool,
    made: i64,
    ended: i64,
) -> String {
    let when = match (listed, gone) {
        (false, _) => "the host's tree never listed it, so when it ended is not said".to_owned(),
        (true, None) => "it was never read gone".to_owned(),
        (true, Some(at)) => format!("it was first read gone at {}", plus(at, made)),
    };
    format!(
        "the command, at the end of the watch ({}): {at_the_end}; {when}",
        plus(ended, made)
    )
}

/// What shows the command running, in the order the case trusts it: its process in the
/// host's tree, the seconds its file counts (`ticked`, asked only if the tree does not show
/// it), its replies on the screen (`on_screen`), its call on the wire (`in_wire`, asked
/// last). None while nothing does.
fn started(
    tree: &Read,
    command: &Command,
    ticked: impl FnOnce() -> Option<u64>,
    on_screen: impl FnOnce() -> bool,
    in_wire: impl FnOnce() -> bool,
) -> Option<String> {
    let process = tree
        .as_ref()
        .ok()
        .and_then(|tree| tree.holding(&command.mark).into_iter().next());
    if let Some(process) = process {
        return Some(format!(
            "{IN_TREE} ({} {})",
            process.pid,
            cut(&process.command, 60)
        ));
    }
    if let Some(seconds) = ticked() {
        return Some(format!(
            "its file says it has run {seconds} s, which the host's tree does not list"
        ));
    }
    if on_screen() {
        return Some("its replies are on its screen".to_owned());
    }
    in_wire().then(|| "the wire has its call".to_owned())
}

/// The reading of a member at work: whether its command runs ([`started`]), and what
/// showed it. `devin` has the wire read too.
pub fn running(
    command: &Command,
    devin: bool,
) -> impl FnMut(&Members, &MemberWindow, Duration) -> Option<String> + '_ {
    let mut moving = Moving::default();
    move |members, window, _worked| {
        let tree = tree::read(members.rig(), &window.pane);
        started(
            &tree,
            command,
            // Its file counts only where it went up since the look before: the file of an
            // earlier run does not.
            || {
                moving.moved(
                    command
                        .ticks
                        .as_deref()
                        .and_then(|file| ticks(members.folder(), file)),
                )
            },
            || {
                command.replies.is_some_and(|replies| {
                    members.rig().screen(window.id()).contains(replies)
                        || screen_drawn(members, window)
                            .iter()
                            .any(|line| line.contains(replies))
                })
            },
            || {
                devin
                    && Wire::open(&members.rig().home())
                        .is_some_and(|wire| wire.called(&command.mark))
            },
        )
    }
}

/// How the stop of the task paused while the command runs is held to account: watched
/// for [`STUCK_WATCH`] at most (the command is a minute long, and the turn of a window that
/// waits for it ends with it, a good while after the pause), whatever the board says of
/// it, and the command's fate is said at each look. `listed` is whether the host's tree
/// was seen to have the command before the pause.
pub fn hold(command: &Command, listed: bool) -> Hold<'_> {
    Hold {
        stop: STUCK_WATCH,
        after_ignored: None,
        command: Some(command),
        listed,
    }
}

/// The case, on the harness the member `ctx.agent` runs on.
pub fn run(ctx: &Ctx, case: &mut Account, harness: Harness, command: Command) -> Result<()> {
    let devin = harness == Harness::Devin;
    if devin {
        // What the daemon's reader makes of Devin's store is asked of the product's own
        // code, built before anything is spent.
        ask::prepare()?;
    }
    // The case's own tick files, by their names (this run's and this command's alone), go when
    // it ends; what an earlier run left under other names is left where it is.
    let _own_files = OwnFiles(ctx.members.folder(), &command);
    let Some(begun) = begin(
        ctx,
        case,
        Some(harness),
        &command.brief(),
        WORK,
        DOING,
        running(&command, devin),
    )?
    else {
        return Ok(());
    };
    // Where the host's tree was not what showed the command running, it may not list it
    // at all, and what it says of it after the stop says nothing.
    let listed = begun.seen.starts_with(IN_TREE);
    let folder = ctx.members.folder();
    let counted = |command: &Command| {
        command
            .ticks
            .as_deref()
            .and_then(|file| ticks(folder, file))
    };
    let background = command.background.as_deref();
    let pane = &begun.window.pane;
    let before = background.map(|background| {
        (
            holding(&tree::read(ctx.members.rig(), pane), &background.mark),
            counted(background),
        )
    });
    let at_the_pause = counted(&command);
    // The background command counts as left running only if it was running at the pause:
    // its count went up once more in the moments before it. A file that stood still then
    // (a command that ended, or an old file) says nothing of what the pause did to it.
    let before = before.map(|(listed, counted_before)| {
        let alive = background.and_then(|background| {
            let mut moving = Moving::default();
            let mut last = None;
            for _ in 0..8 {
                last = moving.moved(counted(background));
                if last.is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            last
        });
        (listed, counted_before, alive)
    });
    stop_it(
        ctx,
        case,
        begun.number,
        &begun.window,
        Some(harness),
        hold(&command, listed),
    )?;
    if command.ticks.is_some() {
        // Whether the stop ended the command, by its own count: a harness may end it, or let
        // go of it (Codex keeps it as a background terminal).
        case.say(format!(
            "the command's file: {} at the pause, {} at the end of the watch",
            seconds_said(at_the_pause),
            seconds_said(counted(&command))
        ));
    }
    if let (Some(background), Some((listed, counted_before, alive))) = (background, before) {
        let after = holding(&tree::read(ctx.members.rig(), pane), &background.mark);
        let counted_after = counted(background);
        // Its own count says it ran on whether or not the host's tree lists it: a harness
        // that starts a program with no shell (Codex) leaves it unlisted (T-1137). One that never
        // wrote its file before the pause never ran, or ended at once (a job a shell started
        // with `&` in a terminal that closed with it): there is nothing the pause could leave.
        let said_of_it = format!(
            "its file: {} at the pause, {} after the watch; the host's tree before the pause: {}; after the watch: {}",
            seconds_said(counted_before),
            seconds_said(counted_after),
            said(&listed),
            said(&after)
        );
        if let Some(why) = background_unmet(counted_before, alive) {
            // No job was running at the pause to leave running or not: what the case is for
            // was not tried, and it is said so (`unmet`), never `ok`.
            case.unmet(format!("{why}: {said_of_it}"));
        } else {
            case.check(
                "the pause left the work started in the background running",
                counted_after > counted_before,
                said_of_it,
            );
        }
    }
    Ok(())
}

/// Why the background command is no precondition of the case, where it is none: it never wrote
/// its file (`before`), or its count did not go up in the moments before the pause (`alive`): a
/// command that ended, or a file nobody writes. None where it was counting at the pause.
fn background_unmet(before: Option<u64>, alive: Option<u64>) -> Option<&'static str> {
    if before.is_none() {
        Some("the command started in the background never ran, or ended before it began, so there was no job at the pause to leave running")
    } else if alive.is_none() {
        Some("the command started in the background was not counting just before the pause (its file stood still), so there was no job at the pause to leave running")
    } else {
        None
    }
}

/// What a count of seconds says: so many, or that its file was never written.
fn seconds_said(seconds: Option<u64>) -> String {
    seconds.map_or_else(|| "no file".to_owned(), |seconds| format!("{seconds} s"))
}

/// The processes of `read` whose line holds `mark`, each its number and line; none where it
/// could not be read.
fn holding(read: &Read, mark: &str) -> Vec<String> {
    read.as_ref().map_or_else(
        |_| Vec::new(),
        |tree| {
            tree.holding(mark)
                .into_iter()
                .map(|process| format!("{} {}", process.pid, cut(&process.command, 80)))
                .collect()
        },
    )
}

/// What [`holding`] found, as a line says it.
fn said(found: &[String]) -> String {
    if found.is_empty() {
        "the host's tree does not have it".to_owned()
    } else {
        format!("the host's tree has it ({})", found.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use cf_e2e::live::tree::Tree;
    use serde_json::json;

    use super::*;

    fn tree(processes: &[(u64, &str)]) -> Read {
        Ok(Tree::of(&json!({
            "ok": true,
            "total": processes.len(),
            "processes": processes
                .iter()
                .map(|(pid, command)| json!({ "pid": pid, "command": command }))
                .collect::<Vec<_>>(),
        })))
    }

    #[test]
    fn the_command_is_the_ticker_which_every_shell_starts_and_whose_file_counts_its_seconds() {
        let here = Command::here();
        // Its file's name is this run's and this command's own (`tick_file`).
        let file = here.ticks.clone().unwrap();
        assert!(
            file.starts_with("tool-ticks-") && file.ends_with(".txt"),
            "{file}"
        );
        assert_eq!(here, Command::ticking(SECONDS, &file));
        let line = format!("tick-cf-stops-live.cjs 60 {file}");
        assert_eq!(here.text(), format!("node ./{line}"));
        assert_eq!(here.mark, line);
        assert_eq!(
            here.brief(),
            format!(
                "Run exactly this one shell command, wait for it to end, then stop: node ./{line}"
            )
        );
        // As the Mac and PowerShell list it.
        assert!(here.runs_in(&format!("node ./{line}")));
        assert!(here.runs_in(&format!(r#""C:\Program Files\nodejs\node.exe" ./{line}"#)));
        assert!(ticker_text().contains("writeFileSync(file, String(++count))"));
        // Its background one counts in a file of its own, for longer than the watch.
        let after = Command::after_background();
        let background = after.background.as_deref().unwrap();
        assert!(background
            .ticks
            .as_deref()
            .unwrap()
            .starts_with("background-ticks-"));
        assert!(background
            .text
            .starts_with("node ./tick-cf-stops-live.cjs "));
        assert!(
            after.brief().contains("in the background"),
            "{}",
            after.brief()
        );
        let foreground = Command::ticking(SECONDS, after.ticks.as_deref().unwrap());
        assert!(
            after.brief().ends_with(&foreground.brief()),
            "{}",
            after.brief()
        );
    }

    #[test]
    fn a_background_command_that_was_not_counting_at_the_pause_is_an_unmet_precondition_never_an_ok(
    ) {
        assert!(background_unmet(None, None).unwrap().contains("never ran"));
        assert!(background_unmet(Some(5), None)
            .unwrap()
            .contains("not counting just before"));
        // Counting before the pause: judged by its count after it, as before.
        assert_eq!(background_unmet(Some(5), Some(6)), None);
        // And an unmet case holds (no failure) and is said unmet, whatever else held.
        let mut case = Account::new();
        case.check("the pause left the foreground", true, "x");
        case.unmet(format!("{}: ...", background_unmet(Some(5), None).unwrap()));
        assert!(case.held() && case.is_unmet());
    }

    #[test]
    fn a_stale_tick_file_is_no_running_command() {
        let command = Command::here();
        let unread = tree(&[(7, "bash")]);
        let boom = || -> bool { panic!("not asked") };
        let nothing = || false;
        // A file of an earlier run: 60 s, and it stands still at every look since the brief.
        let mut moving = Moving::default();
        for _ in 0..3 {
            let said = started(
                &unread,
                &command,
                || moving.moved(Some(60)),
                nothing,
                nothing,
            );
            assert_eq!(
                said, None,
                "a counter that has not moved is no command running"
            );
        }
        // One that counts: the first look has nothing to compare, the next sees it go up.
        let mut moving = Moving::default();
        assert_eq!(moving.moved(None), None, "no file yet");
        assert_eq!(moving.moved(Some(1)), None, "the first look");
        assert_eq!(
            moving.moved(Some(1)),
            None,
            "half a second on, the same second"
        );
        assert_eq!(moving.moved(Some(2)), Some(2));
        let said = started(&unread, &command, || Some(2), boom, boom);
        assert!(said.unwrap().starts_with("its file says it has run 2 s"));
        // A count that goes back (a file written again from 1) is a new run's, and says so
        // only once it climbs.
        let mut moving = Moving::default();
        assert_eq!(moving.moved(Some(40)), None);
        assert_eq!(moving.moved(Some(1)), None);
        assert_eq!(moving.moved(Some(2)), Some(2));
    }

    #[test]
    fn a_file_under_another_runs_name_that_keeps_growing_is_no_running_command_of_this_run() {
        let folder = tempfile::tempdir().unwrap();
        let this = Command::here();
        // Another run's command (an earlier one, still writing its file each second): its file
        // has the name of its own run, so this run's command never reads it.
        let earlier = Command::here();
        assert_ne!(this.ticks, earlier.ticks, "a name of this run alone");
        assert_ne!(this.text(), earlier.text());
        let theirs = earlier.ticks.as_deref().unwrap();
        let mut moving = Moving::default();
        for count in 1..=4 {
            std::fs::write(folder.path().join(theirs), count.to_string()).unwrap();
            let seen = this
                .ticks
                .as_deref()
                .and_then(|file| ticks(folder.path(), file));
            assert_eq!(moving.moved(seen), None, "look {count}");
        }
        // The old fixed names are no part of it either.
        std::fs::write(folder.path().join("tool-ticks.txt"), "9").unwrap();
        assert_eq!(ticks(folder.path(), this.ticks.as_deref().unwrap()), None);
        let background = Command::after_background();
        let file = background
            .background
            .as_deref()
            .unwrap()
            .ticks
            .clone()
            .unwrap();
        assert_ne!(file, "background-ticks.txt");
        assert!(file.contains("background-ticks-"), "{file}");
    }

    #[test]
    fn the_case_removes_its_own_tick_files_by_their_names_and_no_other_file() {
        let folder = tempfile::tempdir().unwrap();
        let command = Command::after_background();
        let own = [
            command.ticks.clone().unwrap(),
            command
                .background
                .as_deref()
                .unwrap()
                .ticks
                .clone()
                .unwrap(),
        ];
        // Its own, files of an earlier run under the old fixed names and under another run's,
        // and another file: only its own go.
        let others = [
            "tool-ticks.txt",
            "background-ticks.txt",
            "tool-ticks-1-2-3.txt",
            "notes.txt",
        ];
        for name in own.iter().map(String::as_str).chain(others) {
            std::fs::write(folder.path().join(name), "60").unwrap();
        }
        let cleared = clear_ticks(folder.path(), &command);
        assert_eq!(cleared, own);
        for name in &own {
            assert!(!folder.path().join(name).exists(), "{name}");
        }
        for name in others {
            assert!(folder.path().join(name).exists(), "{name}");
        }
        // Where there are none, there is nothing to say.
        assert!(clear_ticks(folder.path(), &Command::here()).is_empty());
    }

    #[test]
    fn a_tick_file_says_how_many_seconds_its_command_ran_and_none_before_it_is_written() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(ticks(folder.path(), "tool-ticks.txt"), None);
        std::fs::write(folder.path().join("tool-ticks.txt"), "7").unwrap();
        assert_eq!(ticks(folder.path(), "tool-ticks.txt"), Some(7));
        std::fs::write(folder.path().join("tool-ticks.txt"), "").unwrap();
        assert_eq!(
            ticks(folder.path(), "tool-ticks.txt"),
            None,
            "a file being written"
        );
    }

    #[test]
    fn the_fields_command_is_a_quiet_sleep_in_wsl_found_in_a_tree_by_wsls_path() {
        let wsl = Command::wsl();
        assert_eq!(wsl.text(), "wsl -d Ubuntu -- sleep 60");
        assert!(wsl.runs_in(r#""C:\WINDOWS\system32\wsl.exe" -d Ubuntu -- sleep 60"#));
        assert!(!wsl.runs_in("sleep 60"));
        assert_eq!(wsl.replies, None);
    }

    #[test]
    fn a_quiet_command_is_a_sleep_where_there_is_one_and_a_ping_that_is_not_looked_at_elsewhere() {
        let sleep = Command::quiet(119, false);
        assert_eq!(
            (sleep.text(), sleep.mark.as_str()),
            ("sleep 119", "sleep 119")
        );
        assert_eq!(sleep.replies, None);
        let ping = Command::quiet(119, true);
        assert_eq!(ping.text(), "ping -n 120 127.0.0.1");
        assert_eq!(ping.mark, "-n 120 127.0.0.1");
        assert_eq!(ping.replies, None);
    }

    #[test]
    fn the_command_is_told_running_by_the_hosts_tree_then_its_file_then_the_screen_then_the_wire() {
        let command = Command::here();
        let nothing = || false;
        let none = || None;
        let boom = || -> bool { panic!("not asked") };
        let unasked = || -> Option<u64> { panic!("not asked") };
        // Its process, among the turn's others, which the host lists.
        let line = format!(
            "node ./tick-cf-stops-live.cjs 60 {}",
            command.ticks.as_deref().unwrap()
        );
        let bash = format!("bash -c {line}");
        let shown = tree(&[(4101, bash.as_str()), (4102, line.as_str())]);
        assert_eq!(
            started(&shown, &command, unasked, boom, boom),
            Some(format!(
                "its process is in the host's tree (4101 {})",
                cut(&bash, 60)
            ))
        );
        // None in the tree (Codex starts it with no shell): its file; then its replies on the
        // screen; or its call on the wire.
        let other = tree(&[(7, "bash")]);
        assert_eq!(
            started(&other, &command, || Some(3), boom, boom).as_deref(),
            Some("its file says it has run 3 s, which the host's tree does not list")
        );
        assert_eq!(
            started(&other, &command, none, || true, boom).as_deref(),
            Some("its replies are on its screen")
        );
        assert_eq!(
            started(&other, &command, none, nothing, || true).as_deref(),
            Some("the wire has its call")
        );
        // Nothing yet, and a tree the host could not tell: the file, the screen and the wire go on.
        assert_eq!(started(&other, &command, none, nothing, nothing), None);
        let unread: Read = Err("(could not be asked: timed out)".to_owned());
        assert_eq!(started(&unread, &command, none, nothing, nothing), None);
        assert_eq!(
            started(&unread, &command, || Some(1), boom, boom).as_deref(),
            Some("its file says it has run 1 s, which the host's tree does not list")
        );
    }

    #[test]
    fn the_process_the_case_saw_in_the_tree_runs_the_command_on_a_mac_and_on_windows() {
        // On a Mac the stand-in's shell and its `sleep`: either line is the command's.
        let sleep = Command::quiet(119, false);
        let mac = "T-1: the member is running its command: its process is in the host's tree (4102 sleep 119); its activity: working";
        assert_eq!(seen_in_tree(mac), Some("sleep 119"));
        assert!(sleep.runs_in("sleep 119"));
        assert!(sleep.runs_in("/bin/sh -c \"$0\" \"$@\" sleep 119"));
        // On zeewin PowerShell started the ping by its path, which the tree shows.
        let ping = Command::quiet(119, true);
        let zeewin = r#"T-1: the member is running its command: its process is in the host's tree (5864 "C:\WINDOWS\System32\PING.EXE" -n 120 127.0.0.1); its activity: working"#;
        let line = seen_in_tree(zeewin).unwrap();
        assert_eq!(line, r#""C:\WINDOWS\System32\PING.EXE" -n 120 127.0.0.1"#);
        assert!(ping.runs_in(line));
        // Another command, or another program with its words, is not it.
        assert!(!ping.runs_in(r#""C:\WINDOWS\System32\PING.EXE" -n 61 127.0.0.1"#));
        assert!(!ping.runs_in("tracert -n 120 127.0.0.1"));
        assert!(!sleep.runs_in("sleep 61"));
        // Nor is what the old ` sleep 119)` refused: a longer name, a longer word, more words.
        assert!(!sleep.runs_in("nosleep 119"));
        assert!(!sleep.runs_in("sleep 1190"));
        assert!(!sleep.runs_in("sleep 119 30"));
        assert!(!ping.runs_in(r#""C:\WINDOWS\System32\PING.EXE" -n 120 127.0.0.10"#));
        // A case that saw it some other way names no process.
        assert_eq!(
            seen_in_tree("T-1: its replies are on its screen; its activity: working"),
            None
        );
    }

    #[test]
    fn the_tree_says_whether_it_still_has_the_command_and_what_else_it_has_of_the_turn() {
        let command = Command::quiet(119, false);
        assert_eq!(
            tree_phrase(&tree(&[(4102, "sleep 119")]), &command, true),
            "the host's tree still has the command (4102 sleep 119)"
        );
        assert_eq!(
            tree_phrase(&tree(&[]), &command, true),
            "the host's tree no longer has the command (and no other process of the turn)"
        );
        assert_eq!(
            tree_phrase(&tree(&[(7, "bash")]), &command, true),
            "the host's tree no longer has the command (and 1 of the turn: 7 bash (1))"
        );
        assert_eq!(
            tree_phrase(&Err("(the host said: no)".to_owned()), &command, true),
            "the host's tree could not be read: (the host said: no)"
        );
    }

    #[test]
    fn a_tree_that_never_listed_the_command_says_nothing_of_it_where_it_has_none() {
        let command = Command::quiet(119, false);
        // A process that began before the turn's mark is none the host lists: that the
        // tree has not the command is no word of it.
        assert_eq!(
            tree_phrase(&tree(&[]), &command, false),
            "the host's tree does not list the command, and did not before the pause, so it says nothing of it (and no other process of the turn)"
        );
        // Where it lists it, it is there whatever was seen before.
        assert_eq!(
            tree_phrase(&tree(&[(4102, "sleep 119")]), &command, false),
            "the host's tree still has the command (4102 sleep 119)"
        );
    }

    #[test]
    fn the_command_is_gone_when_the_tree_was_read_without_it_and_a_tree_not_read_says_nothing() {
        let command = Command::quiet(119, false);
        assert!(!is_gone(&tree(&[(4102, "sleep 119")]), &command));
        assert!(is_gone(&tree(&[]), &command));
        assert!(is_gone(&tree(&[(7, "bash")]), &command));
        assert!(!is_gone(&Err("(the host said: no)".to_owned()), &command));
    }

    #[test]
    fn the_fate_of_the_command_is_said_with_when_it_was_first_read_gone_if_it_was() {
        assert_eq!(
            fate_line(
                "the host's tree still has the command (4102 sleep 119)",
                None,
                true,
                1_000,
                91_000
            ),
            "the command, at the end of the watch (+90.0 s): the host's tree still has the command (4102 sleep 119); it was never read gone"
        );
        assert_eq!(
            fate_line(
                "the host's tree no longer has the command (and no other process of the turn)",
                Some(56_300),
                true,
                1_000,
                73_000
            ),
            "the command, at the end of the watch (+72.0 s): the host's tree no longer has the command (and no other process of the turn); it was first read gone at +55.3 s"
        );
        // A tree that never listed it cannot say when it ended.
        assert_eq!(
            fate_line("the host's tree does not list the command", Some(1_000), false, 1_000, 13_000),
            "the command, at the end of the watch (+12.0 s): the host's tree does not list the command; the host's tree never listed it, so when it ended is not said"
        );
    }

    #[test]
    fn the_stop_is_watched_for_longer_than_the_command_runs_and_its_fate_is_said() {
        let command = Command::here();
        let held = hold(&command, true);
        assert!(held.listed && !hold(&command, false).listed);
        assert_eq!(held.stop, STUCK_WATCH);
        assert!(held.stop > Duration::from_secs(SECONDS));
        // The board's word that the window did not stop does not end the watch: when the
        // turn ends, with the command or without, is what is looked for.
        assert_eq!(held.after_ignored, None);
        assert_eq!(held.command, Some(&command));
    }
}

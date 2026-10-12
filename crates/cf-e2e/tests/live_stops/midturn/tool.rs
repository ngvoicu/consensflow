//! `midtool`: a window in the middle of a shell command, on any harness. The member is
//! asked to run one long command, a ping of the machine's own address (a reply a second
//! for a minute, with the platform's own flags; a `sleep` where the machine has no `ping`),
//! and the case waits until the command runs and pauses the task. It was written for the
//! Devin window of T-5 (2026-10-12, 3.0.1; Devin 3000.10.21 on Windows), which was running
//! a shell command (headless Edge checks) when the pause came and went on for about 70 s
//! through three rounds of Escape, where a Devin of 3000.11.3 stops in the middle of its
//! answer (`midturn`): a turn that is at a tool may wait for it, whatever the keys do.
//!
//! What shows the command running is the host's process tree (`pane.background`: what the
//! window's tree started after its turn began, on every platform, with no `ps`), its
//! replies on the screen, or, for Devin, its call on the wire. The case holds when the
//! daemon read the window at rest within the rounds and paid the stop there, as `midturn`
//! does ([`super::judge`]). The command is no check but a thing to know, said at each look
//! after a round and at the end: whether the host's tree still has it once the stop is
//! paid (a harness that cancels a turn may or may not end the tool it ran), and when it was
//! first read gone. The stop is watched until the window has been at rest as long as a
//! window at rest is watched, or for as long as a window that may not stop is, which is
//! longer than the command runs: when a turn that waits for its tool ends is told whichever
//! way it goes.

use std::time::Duration;

use cf_e2e::live::ask;
use cf_e2e::live::members::{plus, Account, MemberWindow, Members};
use cf_e2e::live::tree::{self, Read};
use cf_e2e::live::Harness;
use cf_e2e::live::{command, cut};
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
};

/// What the case says where the command was seen by its process in the host's tree.
const IN_TREE: &str = "its process is in the host's tree";

/// How many seconds the command runs for.
pub const SECONDS: u64 = 60;

/// The command the member is asked to run, and what tells it in a process list and on a
/// screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// The command line, as the member is told to run it.
    text: String,
    /// What the line of its process holds, and nothing else of the machine's does.
    mark: String,
    /// What it prints as it goes, which no other thing on the screen says.
    replies: Option<&'static str>,
}

impl Command {
    /// A ping of the machine's own address, [`SECONDS`] seconds and one reply a second:
    /// the platform's own `ping` (`-n` where `windows`, `-c` elsewhere), which Windows and
    /// the others have, and which is a program of its own in the tree, with the line it
    /// was started with.
    pub fn ping(windows: bool) -> Self {
        let flag = if windows { "-n" } else { "-c" };
        let mark = format!("{flag} {} 127.0.0.1", SECONDS + 1);
        Self {
            text: format!("ping {mark}"),
            mark,
            replies: Some("from 127.0.0.1"),
        }
    }

    /// A command that says nothing, for `seconds` seconds: `sleep` where there is one, and
    /// a ping of the machine's own address (which says its lines to nobody who looks) on
    /// Windows, which has none. The stand-in agent runs it.
    pub fn quiet(seconds: u64, windows: bool) -> Self {
        if windows {
            let mark = format!("-n {} 127.0.0.1", seconds + 1);
            Self {
                text: format!("ping {mark}"),
                mark,
                replies: None,
            }
        } else {
            let text = format!("sleep {seconds}");
            Self {
                mark: text.clone(),
                text,
                replies: None,
            }
        }
    }

    /// The command for the machine this runs on: a ping of its own address where it has
    /// a `ping` to run (Windows has, and elsewhere the program is looked for on the `PATH`
    /// of the run), and else a `sleep` of the same time, which says nothing as it goes.
    pub fn here() -> Self {
        let windows = cfg!(windows);
        if windows || command("ping").is_ok() {
            Self::ping(windows)
        } else {
            Self::quiet(SECONDS, false)
        }
    }

    /// What the member is told to do: run this, and nothing else.
    pub fn brief(&self) -> String {
        format!(
            "Run exactly this one shell command, wait for it to end, then stop: {}",
            self.text
        )
    }

    /// The command line.
    pub fn text(&self) -> &str {
        &self.text
    }
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
/// host's tree, its replies on the screen (`on_screen`, asked only if the tree does not
/// show it), its call on the wire (`in_wire`, asked last). None while nothing does.
fn started(
    tree: &Read,
    command: &Command,
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
    move |members, window, _worked| {
        let tree = tree::read(members.rig(), &window.pane);
        started(
            &tree,
            command,
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
pub fn run(ctx: &Ctx, case: &mut Account, harness: Harness) -> Result<()> {
    let devin = harness == Harness::Devin;
    if devin {
        // What the daemon's reader makes of Devin's store is asked of the product's own
        // code, built before anything is spent.
        ask::prepare()?;
    }
    let command = Command::here();
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
    stop_it(
        ctx,
        case,
        begun.number,
        &begun.window,
        Some(harness),
        hold(&command, listed),
    )
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
    fn the_command_is_a_ping_of_the_machines_own_address_in_the_platforms_own_flags() {
        let unix = Command::ping(false);
        assert_eq!(unix.text(), "ping -c 61 127.0.0.1");
        assert_eq!(unix.mark, "-c 61 127.0.0.1");
        let windows = Command::ping(true);
        assert_eq!(windows.text(), "ping -n 61 127.0.0.1");
        assert_eq!(windows.mark, "-n 61 127.0.0.1");
        // Both say a reply a second that nothing else on a screen says.
        assert_eq!(unix.replies, windows.replies);
        assert_eq!(
            windows.brief(),
            "Run exactly this one shell command, wait for it to end, then stop: ping -n 61 127.0.0.1"
        );
    }

    #[test]
    fn the_command_of_this_machine_is_its_own_ping_or_a_sleep_of_the_same_time() {
        let here = Command::here();
        let ping = Command::ping(cfg!(windows));
        let sleep = Command::quiet(SECONDS, false);
        assert!(here == ping || here == sleep, "{here:?}");
        // Windows has a ping, whatever its PATH.
        if cfg!(windows) {
            assert_eq!(here, ping);
        }
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
    fn the_command_is_told_running_by_the_hosts_tree_then_the_screen_then_the_wire() {
        let command = Command::ping(false);
        let nothing = || false;
        let boom = || -> bool { panic!("not asked") };
        // Its process, among the turn's others, which the host lists.
        let shown = tree(&[
            (4101, "bash -c ping -c 61 127.0.0.1"),
            (4102, "ping -c 61 127.0.0.1"),
        ]);
        assert_eq!(
            started(&shown, &command, boom, boom).as_deref(),
            Some("its process is in the host's tree (4101 bash -c ping -c 61 127.0.0.1)")
        );
        // None in the tree: its replies on the screen; or its call on the wire.
        let other = tree(&[(7, "bash")]);
        assert_eq!(
            started(&other, &command, || true, boom).as_deref(),
            Some("its replies are on its screen")
        );
        assert_eq!(
            started(&other, &command, nothing, || true).as_deref(),
            Some("the wire has its call")
        );
        // Nothing yet, and a tree the host could not tell: the screen and the wire go on.
        assert_eq!(started(&other, &command, nothing, nothing), None);
        let unread: Read = Err("(could not be asked: timed out)".to_owned());
        assert_eq!(started(&unread, &command, nothing, nothing), None);
        assert_eq!(
            started(&unread, &command, || true, boom).as_deref(),
            Some("its replies are on its screen")
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
        let command = Command::ping(false);
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

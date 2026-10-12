//! `midturn` and `midtool`: a window in the middle of a turn, on any harness, which a
//! pause lands in, and the daemon's rounds of its harness's interrupt (Escape, as many
//! presses as its adapter says, three rounds three seconds apart) must end within them.
//! The two differ in what the turn is in:
//!
//! - `midturn` (this file): the member is asked to write the whole numbers from 1 to
//!   [`NUMBERS`] as its reply, a line each, so that the model is still writing when the
//!   task is paused. It was written for a Devin window that did not stop for a pause on
//!   Windows (T-5 of 2026-10-12, 3.0.1: three rounds of Escape, and the turn ended by
//!   itself 70 s later), which stopped in the middle of its answer when it was tried.
//! - `midtool` ([`tool`]): the member is asked to run one long shell command, and is
//!   paused while it runs: T-5's window was running a command when the pause came.
//!
//! [`begin`] gets the member to the point to pause it at (what the case looks for there
//! is its own: [`numbers`], [`tool`]); [`stop_it`] pauses it and watches. The case holds
//! when the daemon read the window at rest within the rounds, said nothing of an ignored
//! stop, and pressed nothing more once the window stopped ([`judge`]). It says, whether
//! it holds or not, for each round: the Escapes pressed and the gaps between them, what
//! the lane read a moment after the last, what the window printed since the pause
//! ([`printed`]: a hint of its own such as "esc again to interrupt" says the first press
//! was taken and the second was not), its screen as its pane host draws it (what a person
//! sees, whatever way a console host repaints it), what the host's process tree says of
//! the command where there is one, and, for Devin, what its wire log says since the pause
//! ([`wire`]: `method:session/cancel` is the TUI asking its agent to cancel,
//! `cause:cancelled` the agent's turn ended by it, and a wire that goes on with message
//! chunks is a turn that went on). The rounds are told from the presses ([`rounds`]).
//!
//! The case that never gets to pause says why, and does not wait for what cannot come:
//! a member whose window never shows its first message has its task failed by the
//! daemon, and the wait ends there ([`unstarted`]).

#[path = "midturn/begin.rs"]
mod begin;
#[path = "midturn/judge.rs"]
mod judge;
#[path = "midturn/numbers.rs"]
mod numbers;
#[path = "midturn/printed.rs"]
mod printed;
#[path = "midturn/rounds.rs"]
mod rounds;
#[cfg(test)]
#[path = "midturn/stand_in.rs"]
mod stand_in;
#[path = "midturn/tool.rs"]
pub mod tool;
#[path = "midturn/unstarted.rs"]
mod unstarted;
#[path = "midturn/wire.rs"]
mod wire;

use std::thread::sleep;
use std::time::Duration;

use cf_e2e::live::ask;
use cf_e2e::live::members::{plus, squash, Account, MemberWindow, Members};
use cf_e2e::live::Harness;
use cf_e2e::Result;
use serde_json::Value;

use crate::stop::{rested_at, said_pause};
use crate::stuck::{at_rest, word_of};
use crate::tree;
use crate::watch::{say_events, settlement_of, told_the_human, told_the_requester, unstopped};
use crate::{now_ms, Ctx, STOP, WORK};
use begin::begin;
use judge::{judge, show, Look, Seen, Watched};
use printed::{printed_since, tail_lines};
use rounds::due;
use tool::Command;
use wire::Wire;

/// How many numbers the member is asked to write: a turn of a minute or more, which
/// a pause lands in the middle of.
pub const NUMBERS: usize = 5_000;

/// What the folder of the case tells every window: the bench's rule of doing what
/// each message asks, without its one line of reply, which this case's is not.
pub const FOLDER_BRIEF: &str = "This folder is an automated ConsensFlow test. Do exactly what each message asks and nothing more. Run no tools unless a message tells you to.\n";

/// How many lines of its screen the pane host is asked for: the most it gives. And how
/// many of the last of them, and how much of each, a look at the window says.
const SCREEN: usize = 12;
const DRAWN: usize = 5;
const DRAWN_WIDTH: usize = 100;

/// How often the lane is looked at while the stop is watched.
const LOOK: Duration = Duration::from_millis(300);

/// How long the case goes on looking once the board says the window did not stop,
/// for the notes that say so to land.
const AFTER_IGNORED_MS: i64 = 4_000;

/// How the stop of the paused task is held to account: how long it is watched at most,
/// whether the board's word that the window did not stop ends the watch (a little
/// after, for the notes that say so to land) or the watch goes on until the window
/// rests or the time is up, and the command the member runs, which the looks say the
/// fate of (`listed`: whether the host's tree was seen to have it before the pause,
/// without which that it has not says nothing).
#[derive(Debug, Clone, Copy)]
pub struct Hold<'a> {
    pub stop: Duration,
    pub after_ignored: Option<i64>,
    pub command: Option<&'a Command>,
    pub listed: bool,
}

impl Hold<'static> {
    /// A member writing numbers: [`STOP`] is the most, and the watch ends soon after the
    /// board says the window did not stop.
    pub const NUMBERS: Self = Self {
        stop: STOP,
        after_ignored: Some(AFTER_IGNORED_MS),
        command: None,
        listed: false,
    };
}

/// Whether the watch of the stop is over at `now`: the time is up (`end`), or the lane
/// has been at rest since `at_rest` for as long as the window is watched at rest
/// (`rest`), or the board has said since `ignored_at` that the window did not stop for
/// as long as the notes that say so take to land (`after_ignored`, where it is a reason
/// to end the watch at all).
fn over(
    now: i64,
    end: i64,
    at_rest: Option<i64>,
    ignored_at: Option<i64>,
    (rest, after_ignored): (i64, Option<i64>),
) -> bool {
    now >= end
        || at_rest.is_some_and(|since| now - since >= rest)
        || ignored_at
            .zip(after_ignored)
            .is_some_and(|(since, after)| now - since >= after)
}

/// What the member is asked to do: to write the numbers itself (a harness that runs
/// a script for them has a turn of a moment), all of them.
pub fn brief() -> String {
    format!(
        "Write the whole numbers from 1 to {NUMBERS} yourself, as your reply, one on each line and nothing else: no commentary, no summary, no shortening, and run no tools."
    )
}

/// The last lines of the screen of `window` as its pane host draws it, none where the
/// host says nothing of it. The text of the screen the rig keeps is all that was printed,
/// and a console host repaints in place; the host's own screen is the one a person sees.
fn screen_drawn(members: &Members, window: &MemberWindow) -> Vec<String> {
    members
        .rig()
        .snapshot(&window.pane, SCREEN)
        .map(|snapshot| tail_lines(&snapshot))
        .unwrap_or_default()
}

/// The window, and what the looks of the stop are taken against.
struct Marks<'a> {
    members: &'a Members,
    window: &'a MemberWindow,
    /// What the window's screen held at the pause, in characters.
    screen: usize,
    /// Devin's wire log, for a window that has one.
    wire: Option<&'a Wire>,
    /// The command the member runs, whose fate in the host's tree each look says, and
    /// whether the tree was seen to list it before the pause.
    command: Option<&'a Command>,
    listed: bool,
}

impl Marks<'_> {
    /// The window as it is at `at`, the lane being `lane`.
    fn look(&self, lane: &Value, at: i64) -> Look {
        let screen = self.members.rig().screen(self.window.id());
        let drawn = screen_drawn(self.members, self.window);
        let printed = printed_since(&screen, self.screen);
        Look {
            at,
            lane: word_of(lane),
            // A member that writes numbers is told by how many; one that runs a command by
            // the lines it printed.
            printed: if self.command.is_some() {
                printed.others_line()
            } else {
                printed.line()
            },
            drawn: drawn[drawn.len().saturating_sub(DRAWN)..]
                .iter()
                .map(|line| squash(line, DRAWN_WIDTH))
                .collect(),
            tree: self.command.map(|command| {
                let read = tree::read(self.members, &self.window.pane);
                tool::tree_phrase(&read, command, self.listed)
            }),
            also: self
                .wire
                .map(|wire| self.devin_says(wire))
                .unwrap_or_default(),
        }
    }

    /// What Devin's own files say now: its wire log since the pause, and what the
    /// daemon's record reader makes of the conversation the wire names (a record the
    /// reader finds settled is a turn the daemon reads as over).
    fn devin_says(&self, wire: &Wire) -> Vec<String> {
        let mut said = vec![format!("its wire log, {}", wire.since())];
        if let Some(session) = wire.session() {
            let read = ask::records("devin", &session, self.members.rig().environment())
                .ok()
                .flatten();
            said.push(settlement_of(read.as_ref()));
        }
        said
    }
}

/// Watches the stop of the task paused at `made`: the lane every [`LOOK`], and the
/// window a moment after each round of Escapes. It ends when the lane has been at
/// rest for as long as the window is watched at rest (`ctx.rest`), when the board
/// has said the window did not stop for a little while (where `hold` says so), or
/// when the time `hold` gives is up.
fn watch(ctx: &Ctx, marks: &Marks, made: i64, hold: &Hold) -> Result<Watched> {
    let end = made + i64::try_from(hold.stop.as_millis()).unwrap_or(i64::MAX);
    let rest = i64::try_from(ctx.rest.as_millis()).unwrap_or(i64::MAX);
    let mut words: Vec<String> = Vec::new();
    let mut looks: Vec<Look> = Vec::new();
    let mut at_rest_since: Option<i64> = None;
    let mut ignored_at: Option<i64> = None;
    let mut flagged: Option<Value> = None;
    let mut command_gone: Option<i64> = None;
    let mut last: Option<String> = None;
    loop {
        let now = now_ms();
        let lane = ctx.members.lane(ctx.agent)?;
        if command_gone.is_none() {
            command_gone = marks
                .command
                .filter(|command| {
                    tool::is_gone(&tree::read(marks.members, &marks.window.pane), command)
                })
                .map(|_| now);
        }
        let word = word_of(&lane);
        if last.as_ref() != Some(&word) {
            words.push(format!("{word} at {}", plus(now, made)));
            last = Some(word);
        }
        let presses = ctx.members.rig().presses(marks.window.id(), &[27]);
        for _ in due(&presses, looks.len(), now) {
            looks.push(marks.look(&lane, now));
        }
        at_rest_since = at_rest(&lane).then(|| at_rest_since.unwrap_or(now));
        if let Some(flag) = unstopped(&lane) {
            flagged.get_or_insert(flag);
            ignored_at.get_or_insert(now);
        }
        if over(
            now,
            end,
            at_rest_since,
            ignored_at,
            (rest, hold.after_ignored),
        ) {
            return Ok(Watched {
                words,
                looks,
                end: marks.look(&lane, now),
                flagged,
                ended_at: now,
                command_gone,
            });
        }
        sleep(LOOK);
    }
}

/// The task `number`, whose member is in the middle of a turn in `window`, is
/// paused, and what came of the stop is said in `case`: the checks, and what they
/// were made from. `harness` is the real one the member runs on, whose own record
/// adds to what is said (Devin's wire log); none for the stand-in agent. `hold` says
/// how the stop is held to account.
pub fn stop_it(
    ctx: &Ctx,
    case: &mut Account,
    number: i64,
    window: &MemberWindow,
    harness: Option<Harness>,
    hold: Hold,
) -> Result<()> {
    let members = ctx.members;
    let wire = harness
        .filter(|harness| *harness == Harness::Devin)
        .and_then(|_| Wire::open(&members.rig().home()));
    let marks = Marks {
        members,
        window,
        screen: members.rig().screen(window.id()).chars().count(),
        wire: wire.as_ref(),
        command: hold.command,
        listed: hold.listed,
    };
    let paused = members.pause(number)?;
    let made = paused.made_ms;
    said_pause(case, number, &paused.answer);
    let watched = watch(ctx, &marks, made, &hold)?;
    let events = members.events();
    let handle = window.handle.as_str();
    let human = members.inbox(None)?;
    let requester = members.inbox(Some("chief"))?;
    let seen = Seen {
        made,
        until: watched.ended_at,
        presses: members.rig().presses(window.id(), &[27]),
        rested: rested_at(&events, handle, made),
        flagged: watched.flagged.clone(),
        told_human: told_the_human(&human).len(),
        told_requester: told_the_requester(&requester).len(),
        task_state: members.task(number)?["state"]
            .as_str()
            .unwrap_or("undefined")
            .to_owned(),
    };
    judge(case, &seen);
    show(case, &seen, &watched, &events, handle);
    if let Some(tree) = &watched.end.tree {
        case.say(format!(
            "     {}",
            tool::fate_line(
                tree,
                watched.command_gone,
                hold.listed,
                made,
                watched.ended_at
            )
        ));
    }
    if !case.held() {
        case.say("     reading it: the lines of each round say whether its keys reached the window (a hint of the window's own after the first press and none after the second means the second was not taken as a second) and whether the turn went on (lines of numbers, wire records of message chunks, a command the host's tree still has)");
    }
    say_events(case, members, number);
    Ok(())
}

/// The `midturn` case, on the harness the member `ctx.agent` runs on.
pub fn run(ctx: &Ctx, case: &mut Account, harness: Harness) -> Result<()> {
    if harness == Harness::Devin {
        // What the daemon's reader makes of Devin's store is asked of the product's own
        // code, built before anything is spent.
        ask::prepare()?;
    }
    let Some(begun) = begin(
        ctx,
        case,
        Some(harness),
        &brief(),
        WORK,
        numbers::DOING,
        numbers::writing(numbers::UNSEEN),
    )?
    else {
        return Ok(());
    };
    stop_it(
        ctx,
        case,
        begun.number,
        &begun.window,
        Some(harness),
        Hold::NUMBERS,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_member_is_asked_for_a_reply_that_takes_minutes_and_is_told_to_run_nothing() {
        assert_eq!(
            brief(),
            "Write the whole numbers from 1 to 5000 yourself, as your reply, one on each line and nothing else: no commentary, no summary, no shortening, and run no tools."
        );
        // The folder's own words do not ask for the one line the bench's do.
        assert!(!FOLDER_BRIEF.contains("one line"), "{FOLDER_BRIEF}");
    }

    #[test]
    fn the_watch_is_over_when_the_time_is_up_the_window_has_rested_long_enough_or_the_notes_landed()
    {
        let (end, rest) = (40_000, 12_000);
        let numbers = (rest, Hold::NUMBERS.after_ignored);
        assert!(!over(5_000, end, None, None, numbers));
        // At rest, but not for as long as it is watched at rest.
        assert!(!over(20_000, end, Some(9_000), None, numbers));
        assert!(over(21_000, end, Some(9_000), None, numbers));
        // Said to have ignored the stop, and the notes that say so have had their time.
        assert!(!over(13_999, end, None, Some(10_000), numbers));
        assert!(over(14_000, end, None, Some(10_000), numbers));
        assert!(over(40_000, end, None, None, numbers));
    }

    #[test]
    fn a_watch_that_does_not_end_when_the_window_is_said_to_have_ignored_the_stop_goes_on() {
        let (end, rest) = (90_000, 12_000);
        let tool = (rest, None);
        // Said to have ignored the stop a long while ago: the watch goes on.
        assert!(!over(60_000, end, None, Some(10_000), tool));
        // Until the window rests, or the time is up.
        assert!(over(72_000, end, Some(60_000), Some(10_000), tool));
        assert!(over(90_000, end, None, Some(10_000), tool));
        // A watch of the numbers' sort is as long as [`STOP`], and a tool's longer.
        assert_eq!(Hold::NUMBERS.stop, STOP);
        assert!(tool::hold(&Command::ping(false), true).stop > STOP);
    }
}

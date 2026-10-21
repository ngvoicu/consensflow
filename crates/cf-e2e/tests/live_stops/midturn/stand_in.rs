//! The cases against the stand-in agent, which the real daemon and pane host take for
//! a harness: a member in a turn of two minutes, or in a command of two minutes that it
//! runs as a process of its window. What this proves is that the cases read what the
//! daemon and the pane host say (the keys, the rounds, the lane, the task, the window,
//! the process tree, the log) and say what they saw, as they do of a real harness, which
//! is all they read of any harness but Devin's, whose own files they add.

use std::path::Path;
use std::time::{Duration, Instant};

use cf_e2e::live::members::{stand_in_chief, stand_in_worker, Account, Members, Plan};
use cf_e2e::live::Log;

use super::begin::{begin, Begun};
use super::{numbers, stop_it, Hold};
use crate::{Ctx, FAKE_AGENT};

#[path = "stand_in/command.rs"]
mod command;

/// Whether `said` has the line of a lane that was working at the watch's first look and
/// closed later: `working at +<first> s, closed at +<later> s`. The first look is not at
/// the moment of the pause: the watch starts when the case has the pause's answer and
/// has said it, which a loaded machine takes a fifth of a second over; what the line
/// holds the case to is that the window was working when the watch began and was
/// closed after.
fn lane_worked_and_was_closed(said: &str) -> bool {
    let line = regex::Regex::new(
        r"(?m)^     the lane over the stop: working at \+(\d+\.\d) s, closed at \+(\d+\.\d) s",
    )
    .unwrap();
    line.captures(said).is_some_and(|found| {
        let at = |index: usize| found[index].parse::<f64>().unwrap();
        at(1) <= at(2)
    })
}

#[test]
fn a_lane_that_worked_when_the_watch_began_and_was_closed_after_is_told_from_one_that_was_not() {
    let line = |words: &str| format!("     the lane over the stop: {words}");
    // The first look is when the watch got to it, a fifth of a second late or ten.
    for words in [
        "working at +0.0 s, closed at +13.0 s",
        "working at +0.2 s, closed at +13.0 s",
        "working at +10.0 s, closed at +10.0 s",
    ] {
        assert!(lane_worked_and_was_closed(&line(words)), "{words}");
    }
    for words in [
        "working at +0.2 s",
        "working at +0.2 s, idle at +1.0 s",
        "idle at +0.0 s, closed at +13.0 s",
        "closed at +13.0 s, working at +14.0 s",
        "working at +14.0 s, closed at +13.0 s",
    ] {
        assert!(!lane_worked_and_was_closed(&line(words)), "{words}");
    }
}

/// What the stand-in member is given: a turn of two minutes, which the stand-in
/// spends at work and prints no numbers in.
const TURN: &str = "SLEEP 120 Reply with exactly: MIDTURN";

/// How long a case waits at most for the stand-in to be at what it looks for.
const WORK: Duration = Duration::from_secs(60);

/// How long the stand-in is at work before the numbers case takes it to be writing: it
/// prints none of the numbers the case looks for on a screen.
const UNSEEN: Duration = Duration::from_secs(3);

/// The rig of a case against the stand-in: a project whose chief is the stand-in
/// and whose member, `worker`, is the stand-in too, with `variables` in the
/// environment of the daemon and every window.
fn project(folder: &str, variables: &[(&str, &str)]) -> (Members, Log) {
    let log = Log::new(&format!("live stops {folder} test"));
    let plan = variables.iter().fold(
        Plan::new(folder, stand_in_chief()).with(stand_in_worker()),
        |plan, (name, value)| plan.with_var(name, *value),
    );
    let mut members = Members::open(&plan, Path::new(FAKE_AGENT), &log).unwrap();
    members.add("worker", &[]).unwrap();
    (members, log)
}

/// The case's view of the project: its member and what the check says while it waits.
fn context<'a>(members: &'a Members, log: &'a Log) -> Ctx<'a> {
    Ctx {
        members,
        log,
        agent: "worker",
        question: "",
        rest: Duration::from_secs(2),
    }
}

/// The member given a turn, waited for as the `midturn` case waits for it: at work for
/// `unseen`, its numbers none (the stand-in writes none), for `work` at most.
fn begin_writing(ctx: &Ctx, case: &mut Account, work: Duration, unseen: Duration) -> Option<Begun> {
    begin(
        ctx,
        case,
        None,
        TURN,
        work,
        numbers::DOING,
        numbers::writing(unseen),
    )
    .unwrap()
}

/// The `midturn` case's stop: the member is waited for until it is at work, and paused.
#[test]
fn a_stand_in_paused_in_the_middle_of_its_turn_is_a_stop_the_daemon_pays_and_each_round_is_said() {
    let (members, log) = project("stops-midturn-test", &[]);
    let ctx = context(&members, &log);
    let mut case = Account::new();
    let begun = begin_writing(&ctx, &mut case, WORK, UNSEEN).expect("the member never got to work");
    // The member is said to be writing, with how far its numbers have come two ways.
    assert!(
        case.text().contains(
            "T-1: the member is writing its numbers: 0 lines in the text of its screen, and as far as 0 on the screen the pane host draws; its activity: working"
        ),
        "{}",
        case.text()
    );
    stop_it(
        &ctx,
        &mut case,
        begun.number,
        &begun.window,
        None,
        Hold::NUMBERS,
    )
    .unwrap();
    assert!(case.held(), "{}", case.text());
    let said = case.text();
    for name in [
        "the daemon pressed Escape",
        "the daemon read the window at rest, and paid the stop there",
        "it is not said to have ignored the stop",
        "nothing is pressed once it has stopped",
        "the task is paused",
    ] {
        assert!(said.contains(&format!("ok   {name}: ")), "{name}: {said}");
    }
    assert!(said.contains("task.pause made T-1 paused"), "{said}");
    // The stand-in stops at one Escape, as Claude does: one round of one key, looked at after it.
    assert!(said.contains("     round 1: 1 Escape: +"), "{said}");
    assert!(
        said.contains("s after its last key: the lane read ")
            && said.contains("; the window printed 0 lines of numbers; besides: nothing"),
        "{said}"
    );
    // Each look says the window's screen as its pane host draws it: the stand-in's own words.
    assert!(
        said.contains("; its screen as the pane host draws it: ")
            && said.contains("fake agent started on"),
        "{said}"
    );
    assert!(
        said.contains("     the lane over the stop: working at +"),
        "{said}"
    );
    // No command is run: the host's tree is not said.
    assert!(!said.contains("the host's tree"), "{said}");
    assert!(!said.contains("reading it:"), "{said}");
    members.close().unwrap();
}

/// The same case against a window that does not stop for the pause: the stand-in takes the
/// Escape and then a minute and a half to wind its turn down (`CF_TEST_WIND_DOWN_MS`), as
/// a window does that is slow to end its turn, so the three rounds are ignored, the daemon
/// closes the window and says so to the human and the chief, and the case does not hold.
/// It says each round all the same, which is what it is for.
#[test]
fn a_stand_in_that_goes_on_through_three_rounds_is_a_stop_the_case_fails_and_says_each_round_of() {
    let (members, log) = project(
        "stops-midturn-ignored-test",
        &[("CF_TEST_WIND_DOWN_MS", "90000")],
    );
    let ctx = context(&members, &log);
    let mut case = Account::new();
    let begun = begin_writing(&ctx, &mut case, WORK, UNSEEN).expect("the member never got to work");
    stop_it(
        &ctx,
        &mut case,
        begun.number,
        &begun.window,
        None,
        Hold::NUMBERS,
    )
    .unwrap();
    let said = case.text();
    assert!(!case.held(), "{said}");
    assert!(
        said.contains("ok   the daemon pressed Escape: 3 Escapes: +"),
        "{said}"
    );
    assert!(said.contains(" in 3 rounds"), "{said}");
    for round in 1..=3 {
        assert!(
            said.contains(&format!("     round {round}: 1 Escape: +")),
            "{said}"
        );
    }
    assert!(
        said.contains("FAIL the daemon read the window at rest, and paid the stop there: its reading of the window never turned idle in the "),
        "{said}"
    );
    // The window that did not stop was closed after its third round (whether the notes
    // that say so had landed by the end of the watch is the host's grace's).
    assert!(lane_worked_and_was_closed(&said), "{said}");
    // Each round was looked at, and the window was working at each look.
    assert_eq!(
        said.matches("s after its last key: the lane read working")
            .count(),
        3,
        "{said}"
    );
    assert!(
        said.contains("     reading it: the lines of each round"),
        "{said}"
    );
    members.close().unwrap();
}

/// A member whose window never shows its first message (the stand-in prints that it
/// has no login and stays, and the daemon is told to wait eight seconds for the
/// message, not three minutes): the daemon fails the task and closes the window, and
/// the case, which waits for the lane to work, reads that from the task, does not wait
/// out its time, and says why in the daemon's words and in what it finds: the thread,
/// the notes, the lane, the window as it printed, and the log.
#[test]
fn a_member_whose_window_never_shows_its_first_message_is_said_to_have_failed_as_the_daemon_says_it(
) {
    let (members, log) = project(
        "stops-midturn-unstarted-test",
        &[
            ("CONSENSFLOW_LAUNCH_TIMEOUT_MS", "8000"),
            ("CF_TEST_NO_LOGIN", "worker"),
        ],
    );
    let ctx = context(&members, &log);
    let mut case = Account::new();
    let started = Instant::now();
    let begun = begin_writing(&ctx, &mut case, WORK, UNSEEN);
    let said = case.text();
    assert!(begun.is_none(), "{said}");
    assert!(!case.held(), "{said}");
    // Not the five minutes the case waits at most.
    assert!(started.elapsed() < Duration::from_secs(120), "{said}");
    for words in [
        // The failure, in the daemon's words, from what the chief was told: the stand-in's
        // screen names an API key, so the daemon quotes none of it.
        "FAIL the member did not write its numbers: its task is failed, and the chief was told: T-1 failed: the window never showed its first message; its last screen lines named a secret, so they are left out",
        "     the task: T-1 is failed",
        "     what the daemon wrote:",
        "     the chief was told (",
        "     the human was told (",
        "     its lane over the wait: ",
        "     its window p1-worker-",
        ", opened as: ",
        "its last lines as they came (␛ is Escape, ␍ a carriage return):",
        "No API key found for the selected model",
        "     the daemon's log, its last ",
        "warn the launch of p1-worker-",
    ] {
        assert!(said.contains(words), "{words}: {said}");
    }
    // The window is said by how it ended, or by what its pane host says, or that it has none.
    assert!(
        [
            "it ended with",
            "its pane host says:",
            "its pane host no longer has it"
        ]
        .iter()
        .any(|how| said.contains(how)),
        "{said}"
    );
    // The stand-in is no Devin: its files are not read.
    assert!(!said.contains("its wire log"), "{said}");
    members.close().unwrap();
}

/// A member whose window has not shown its first message when the case's time is up
/// (the daemon waits three minutes for it, the case twenty seconds): its lane reads
/// working all the while, as the daemon's does for a window it delivers to, but its
/// brief has not reached it, so it is not writing. The case says it never wrote, with
/// the brief on its way in and the window as it stands: what its pane host draws, and
/// what it printed.
#[test]
fn a_member_whose_brief_is_still_on_its_way_in_when_the_time_is_up_is_not_taken_to_be_writing() {
    let (members, log) = project("stops-midturn-slow-test", &[("CF_TEST_NO_LOGIN", "worker")]);
    let ctx = context(&members, &log);
    let mut case = Account::new();
    let begun = begin_writing(
        &ctx,
        &mut case,
        Duration::from_secs(20),
        Duration::from_secs(1),
    );
    let said = case.text();
    assert!(begun.is_none(), "{said}");
    assert!(!case.held(), "{said}");
    for words in [
        "FAIL the member never wrote its numbers in 20 s: its window opened, and its brief never reached it (its task's first message is not delivered)",
        "     the task: T-1 is queued",
        "m-1 task delivering from @chief: SLEEP 120 Reply with exactly: MIDTURN",
        "     its lane over the wait: ",
        "     its window p1-worker-",
        // The window is open: its pane host says what it printed and draws its screen.
        "its pane host says: it printed last ",
        "its screen as the pane host draws it: No API key found for the selected model. ⏎ Use /login to log into a provider.",
        // And its process tree, which holds nothing of the turn.
        "its process tree (what began after its turn did): ",
        // And as it came, whatever the terminal added to it.
        "its last lines as they came (␛ is Escape, ␍ a carriage return):",
    ] {
        assert!(said.contains(words), "{words}: {said}");
    }
    // The daemon has not given up on it: nothing was told of it as a failure.
    assert!(!said.contains("T-1 failed"), "{said}");
    members.close().unwrap();
}

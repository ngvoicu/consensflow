//! The `midtool` case against the stand-in agent: a member that runs a command of two
//! minutes, which the stand-in does as a process of its window (`RUN`), so that the
//! host's process tree has it, and which its stop ends with the turn or leaves running,
//! as the line says.

use std::time::{Duration, Instant};

use cf_e2e::live::members::Account;

use super::super::begin::{begin, Begun};
use super::super::tool::{self, Command};
use super::super::{stop_it, Hold};
use super::{context, project, WORK};
use crate::Ctx;

/// How long the stop of a member whose command runs is watched at most, in these tests:
/// the live case watches `STUCK_WATCH`, which is longer than its command runs.
const WATCH: Duration = Duration::from_secs(15);

/// The member told to run `command`, which the stand-in does (`RUN`), and which its
/// stop leaves running where `survives`; waited for as the `midtool` case waits for it.
fn begin_running(
    ctx: &Ctx,
    case: &mut Account,
    command: &Command,
    survives: bool,
) -> Option<Begun> {
    let brief = format!(
        "RUN {}{}",
        if survives { "--survive " } else { "" },
        command.text()
    );
    begin(
        ctx,
        case,
        None,
        &brief,
        WORK,
        tool::DOING,
        tool::running(command, false),
    )
    .unwrap()
}

/// The stop of the member that runs `command`, watched for [`WATCH`].
fn hold(command: &Command) -> Hold<'_> {
    Hold {
        stop: WATCH,
        ..tool::hold(command, true)
    }
}

/// The `midtool` case's stop of a member that runs a command, which its harness leaves
/// running when it stops the turn (the stand-in's `--survive`): the member is waited for
/// until the host's tree has its command, and paused; the turn stops at the one Escape the
/// stand-in takes, the daemon pays the stop, and each look says the host's tree still has
/// the command, which the end of the watch says too.
#[test]
fn a_member_paused_while_its_command_runs_stops_and_the_hosts_tree_says_the_command_runs_on() {
    let (members, log) = project("stops-midtool-survives-test", &[]);
    let ctx = context(&members, &log);
    let mut case = Account::new();
    let command = Command::quiet(119, cfg!(windows));
    let begun =
        begin_running(&ctx, &mut case, &command, true).expect("the member never ran its command");
    // Seen running by its process in the host's tree, before anything is pressed.
    let began = case.text();
    assert!(
        began.contains(
            "T-1: the member is running its command: its process is in the host's tree ("
        ) && began.contains(&format!(" {}); its activity: working", command.text())),
        "{began}"
    );
    stop_it(
        &ctx,
        &mut case,
        begun.number,
        &begun.window,
        None,
        hold(&command),
    )
    .unwrap();
    let said = case.text();
    assert!(case.held(), "{said}");
    for name in [
        "the daemon pressed Escape",
        "the daemon read the window at rest, and paid the stop there",
        "it is not said to have ignored the stop",
        "nothing is pressed once it has stopped",
        "the task is paused",
    ] {
        assert!(said.contains(&format!("ok   {name}: ")), "{name}: {said}");
    }
    // The first round was looked at, and the command ran on; the end says so, and what the
    // case reads of it is what it looks for in the host's tree.
    assert!(
        said.contains("; the host's tree still has the command ("),
        "{said}"
    );
    assert!(
        said.contains("     the command, at the end of the watch (+")
            && said.contains("): the host's tree still has the command ("),
        "{said}"
    );
    assert!(!said.contains("no longer has the command"), "{said}");
    assert!(said.contains("; it was never read gone"), "{said}");
    assert!(!said.contains("reading it:"), "{said}");
    members.close().unwrap();
}

/// The same member, whose stop ends the command with the turn (a harness that kills the
/// tool it cancels): the case holds, and the host's tree no longer has the command once
/// the stop is paid.
#[test]
fn a_member_paused_while_its_command_runs_stops_with_it_and_the_hosts_tree_says_it_is_gone() {
    let (members, log) = project("stops-midtool-ends-test", &[]);
    let ctx = context(&members, &log);
    let mut case = Account::new();
    let command = Command::quiet(119, cfg!(windows));
    let begun =
        begin_running(&ctx, &mut case, &command, false).expect("the member never ran its command");
    stop_it(
        &ctx,
        &mut case,
        begun.number,
        &begun.window,
        None,
        hold(&command),
    )
    .unwrap();
    let said = case.text();
    assert!(case.held(), "{said}");
    assert!(
        said.contains("     the command, at the end of the watch (+")
            && said.contains("): the host's tree no longer has the command ("),
        "{said}"
    );
    // It went with the turn, at the Escape: read gone as soon as the watch looked.
    assert!(said.contains("; it was first read gone at +"), "{said}");
    assert!(!said.contains("never read gone"), "{said}");
    assert!(!said.contains("still has the command"), "{said}");
    members.close().unwrap();
}

/// T-5's shape: the turn goes on through the rounds while its command runs (the
/// stand-in takes a minute and a half to wind its turn down, and leaves the command
/// running), so the daemon says the window did not stop, the case does not hold, and it
/// says each round all the same, with the command running at each look and at the end.
#[test]
fn a_member_whose_turn_goes_on_while_its_command_runs_is_a_stop_the_case_fails_and_says_each_round_of(
) {
    let (members, log) = project(
        "stops-midtool-ignored-test",
        &[("CF_TEST_WIND_DOWN_MS", "90000")],
    );
    let ctx = context(&members, &log);
    let mut case = Account::new();
    let command = Command::quiet(119, cfg!(windows));
    let begun =
        begin_running(&ctx, &mut case, &command, true).expect("the member never ran its command");
    stop_it(
        &ctx,
        &mut case,
        begun.number,
        &begun.window,
        None,
        hold(&command),
    )
    .unwrap();
    let said = case.text();
    assert!(!case.held(), "{said}");
    assert!(said.contains(" in 3 rounds"), "{said}");
    assert!(
        said.contains("FAIL the daemon read the window at rest, and paid the stop there: its reading of the window never turned idle in the "),
        "{said}"
    );
    assert!(
        said.contains("FAIL it is not said to have ignored the stop: unstopped on its lane: {"),
        "{said}"
    );
    // At each of the three looks, at the end, and in the line that tells it, the command
    // is there.
    assert_eq!(
        said.matches("the host's tree still has the command (")
            .count(),
        3 + 1 + 1,
        "{said}"
    );
    assert!(!said.contains("no longer has the command"), "{said}");
    assert!(said.contains("; it was never read gone"), "{said}");
    // The watch was not ended by the board's word: it ran to the end of its time.
    assert!(said.contains("     at the end, +15."), "{said}");
    members.close().unwrap();
}

/// A member that answers without running its command (the stand-in is not told to run
/// anything): its turn is over before it is paused, its task done, and the case says so,
/// and does not wait out its time.
#[test]
fn a_member_that_never_runs_its_command_is_said_to_have_got_no_further_than_its_task_done() {
    let (members, log) = project("stops-midtool-never-test", &[]);
    let ctx = context(&members, &log);
    let mut case = Account::new();
    let command = Command::quiet(119, cfg!(windows));
    let started = Instant::now();
    let begun = begin(
        &ctx,
        &mut case,
        None,
        "Reply with exactly: DONE",
        WORK,
        tool::DOING,
        tool::running(&command, false),
    )
    .unwrap();
    let said = case.text();
    assert!(begun.is_none(), "{said}");
    assert!(!case.held(), "{said}");
    assert!(started.elapsed() < Duration::from_secs(120), "{said}");
    assert!(
        said.contains("FAIL the member did not run its command: its task is "),
        "{said}"
    );
    assert!(said.contains("     the task: T-1 is "), "{said}");
    // The host's tree says what the window had of the turn: not the command.
    assert!(
        !said.contains("its process is in the host's tree"),
        "{said}"
    );
    members.close().unwrap();
}

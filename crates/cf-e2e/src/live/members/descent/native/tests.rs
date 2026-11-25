//! The hold on a process, on the machine the tests run on: the Mac's, and Windows'
//! handle on zeewin.

use std::time::Duration;

use super::super::{end_chosen, Identity, Link, Machine, Running, ThisMachine};
use crate::process::{Run, Spawned};

/// A program that waits half a minute and says nothing.
fn waiting() -> Spawned {
    let program = if cfg!(windows) {
        Run::new("ping").args(["-n", "30", "127.0.0.1"])
    } else {
        Run::new("sleep").args(["30"])
    };
    program.closed_input().spawn().expect("the program starts")
}

/// The process `identity`, as a check that chose it has it.
fn chosen(identity: Identity) -> Running {
    Running {
        identity,
        ppid: std::process::id(),
        command: "waiting".to_owned(),
        exe: None,
        link: Link::Number,
    }
}

#[test]
fn a_process_is_ended_through_what_holds_it_only_while_it_began_when_it_was_chosen() {
    // Between the look that chose a process and its end, its number may be another
    // process's: the end is checked on what holds the process (on Windows a handle, which
    // keeps the number its own until the end; elsewhere its number), against when the one
    // chosen began (Astra and Sol on round 6).
    let mut program = waiting();
    let identity = ThisMachine
        .identity(program.id())
        .unwrap()
        .expect("the program is there");
    // As the listing has it: the same, on the same machine.
    let listed = ThisMachine.processes().unwrap();
    assert!(
        listed.iter().any(|one| one.identity == identity),
        "the listing has {identity:?}"
    );

    let another = Identity {
        started: identity.started + 1,
        ..identity
    };
    let said = end_chosen(&[chosen(another)]);
    assert_eq!(said.gone, [chosen(another)], "{said:?}");
    assert!(said.ended.is_empty() && said.failed.is_empty(), "{said:?}");
    assert!(
        program.wait(Duration::from_millis(500)).unwrap().is_none(),
        "the process whose number it was runs on"
    );

    let said = end_chosen(&[chosen(identity)]);
    assert_eq!(said.ended, [chosen(identity)], "{said:?}");
    assert!(
        program.wait(Duration::from_secs(10)).unwrap().is_some(),
        "the process chosen was ended"
    );
    // Once it is gone, nothing has its number to end.
    let said = end_chosen(&[chosen(identity)]);
    assert!(said.ended.is_empty() && said.failed.is_empty(), "{said:?}");
}

#[test]
fn a_number_no_process_has_is_nothing_to_end() {
    let nobody = Identity {
        pid: u32::MAX - 7,
        started: 1,
    };
    let said = end_chosen(&[chosen(nobody)]);
    assert_eq!(said.gone, [chosen(nobody)], "{said:?}");
}

#[cfg(target_os = "macos")]
#[test]
fn a_mac_says_when_a_process_began_to_the_microsecond() {
    // `ps`'s `lstart` says it to the second, and two processes that took one number within a
    // second were the same to a check (Astra and Sol on round 6). One of three programs
    // begins on a second's very microsecond once in a million million million times.
    let programs: Vec<Spawned> = (0..3).map(|_| waiting()).collect();
    let started: Vec<u64> = programs
        .iter()
        .map(|program| {
            ThisMachine
                .identity(program.id())
                .unwrap()
                .expect("the program is there")
                .started
        })
        .collect();
    assert!(
        started.iter().any(|micros| micros % 1_000_000 != 0),
        "{started:?}"
    );
    // In microseconds since 1970: a moment ago.
    let now = jiff::Timestamp::now().as_microsecond();
    for micros in started {
        let ago = now - i64::try_from(micros).unwrap();
        assert!((0..120_000_000).contains(&ago), "{micros} is {ago} µs ago");
    }
    // A process the machine says nothing of has no identity.
    assert_eq!(ThisMachine.identity(u32::MAX - 7).unwrap(), None);
}

/// The listing this machine gives is read with what proves each child against the process that
/// made it, and a child its own shell started is the shell's by that proof, not by a number:
/// the real read, of a program the test started and holds, with a child of its own.
#[test]
fn what_this_machine_lists_proves_a_child_against_the_process_that_made_it() {
    use super::super::{descending_from, Link};

    // A program that runs a child of its own and waits for it: `cmd`, and `find` with `-exec`.
    let mut shell = if cfg!(windows) {
        Run::new("cmd").args(["/c", "ping -n 31 127.0.0.1"])
    } else {
        Run::new("find").args(["/", "-maxdepth", "0", "-exec", "sleep", "30.4231", ";"])
    }
    .closed_input()
    .spawn()
    .expect("the program starts");
    let root = ThisMachine
        .identity(shell.id())
        .unwrap()
        .expect("the shell is there");
    let mark = if cfg!(windows) { "ping" } else { "30.4231" };
    let mut child = None;
    for _ in 0..50 {
        let all = ThisMachine.processes().unwrap();
        let tree = descending_from(&all, root);
        if let Some(found) = tree.iter().find(|one| one.command.contains(mark)) {
            child = Some(found.clone());
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let child = child.expect("the shell's child is in its tree");
    assert_ne!(child.link, Link::Number, "{child:?}");
    assert_ne!(child.link, Link::Unread, "{child:?}");
    // It is ended through what holds it, being the one chosen; then its shell, which this test
    // started.
    let said = end_chosen(std::slice::from_ref(&child));
    assert_eq!(said.ended, [child], "{said:?}");
    shell.kill();
}

/// The kit's own process is a root like any other: a Claude the parity case started from the test
/// is its child, and the attribution reads the run's processes under it (T-2314, finding 2). The
/// real read, rooted at this test process, with a child of its own the test holds.
#[test]
fn a_child_of_the_tests_own_process_is_in_the_tree_of_the_tests_own_process() {
    use super::super::{descending_from, unproved_under, Link};

    let mut program = waiting();
    let me = ThisMachine
        .identity(std::process::id())
        .unwrap()
        .expect("this process is there");
    let all = ThisMachine.processes().unwrap();
    let own = all.iter().find(|one| one.identity == me).expect("it is listed");
    assert_ne!(own.link, Link::Unread, "{own:?}");
    assert_ne!(own.link, Link::Number, "{own:?}");
    let child = all
        .iter()
        .find(|one| one.identity.pid == program.id())
        .expect("the child is listed")
        .clone();
    let tree = descending_from(&all, me);
    let named = unproved_under(&all, me);
    program.kill();
    assert!(
        tree.iter().any(|one| one.identity == child.identity),
        "the test's own child is not in its tree: {child:?}; named: {named:?}"
    );
}

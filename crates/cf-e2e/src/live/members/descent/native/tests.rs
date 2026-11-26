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
    let own = all
        .iter()
        .find(|one| one.identity == me)
        .expect("it is listed");
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

/// What Windows' listing says of processes the test started and holds, read with no program
/// between (T-2363: a PowerShell took 20 to 30 s to start on a machine with many modules): a
/// parent, its child and a grandchild each place as the table that listing replaced placed them,
/// by their numbers and when each began, with the image each runs and the command line each was
/// started with; a process that has ended is not listed as itself; and one that cannot be opened
/// (the system's own) is listed and proves no edge, so that no process is the window's by it.
#[cfg(windows)]
#[test]
fn windows_lists_a_parent_its_child_and_grandchild_an_ended_process_and_the_rest_of_the_machine() {
    use super::super::descending_from;

    // `cmd` runs a `cmd` that runs `ping`: the root, its child, and the child's own.
    let mut root = Run::new("cmd")
        .args(["/c", "cmd /c ping -n 31 127.0.0.1"])
        .closed_input()
        .spawn()
        .expect("the program starts");
    // A process that is there now and ended before the listing.
    let mut ended = waiting();
    let gone = ThisMachine
        .identity(ended.id())
        .unwrap()
        .expect("it is there");
    ended.kill();
    let root_identity = ThisMachine
        .identity(root.id())
        .unwrap()
        .expect("the root is there");

    // The test waits on what the listing says of the grandchild, as the case above does.
    let mut found = None;
    for _ in 0..100 {
        let all = ThisMachine.processes().unwrap();
        let tree = descending_from(&all, root_identity);
        if tree.iter().any(|one| one.command.contains("ping -n 31")) {
            found = Some((all, tree));
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let (all, tree) = found.expect("the root's tree holds its grandchild");

    let row = |pid: u32| all.iter().find(|one| one.identity.pid == pid).cloned();
    let me = row(std::process::id()).expect("this process is listed");
    let top = row(root.id()).expect("the root is listed");
    // Its creator is this process, proved on its own handle; its time is the handle's.
    assert_eq!(top.ppid, me.identity.pid, "{top:?}");
    assert_eq!(top.identity, root_identity, "{top:?}");
    assert!(
        matches!(top.link, Link::Created { by, .. } if by == me.identity.pid),
        "{top:?}"
    );
    assert!(
        top.exe
            .as_deref()
            .is_some_and(|exe| exe.to_ascii_lowercase().ends_with("cmd.exe")),
        "{top:?}"
    );
    assert!(top.command.contains("cmd /c ping -n 31"), "{top:?}");
    // The child is the root's, the grandchild the child's: three generations, each once.
    let child = tree
        .iter()
        .find(|one| one.ppid == top.identity.pid && one.command.contains("ping -n 31"))
        .expect("the root's child is in its tree");
    let grandchild = tree
        .iter()
        .find(|one| {
            one.ppid == child.identity.pid
                && one
                    .exe
                    .as_deref()
                    .is_some_and(|exe| exe.to_ascii_lowercase().ends_with("ping.exe"))
        })
        .expect("the child's own is in the tree");
    assert!(
        matches!(grandchild.link, Link::Created { by, .. } if by == child.identity.pid),
        "{grandchild:?}"
    );
    assert!(
        grandchild.identity.started >= child.identity.started,
        "{grandchild:?} {child:?}"
    );
    assert_eq!(
        tree.iter()
            .filter(|one| one.identity == grandchild.identity)
            .count(),
        1
    );

    // The ended process is not listed as the process it was.
    assert!(
        all.iter().all(|one| one.identity != gone),
        "an ended process is listed: {gone:?}"
    );

    // Every row went through a handle or failed to: none is believed by its number alone (the
    // system's own processes are the ones a handle may be refused of), and one a handle was
    // refused of says nothing of what it runs and proves no edge, so no process is the window's
    // by it.
    for one in &all {
        assert_ne!(one.link, Link::Number, "{one:?}");
        if one.link == Link::Unread {
            assert!(one.command.is_empty() && one.exe.is_none(), "{one:?}");
            assert!(
                descending_from(&all, one.identity)
                    .iter()
                    .all(|under| under.link != Link::Number),
                "{one:?}"
            );
        }
    }
    root.kill();
}

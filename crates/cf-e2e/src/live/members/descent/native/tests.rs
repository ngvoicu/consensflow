//! The hold on a process, on the machine the tests run on: the Mac's, and Windows'
//! handle on zeewin.

use std::time::Duration;

use super::super::{end_chosen, Identity, Machine, Running, ThisMachine};
use crate::process::{Run, Spawned};

/// A program that waits half a minute and says nothing.
fn waiting() -> Spawned {
    let program = if cfg!(windows) {
        Run::new("ping").args(["-n", "30", "127.0.0.1"])
    } else {
        Run::new("sleep").args(["30"])
    };
    program
        .inheriting_env()
        .closed_input()
        .spawn()
        .expect("the program starts")
}

/// The process `identity`, as a check that chose it has it.
fn chosen(identity: Identity) -> Running {
    Running {
        identity,
        ppid: std::process::id(),
        command: "waiting".to_owned(),
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

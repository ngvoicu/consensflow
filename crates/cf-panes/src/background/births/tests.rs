use super::*;

fn key(pid: u32, began: u64) -> ProcessKey {
    ProcessKey { pid, began }
}

#[test]
fn a_process_is_proved_started_by_a_shell_only_if_its_birth_says_so_for_the_process_it_names() {
    let mut births = Births::default();
    births.record(10, 100, true);
    births.record(11, 110, false);

    assert!(births.started_by_a_shell(key(10, 100)));
    assert!(
        !births.started_by_a_shell(key(11, 110)),
        "its starter was no shell"
    );
    assert!(
        !births.started_by_a_shell(key(12, 120)),
        "no birth was read"
    );
    assert!(
        !births.started_by_a_shell(key(10, 105)),
        "a process that took the number later is not the one read"
    );
}

#[test]
fn a_number_recorded_again_is_the_newer_processs() {
    let mut births = Births::default();
    births.record(10, 100, true);
    births.record(10, 200, false);

    assert!(!births.started_by_a_shell(key(10, 100)));
    assert!(!births.started_by_a_shell(key(10, 200)));
    births.record(10, 300, true);
    assert!(births.started_by_a_shell(key(10, 300)));
}

#[test]
fn the_oldest_births_are_let_go_of_past_the_bound() {
    let mut births = Births::default();
    for pid in 0..u32::try_from(BIRTHS).expect("a bound") + 5 {
        births.record(pid, u64::from(pid) + 1, true);
    }

    assert!(!births.started_by_a_shell(key(0, 1)), "the oldest is gone");
    assert!(!births.started_by_a_shell(key(4, 5)));
    assert!(births.started_by_a_shell(key(5, 6)), "the rest are kept");
    let last = u32::try_from(BIRTHS).expect("a bound") + 4;
    assert!(births.started_by_a_shell(key(last, u64::from(last) + 1)));
    assert_eq!(births.order.len(), BIRTHS);
}

/// A process that runs keeps its birth however many short-lived ones come after it: the cap
/// lets go of the oldest, and a server started at the beginning of a day's work is the oldest
/// (T-1153's D2).
#[test]
fn a_birth_of_a_process_that_runs_survives_more_short_lived_ones_than_the_bound() {
    let mut births = Births::default();
    births.record(1, 10, true);
    for pid in 100..100 + 2 * u32::try_from(BIRTHS).expect("a bound") {
        births.record(pid, u64::from(pid), true);
        births.forget(pid);
    }

    assert!(
        births.started_by_a_shell(key(1, 10)),
        "the server's birth is kept"
    );
    assert_eq!(births.order.len(), 1, "and the exited ones are not");
}

#[test]
fn a_birth_is_let_go_of_when_its_process_exits_and_an_unknown_number_is_no_matter() {
    let mut births = Births::with_cap(8);
    births.record(10, 100, true);
    births.record(11, 110, true);

    births.forget(10);
    births.forget(77);

    assert!(!births.started_by_a_shell(key(10, 100)));
    assert!(births.started_by_a_shell(key(11, 110)));
    assert_eq!(births.order.len(), 1);
}

#[test]
fn a_number_recorded_again_goes_to_the_back_of_the_line_to_be_let_go_of() {
    let mut births = Births::with_cap(3);
    births.record(1, 10, true);
    births.record(2, 20, true);
    births.record(3, 30, true);
    // The first number is taken by a new process (the old one exited and its exit was lost).
    births.record(1, 40, true);
    births.record(4, 50, true);

    assert!(
        births.started_by_a_shell(key(1, 40)),
        "the newer process of the number is kept"
    );
    assert!(
        !births.started_by_a_shell(key(2, 20)),
        "the oldest birth is the one let go of"
    );
    assert!(births.started_by_a_shell(key(3, 30)));
    assert!(births.started_by_a_shell(key(4, 50)));
}

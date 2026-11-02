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

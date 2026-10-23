use super::*;

/// 12:00:00 on a day, in microseconds since the epoch.
const NOON: i64 = 1_792_670_400_000_000;

/// A reading `at` microseconds after the pane opened, the wall clock `off` ahead of where
/// it was then, after `slept` microseconds of sleep.
fn at(at: i64, off: i64, slept: i64) -> Reading {
    Reading {
        wall: NOON + at + slept + off,
        awake: 5_000_000 + at,
        continuous: 9_000_000 + at + slept,
    }
}

/// A pane's clock read every 100 ms for `seconds`, the wall clock `off(t)` ahead, no sleep.
fn watched(seconds: i64, off: impl Fn(i64) -> i64) -> PaneClock {
    let mut clock = PaneClock::new(at(0, 0, 0));
    for tick in 1..=seconds * 10 {
        let t = tick * 100_000;
        clock.observe(at(t, off(t), 0));
    }
    clock
}

/// A process that began `t` microseconds after the pane opened, its wall start `off` ahead.
fn began(clock: &PaneClock, t: i64, off: i64, after: i64) -> bool {
    clock.began_before(NOON + t + off, 5_000_000 + t, NOON + after)
}

#[test]
fn a_server_that_began_before_the_call_is_proved_older_on_a_clock_that_kept_its_order() {
    // The server began 1 s after the open; the call is stamped at 10 s; the look names 9 s.
    let clock = watched(20, |_| 0);
    assert!(began(&clock, 1_000_000, 0, 9_000_000));
    // The command, begun 20 ms after the call, is not.
    assert!(!began(&clock, 10_020_000, 0, 9_000_000));
}

#[test]
fn after_the_clock_is_set_back_between_a_call_and_its_command_nothing_is_proved_older() {
    // Astraeus on 02a6cd4f: the call is stamped at 10 s; the clock is set back 2 s at
    // 10.010 s, and the command begins at 10.020 s, 8.020 s by the clock. The look names
    // 9 s: the command, and the server, are counted.
    let clock = watched(20, |t| if t > 10_010_000 { -2_000_000 } else { 0 });
    assert!(
        !began(&clock, 10_020_000, -2_000_000, 9_000_000),
        "the command"
    );
    assert!(
        !began(&clock, 1_000_000, 0, 9_000_000),
        "the server, as the clock is unknown"
    );
}

#[test]
fn steps_back_too_small_to_see_alone_are_seen_together() {
    // Ten steps back of 90 ms each, one a second: none is half a second, the ten are.
    let clock = watched(20, |t| -90_000 * (t / 1_000_000).min(10));
    assert!(!began(&clock, 1_000_000, 0, 9_000_000));
    let few = watched(20, |t| -90_000 * (t / 1_000_000).min(4));
    assert!(began(&few, 1_000_000, 0, 9_000_000), "less than the budget");
}

#[test]
fn a_clock_slewed_or_set_forward_keeps_its_order() {
    // 9 ppm behind, as measured, over an hour of readings; then set forward 3 s.
    let mut clock = PaneClock::new(at(0, 0, 0));
    for tick in 1..=36_000 {
        let t = tick * 100_000;
        clock.observe(at(t, -t * 9 / 1_000_000, 0));
    }
    assert!(began(&clock, 1_000_000, 0, 3_000_000_000));
    clock.observe(at(3_600_100_000, 3_000_000, 0));
    assert!(began(&clock, 1_000_000, 0, 3_000_000_000), "a step forward");
    assert!(
        clock.kept.len() < 200,
        "a few readings kept: {}",
        clock.kept.len()
    );
}

#[test]
fn a_clock_slewed_as_fast_as_macos_slews_keeps_its_order() {
    // 500 ppm behind for an hour: the wall clock fell 1.8 s behind the continuous one, and
    // never ran back.
    let mut clock = PaneClock::new(at(0, 0, 0));
    for tick in 1..=36_000 {
        let t = tick * 100_000;
        clock.observe(at(t, -t / 2_000, 0));
    }
    assert!(began(&clock, 1_000_000, -500, 3_000_000_000));
}

#[test]
fn a_process_whose_two_starts_disagree_with_the_readings_around_it_is_counted() {
    // A clock set back 2 s and forward again between two readings, the process begun in
    // between: no reading shows it, its own two starts do.
    let clock = watched(20, |_| 0);
    assert!(!began(&clock, 5_050_000, -2_000_000, 9_000_000));
    assert!(
        began(&clock, 5_050_000, -4_000, 9_000_000),
        "within the noise"
    );
}

#[test]
fn a_gap_in_the_readings_leaves_the_clock_unknown() {
    let mut clock = PaneClock::new(at(0, 0, 0));
    clock.observe(at(100_000, 0, 0));
    clock.observe(at(2_200_000, 0, 0));
    clock.observe(at(2_300_000, 0, 0));
    assert!(!began(&clock, 50_000, 0, 1_000_000));
}

#[test]
fn a_sleep_keeps_the_clock_but_a_process_begun_across_it_is_counted() {
    // A minute of sleep between the readings at 1 s and 1.1 s: the wall clock kept its
    // order. A process begun at 1.05 s of awake time lies before or after the sleep.
    let mut clock = PaneClock::new(at(0, 0, 0));
    for tick in 1..=10 {
        clock.observe(at(tick * 100_000, 0, 0));
    }
    for tick in 11..=30 {
        clock.observe(at(tick * 100_000, 0, 60_000_000));
    }
    assert!(began(&clock, 500_000, 0, 2_000_000), "before the sleep");
    assert!(!began(&clock, 1_050_000, 0, 2_000_000), "across it");
    // After it, its wall start is a minute later than its awake start says.
    assert!(clock.began_before(
        NOON + 2_000_000 + 60_000_000,
        5_000_000 + 2_000_000,
        NOON + 70_000_000
    ));
}

#[test]
fn a_process_begun_before_the_pane_or_after_the_latest_reading_or_not_read_is_counted() {
    let clock = watched(5, |_| 0);
    assert!(!clock.began_before(NOON - 1_000_000, 4_000_000, NOON + 2_000_000));
    assert!(!began(&clock, 6_000_000, 0, 9_000_000));
    let member = Member {
        key: super::super::ProcessKey { pid: 11, began: 0 },
        command: String::new(),
        program: String::new(),
        exe: String::new(),
        group: None,
        parent: None,
    };
    assert!(!began_before(&clock, (NOON + 2_000_000) / 1_000, |_| Some(
        5_100_000
    ))(&member));
    let read = Member {
        key: super::super::ProcessKey {
            pid: 11,
            began: u64::try_from(NOON + 100_000).unwrap(),
        },
        ..member
    };
    assert!(began_before(&clock, (NOON + 2_000_000) / 1_000, |_| Some(
        5_100_000
    ))(&read));
    assert!(!began_before(&clock, (NOON + 2_000_000) / 1_000, |_| None)(
        &read
    ));
}

use super::*;

/// What `KERN_PROCARGS2` answers for `sleep 31` started with `environment`:
/// the count, the program's path and its padding, the arguments, the
/// environment, and the NULs that end each.
fn procargs_of(environment: &[&str]) -> Vec<u8> {
    let mut bytes = 2i32.to_ne_bytes().to_vec();
    bytes.extend_from_slice(b"/bin/sleep\0\0\0\0\0");
    bytes.extend_from_slice(b"sleep\0");
    bytes.extend_from_slice(b"31\0");
    for variable in environment {
        bytes.extend_from_slice(variable.as_bytes());
        bytes.push(0);
    }
    bytes.push(0);
    bytes
}

#[test]
fn a_command_line_is_the_arguments_the_process_was_started_with_and_not_its_environment() {
    let bytes = procargs_of(&["PATH=/usr/bin", "HOME=/Users/zeus"]);
    assert_eq!(command_line(&bytes).as_deref(), Some("sleep 31"));
    assert_eq!(command_line(&procargs_of(&[])).as_deref(), Some("sleep 31"));
}

#[test]
fn a_process_the_system_shows_nothing_of_has_no_command_line() {
    assert_eq!(command_line(&[]), None);
    assert_eq!(command_line(&0i32.to_ne_bytes()), None);
    assert_eq!(command_line(b"\x02\0\0\0/bin/sleep"), None);
}

#[test]
fn the_marker_is_found_among_the_environment_and_not_among_the_arguments() {
    let marker = format!("{OWNER_ENV}=abc123");
    let owned = procargs_of(&["PATH=/usr/bin", &marker]);
    assert!(environment_has_marker(&owned, "abc123"));
    assert!(!environment_has_marker(&owned, "abc124"));
    assert!(!environment_has_marker(&procargs_of(&[]), "abc123"));
    // A marker typed as an argument is no marker.
    let mut typed = 2i32.to_ne_bytes().to_vec();
    typed.extend_from_slice(b"/bin/echo\0\0\0\0echo\0");
    typed.extend_from_slice(marker.as_bytes());
    typed.extend_from_slice(b"\0\0");
    assert!(!environment_has_marker(&typed, "abc123"));
}

#[allow(clippy::disallowed_methods)] // A test's own program.
#[test]
fn a_process_is_read_while_it_runs_ended_once_it_exits_and_unreadable_where_another_users() {
    let mut child = std::process::Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .expect("sleep starts");
    let pid = child.id() as i32;
    assert_eq!(looked(pid), Looked::Readable);
    // Killed and not yet reaped, it is a zombie: ended.
    child.kill().expect("sleep is killed");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while looked(pid) != Looked::Ended && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(looked(pid), Looked::Ended, "a zombie");
    child.wait().expect("sleep is reaped");
    assert_eq!(looked(pid), Looked::Ended, "none of that number");
    // launchd is root's.
    assert_eq!(looked(1), Looked::Unreadable);
}

#[allow(clippy::disallowed_methods)] // A test's own programs.
#[test]
fn a_processs_two_starts_agree_with_the_clocks_read_around_it() {
    // T-334: the noise the pane's clock allows a process (`background::clock`'s
    // `PROCESS_NOISE`, 5 ms) is far above what a Mac shows (±4 µs over 300 processes).
    let mut worst = 0;
    for _ in 0..50 {
        let before = clocks();
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("5")
            .spawn()
            .expect("sleep starts");
        let pid = child.id();
        let after = clocks();
        let started = process(i32::try_from(pid).unwrap()).expect("its start");
        let wall =
            i64::try_from(started.identity.born.0 * 1_000_000 + started.identity.born.1).unwrap();
        let awake = awake_start(pid).expect("its awake start");
        child.kill().expect("sleep is killed");
        child.wait().expect("sleep is reaped");
        assert!(
            before.awake <= awake && awake <= after.awake,
            "read between"
        );
        let ahead = wall - (awake + (before.continuous - before.awake));
        let expected = before.wall - before.continuous;
        worst = worst.max((ahead - expected).abs());
    }
    assert!(worst < 1_000, "{worst} µs");
}

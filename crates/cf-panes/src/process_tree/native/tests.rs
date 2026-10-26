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

/// A number some process of another user has: the system answers a signal 0 to it with
/// `EPERM`. The system's own daemons are root's, and run on every Mac.
fn a_number_another_user_holds() -> i32 {
    // SAFETY: a null buffer of size 0 only asks how many processes there are.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    let mut pids = vec![0i32; usize::try_from(count).expect("a count") + 256];
    let size = i32::try_from(pids.len() * std::mem::size_of::<i32>()).expect("a size");
    // SAFETY: the buffer is `pids`, writable for the `size` bytes passed with it.
    let read = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), size) };
    pids.truncate(usize::try_from(read).expect("a count"));
    pids.into_iter()
        .filter(|pid| *pid > 1)
        .find(|pid| {
            // SAFETY: signal 0 delivers nothing.
            (unsafe { libc::kill(*pid, 0) }) != 0
                && io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
        })
        .expect("a process of another user")
}

#[test]
#[allow(clippy::disallowed_methods)] // A test's own programs.
fn nobody_holds_a_number_only_where_the_system_answers_that_no_process_has_it() {
    assert!(!nobody_holds(1), "init's");
    assert!(!nobody_holds(0) && !nobody_holds(-1), "no process's number");
    assert!(!nobody_holds(std::process::id() as i32), "ours, running");
    assert!(
        !nobody_holds(a_number_another_user_holds()),
        "another user's process (EPERM)"
    );

    // A child that has exited and is not reaped: a zombie holds its number.
    let mut child = std::process::Command::new("/usr/bin/true")
        .spawn()
        .expect("start true");
    let pid = child.id() as i32;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while process(pid).is_some() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(!nobody_holds(pid), "a zombie holds its number");
    child.wait().expect("reap it");
    assert!(nobody_holds(pid), "nobody, once it is reaped");
}

#[test]
fn reads_of_the_marker_share_one_buffer_for_what_the_system_says_of_a_process() {
    // A report reads the marker of every candidate; each read made a buffer of a megabyte
    // before (Calliope's review T-421).
    let me = std::process::id() as i32;
    let before = BUFFERS_MADE.with(std::cell::Cell::get);
    for _ in 0..3 {
        assert!(!has_marker(me, "a marker no process carries"));
    }
    assert!(procargs(me).is_some(), "this process's own arguments");
    let made = BUFFERS_MADE.with(std::cell::Cell::get) - before;
    assert!(made <= 1, "{made} buffers for four reads on one thread");
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
    let reading = || loop {
        if let Some(reading) = crate::background::Reading::of(clocks()) {
            break reading;
        }
    };
    let mut worst = 0;
    for _ in 0..50 {
        let before = reading();
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("5")
            .spawn()
            .expect("sleep starts");
        let pid = child.id();
        let after = reading();
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

#[test]
fn the_clocks_are_read_well_within_the_bound_on_their_reads() {
    // T-349: how far apart a read's two monotonic reads lie, which `Reading::of` bounds.
    let mut widths: Vec<i64> = (0..20_000)
        .map(|_| {
            let read = clocks();
            (read.awake.1 - read.awake.0).max(read.continuous.1 - read.continuous.0)
        })
        .collect();
    widths.sort_unstable();
    let at = |share: f64| widths[((widths.len() - 1) as f64 * share) as usize];
    println!(
        "a read's monotonic reads apart, µs: p50 {} p99 {} p99.9 {} max {}",
        at(0.5),
        at(0.99),
        at(0.999),
        at(1.0)
    );
    assert!(at(0.99) < 1_000, "most reads within a millisecond");
}

#[test]
fn a_process_is_another_users_only_where_its_owner_says_so() {
    // Astraeus on eb9f25ef: a child of ours that passed the first read and ended before the
    // whole read was said another user's.
    let mine = 501;
    assert_eq!(
        looked_of(Ok(mine), mine, || false, || true),
        Looked::Ended,
        "ours, ended between the two reads"
    );
    assert_eq!(
        looked_of(Ok(0), mine, || false, || false),
        Looked::Unreadable,
        "root's"
    );
    assert_eq!(
        looked_of(Ok(mine), mine, || true, || false),
        Looked::Readable
    );
    assert_eq!(
        looked_of(Ok(mine), mine, || false, || false),
        Looked::Changed,
        "ours, read differently the second time"
    );
    assert_eq!(
        looked_of(Err(Some(libc::ESRCH)), mine, || true, || true),
        Looked::Ended
    );
    assert_eq!(
        looked_of(Err(Some(libc::EPERM)), mine, || true, || false),
        Looked::Unreadable,
        "the system will not read it to us"
    );
    assert_eq!(
        looked_of(Err(Some(libc::EIO)), mine, || true, || false),
        Looked::Changed,
        "no read said whose it is"
    );
}

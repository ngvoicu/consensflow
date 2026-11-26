use super::*;

/// What `KERN_PROCARGS2` answers for a process: the count, its path and the NULs that
/// pad the path (with its own NUL) to a multiple of 8, as the kernel does, the arguments
/// (an empty one is one NUL), the environment, and the NULs that end each.
fn procargs_for(path: &str, arguments: &[&str], environment: &[&str]) -> Vec<u8> {
    let mut bytes = i32::try_from(arguments.len())
        .expect("a count")
        .to_ne_bytes()
        .to_vec();
    bytes.extend_from_slice(path.as_bytes());
    bytes.resize(4 + (path.len() + 1).next_multiple_of(8), 0);
    for field in arguments.iter().chain(environment) {
        bytes.extend_from_slice(field.as_bytes());
        bytes.push(0);
    }
    bytes.push(0);
    bytes
}

/// [`procargs_for`] `sleep 31` started with `environment`.
fn procargs_of(environment: &[&str]) -> Vec<u8> {
    procargs_for("/bin/sleep", &["sleep", "31"], environment)
}

#[test]
fn a_command_line_is_the_arguments_the_process_was_started_with_and_not_its_environment() {
    let bytes = procargs_of(&["PATH=/usr/bin", "HOME=/Users/zeus"]);
    assert_eq!(command_line(&bytes).as_deref(), Some("sleep 31"));
    assert_eq!(command_line(&procargs_of(&[])).as_deref(), Some("sleep 31"));
}

#[test]
fn an_empty_argument_is_an_argument_and_the_environment_never_gets_into_a_command_line() {
    // An empty first argument is one NUL after the path's padding; it was skipped as more
    // padding, and the first variable of the environment was the last argument.
    let marker = format!("{OWNER_ENV}=abc123");
    let canary = "CANARY_TOKEN=sk-live-0123456789";
    for path in [
        "/bin/sleep",
        "/bin/sh",
        "/usr/bin/yes",
        "/p",
        "/a/path/of/fifteen",
    ] {
        let bytes = procargs_for(path, &["", "31"], &[canary, &marker]);
        let line = command_line(&bytes).expect("a command line");
        assert_eq!(line, " 31", "{path}");
        assert!(!line.contains("CANARY"), "{path}: {line}");
        assert!(environment_has_marker(&bytes, "abc123"), "{path}: marked");
        // Two empty arguments, and only empty ones.
        let bytes = procargs_for(path, &["", ""], &[canary, &marker]);
        assert_eq!(command_line(&bytes).as_deref(), Some(" "), "{path}");
        assert!(environment_has_marker(&bytes, "abc123"), "{path}: marked");
        let bytes = procargs_for(path, &[""], &[canary]);
        assert_eq!(command_line(&bytes).as_deref(), Some(""), "{path}");
        // An empty argument last, before the environment.
        let bytes = procargs_for(path, &["sleep", ""], &[canary, &marker]);
        assert_eq!(command_line(&bytes).as_deref(), Some("sleep "), "{path}");
        assert!(environment_has_marker(&bytes, "abc123"), "{path}: marked");
    }
    // Fewer arguments in the buffer than the count says: nothing is made of what is there.
    let mut short = procargs_for("/bin/sleep", &["sleep"], &[]);
    short[..4].copy_from_slice(&3i32.to_ne_bytes());
    assert_eq!(command_line(&short), None);
}

/// A child of the test's own, kept by its handle, started with an empty `argv[0]` and the
/// environment `environment` alone, and ended with the test.
struct Child(std::process::Child);

impl Child {
    #[allow(clippy::disallowed_methods)] // A test's own program, ended with the test.
    fn started(program: &str, arguments: &[&str], environment: &[(&str, &str)]) -> Child {
        use std::os::unix::process::CommandExt;
        let mut command = std::process::Command::new(program);
        command
            .arg0("")
            .args(arguments)
            .env_clear()
            .envs(environment.iter().copied())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped());
        Child(command.spawn().expect("the program starts"))
    }

    fn pid(&self) -> i32 {
        self.0.id() as i32
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

const HELPER: &str = "CF_PROCARGS_TEST_HELPER";
const READY: &str = "cf-procargs-helper-ready";

/// What [`a_child_started_with_an_empty_first_argument_shows_no_environment_in_its_command_line`]
/// starts again (the test's own executable, which the system does not restrict, so it shows
/// the environment): it says it is ready, then waits for its input to end, which the
/// test's end brings.
#[test]
#[allow(clippy::disallowed_methods)] // A test's own helper, told what it is by its start.
fn the_helper_waits_for_its_parent() {
    if std::env::var_os(HELPER).is_some() {
        println!("{READY}");
        let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut Vec::new());
    }
}

#[test]
fn a_child_started_with_an_empty_first_argument_shows_no_environment_in_its_command_line() {
    let canary = "CF_CANARY_SECRET=do-not-show-0123456789";
    let canary_pair = ("CF_CANARY_SECRET", "do-not-show-0123456789");
    // The test's own executable: the environment is the child's to show.
    let exe = std::env::current_exe().expect("this executable");
    // The name libtest filters by has no crate in front.
    let module = module_path!();
    let test = format!(
        "{}::the_helper_waits_for_its_parent",
        module.split_once("::").expect("a crate and a module").1
    );
    let arguments = ["--exact", test.as_str(), "--nocapture"];
    let child = Child::started(
        exe.to_str().expect("a path"),
        &arguments,
        &[canary_pair, (HELPER, "1"), (OWNER_ENV, "native42")],
    );
    // Read nothing before the child says it is in place: its own line, not a wait of time.
    let mut child = child;
    let output = child.0.stdout.take().expect("its output");
    let said = std::io::BufRead::lines(std::io::BufReader::new(output))
        .map_while(Result::ok)
        .any(|line| line == READY);
    assert!(said, "the helper ran its test and said it was ready");
    let bytes = procargs(child.pid()).expect("its arguments");
    assert!(
        bytes.windows(canary.len()).any(|w| w == canary.as_bytes()),
        "the canary is in its environment, as the test made it"
    );
    let line = command_line(&bytes).expect("its command line");
    assert_eq!(line, format!(" {}", arguments.join(" ")));
    assert!(has_marker(child.pid(), "native42"), "its mark is found");
    assert!(!has_marker(child.pid(), "native43"), "and only its own");

    // A restricted program (the system's own): the system withholds the environment, and
    // its own cut of the arguments then ends one field late, with a variable.
    let sleep = Child::started("/bin/sleep", &["30"], &[canary_pair]);
    let line = procargs(sleep.pid()).and_then(|bytes| command_line(&bytes));
    assert_eq!(line.as_deref(), Some(" 30"), "its command line");
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
    let typed = procargs_for("/bin/echo", &["echo", &marker], &[]);
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

// A test's own program, bare: it is ended within milliseconds.
#[allow(clippy::disallowed_methods)]
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

// A test's own programs, bare: the clocks are read around a start, and a start that closes the
// descriptors first is another start (`what_a_start_by_fork_costs`, cf-process).
#[allow(clippy::disallowed_methods)]
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

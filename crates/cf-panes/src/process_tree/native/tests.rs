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

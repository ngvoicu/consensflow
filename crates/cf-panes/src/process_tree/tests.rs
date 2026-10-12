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

/// A Python that starts a Python that sleeps, says its number, and waits to be
/// told to go.
#[cfg(target_os = "macos")]
const PARENT: &str = "import subprocess, sys; \
    child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(41)']); \
    print(child.pid, flush=True); input()";

#[cfg(target_os = "macos")]
#[allow(clippy::disallowed_methods)] // A test's own programs.
#[test]
fn a_process_has_the_process_that_started_it_as_its_parent_until_that_has_gone() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let mut tree = ProcessTree::new();
    let mut parent = Command::new("/usr/bin/python3")
        .args(["-c", PARENT])
        .env(OWNER_ENV, &tree.marker)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start the parent");
    tree.attach(i32::try_from(parent.id()).expect("a process number"));
    let mut line = String::new();
    BufReader::new(parent.stdout.take().expect("its output"))
        .read_line(&mut line)
        .expect("the child's number");
    let child: u32 = line.trim().parse().expect("a process number");

    let members = tree.members().expect("look at the tree");
    let root = members.iter().find(|m| m.key.pid == parent.id());
    let started = members.iter().find(|m| m.key.pid == child);
    assert_eq!(
        started.and_then(|member| member.parent),
        root.map(|member| member.key),
        "its parent is the process that started it: {members:?}"
    );

    // The parent goes. The child is still the pane's, found by the marker its
    // environment carries; the parent it names is no process of the tree, so
    // it names none, whether the system names it still or names launchd.
    parent
        .stdin
        .take()
        .expect("its input")
        .write_all(b"\n")
        .expect("tell it to go");
    parent.wait().expect("the parent ends");
    let members = tree.members().expect("look at the tree");
    let orphan = members.iter().find(|m| m.key.pid == child);
    // SAFETY: getpgid takes a process number and touches no memory of ours.
    let group = unsafe { libc::getpgid(i32::try_from(child).expect("a process number")) };
    assert_eq!(
        orphan.map(|member| (member.parent, member.group)),
        Some((None, Some(group))),
        "{members:?}"
    );
    // SAFETY: kill takes two integers and touches no memory of ours.
    unsafe {
        libc::kill(
            i32::try_from(child).expect("a process number"),
            libc::SIGKILL,
        )
    };
}

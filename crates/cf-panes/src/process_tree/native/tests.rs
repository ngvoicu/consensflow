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

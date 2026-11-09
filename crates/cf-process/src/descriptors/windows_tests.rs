//! A program's first code closes the handles it was left: on Windows a process created with
//! inheritance on (std's `Command` does) is given every handle of its parent's that is marked so,
//! at the value it has there, and keeps it for as long as it runs, as macOS does with a descriptor
//! that is not close-on-exec (T-1338).

use std::fs::File;
use std::os::windows::io::AsRawHandle;

use windows_sys::Win32::Foundation::{
    GetHandleInformation, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT,
};

/// The value of the handle the parent leaves, which the child it starts is given in its
/// environment.
const LEFT: &str = "CF_PROCESS_LEFT_HANDLE";

/// The flags of the handle `value`, or none where it is no handle of this process's.
fn flags_of(value: usize) -> Option<u32> {
    let mut flags = 0_u32;
    // SAFETY: asks about a value, and is answered 0 for one that is no handle.
    #[allow(unsafe_code)]
    let is_one = unsafe { GetHandleInformation(value as HANDLE, &mut flags) } != 0;
    is_one.then_some(flags)
}

/// A child that std starts is given the handle its parent marked inheritable, and closes it as
/// its first code, bar its standard streams and the handles it opened itself.
#[test]
#[allow(clippy::disallowed_methods)] // std's start, bare: the start that leaves what it may.
fn what_the_process_was_left_is_closed_and_what_is_its_own_is_not() {
    let file = File::open(std::env::current_exe().unwrap()).unwrap();
    let handle = file.as_raw_handle();
    // SAFETY: a handle of this process's own, marked so a child is given it.
    #[allow(unsafe_code)]
    let marked =
        unsafe { SetHandleInformation(handle as HANDLE, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) };
    assert_ne!(marked, 0, "the handle was not marked inheritable");
    let ran = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "descriptors::windows_tests::left",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(LEFT, (handle as usize).to_string())
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr)
    );
    assert!(ran.status.success(), "{said}");
    assert!(said.contains("1 passed"), "the test did not run: {said}");
    assert!(
        said.contains("the child ran to its end"),
        "the test's body did not run: {said}"
    );
}

#[test]
#[ignore = "run by the test above it, in a process of its own"]
#[allow(clippy::print_stdout)] // To the run that started this one, which reads it.
#[allow(clippy::disallowed_methods)] // The run that started this one hands it the handle by its environment.
fn left() {
    let Some(left) = std::env::var(LEFT)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
    else {
        return;
    };
    // The handle was given at the value it has in the parent, and is inheritable here too.
    assert!(
        flags_of(left).is_some_and(|flags| flags & HANDLE_FLAG_INHERIT != 0),
        "the child was not left the handle {left}: the check proves nothing"
    );
    // The child's own: opened by it, and so not inheritable.
    let own = File::open(std::env::current_exe().unwrap()).unwrap();
    let own = own.as_raw_handle() as usize;

    let closed = crate::close_inherited().unwrap();

    assert!(
        closed.contains(&(left as u64)),
        "the handle {left} was left and not closed: {closed:?}"
    );
    assert_eq!(flags_of(left), None, "the handle {left} is still open");
    assert!(
        flags_of(own).is_some(),
        "the handle {own} is the child's own, and was closed"
    );
    assert!(
        !closed.contains(&(own as u64)),
        "the handle {own} is the child's own, and was said to be left"
    );
    // Its standard streams are what it was started with, and stay.
    for which in [
        windows_sys::Win32::System::Console::STD_OUTPUT_HANDLE,
        windows_sys::Win32::System::Console::STD_ERROR_HANDLE,
    ] {
        // SAFETY: asks for this process's own standard handle.
        #[allow(unsafe_code)]
        let stream = unsafe { windows_sys::Win32::System::Console::GetStdHandle(which) } as usize;
        assert!(flags_of(stream).is_some(), "{which} was closed");
    }
    println!("the child ran to its end");
}

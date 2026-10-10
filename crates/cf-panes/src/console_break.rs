//! Ctrl+Break to a pane's console: the way a window's programs are asked to end
//! on Windows, where there is no TERM to send. A program that handles the Break
//! (Node's `SIGBREAK`, Python's, a console handler of its own) can release what
//! it holds; one that does not is ended by it, as by the default handler.
//!
//! `GenerateConsoleCtrlEvent` reaches the processes that share the console of
//! the process that calls it, and the host is not attached to a pane's console
//! (a pseudoconsole of the pane's own). So the host attaches to it for the
//! moment the event takes (`AttachConsole` to the pane's program, which is on
//! it), sends the Break to every process on that console, and lets go
//! (`FreeConsole`). The attachment is the process's, not the thread's, so one
//! close does it at a time. The Break reaches the host too, which shares the
//! console for that moment, and it has a handler that says it was handled:
//! a host that ended by it would be the pane's last close.

use std::io;
use std::sync::{Mutex, Once, PoisonError};

use windows_sys::core::BOOL;
use windows_sys::Win32::System::Console::{
    AttachConsole, FreeConsole, GenerateConsoleCtrlEvent, SetConsoleCtrlHandler, CTRL_BREAK_EVENT,
};

/// One attachment at a time: a process has one console.
static ATTACHED: Mutex<()> = Mutex::new(());
static HANDLER: Once = Once::new();

/// The host's own answer to a Break: handled. Every other event keeps its
/// default, which is what the host did with it before.
unsafe extern "system" fn ignore_break(event: u32) -> BOOL {
    BOOL::from(event == CTRL_BREAK_EVENT)
}

/// Sends Ctrl+Break to every process on the console of `program`, the pane's
/// own program, which started the console's other processes or is on it with
/// them. An error where the console could not be reached: the program has ended
/// already, or is not on one.
pub(crate) fn send(program: u32) -> io::Result<()> {
    let _one_at_a_time = ATTACHED.lock().unwrap_or_else(PoisonError::into_inner);
    HANDLER.call_once(|| {
        // SAFETY: the handler is a function of this file that touches nothing.
        unsafe { SetConsoleCtrlHandler(Some(ignore_break), 1) };
    });
    // SAFETY: a process with a console of its own cannot attach to another's;
    // the host has no console it needs (a window's app has none, and the helper's
    // input and output are pipes).
    unsafe { FreeConsole() };
    // SAFETY: attaches this process to the console of `program`; undone below.
    if unsafe { AttachConsole(program) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: group 0 is every process on the console this process is on.
    let sent = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, 0) };
    let error = io::Error::last_os_error();
    // SAFETY: lets go of the pane's console, which this process was attached to above.
    unsafe { FreeConsole() };
    if sent == 0 {
        return Err(error);
    }
    Ok(())
}

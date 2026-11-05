//! The rig's Muse on Windows: the machine's own Muse, run on its fake model (`--provider
//! echo`), as the Mac's stand-in script runs it (`cf_e2e::muse::write_stand_in`). The rig
//! copies this program beside a `.muse-version` as the `muse-bin-<version>.exe` the
//! product runs, with what it runs in the file of its own name and `.json`
//! ([`cf_e2e::muse::StandIn`]): it writes down every command line it is given, whole, and
//! runs the machine's binary with them, on echo with each turn the rig's length long and
//! without the model and effort echo refuses, or live with the run's own folders.
//!
//! Windows has no `exec`: Muse runs as this program's child, in the window's job, its
//! console and its standard handles this one's. Its session's lock names Muse's process,
//! one under the window's (`muse::paths`); this program ends with Muse's status.

use std::io::{self, Write};
use std::process::{Command, ExitCode};

use cf_e2e::muse::StandIn;

#[allow(clippy::disallowed_methods)] // Muse is this program's one child, run to its end.
fn main() -> ExitCode {
    let words: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|word| word.to_string_lossy().into_owned())
        .collect();
    let stand_in = match StandIn::beside_this_program() {
        Ok(stand_in) => stand_in,
        Err(failed) => {
            let _ = writeln!(io::stderr(), "the rig's Muse: {failed}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(failed) = stand_in.note(&words) {
        let _ = writeln!(
            io::stderr(),
            "the rig's Muse could not write its command line down: {failed}"
        );
        return ExitCode::FAILURE;
    }
    ignore_control_c();
    let args = stand_in.args(&words);
    // A program the rig is given, held to the apps a test may start, as the rig's Run is.
    if let Err(refused) = cf_base::bundle::command_startable_by_a_test(&stand_in.binary, &args) {
        let _ = writeln!(io::stderr(), "the rig's Muse: {refused}");
        return ExitCode::FAILURE;
    }
    let mut muse = Command::new(&stand_in.binary);
    muse.args(args);
    for (name, value) in stand_in.env() {
        muse.env(name, value);
    }
    match muse.spawn() {
        Ok(mut child) => {
            let _ = stand_in.note_pids(std::process::id(), child.id());
            child
                .wait()
                .ok()
                .and_then(|status| status.code())
                .and_then(|code| u8::try_from(code).ok())
                .map_or(ExitCode::FAILURE, ExitCode::from)
        }
        Err(failed) => {
            let _ = writeln!(
                io::stderr(),
                "the rig's Muse could not start {}: {failed}",
                stand_in.binary.display()
            );
            ExitCode::FAILURE
        }
    }
}

/// A Control-C the console sends its processes is Muse's to take, never this program's
/// end: a window whose program ended is closed, and Muse with it.
#[cfg(windows)]
fn ignore_control_c() {
    use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
    // SAFETY: no handler, and TRUE: this process ignores Control-C, as a child it starts
    // then does until it sets its own; nothing of Rust's memory is touched.
    unsafe {
        SetConsoleCtrlHandler(None, 1);
    }
}

#[cfg(not(windows))]
fn ignore_control_c() {}

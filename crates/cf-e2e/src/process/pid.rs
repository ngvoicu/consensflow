//! A process seen by its id alone: whether it is there, and a signal sent to
//! it. The cases that look for what a window left behind (a Codex server that
//! must have gone with its window, a stand-in harness that must be gone once the
//! pane host is) have only the id the stand-in wrote down.
//!
//! On Unix the system call, through `nix` and no unsafe code of ours. Windows
//! has neither signals nor a call std offers, so `taskkill` and `tasklist` do:
//! a signal there ends the process, which is what Node's `process.kill` did.

use std::io;

#[cfg(windows)]
use super::Run;

/// A signal a case sends. Windows has no `Interrupt`, and its `Terminate` is a
/// kill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// SIGTERM: the polite end, which a window's supervisor answers by ending
    /// the programs it started.
    Terminate,
    /// SIGINT: the interrupt a terminal sends.
    Interrupt,
    /// SIGKILL: the end nothing answers to.
    Kill,
}

/// Sends `signal` to the process `pid`.
#[cfg(unix)]
pub fn signal(pid: u32, signal: Signal) -> io::Result<()> {
    use nix::sys::signal::{kill, Signal as Sent};

    let sent = match signal {
        Signal::Terminate => Sent::SIGTERM,
        Signal::Interrupt => Sent::SIGINT,
        Signal::Kill => Sent::SIGKILL,
    };
    kill(as_pid(pid)?, sent).map_err(io::Error::from)
}

/// Whether the process `pid` is there to be signalled: the answer of `kill`
/// with no signal, which counts a process this user may not signal as gone, as
/// Node's `process.kill(pid, 0)` did.
#[cfg(unix)]
pub fn is_alive(pid: u32) -> bool {
    as_pid(pid).is_ok_and(|pid| nix::sys::signal::kill(pid, None).is_ok())
}

#[cfg(unix)]
fn as_pid(pid: u32) -> io::Result<nix::unistd::Pid> {
    i32::try_from(pid)
        .map(nix::unistd::Pid::from_raw)
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{pid} is no process id"),
            )
        })
}

/// Ends the process `pid`: Windows has no signal to send, and `taskkill /F`
/// is what ends a process there.
#[cfg(windows)]
pub fn signal(pid: u32, signal: Signal) -> io::Result<()> {
    if signal == Signal::Interrupt {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows has no SIGINT to send",
        ));
    }
    let ran = Run::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .run()
        .map_err(|cause| io::Error::other(cause.to_string()))?;
    if ran.code == Some(0) {
        Ok(())
    } else {
        Err(io::Error::other(format!("taskkill {pid}: {ran}")))
    }
}

/// Whether the process `pid` is there: `tasklist` lists it, in quotes, when it
/// is.
#[cfg(windows)]
pub fn is_alive(pid: u32) -> bool {
    Run::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .run()
        .is_ok_and(|ran| ran.stdout.contains(&format!("\"{pid}\"")))
}

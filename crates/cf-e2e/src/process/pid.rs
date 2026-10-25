//! A process seen by its id alone: whether it is there, when it began, and a signal
//! sent to it. The cases that look for what a window left behind (a Codex server that
//! must have gone with its window, a stand-in harness that must be gone once the
//! pane host is) have only the id the stand-in wrote down, and one that holds a
//! process to be the one it was (a harness's server that must have lived through an
//! ending) holds it to when it began as well.
//!
//! On Unix the system call, through `nix` and no unsafe code of ours. Windows
//! has neither signals nor a call std offers, so `taskkill` and `tasklist` do:
//! a signal there ends the process, which is what Node's `process.kill` did.

use std::io;

use super::Run;

/// A signal a case sends. Windows has no `Interrupt`, `Hangup`, `Stop` or `Continue`,
/// and its `Terminate` is a kill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// SIGTERM: the polite end, which a window's supervisor answers by ending
    /// the programs it started.
    Terminate,
    /// SIGINT: the interrupt a terminal sends.
    Interrupt,
    /// SIGHUP: a terminal hung up, which the pane host sends right after SIGTERM
    /// when it closes a window.
    Hangup,
    /// SIGKILL: the end nothing answers to.
    Kill,
    /// SIGSTOP: the program is stopped where it is, and runs no more until it is
    /// continued. What is sent to it meanwhile waits in its pipes.
    Stop,
    /// SIGCONT: a program that was stopped goes on.
    Continue,
}

/// Sends `signal` to the process `pid`.
#[cfg(unix)]
pub fn signal(pid: u32, signal: Signal) -> io::Result<()> {
    use nix::sys::signal::{kill, Signal as Sent};

    let sent = match signal {
        Signal::Terminate => Sent::SIGTERM,
        Signal::Interrupt => Sent::SIGINT,
        Signal::Hangup => Sent::SIGHUP,
        Signal::Kill => Sent::SIGKILL,
        Signal::Stop => Sent::SIGSTOP,
        Signal::Continue => Sent::SIGCONT,
    };
    kill(as_pid(pid)?, sent).map_err(io::Error::from)
}

/// Stops the process `pid` where it is and lets it go on after `span`: what a machine
/// too loaded to run it does for a moment. A request made of it meanwhile is answered
/// when it goes on, as late as the stop was long. It is stopped when this returns.
#[cfg(unix)]
pub fn stall(pid: u32, span: std::time::Duration) -> io::Result<()> {
    signal(pid, Signal::Stop)?;
    std::thread::spawn(move || {
        std::thread::sleep(span);
        // A process that is gone by then is the end of its case, not a failure of this.
        let _ = signal(pid, Signal::Continue);
    });
    Ok(())
}

/// Whether the process `pid` is there to be signalled: the answer of `kill`
/// with no signal, which counts a process this user may not signal as gone, as
/// Node's `process.kill(pid, 0)` did.
#[cfg(unix)]
pub fn is_alive(pid: u32) -> bool {
    as_pid(pid).is_ok_and(|pid| nix::sys::signal::kill(pid, None).is_ok())
}

/// The process group the process `pid` is in, or none where it is gone or has no
/// group to tell: for a case that holds a stand-in to the group it was told to be
/// in (a server in its window's, a browser in a session of its own).
#[cfg(unix)]
pub fn group_of(pid: u32) -> Option<u32> {
    let group = nix::unistd::getpgid(Some(as_pid(pid).ok()?)).ok()?;
    u32::try_from(group.as_raw()).ok()
}

/// When the process `pid` began, in the words the system says it in: the same for the
/// same process, and others for one that took its number after it. None where it is
/// not there or the system will not say. A case that holds a process by its id holds it
/// to this too: a number is no process.
#[cfg(unix)]
pub fn started(pid: u32) -> Option<String> {
    let ran = Run::new("ps")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .inheriting_env()
        .run()
        .ok()?;
    let said = ran.stdout.trim();
    (ran.code == Some(0) && !said.is_empty()).then(|| said.to_owned())
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
    if matches!(
        signal,
        Signal::Interrupt | Signal::Hangup | Signal::Stop | Signal::Continue
    ) {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("Windows has no {signal:?} to send"),
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

/// The ones of `pids` that are there.
#[cfg(unix)]
pub fn alive_among(pids: &[u32]) -> Vec<u32> {
    pids.iter().copied().filter(|&pid| is_alive(pid)).collect()
}

/// The ones of `pids` that are there, by one look at the system's processes rather than
/// one for each (`tasklist` says a process's id second in each line, in quotes). All of
/// them where the system will not say: a case that asks for what is gone must not take
/// silence for it.
#[cfg(windows)]
pub fn alive_among(pids: &[u32]) -> Vec<u32> {
    let Ok(ran) = Run::new("tasklist").args(["/NH", "/FO", "CSV"]).run() else {
        return pids.to_vec();
    };
    let there: std::collections::HashSet<u32> = ran
        .stdout
        .lines()
        .filter_map(|line| line.split("\",\"").nth(1)?.parse().ok())
        .collect();
    // The system's idle process is always listed: a list without it was not read.
    if !there.contains(&0) {
        return pids.to_vec();
    }
    pids.iter()
        .copied()
        .filter(|pid| there.contains(pid))
        .collect()
}

/// Ends every one of `pids` that is there, each by itself and none of what it started:
/// a case's guard against what it started and a failed case would leave running.
#[cfg(unix)]
pub fn end_all(pids: &[u32]) {
    for &pid in pids {
        let _ = signal(pid, Signal::Kill);
    }
}

/// Ends every one of `pids` that is there, each by itself and none of what it started,
/// by one `taskkill`.
#[cfg(windows)]
pub fn end_all(pids: &[u32]) {
    if pids.is_empty() {
        return;
    }
    let mut taskkill = Run::new("taskkill").arg("/F");
    for pid in pids {
        taskkill = taskkill.args(["/PID", &pid.to_string()]);
    }
    let _ = taskkill.run();
}

/// How long the PowerShell that says when a process began is given: well past what
/// it takes on a loaded runner.
#[cfg(windows)]
const STARTED_LIMIT: std::time::Duration = std::time::Duration::from_secs(120);

/// When the process `pid` began, in the words the system says it in (the 100 ns
/// since 1601 that PowerShell's `StartTime` is): the same for the same process, and
/// others for one that took its number after it. None where it is not there or the
/// system will not say. A PowerShell takes its time to start: 11 to 17 s on GitHub's
/// Windows runner (2026-10-15), and more under a suite's load, past the limit a
/// program is given by default, which ended it, and a process that ran was said
/// to be none ([`STARTED_LIMIT`]).
#[cfg(windows)]
pub fn started(pid: u32) -> Option<String> {
    // On GitHub's runner a PowerShell asked of a process that ran said nothing once,
    // and the next one asked answered (2026-10-15): one that says nothing is asked
    // again while the process is there.
    for _ in 0..STARTED_TRIES {
        if let Some(said) = asked_when_started(pid) {
            return Some(said);
        }
        if !is_alive(pid) {
            return None;
        }
    }
    None
}

/// How many PowerShells [`started`] asks of a process that is there.
#[cfg(windows)]
const STARTED_TRIES: usize = 3;

/// What one PowerShell says of when the process `pid` began, or none.
#[cfg(windows)]
fn asked_when_started(pid: u32) -> Option<String> {
    let script = format!("(Get-Process -Id {pid} -ErrorAction Stop).StartTime.ToFileTimeUtc()");
    let ran = Run::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .finding_programs()
        .limit(STARTED_LIMIT)
        .run()
        .ok()?;
    let said = ran.stdout.trim();
    (ran.code == Some(0) && !said.is_empty()).then(|| said.to_owned())
}

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

/// What the process `pid` is, in the words of `ps`: its state (`Z` is a process that has
/// ended and is not yet reaped), its parent, how long it has run and what it runs. None
/// where it is not there or the system will not say. For an error that names a process
/// that was not to be there, so that the next one says what it was.
#[cfg(unix)]
pub fn described(pid: u32) -> Option<String> {
    let ran = Run::new("ps")
        .args(["-o", "stat=,ppid=,etime=,command=", "-p", &pid.to_string()])
        .inheriting_env()
        .run()
        .ok()?;
    let said = ran.stdout.trim();
    (ran.code == Some(0) && !said.is_empty()).then(|| {
        let mut rest = said;
        let mut word = || {
            rest = rest.trim_start();
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let (word, tail) = rest.split_at(end);
            rest = tail;
            word
        };
        let (state, parent, ran_for) = (word(), word(), word());
        let command = rest.trim_start();
        format!("state {state}, parent {parent}, running for {ran_for}, command {command}")
    })
}

/// How long the process `pid` has run, in whole seconds: the `etime` of `ps`. None where
/// it is not there or the system will not say. A process that has run for less than
/// another is one that began after it.
#[cfg(unix)]
pub fn age(pid: u32) -> Option<std::time::Duration> {
    let ran = Run::new("ps")
        .args(["-o", "etime=", "-p", &pid.to_string()])
        .inheriting_env()
        .run()
        .ok()?;
    (ran.code == Some(0))
        .then(|| elapsed(ran.stdout.trim()))
        .flatten()
}

/// The time `ps` says in `[[dd-]hh:]mm:ss`.
#[cfg(unix)]
fn elapsed(said: &str) -> Option<std::time::Duration> {
    let (days, clock) = said.split_once('-').map_or((0, said), |(days, clock)| {
        (days.parse().unwrap_or(0), clock)
    });
    let mut seconds = 0_u64;
    for part in clock.split(':') {
        seconds = seconds * 60 + part.parse::<u64>().ok()?;
    }
    Some(std::time::Duration::from_secs(days * 86_400 + seconds))
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

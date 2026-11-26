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

/// The command line of the process `pid`, as `ps` says it: none where there is no such
/// process or `ps` says nothing of it.
#[cfg(unix)]
pub fn command(pid: u32) -> Option<String> {
    let ran = Run::new("ps")
        .args(["-o", "command=", "-p", &pid.to_string()])
        .run()
        .ok()?;
    let said = ran.stdout.trim();
    (ran.code == Some(0) && !said.is_empty()).then(|| said.to_owned())
}

/// How long the process `pid` has run, in whole seconds: the `etime` of `ps`. None where
/// it is not there or the system will not say. A process that has run for less than
/// another is one that began after it.
#[cfg(unix)]
pub fn age(pid: u32) -> Option<std::time::Duration> {
    let ran = Run::new("ps")
        .args(["-o", "etime=", "-p", &pid.to_string()])
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

/// When the process `pid` began, in the words the system says it in (the 100 ns since 1601 that
/// a file time of its creation is): the same for the same process, and others for one that took
/// its number after it. None where it is not there or the system will not say. Read on a handle
/// to the process, with no program started: a PowerShell took 20 to 30 s to start on a machine
/// with many modules (T-2363), and one that said nothing made a process that ran be said to be
/// none.
#[cfg(windows)]
pub fn started(pid: u32) -> Option<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SYNCHRONIZE,
    };

    #[allow(unsafe_code)]
    // SAFETY: OpenProcess takes plain values and returns a handle or null.
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    if handle.is_null() {
        return None;
    }
    let (mut created, mut ended, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    #[allow(unsafe_code)]
    // SAFETY: the handle is open with the rights to wait on it and read its times, the four
    // structures are writable, and the handle is closed once, here.
    let said = unsafe {
        // A process that has exited keeps its number only while someone holds it: it is no
        // process that is there.
        let there = WaitForSingleObject(handle, 0) != WAIT_OBJECT_0;
        let read = GetProcessTimes(handle, &mut created, &mut ended, &mut kernel, &mut user);
        CloseHandle(handle);
        (there && read != 0)
            .then(|| (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    };
    said.map(|time| time.to_string())
}

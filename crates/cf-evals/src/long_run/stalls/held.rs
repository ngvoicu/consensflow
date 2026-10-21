//! What the system says of a stalled window's processes, for the evidence of a stall: the
//! three Pi windows that froze in the 2-hour runs of 2026-10-19 and 2026-10-20 (twice on
//! zeewin mid-stream, once on the Mac mid-bash with Pi's own tool timeout never firing)
//! left no process to look at once the runs had ended. What tells a program that hung
//! by itself from one that waits in a write to a terminal its pane host stopped reading
//! is the state of each of its processes, the call its program waits in, and how much it
//! wrote that the pane host has not read.
//!
//! Taken of the window's tree (the program the pane host opened it on, what the host
//! named its tree runs, and every process under either), each time by the system, as it
//! is then:
//! - **macOS:** each process's state (`ps`'s `stat`: `T` stopped, `S` asleep, `R` at
//!   work, `U` in an uninterruptible wait); `ps` names no call a process waits in there
//!   (its `wchan` is always `-`), so a `sample` of the program, kept in a file beside the
//!   report, says where each of its threads is; and the terminal's unread bytes, asked of
//!   the program's terminal ([`cf_e2e::terminal::unread`]). Where the program wrote bytes
//!   the pane host has not read, or is gone while its window still reads working (the pane
//!   host's own proof that a program went), a sample of the pane host (the process that
//!   holds the windows' terminals and reads them, `cf-panes`' `output_end`; the one of the
//!   look, a restart of the app starting another), once for all the windows a look takes,
//!   says why: its reader thread on the pane table's lock, gone, or in the middle of a read.
//!   In a sample of a second, a program blocked in its write shows the `write` leaf on its
//!   main thread at the full count of samples; an idle Node program, `kevent` under
//!   `uv__io_poll`.
//! - **Linux:** each process's state and the kernel call it waits in (`wchan`); no sample,
//!   and no unread bytes (its terminal keeps no count of them on the program's end).
//! - **Windows:** each process, with its threads' states and what each waits for, named
//!   by the kernel's `KWAIT_REASON` with the number beside (`Get-CimInstance
//!   Win32_Thread`): a thread in a write nobody reads waits in `Executive`, one of a
//!   process stopped waits as `Suspended`. Its output goes through its console host into
//!   a pipe only the pane host holds: what is unread there cannot be read outside it.
//!
//! It is evidence only: it never changes a verdict, and never stops, signals or types into
//! a window. A `sample` suspends the process at each 1 ms interval to read its threads'
//! stacks and resumes it (its manual's words): an observation of a second, not a stop. The
//! terminal is opened and closed in one call, never read or written. The system's list of
//! every process is asked once for all the windows a look takes, as on Windows it is a
//! PowerShell over every thread of the machine, tens of seconds under load.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::time::Duration;

use cf_e2e::process::Run;

/// What Windows says in place of the terminal's unread bytes.
pub const WINDOWS_UNREAD: &str = "Windows: the program's output goes through its console host into a pipe only the pane host holds, so what it has not read cannot be read outside it";

/// What Linux says in place of the terminal's unread bytes.
pub const LINUX_UNREAD: &str = "Linux: the program's end of its terminal keeps no count of what the pane host has not read; asked on macOS alone";

/// When the system was asked: as the stall was found, or at the run's end, the window
/// still stalled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    Found,
    End,
}

impl When {
    pub fn word(self) -> &'static str {
        match self {
            Self::Found => "found",
            Self::End => "end",
        }
    }
}

/// What the system said of a window's processes, as it said it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Taken {
    /// A line for each process of its tree, the program first; or why the system could
    /// not be asked.
    pub processes: Result<Vec<String>, String>,
    /// How many bytes its program wrote that the pane host has not read; or why that is
    /// not known.
    pub unread: Result<u64, String>,
    /// Where a sample of its program's threads was kept, or why none was; none where the
    /// system has no sampler.
    pub sample: Option<Result<PathBuf, String>>,
}

impl Taken {
    /// Nothing asked of the system, for `why`.
    pub fn none(why: &str) -> Self {
        Self {
            processes: Err(why.to_owned()),
            unread: Err(why.to_owned()),
            sample: None,
        }
    }
}

/// The system's list of every process, or why it could not be had.
pub type Listing = Result<Vec<Listed>, String>;

/// What asks the system: [`System`], or a test's.
pub trait Prober {
    /// The system's list of every process: asked once for all the windows a look takes.
    fn list(&mut self) -> Listing;

    /// What the system says now of the window's tree, as `listed` lists it: the program
    /// `pid` the pane host opened it on, the processes `others` the host named its tree
    /// runs, and every one under them; a sample of the program kept under `label`
    /// (`chief-1-found`).
    fn take(&mut self, listed: &Listing, pid: u32, others: &[u32], label: &str) -> Taken;

    /// A sample of the pane host, the process `host`, kept under `label`
    /// (`chief-1-found-host`); none where the system has no sampler.
    fn sample_host(&mut self, host: u32, label: &str) -> Option<Result<PathBuf, String>>;
}

/// A process as the system lists it: its number, its parent's, its terminal (where it
/// has one), and the line the evidence keeps of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub pid: u32,
    pub ppid: u32,
    pub tty: Option<String>,
    pub line: String,
}

/// What `ps -A -o pid=,ppid=,stat=,wchan=,tty=,command=` said: a process a line, its
/// line kept as the system said it, its spaces squeezed. A process with no terminal says
/// `??` (macOS) or `?` (Linux).
pub fn parse_ps(said: &str) -> Vec<Listed> {
    said.lines()
        .filter_map(|row| {
            let mut fields = row.split_whitespace();
            let pid = fields.next()?.parse().ok()?;
            let ppid = fields.next()?.parse().ok()?;
            let (stat, wchan, tty) = (fields.next()?, fields.next()?, fields.next()?);
            let command: Vec<&str> = fields.collect();
            Some(Listed {
                pid,
                ppid,
                tty: (!matches!(tty, "??" | "?" | "-")).then(|| tty.to_owned()),
                line: format!("{pid} {ppid} {stat} {wchan} {tty} {}", command.join(" ")),
            })
        })
        .collect()
}

/// The names Windows gives a thread's state (`ThreadState`).
const STATES: [&str; 8] = [
    "Initialized",
    "Ready",
    "Running",
    "Standby",
    "Terminated",
    "Waiting",
    "Transition",
    "DeferredReady",
];

/// The kernel's names of what a waiting thread waits for (`KWAIT_REASON`), which
/// `ThreadWaitReason` gives as a number (the CIM's own table of them differs).
const WAITS: [&str; 38] = [
    "Executive",
    "FreePage",
    "PageIn",
    "PoolAllocation",
    "DelayExecution",
    "Suspended",
    "UserRequest",
    "WrExecutive",
    "WrFreePage",
    "WrPageIn",
    "WrPoolAllocation",
    "WrDelayExecution",
    "WrSuspended",
    "WrUserRequest",
    "WrEventPair",
    "WrQueue",
    "WrLpcReceive",
    "WrLpcReply",
    "WrVirtualMemory",
    "WrPageOut",
    "WrRendezvous",
    "WrKeyedEvent",
    "WrTerminated",
    "WrProcessInSwap",
    "WrCpuRateControl",
    "WrCalloutStack",
    "WrKernel",
    "WrResource",
    "WrPushLock",
    "WrMutex",
    "WrQuantumEnd",
    "WrDispatchInt",
    "WrPreempted",
    "WrYieldExecution",
    "WrFastMutex",
    "WrGuardedMutex",
    "WrRundown",
    "WrAlertByThreadId",
];

/// A thread's state as the evidence says it: its name, and for a waiting one what it
/// waits for, by name and number (`Waiting/Executive(0)`).
pub fn thread_state(state: u32, wait: u32) -> String {
    let named = |names: &[&str], number: u32| {
        usize::try_from(number)
            .ok()
            .and_then(|at| names.get(at))
            .map_or_else(|| format!("#{number}"), |name| (*name).to_owned())
    };
    if state == 5 {
        format!("{}/{}({wait})", named(&STATES, state), named(&WAITS, wait))
    } else {
        named(&STATES, state)
    }
}

/// What the PowerShell of [`WINDOWS_SCRIPT`] said: `P <pid> <ppid> <command>` for each
/// process, `T <pid> <state> <wait>` for each thread. Each process's line says its threads,
/// counted by state, the most first, before its command.
pub fn parse_windows(said: &str) -> Vec<Listed> {
    let mut processes = Vec::new();
    let mut threads: BTreeMap<u32, BTreeMap<String, usize>> = BTreeMap::new();
    for row in said.lines() {
        let mut fields = row.split_whitespace();
        match fields.next() {
            Some("P") => {
                let (Some(pid), Some(ppid)) = (
                    fields.next().and_then(|pid| pid.parse::<u32>().ok()),
                    fields.next().and_then(|ppid| ppid.parse::<u32>().ok()),
                ) else {
                    continue;
                };
                let command: Vec<&str> = fields.collect();
                processes.push((pid, ppid, command.join(" ")));
            }
            Some("T") => {
                let numbers: Vec<u32> = fields.filter_map(|field| field.parse().ok()).collect();
                if let [pid, state, wait] = numbers[..] {
                    *threads
                        .entry(pid)
                        .or_default()
                        .entry(thread_state(state, wait))
                        .or_default() += 1;
                }
            }
            _ => {}
        }
    }
    processes
        .into_iter()
        .map(|(pid, ppid, command)| {
            let mut counted: Vec<(String, usize)> = threads
                .remove(&pid)
                .unwrap_or_default()
                .into_iter()
                .collect();
            counted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            let said: Vec<String> = counted
                .iter()
                .map(|(state, count)| format!("{state}×{count}"))
                .collect();
            Listed {
                pid,
                ppid,
                tty: None,
                line: format!("{pid} {ppid} threads: {} · {command}", said.join(", ")),
            }
        })
        .collect()
}

/// The processes of `listed` in the tree of `pid` and `others`: each of them the system
/// lists, and every process under one, the program first. A process whose parent ended
/// is under no one, so the pane host's word on what the tree runs is asked as well.
pub fn tree<'l>(listed: &'l [Listed], pid: u32, others: &[u32]) -> Vec<&'l Listed> {
    let by_pid: BTreeMap<u32, &Listed> = listed.iter().map(|one| (one.pid, one)).collect();
    let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for one in listed {
        children.entry(one.ppid).or_default().push(one.pid);
    }
    let mut seen = BTreeSet::new();
    let mut found = Vec::new();
    let mut next: VecDeque<u32> = std::iter::once(pid).chain(others.iter().copied()).collect();
    while let Some(at) = next.pop_front() {
        if !seen.insert(at) {
            continue;
        }
        if let Some(one) = by_pid.get(&at) {
            found.push(*one);
        }
        next.extend(children.get(&at).into_iter().flatten());
    }
    found
}

/// The terminal of the program `pid`, as `listed` says it; or why it has none.
pub fn terminal_of(listed: &[Listed], pid: u32) -> Result<&str, String> {
    let program = listed
        .iter()
        .find(|one| one.pid == pid)
        .ok_or_else(|| format!("the system lists no process {pid}"))?;
    program
        .tty
        .as_deref()
        .ok_or_else(|| "its program has no terminal".to_owned())
}

/// The PowerShell that says every process and every thread, a line each: what
/// [`parse_windows`] reads. One asking, as a PowerShell takes seconds to start.
pub const WINDOWS_SCRIPT: &str = "$ErrorActionPreference = 'Stop'; \
Get-CimInstance Win32_Process | ForEach-Object { $c = $_.CommandLine; if (-not $c) { $c = $_.Name }; 'P {0} {1} {2}' -f $_.ProcessId, $_.ParentProcessId, ($c -replace '\\s+', ' ') }; \
Get-CimInstance Win32_Thread | ForEach-Object { 'T {0} {1} {2}' -f $_.ProcessHandle, $_.ThreadState, $_.ThreadWaitReason }";

/// The system's word, as the run asks it: its list of processes, the terminal's unread
/// bytes, and on macOS a sample kept in the reports' folder.
pub struct System {
    /// Where a sample is kept, and the name it begins with: the report's.
    keep: Box<Keep>,
}

/// What says where a sample of `label` is kept, or why it cannot be.
type Keep = dyn FnMut(&str) -> Result<PathBuf, String>;

/// How a system is asked for its processes: the program, its words, and what reads its
/// answer.
type Lister = (
    &'static str,
    &'static [&'static str],
    fn(&str) -> Vec<Listed>,
);

impl System {
    /// The system's word, a sample of `label` kept where `keep` says (the run's edge
    /// makes the folder and knows the file).
    pub fn new(keep: impl FnMut(&str) -> Result<PathBuf, String> + 'static) -> Self {
        Self {
            keep: Box::new(keep),
        }
    }
}

impl Prober for System {
    fn list(&mut self) -> Listing {
        list()
    }

    fn take(&mut self, listed: &Listing, pid: u32, others: &[u32], label: &str) -> Taken {
        let processes = listed.as_ref().map_err(Clone::clone).map(|listed| {
            tree(listed, pid, others)
                .into_iter()
                .map(|one| one.line.clone())
                .collect()
        });
        let unread = match listed {
            Ok(listed) => unread(listed, pid),
            Err(why) => Err(format!("the system's list could not be read: {why}")),
        };
        let program = sample(pid, label, &mut self.keep);
        Taken {
            processes,
            unread,
            sample: program,
        }
    }

    fn sample_host(&mut self, host: u32, label: &str) -> Option<Result<PathBuf, String>> {
        sample(host, label, &mut self.keep)
    }
}

/// How long an asking is given: a PowerShell, or a sample's symbols, on a loaded machine
/// takes its time.
const ASKING: Duration = Duration::from_secs(120);

/// The system's list of every process: `ps` on Unix, the PowerShell of [`WINDOWS_SCRIPT`]
/// on Windows; or why it could not be had.
fn list() -> Result<Vec<Listed>, String> {
    let (program, args, parse): Lister = if cfg!(windows) {
        (
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command", WINDOWS_SCRIPT],
            parse_windows,
        )
    } else {
        (
            "ps",
            &["-A", "-ww", "-o", "pid=,ppid=,stat=,wchan=,tty=,command="],
            parse_ps,
        )
    };
    let ran = Run::new(program)
        .args(args)
        .inheriting_env()
        .limit(ASKING)
        .run()
        .map_err(|failed| failed.to_string())?;
    if ran.timed_out || ran.code != Some(0) {
        return Err(format!(
            "the asking failed ({:?}): {}",
            ran.code,
            ran.stderr.trim()
        ));
    }
    Ok(parse(&ran.stdout))
}

/// The bytes the program `pid` wrote to its terminal that the pane host has not read, as
/// macOS says them; or why they are not known.
fn unread(listed: &[Listed], pid: u32) -> Result<u64, String> {
    if cfg!(windows) {
        return Err(WINDOWS_UNREAD.to_owned());
    }
    if !cfg!(target_os = "macos") {
        return Err(LINUX_UNREAD.to_owned());
    }
    asked(terminal_of(listed, pid)?)
}

/// What macOS says of the terminal `tty` (`ttys014`): [`cf_e2e::terminal::unread`].
#[cfg(target_os = "macos")]
fn asked(tty: &str) -> Result<u64, String> {
    cf_e2e::terminal::unread(&std::path::Path::new("/dev").join(tty))
        .map_err(|failed| format!("its terminal could not be asked: {failed}"))
}

/// Another system's terminal is not asked.
#[cfg(not(target_os = "macos"))]
fn asked(_tty: &str) -> Result<u64, String> {
    Err(LINUX_UNREAD.to_owned())
}

/// A sample of the program `pid` for a second, kept under `label` where `keep` says; none
/// where the system has no sampler (macOS alone has `sample`).
fn sample(pid: u32, label: &str, keep: &mut Keep) -> Option<Result<PathBuf, String>> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    Some(keep(label).and_then(|file| {
        let ran = Run::new("sample")
            .arg(pid.to_string())
            .arg("1")
            .arg("-file")
            .arg(&file)
            .inheriting_env()
            .limit(ASKING)
            .run()
            .map_err(|failed| failed.to_string())?;
        let kept = std::fs::metadata(&file).is_ok_and(|kept| kept.len() > 0);
        if ran.code == Some(0) && kept {
            Ok(file)
        } else {
            Err(format!(
                "the sample failed ({:?}): {}",
                ran.code,
                ran.stderr.trim()
            ))
        }
    }))
}

/// A prober of tests: it keeps what it was asked (the program, the others, the label), how
/// many times it listed, and each sample of the pane host (its process and label); it
/// answers each time with the program's line and its children's (none where the program
/// is one of `gone`), `unread` bytes unread, and samples named by their labels.
#[cfg(test)]
#[derive(Debug, Default)]
pub struct Asked {
    pub asked: Vec<(u32, Vec<u32>, String)>,
    /// How many times the system's list was asked.
    pub lists: usize,
    /// Each sample of the pane host: its process, and the label it was kept under.
    pub hosts: Vec<(u32, String)>,
    /// What each program wrote that the pane host has not read.
    pub unread: u64,
    /// The programs the system lists no more.
    pub gone: Vec<u32>,
}

#[cfg(test)]
impl Prober for Asked {
    fn list(&mut self) -> Listing {
        self.lists += 1;
        Ok(Vec::new())
    }

    fn take(&mut self, _: &Listing, pid: u32, others: &[u32], label: &str) -> Taken {
        self.asked.push((pid, others.to_vec(), label.to_owned()));
        let lines = if self.gone.contains(&pid) {
            Vec::new()
        } else {
            std::iter::once(format!("{pid} 1 S - ttys001 pi"))
                .chain(
                    others
                        .iter()
                        .map(|other| format!("{other} {pid} S - ttys001 bash")),
                )
                .collect()
        };
        Taken {
            processes: Ok(lines),
            unread: Ok(self.unread),
            sample: Some(Ok(PathBuf::from(format!("{label}-sample.txt")))),
        }
    }

    fn sample_host(&mut self, host: u32, label: &str) -> Option<Result<PathBuf, String>> {
        self.hosts.push((host, label.to_owned()));
        Some(Ok(PathBuf::from(format!("{label}-sample.txt"))))
    }
}

#[cfg(test)]
mod tests;

//! Windows pane ownership: one Job Object per pane, kill-on-close. Everything
//! the harness starts joins its job, so ending the job ends the whole tree, and
//! when the app itself dies Windows closes the handle and ends it too. A process
//! leaves a job only by being started with `CREATE_BREAKAWAY_FROM_JOB`, which a job
//! with no `JOB_OBJECT_LIMIT_BREAKAWAY_OK` refuses (this one has only kill-on-close),
//! and a job object made inside it holds its processes in this one too: a job an
//! agent detached from a shell that ended (`Start-Process`, `start`) is in the job
//! still, with no parent, which the window's close ends and an ending of the turn does
//! not: a job with no parent is no proof of whose work it is
//! ([`crate::background`]). What the job does not hold, and a close does not end:
//! - what another program starts for the harness: a service (`schtasks`, a WMI
//!   `Win32_Process.Create`), the shell or COM activating a program, UAC starting one
//!   elevated; none of it is a child of the pane's;
//! - a process started with `PROC_THREAD_ATTRIBUTE_PARENT_PROCESS` naming a parent
//!   outside the job, which takes its jobs from that parent;
//! - what is no Windows process of the job at all: a WSL distribution's, a container
//!   daemon's;
//! - the console host, which is the pane host's, and ends when the pane's pseudoconsole
//!   is closed.
//!
//! The job is also the list of what the harness started: its processes are what a
//! pane's tree has ([`JobObject::members`]). Each is held open as it is looked at,
//! and everything said of it (when it began, the program and the path it runs
//! from, the command line, the process that started it) is read from that handle, and
//! the job is asked whether it holds the process the handle is for. The system's
//! list of processes is never consulted for what a number is: a number another
//! process takes is another process's, and a handle that is held keeps its
//! number (the system gives it to none while the process object is there, gone
//! or not), so a parent named by number is the process held under it. What a job
//! cannot do is end some of its tree: [`JobObject::end`] ends the processes the
//! baseline proves the turn's, each by itself, by the handle it holds, and waits for
//! each to be gone. It chooses them once, when it begins, and goes on to what they
//! started, which the handles it holds name as their parent whether or not that
//! process ran on: never to what a later look takes for the turn's work, which a new
//! turn's is. The whole of it is held to [`BUDGET`], inside what the daemon waits for
//! ([`cf_proto::panes::ENDING_BOUND`]).
//!
//! A program of Git Bash's own kind that a script started (`sleep`, `grep`) names a parent that
//! has gone, a forked copy of the shell (it goes a few milliseconds after the program has
//! begun, T-375: on zeewin 13 ms, and 64 ms beside five computing threads for each core; a
//! look before that finds a parent that is a process of the job, a chain whole without the
//! repair, and `ps` is asked for none): the parent
//! `ps -W` of Git Bash says it has is given instead ([`repair_cygwin_parents`],
//! [`crate::cygwin`]), so that the chain up from it to the
//! shell the harness started is whole and a stop ends it with the script. That is not a matter
//! of the job: the program is a member of it (whatever a job holds, its descendants too), and
//! it is the parent it names that is no process. The harness being in the job from its first
//! instruction changes none of it (a real pane, with the job in its attributes, has the same
//! sleep with the same dead parent: `pty`'s `ending_tests`).
//!
//! The harness is in the job from its first instruction: it is started with the job
//! in its attributes ([`crate::pty`]'s `conpty`), so nothing it starts itself, however
//! soon, is outside it.
use std::collections::{HashMap, HashSet};
use std::io;
use std::mem::{offset_of, size_of, size_of_val};
use std::path::{Path, PathBuf};
use std::ptr;
use std::thread;
use std::time::{Duration, Instant};

use cf_proto::panes::{Ending, ENDING_BOUND};
use windows_sys::Wdk::System::Threading::{
    NtQueryInformationProcess, ProcessBasicInformation, ProcessCommandLineInformation,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_MORE_DATA, FILETIME, HANDLE, UNICODE_STRING, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, IsProcessInJob, JobObjectBasicProcessIdList,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, TerminateProcess,
    WaitForSingleObject, PROCESS_BASIC_INFORMATION, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};

use crate::background::{report, started_by, steady, Baseline, Member, ProcessKey};

/// How many times an ending looks again for what the processes it ended had
/// started: a process that is ended starts no more, and none of them is ended
/// faster than the next can start one.
const PASSES: usize = 16;

/// How long the processes of one pass of an ending are waited for to be gone, in all:
/// the system ends a process at once, and says it has when its last thread is gone.
const EXIT_WAIT: Duration = Duration::from_secs(5);

/// How long an ending lasts in all, whatever the passes it makes and the moment it waits
/// before it looks again: what is not gone by then is said to be left. It is less than
/// the bound the daemon waits for the answer by, which is the host's word that it
/// answers in it.
const BUDGET: Duration = Duration::from_secs(15);

const _: () = assert!(BUDGET.as_secs() < ENDING_BOUND.as_secs());

/// What says `ps -W` of a folder ([`ps_w`]; a test gives its own, which counts what it is asked).
pub(crate) type Ps<'a> = &'a dyn Fn(&Path) -> Option<String>;

pub(crate) struct JobObject {
    handle: HANDLE,
}

// SAFETY: a job handle is a kernel handle, usable from any thread.
unsafe impl Send for JobObject {}
// SAFETY: the handle is all a JobObject holds, and the kernel serializes calls
// made on one handle from several threads.
unsafe impl Sync for JobObject {}

impl JobObject {
    pub fn new() -> io::Result<Self> {
        // SAFETY: no attributes and no name: a private job for this pane.
        let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = Self { handle };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: the structure is the one the information class names, at its size.
        let set = unsafe {
            SetInformationJobObject(
                job.handle,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if set == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    /// The job's handle, which a program is started in (`pty::conpty`): open for as long
    /// as the job is.
    pub fn handle(&self) -> HANDLE {
        self.handle
    }

    pub fn terminate(&self) -> io::Result<()> {
        // SAFETY: the handle is open until drop.
        if unsafe { TerminateJobObject(self.handle, 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// The processes the job has now, each by its number and when it began, with
    /// the command line it was started with, the program and the path it runs from,
    /// and the process of the job that started it, where that is still there. One
    /// that ended between the list and the look at it, or that cannot be looked at,
    /// is left out.
    pub fn members(&self) -> io::Result<Vec<Member>> {
        self.members_with(&ps_w)
    }

    /// [`JobObject::members`], with `ps` saying what `ps -W` says: a look at the job as
    /// Windows names the parents is `&|_| None`.
    pub(crate) fn members_with(&self, ps: Ps) -> io::Result<Vec<Member>> {
        Ok(members_of(self.hold_new(&HashMap::new(), ps)?.iter()))
    }

    /// Ends what `baseline` proves the turn's work, each process by itself and
    /// the rest of the job staying as it is. The processes are held open for as long
    /// as this lasts: a number is one process while a handle to it is open, so what
    /// an earlier look learned of a process is learned of that one, a process that
    /// was ended and gone is still the parent it names to what it started, and a
    /// process is not ended on what was said of another that had its number. What it
    /// ends is chosen once, before it ends anything, and then only what those processes
    /// started (a process that is ended starts nothing, but another that is not yet
    /// may have started one): each pass ends what is fresh, waits for it to be gone,
    /// and looks again for what it started that runs ([`started_by`]: one that ended by
    /// itself meanwhile, which a handle held keeps as the parent it names, is none of
    /// it). Never what a look of the whole job takes for the turn's work by the
    /// baseline once the ending has begun, which a human's or a message's new work is.
    /// The choice is of what two looks, `settle` apart, both find the turn's work and
    /// running (a look that finds none is not made again): a short run of the
    /// harness's own has ended by the second, and is none of it. What was chosen, told
    /// by whether it is gone: ended, or not.
    pub fn end(&self, baseline: &Baseline, settle: Duration) -> io::Result<Ending> {
        let deadline = Instant::now() + BUDGET;
        let mut held: HashMap<u32, Held> = HashMap::new();
        self.hold_into(&mut held)?;
        let mut work = baseline.turn_work(&members_of(held.values()));
        if !work.is_empty() {
            thread::sleep(settle);
            self.hold_into(&mut held)?;
            // What runs now: a process that has ended is held still, to be the parent
            // it names, and is not there.
            let there = members_of(held.values().filter(|one| one.runs()));
            work = steady(&work, baseline.turn_work(&there));
        }
        let mut fresh: Vec<ProcessKey> = work.into_iter().map(|member| member.key).collect();
        let mut chosen: Vec<ProcessKey> = Vec::new();
        for _ in 0..PASSES {
            if fresh.is_empty() {
                break;
            }
            let in_time = Instant::now() < deadline;
            if in_time {
                for key in &fresh {
                    if let Some(held) = held.get(&key.pid) {
                        held.process.end();
                    }
                }
                let until = deadline.min(Instant::now() + EXIT_WAIT);
                for key in &fresh {
                    if let Some(held) = held.get(&key.pid) {
                        held.process.wait_until(until);
                    }
                }
            }
            chosen.append(&mut fresh);
            if !in_time {
                break;
            }
            self.hold_into(&mut held)?;
            let roots: HashSet<ProcessKey> = chosen.iter().copied().collect();
            let running: HashSet<ProcessKey> = held
                .values()
                .filter(|one| one.runs())
                .map(|one| one.member.key)
                .collect();
            fresh = started_by(&roots, &members_of(held.values()), &running);
        }
        // What was found and not ended, for the passes or the time ran out, is said
        // to be left.
        chosen.append(&mut fresh);
        let alive: HashSet<ProcessKey> = chosen
            .iter()
            .filter(|key| held.get(&key.pid).is_some_and(Held::runs))
            .copied()
            .collect();
        let members: Vec<Member> = members_of(held.values())
            .into_iter()
            .filter(|member| chosen.contains(&member.key))
            .collect();
        Ok(report(members, &alive))
    }

    /// The processes of the job, each held open, and each the one the job holds: the
    /// number the system lists may be another's by the time it is opened, and the
    /// handle is asked, not the number.
    ///
    /// Only what `held` does not hold is opened: a process that is held keeps its number, so
    /// the one the job lists under it is the one held, and what was read of it is read. The
    /// parents of Git Bash's programs are mended for what is new alone ([`repair_cygwin_parents`]),
    /// so an ending asks `ps -W` once for a process and not at every look it makes again
    /// (T-194, P3-2: an ending looks up to eighteen times, a slow `ps` waits 3 s).
    fn hold_new(&self, held: &HashMap<u32, Held>, ps: Ps) -> io::Result<Vec<Held>> {
        let mut fresh: Vec<Held> = self
            .process_ids()?
            .into_iter()
            .filter(|pid| !held.contains_key(pid))
            .filter_map(|pid| self.hold_one(pid))
            .collect();
        repair_cygwin_parents(&mut fresh, held, ps);
        Ok(fresh)
    }

    /// Holds what the job has now that `held` has not.
    fn hold_into(&self, held: &mut HashMap<u32, Held>) -> io::Result<()> {
        self.hold_into_with(held, &ps_w)
    }

    /// [`JobObject::hold_into`], `ps` saying what `ps -W` says.
    fn hold_into_with(&self, held: &mut HashMap<u32, Held>, ps: Ps) -> io::Result<()> {
        for now in self.hold_new(held, ps)? {
            held.entry(now.member.key.pid).or_insert(now);
        }
        Ok(())
    }

    fn hold_one(&self, pid: u32) -> Option<Held> {
        // SAFETY: the rights a look at a process, and its end and the wait for it,
        // need; the handle is closed by `Process`.
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if handle.is_null() {
            return None;
        }
        let process = Process(handle);
        // One that ended since the list is no process of the job: its object is still
        // there to open while another handle to it is.
        if process.gone() {
            return None;
        }
        let mut inside = 0;
        // SAFETY: both handles are open, and `inside` is writable.
        let asked = unsafe { IsProcessInJob(process.0, self.handle, &mut inside) };
        if asked == 0 || inside == 0 {
            return None;
        }
        let exe = image_path(&process).unwrap_or_default();
        Some(Held {
            member: Member {
                key: ProcessKey {
                    pid,
                    began: began(&process)?,
                },
                command: command_line(&process).unwrap_or_default(),
                program: exe
                    .rsplit(['\\', '/'])
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
                exe,
                // A job object has no process groups.
                group: None,
                parent: None,
            },
            parent: parent_of(&process),
            process,
        })
    }

    /// The numbers of the job's processes. The system answers with as many as
    /// the room given holds, and says how many there are: the room is made
    /// larger for the second ask.
    fn process_ids(&self) -> io::Result<Vec<u32>> {
        let header =
            offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList) / size_of::<usize>();
        let mut room = 64;
        loop {
            // Words, so that the structure laid over it is aligned.
            let mut buffer = vec![0usize; header + room];
            // SAFETY: the buffer is writable for the size passed with it, and
            // the handle is open until drop.
            let queried = unsafe {
                QueryInformationJobObject(
                    self.handle,
                    JobObjectBasicProcessIdList,
                    buffer.as_mut_ptr().cast(),
                    size_of_val(buffer.as_slice()) as u32,
                    ptr::null_mut(),
                )
            };
            if queried == 0
                && io::Error::last_os_error().raw_os_error() != Some(ERROR_MORE_DATA as i32)
            {
                return Err(io::Error::last_os_error());
            }
            let list = buffer.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
            // SAFETY: the system wrote the structure's two counts at the start
            // of the buffer, which is aligned for it and as long as the header.
            let (assigned, listed) = unsafe {
                (
                    (*list).NumberOfAssignedProcesses as usize,
                    (*list).NumberOfProcessIdsInList as usize,
                )
            };
            if assigned > listed && assigned > room {
                room = assigned + 16;
                continue;
            }
            return Ok(buffer[header..header + listed.min(room)]
                .iter()
                .map(|id| *id as u32)
                .collect());
        }
    }
}

impl Drop for JobObject {
    fn drop(&mut self) {
        // SAFETY: the handle is ours and closed once; kill-on-close ends the tree.
        unsafe { CloseHandle(self.handle) };
    }
}

/// A process of the job, held open: what was read of it is of the process the handle
/// is for.
struct Held {
    process: Process,
    member: Member,
    /// The number of the process that started it, as the system names it.
    parent: Option<u32>,
}

impl Held {
    /// Whether the process runs. A process that has ended is a process object still
    /// while a handle to it is open, and a look of what is held lists it, to be the
    /// parent it names to what it started: it is not a process that is there.
    fn runs(&self) -> bool {
        !self.process.gone()
    }
}

/// A program of Git Bash's own kind that a script started names a parent that has gone (a
/// forked copy of the shell, which started it): the parent Git Bash says it has, a shell of
/// the job, is given instead, so that the chain from it to the shell the harness started is
/// whole ([`crate::cygwin`]). `fresh` is what a look has just held that was not held before,
/// and `held` what was: a parent is a member of either. `ps -W` is asked (`ps`) only where a
/// process of `fresh` is such and a `ps` is in the folder of its program with the Cygwin layer
/// it runs on (Git Bash's); a process it says nothing of, or whose parent is no member, keeps
/// the parent it named.
fn repair_cygwin_parents(fresh: &mut [Held], held: &HashMap<u32, Held>, ps: Ps) {
    let members: HashSet<u32> = fresh
        .iter()
        .map(|one| one.member.key.pid)
        .chain(held.keys().copied())
        .collect();
    let orphans: Vec<(u32, PathBuf)> = fresh
        .iter()
        .filter(|one| one.parent.is_some_and(|parent| !members.contains(&parent)))
        .filter_map(|one| {
            let folder = Path::new(&one.member.exe).parent()?;
            Some((one.member.key.pid, folder.to_path_buf()))
        })
        .filter(|(_, folder)| crate::cygwin::runs_ps_in(folder, Path::is_file))
        .collect();
    if orphans.is_empty() {
        return;
    }
    for (pid, parent) in crate::cygwin::parents_of(&orphans, ps) {
        if !members.contains(&parent) {
            continue;
        }
        if let Some(one) = fresh.iter_mut().find(|one| one.member.key.pid == pid) {
            one.parent = Some(parent);
        }
    }
}

/// What `ps -W` of the folder `folder` says (Git Bash's own, which lists the Cygwin parent of
/// each of its programs): none where it did not answer in [`PS_TIME`]. It is started through
/// cf-process, on a runtime of its own for the moment it takes, with this process's
/// environment as it is: Git's `ps` needs the system's variables.
fn ps_w(folder: &Path) -> Option<String> {
    let run = cf_process::Run {
        program: folder.join("ps.exe"),
        args: vec!["-W".into()],
        verbatim: false,
    };
    let limits = cf_process::Limits {
        timeout: PS_TIME,
        max_buffer: PS_BYTES,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    runtime
        .block_on(cf_process::execute(
            &run,
            None,
            &cf_base::env::Env::from_process(),
            limits,
            |_| {},
        ))
        .ok()
}

/// How long a look waits for `ps -W`, and how much of it is read: a list of every process of the
/// machine, a line each. The wait was set against [`BUDGET`], which an ending shares with
/// everything it does, and the bound the daemon waits by ([`ENDING_BOUND`]): an ending asks once for
/// a process ([`JobObject::hold_new`]), so a `ps` that does not answer costs it this and no more,
/// and four such waits would still be a part of the budget and not all of it.
const PS_TIME: Duration = Duration::from_secs(3);
const PS_BYTES: usize = 8 * 1024 * 1024;

const _: () = assert!(PS_TIME.as_secs() * 4 <= BUDGET.as_secs());

/// The members of `held`, each with the process that started it where that is one
/// of them and began first: a number a process took after its parent went is not its
/// parent's, which began after it.
fn members_of<'a>(held: impl Iterator<Item = &'a Held> + Clone) -> Vec<Member> {
    let keys: HashMap<u32, ProcessKey> = held
        .clone()
        .map(|held| (held.member.key.pid, held.member.key))
        .collect();
    held.map(|held| {
        let parent = held.parent.and_then(|number| {
            keys.get(&number)
                .filter(|parent| {
                    parent.pid != held.member.key.pid && parent.began <= held.member.key.began
                })
                .copied()
        });
        Member {
            parent,
            ..held.member.clone()
        }
    })
    .collect()
}

/// A process handle, closed when it goes.
struct Process(HANDLE);

impl Process {
    /// Ends the process, which the system does in its own time: whether it has is for
    /// [`Process::wait_until`] and [`Process::gone`] to say. One that has gone cannot
    /// be ended again, and is not an error.
    fn end(&self) {
        // SAFETY: the handle is open, and was opened with the right to this.
        unsafe { TerminateProcess(self.0, 1) };
    }

    /// Waits until the process is gone, or `until`.
    fn wait_until(&self, until: Instant) {
        let left = until.saturating_duration_since(Instant::now());
        let millis = u32::try_from(left.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: the handle is open, and was opened with the right to wait on it.
        unsafe { WaitForSingleObject(self.0, millis) };
    }

    /// Whether the process is gone: its handle is signalled once its last thread is.
    fn gone(&self) -> bool {
        // SAFETY: as above; a wait of no time only asks.
        unsafe { WaitForSingleObject(self.0, 0) == WAIT_OBJECT_0 }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by `hold_one`, is not null, and is closed only here.
        unsafe { CloseHandle(self.0) };
    }
}

/// When the process began, as the system keeps it: 100 ns since 1601.
fn began(process: &Process) -> Option<u64> {
    let (mut created, mut ended, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: the handle is open and the four structures are writable.
    let read =
        unsafe { GetProcessTimes(process.0, &mut created, &mut ended, &mut kernel, &mut user) };
    (read != 0)
        .then(|| (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}

/// The number of the process that started it, as the system names it, which may
/// be another process's by now: it is the parent only where that process began first
/// ([`members_of`]).
fn parent_of(process: &Process) -> Option<u32> {
    let mut information = PROCESS_BASIC_INFORMATION::default();
    let mut written = 0u32;
    // SAFETY: the structure is the one the information class names, at its size.
    let status = unsafe {
        NtQueryInformationProcess(
            process.0,
            ProcessBasicInformation,
            (&mut information as *mut PROCESS_BASIC_INFORMATION).cast(),
            size_of::<PROCESS_BASIC_INFORMATION>() as u32,
            &mut written,
        )
    };
    if status < 0 {
        return None;
    }
    u32::try_from(information.InheritedFromUniqueProcessId).ok()
}

/// The whole path the process's program runs from, as the system keeps it.
fn image_path(process: &Process) -> Option<String> {
    // The path a program may have is longer than the one most have.
    for room in [1_024usize, 32_768] {
        let mut path = vec![0u16; room];
        let mut length = room as u32;
        // SAFETY: the buffer is writable for `length` units, which the system
        // lowers to the units it wrote.
        let read = unsafe {
            QueryFullProcessImageNameW(
                process.0,
                PROCESS_NAME_WIN32,
                path.as_mut_ptr(),
                &mut length,
            )
        };
        if read != 0 {
            return Some(String::from_utf16_lossy(&path[..length as usize]));
        }
    }
    None
}

/// The command line the process was started with, as the system keeps it. None
/// where it cannot be read.
fn command_line(process: &Process) -> Option<String> {
    let mut needed = 0u32;
    // SAFETY: no buffer and no room for one: the call only says how much it needs.
    unsafe {
        NtQueryInformationProcess(
            process.0,
            ProcessCommandLineInformation,
            ptr::null_mut(),
            0,
            &mut needed,
        )
    };
    if (needed as usize) < size_of::<UNICODE_STRING>() {
        return None;
    }
    // Words, so that the string laid over it is aligned.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(size_of::<u64>())];
    // SAFETY: the buffer is writable for the size passed with it.
    let status = unsafe {
        NtQueryInformationProcess(
            process.0,
            ProcessCommandLineInformation,
            buffer.as_mut_ptr().cast(),
            size_of_val(buffer.as_slice()) as u32,
            &mut needed,
        )
    };
    if status < 0 {
        return None;
    }
    let text = buffer.as_ptr().cast::<UNICODE_STRING>();
    // SAFETY: on success the buffer holds a UNICODE_STRING whose text follows it
    // in the same buffer: `Length` bytes of UTF-16 at `Buffer`.
    let units = unsafe {
        let (bytes, chars) = ((*text).Length as usize, (*text).Buffer);
        if chars.is_null() {
            return None;
        }
        std::slice::from_raw_parts(chars, bytes / size_of::<u16>())
    };
    Some(String::from_utf16_lossy(units))
}

#[cfg(test)]
mod tests;

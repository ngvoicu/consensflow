//! Windows pane ownership: one Job Object per pane, kill-on-close. Everything
//! the harness starts joins its job, so ending the job ends the whole tree, and
//! when the app itself dies Windows closes the handle and ends it too. A process
//! leaves a job only by being started with `CREATE_BREAKAWAY_FROM_JOB`, which a job
//! with no `JOB_OBJECT_LIMIT_BREAKAWAY_OK` refuses (this one has only kill-on-close),
//! and a job object made inside it holds its processes in this one too: a job an
//! agent detached from a shell that ended (`Start-Process`, `start`) is in the job
//! still, with no parent. What the job does not hold is what a service starts for it
//! (`schtasks`, a WMI `Win32_Process.Create`): that is no child of the pane's.
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
//! each to be gone.
//!
//! The harness joins right after it starts (the PTY library starts it
//! unsuspended); a child it starts in that first instant would escape, which a
//! harness still loading its runtime does not do.
use std::collections::{HashMap, HashSet};
use std::io;
use std::mem::{offset_of, size_of, size_of_val};
use std::ptr;
use std::time::{Duration, Instant};

use cf_proto::panes::Ending;
use windows_sys::Wdk::System::Threading::{
    NtQueryInformationProcess, ProcessBasicInformation, ProcessCommandLineInformation,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_MORE_DATA, FILETIME, HANDLE, UNICODE_STRING, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JobObjectBasicProcessIdList,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, TerminateProcess,
    WaitForSingleObject, PROCESS_BASIC_INFORMATION, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};

use crate::background::{report, Baseline, Member, Parent, ProcessKey};

/// How many times an ending looks again for what the processes it ended had
/// started: a process that is ended starts no more, and none of them is ended
/// faster than the next can start one.
const PASSES: usize = 16;

/// How long a process that was terminated is waited for, in all: the system ends it
/// at once, and says it has when its last thread is gone.
const EXIT_WAIT: Duration = Duration::from_secs(5);

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

    pub fn assign(&self, pid: u32) -> io::Result<()> {
        // SAFETY: the rights asked are the two AssignProcessToJobObject needs.
        let process = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid) };
        if process.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: both handles are open; the process handle is closed after.
        let assigned = unsafe { AssignProcessToJobObject(self.handle, process) };
        let error = io::Error::last_os_error();
        // SAFETY: `process` was opened above, is not null, and is closed only here.
        unsafe { CloseHandle(process) };
        if assigned == 0 {
            return Err(error);
        }
        Ok(())
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
        Ok(members_of(self.hold()?.iter()))
    }

    /// Ends what `baseline` proves the turn's work, each process by itself and
    /// the rest of the job staying as it is. The processes are held open for as long
    /// as this lasts: a number is one process while a handle to it is open, so what
    /// an earlier look learned of a process is learned of that one, a process that
    /// was ended and gone is still the parent it names to what it started, and a
    /// process is not ended on what was said of another that had its number. Each
    /// pass ends what it finds and waits for it to be gone, and looks again: a
    /// process that is ended starts nothing, but another that is not yet may have
    /// started one. What was chosen, told by whether it is gone: ended, or not.
    pub fn end(&self, baseline: &Baseline) -> io::Result<Ending> {
        let mut held: HashMap<u32, Held> = HashMap::new();
        let mut chosen: Vec<ProcessKey> = Vec::new();
        for _ in 0..PASSES {
            for now in self.hold()? {
                held.entry(now.member.key.pid).or_insert(now);
            }
            let work = baseline.turn_work(&members_of(held.values()));
            let fresh: Vec<ProcessKey> = work
                .iter()
                .map(|member| member.key)
                .filter(|key| !chosen.contains(key))
                .collect();
            if fresh.is_empty() {
                break;
            }
            for key in &fresh {
                if let Some(held) = held.get(&key.pid) {
                    held.process.end();
                }
            }
            let until = Instant::now() + EXIT_WAIT;
            for key in &fresh {
                if let Some(held) = held.get(&key.pid) {
                    held.process.wait_until(until);
                }
            }
            chosen.extend(fresh);
        }
        let alive: HashSet<ProcessKey> = chosen
            .iter()
            .filter(|key| held.get(&key.pid).is_some_and(|held| !held.process.gone()))
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
    fn hold(&self) -> io::Result<Vec<Held>> {
        Ok(self
            .process_ids()?
            .into_iter()
            .filter_map(|pid| self.hold_one(pid))
            .collect())
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
                parent: Parent::Unknown,
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

/// The members of `held`, each with the process that started it where that is one
/// of them and began first: a number a process took after its parent went is not its
/// parent's, which began after it.
fn members_of<'a>(held: impl Iterator<Item = &'a Held> + Clone) -> Vec<Member> {
    let keys: HashMap<u32, ProcessKey> = held
        .clone()
        .map(|held| (held.member.key.pid, held.member.key))
        .collect();
    held.map(|held| {
        let parent = match held.parent {
            None => Parent::Unknown,
            Some(number) => keys
                .get(&number)
                .filter(|parent| {
                    parent.pid != held.member.key.pid && parent.began <= held.member.key.began
                })
                .map_or(Parent::Gone, |parent| Parent::Of(*parent)),
        };
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

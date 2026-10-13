//! Windows pane ownership: one Job Object per pane, kill-on-close. Everything
//! the harness starts joins its job, so ending the job ends the whole tree, and
//! when the app itself dies Windows closes the handle and ends it too. The job
//! is also the list of what the harness started: its processes are what a pane's
//! tree has ([`JobObject::members`]), each with the program it runs and the
//! process of the job that started it, which the system's own list of processes
//! says ([`system_processes`]: the parent number each names, held for the process
//! that started it only if that one began first, which a number that was taken
//! over by another process after the parent went did not). What a job cannot do
//! is end some of its tree: [`JobObject::end`] ends the processes it is named, and
//! what descends from them, each by itself.
//!
//! The harness joins right after it starts (the PTY library starts it
//! unsuspended); a child it starts in that first instant would escape, which a
//! harness still loading its runtime does not do.
use std::collections::{HashMap, HashSet};
use std::io;
use std::mem::{offset_of, size_of, size_of_val};
use std::ptr;

use windows_sys::Wdk::System::Threading::{
    NtQueryInformationProcess, ProcessCommandLineInformation,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_INVALID_PARAMETER, ERROR_MORE_DATA, FILETIME, HANDLE, INVALID_HANDLE_VALUE,
    UNICODE_STRING,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicProcessIdList,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SET_QUOTA, PROCESS_TERMINATE,
};

use crate::background::{Member, ProcessKey};

/// How many times an ending looks again for what the processes it ended had
/// started: a process that is ended starts no more, and none of them is ended
/// faster than the next can start one.
const PASSES: usize = 16;

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

    /// The processes the job has now, each by its number and when it began and
    /// with the command line it was started with, the name of its program and the
    /// process of the job that started it, where that is still there. One that
    /// ended between the list and the look at it, or that cannot be looked at, is
    /// left out.
    pub fn members(&self) -> io::Result<Vec<Member>> {
        let system = system_processes();
        let mut members: Vec<Member> = self
            .process_ids()?
            .into_iter()
            .filter_map(|pid| member(pid, system.get(&pid)))
            .collect();
        let keys: HashMap<u32, ProcessKey> = members
            .iter()
            .map(|member| (member.key.pid, member.key))
            .collect();
        for member in &mut members {
            member.parent = system
                .get(&member.key.pid)
                .and_then(|listed| keys.get(&listed.parent))
                .filter(|parent| parent.pid != member.key.pid && parent.began <= member.key.began)
                .copied();
        }
        Ok(members)
    }

    /// Ends the processes `keys` name and what descends from them, each by
    /// itself, the rest of the job staying as it is. A process is the one named
    /// only if it began when it did. Each pass ends what it finds and looks again:
    /// a process that is ended starts nothing, but another that is not yet may have
    /// started one, which the parent it names (an ended one's, held for its parent
    /// as any is) says it descends from.
    pub fn end(&self, keys: &[ProcessKey]) -> io::Result<()> {
        let mut seen: HashSet<ProcessKey> = HashSet::new();
        let mut failure = None;
        for _ in 0..PASSES {
            let system = system_processes();
            let members = self.members()?;
            let alive: HashMap<u32, ProcessKey> = members
                .iter()
                .map(|member| (member.key.pid, member.key))
                .collect();
            // The processes ended or about to be, by their number: what descends
            // from them names one as its parent. A number that a live process of
            // the job has is that one's, whoever had it before.
            let mut known: HashMap<u32, ProcessKey> =
                seen.iter().map(|key| (key.pid, *key)).collect();
            let mut fresh: Vec<ProcessKey> = Vec::new();
            loop {
                let before = fresh.len();
                for member in &members {
                    let key = member.key;
                    let descends = system
                        .get(&key.pid)
                        .and_then(|listed| known.get(&listed.parent))
                        .is_some_and(|parent| {
                            parent.pid != key.pid
                                && parent.began <= key.began
                                && alive.get(&parent.pid).is_none_or(|live| live == parent)
                        });
                    if !seen.contains(&key) && (keys.contains(&key) || descends) {
                        seen.insert(key);
                        known.insert(key.pid, key);
                        fresh.push(key);
                    }
                }
                if fresh.len() == before {
                    break;
                }
            }
            if fresh.is_empty() {
                break;
            }
            for key in fresh {
                if let Err(error) = end_process(key) {
                    failure = Some(error);
                }
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
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

/// A process handle, closed when it goes.
struct Process(HANDLE);

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by `member`, is not null, and is closed only here.
        unsafe { CloseHandle(self.0) };
    }
}

/// What the system lists of one process: the number of the process that started
/// it, and the file name of its program.
struct Listed {
    parent: u32,
    program: String,
}

/// What the system lists of every process it runs, by number. Empty where the list
/// could not be made: a member then has no program and no parent to be told by.
fn system_processes() -> HashMap<u32, Listed> {
    // SAFETY: a snapshot of the system's processes, which asks nothing of any.
    let handle = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if handle == INVALID_HANDLE_VALUE {
        return HashMap::new();
    }
    let snapshot = Process(handle);
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..PROCESSENTRY32W::default()
    };
    let mut listed = HashMap::new();
    // SAFETY: the snapshot is open until `snapshot` goes, and `entry` is writable
    // and says its size, as both calls ask.
    let mut found = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
    while found {
        let end = entry
            .szExeFile
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(entry.szExeFile.len());
        listed.insert(
            entry.th32ProcessID,
            Listed {
                parent: entry.th32ParentProcessID,
                program: String::from_utf16_lossy(&entry.szExeFile[..end]),
            },
        );
        // SAFETY: as above.
        found = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
    }
    listed
}

/// The process `pid` as a member of a tree: none for one that cannot be looked at.
/// Which process started it is for the list of members to say.
fn member(pid: u32, listed: Option<&Listed>) -> Option<Member> {
    // SAFETY: the one right a look at a process needs; the handle is closed by `Process`.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let process = Process(handle);
    Some(Member {
        key: ProcessKey {
            pid,
            began: began(&process)?,
        },
        command: command_line(&process).unwrap_or_default(),
        program: listed
            .map(|listed| listed.program.clone())
            .unwrap_or_default(),
        // A job object has no process groups.
        group: None,
        parent: None,
    })
}

/// Ends the process `key` names, if it is the one that began when the key says: a
/// number that another process has since is not it. A process that is gone is
/// ended.
fn end_process(key: ProcessKey) -> io::Result<()> {
    // SAFETY: the rights a look at when a process began and its end need; the
    // handle is closed by `Process`.
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
            0,
            key.pid,
        )
    };
    if handle.is_null() {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
            Ok(())
        } else {
            Err(error)
        };
    }
    let process = Process(handle);
    if began(&process) != Some(key.began) {
        return Ok(());
    }
    // SAFETY: the handle is open, and was opened for this.
    if unsafe { TerminateProcess(process.0, 1) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
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

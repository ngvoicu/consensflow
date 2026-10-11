//! Windows pane ownership: one Job Object per pane, kill-on-close. Everything
//! the harness starts joins its job, so ending the job ends the whole tree, and
//! when the app itself dies Windows closes the handle and ends it too. The job
//! is also the list of what the harness started: its processes are what a pane's
//! tree has ([`JobObject::members`]).
//!
//! The harness joins right after it starts (the PTY library starts it
//! unsuspended); a child it starts in that first instant would escape, which a
//! harness still loading its runtime does not do.
use std::io;
use std::mem::{offset_of, size_of, size_of_val};
use std::ptr;

use windows_sys::Wdk::System::Threading::{
    NtQueryInformationProcess, ProcessCommandLineInformation,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_MORE_DATA, FILETIME, HANDLE, UNICODE_STRING,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicProcessIdList,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA,
    PROCESS_TERMINATE,
};

use crate::background::{Member, ProcessKey};

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
    /// with the command line it was started with. One that ended between the
    /// list and the look at it, or that cannot be looked at, is left out.
    pub fn members(&self) -> io::Result<Vec<Member>> {
        Ok(self.process_ids()?.into_iter().filter_map(member).collect())
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

/// The process `pid` as a member of a tree: none for one that cannot be looked at.
fn member(pid: u32) -> Option<Member> {
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
    })
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

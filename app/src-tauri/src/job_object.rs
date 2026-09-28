//! Windows pane ownership: one Job Object per pane, kill-on-close. Everything
//! the harness starts joins its job, so ending the job ends the whole tree, and
//! when the app itself dies Windows closes the handle and ends it too.
//!
//! The harness joins right after it starts (the PTY library starts it
//! unsuspended); a child it starts in that first instant would escape, which a
//! harness still loading its runtime does not do.
use std::io;
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

pub(crate) struct JobObject {
    handle: HANDLE,
}

// SAFETY: a job handle is a kernel handle, usable from any thread.
unsafe impl Send for JobObject {}
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
}

impl Drop for JobObject {
    fn drop(&mut self) {
        // SAFETY: the handle is ours and closed once; kill-on-close ends the tree.
        unsafe { CloseHandle(self.handle) };
    }
}

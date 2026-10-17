//! What libuv gives every child it starts on Windows (`src/win/process.c`),
//! given back here: the variables Windows needs that a child's environment
//! lacks, taken from this process's, and a job object that ends the child
//! when this process ends. Elsewhere a child is started as it is given.
//!
//! A program held whole (`capture_whole`) runs in a job of its own instead,
//! which nothing it starts can leave, ended with the capture.

use cf_base::env::Env;

/// The variables a child on Windows is given from this process when its
/// own environment lacks them (libuv's `required_vars`): winsock does not
/// start without `SYSTEMROOT`, for one.
const REQUIRED: [&str; 11] = [
    "HOMEDRIVE",
    "HOMEPATH",
    "LOGONSERVER",
    "PATH",
    "SYSTEMDRIVE",
    "SYSTEMROOT",
    "TEMP",
    "USERDOMAIN",
    "USERNAME",
    "USERPROFILE",
    "WINDIR",
];

/// `env` with what Windows needs of `this` where `env` lacks it, as libuv
/// starts a child on Windows; `env` as it is on other systems.
pub fn with_required(env: &Env, this: &Env) -> Env {
    if !cfg!(windows) {
        return env.clone();
    }
    let missing = REQUIRED
        .iter()
        .filter(|name| env.os(name).is_none())
        .filter_map(|name| {
            this.os(name)
                .map(|value| ((*name).into(), value.to_owned()))
        });
    Env::from_vars(
        env.iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .chain(missing),
    )
}

/// Puts `process` in the job every child of this process is in, which
/// ends them all when this process ends, as libuv's does: no child of a
/// daemon that died is left running. On other systems, nothing.
#[cfg(not(windows))]
pub(crate) fn adopt(_process: &tokio::process::Child) {}

/// Puts `process` in the job every child of this process is in, which
/// ends them all when this process ends, as libuv's does: no child of a
/// daemon that died is left running. A child the system will not put in it
/// (this process's own job forbids it) runs outside it, as with libuv.
#[cfg(windows)]
pub(crate) fn adopt(process: &tokio::process::Child) {
    use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
    let (Some(job), Some(handle)) = (job(), process.raw_handle()) else {
        return;
    };
    #[allow(unsafe_code)]
    // SAFETY: both handles are open: the job's for this process's life, the
    // child's while `process` is borrowed; the call writes no memory of ours.
    unsafe {
        AssignProcessToJobObject(job as _, handle as _);
    }
}

/// The job, made the first time a child is started: none when the system
/// would not make it.
#[cfg(windows)]
fn job() -> Option<usize> {
    use std::sync::OnceLock;
    use windows_sys::Win32::System::JobObjects::{
        CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
        JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
    };
    static JOB: OnceLock<Option<usize>> = OnceLock::new();
    *JOB.get_or_init(|| {
        #[allow(unsafe_code)]
        // SAFETY: CreateJobObjectW takes no attributes and no name here and
        // returns a handle or null; SetInformationJobObject reads `limits`,
        // a whole struct of the size given, for as long as the call lasts.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_BREAKAWAY_OK
                | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK
                | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION
                | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let size = u32::try_from(std::mem::size_of_val(&limits)).ok()?;
            let set = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size,
            );
            (set != 0).then_some(job as usize)
        }
    })
}

/// The job of a program held whole and of every process it starts, which none
/// of them can leave: on Windows, a job of its own, every process of which is
/// ended when it is dropped (and when this process ends, as its handle
/// closes). On other systems nothing: the group the program leads holds what
/// it starts.
pub(crate) struct Job {
    #[cfg(windows)]
    handle: usize,
}

#[cfg(not(windows))]
impl Job {
    /// The job of `process`, which has just started.
    #[allow(clippy::unnecessary_wraps)] // As Windows has it.
    pub(crate) fn of(_process: &tokio::process::Child) -> Result<Self, String> {
        Ok(Self {})
    }
}

#[cfg(windows)]
impl Job {
    /// A job made for `process`, which has just started, and `process` put in it:
    /// what it starts from then on is in it too, with no way out (no breakaway is
    /// allowed, silent or asked for), and a process of it that fails unhandled
    /// ends, as libuv's job has it, with no dialog of Windows' to wait on. Why
    /// it could not be made, if it could not: a process already in a job is put
    /// in this one as a job inside that one (Windows 8 on), and is refused by a
    /// job that allows none. What the program started before it was put in the
    /// job (in the moment after it started) is not in it.
    pub(crate) fn of(process: &tokio::process::Child) -> Result<Self, String> {
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        let failed = |what: &str| format!("{what}: {}", std::io::Error::last_os_error());
        let handle = process
            .raw_handle()
            .ok_or_else(|| "could not be put in a job of its own: it has ended".to_owned())?;
        #[allow(unsafe_code)]
        // SAFETY: CreateJobObjectW takes no attributes and no name here and
        // returns a handle or null, closed by the drop of the job from then on;
        // SetInformationJobObject reads `limits`, a whole struct of the size
        // given, for as long as the call lasts; the process's handle is open while
        // `process` is borrowed. None of them writes memory of ours.
        unsafe {
            let made = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if made.is_null() {
                return Err(failed("could not make a job of its own"));
            }
            let job = Self {
                handle: made as usize,
            };
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let size = u32::try_from(std::mem::size_of_val(&limits))
                .map_err(|_| "could not make a job of its own".to_owned())?;
            let limited = SetInformationJobObject(
                made,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size,
            );
            if limited == 0 {
                return Err(failed("could not make a job of its own"));
            }
            if AssignProcessToJobObject(made, handle as _) == 0 {
                return Err(failed("could not be put in a job of its own"));
            }
            Ok(job)
        }
    }
}

#[cfg(windows)]
impl Drop for Job {
    /// Every process of the job is ended, at once, and its handle closed.
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        #[allow(unsafe_code)]
        // SAFETY: the handle is the job's, open until now, and closed once; the
        // calls write no memory of ours.
        unsafe {
            TerminateJobObject(self.handle as _, 1);
            CloseHandle(self.handle as _);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_child_on_windows_is_given_what_windows_needs_where_its_environment_lacks_it() {
        let this = Env::from_vars([
            ("SYSTEMROOT", r"C:\Windows"),
            ("PATH", r"C:\bin"),
            ("OTHER", "x"),
        ]);
        let given = Env::from_vars([("PATH", r"D:\own")]);
        let started = with_required(&given, &this);
        if cfg!(windows) {
            assert_eq!(started.text("SYSTEMROOT"), Some(r"C:\Windows"));
            assert_eq!(started.text("PATH"), Some(r"D:\own"), "its own kept");
        } else {
            assert_eq!(started.text("SYSTEMROOT"), None);
        }
        assert_eq!(started.text("OTHER"), None, "nothing else comes along");
    }
}

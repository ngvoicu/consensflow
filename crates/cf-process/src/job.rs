//! What libuv gives every child it starts on Windows (`src/win/process.c`),
//! given back here: the variables Windows needs that a child's environment
//! lacks, taken from this process's, and a job object that ends the child
//! when this process ends. Elsewhere a child is started as it is given.

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

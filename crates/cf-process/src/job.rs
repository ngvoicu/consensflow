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

    /// Ends every process of the job: there is none.
    #[allow(clippy::unnecessary_wraps, clippy::unused_self)] // As Windows has it.
    pub(crate) fn end(&self) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(windows)]
impl Job {
    /// A job made for `process`, which was started suspended (`group::lead_whole`),
    /// `process` put in it, and then let run: what it starts is in it too, with no
    /// way out (no breakaway is allowed, silent or asked for), and a process of it
    /// that fails unhandled ends, as libuv's job has it, with no dialog of Windows'
    /// to wait on. Why it could not be made, or the process let run, if it could
    /// not: a process already in a job is put in this one as a job inside that one
    /// (Windows 8 on), and is refused by a job that allows none.
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
            let pid = process
                .id()
                .ok_or_else(|| "could not be let run: it has ended".to_owned())?;
            resume(pid)?;
            Ok(job)
        }
    }
}

/// Lets the process `pid`, started suspended, run: each thread the system has of it
/// (a process started suspended has one), found by a snapshot of the system's threads
/// (Toolhelp32, a documented API), resumed. Each must have been suspended once: one
/// that was not tells a process that was not started suspended, and ran before it
/// was put in its job. Why it could not be let run, if it could not.
#[cfg(windows)]
fn resume(pid: u32) -> Result<(), String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};
    let failed = |what: &str| {
        format!(
            "could not be let run: {what}: {}",
            std::io::Error::last_os_error()
        )
    };
    let size = u32::try_from(std::mem::size_of::<THREADENTRY32>())
        .map_err(|_| "could not be let run".to_owned())?;
    #[allow(unsafe_code)]
    // SAFETY: the snapshot's handle is closed once, below, whatever came of the walk;
    // Thread32First and Thread32Next write the entry they are given, ours, whose size
    // it says; each thread's handle is closed once it is resumed. None writes other
    // memory of ours.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(failed("no snapshot of the system's threads"));
        }
        let mut entry = THREADENTRY32 {
            dwSize: size,
            ..THREADENTRY32::default()
        };
        let (mut resumed, mut refused) = (0, None);
        let mut more = Thread32First(snapshot, &mut entry) != 0;
        while more {
            if entry.th32OwnerProcessID == pid && refused.is_none() {
                let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                if thread.is_null() {
                    refused = Some(failed("its thread could not be opened"));
                } else {
                    let before = ResumeThread(thread);
                    if before == u32::MAX {
                        refused = Some(failed("its thread could not be resumed"));
                    }
                    CloseHandle(thread);
                    match before {
                        1 => resumed += 1,
                        u32::MAX => {}
                        times => {
                            refused = Some(format!(
                                "could not be let run: its thread was suspended {times} times, not once: it was not started suspended"
                            ));
                        }
                    }
                }
            }
            entry.dwSize = size;
            more = Thread32Next(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        match (refused, resumed) {
            (Some(why), _) => Err(why),
            (None, 0) => Err("could not be let run: the system has no thread of it".to_owned()),
            (None, _) => Ok(()),
        }
    }
}

/// How long the processes of a job that was ended are waited for, until each has ended.
#[cfg(windows)]
const JOB_ENDS_WITHIN: std::time::Duration = std::time::Duration::from_secs(2);

/// Waits until no process of a job that was ended is left, as `left` names those that
/// have not ended, for `within` at most: why not, naming them, where the time runs out.
#[cfg(any(windows, test))]
fn ended_within(within: std::time::Duration, left: impl Fn() -> Vec<usize>) -> Result<(), String> {
    let until = std::time::Instant::now() + within;
    loop {
        let left = left();
        if left.is_empty() {
            return Ok(());
        }
        if std::time::Instant::now() >= until {
            return Err(format!(
                "processes {left:?} of its job had not ended {within:?} after it was ended"
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// The processes of a job, each held by a handle that says when it has ended (it is
/// signalled), opened before the job is ended; closed when this is dropped. A process
/// that ended before it could be opened is not held.
#[cfg(windows)]
struct Held(Vec<(usize, usize)>);

#[cfg(windows)]
impl Held {
    /// The processes `pids`, those of them still there.
    fn open(pids: Vec<usize>) -> Self {
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};
        Self(
            pids.into_iter()
                .filter_map(|pid| {
                    let id = u32::try_from(pid).ok()?;
                    #[allow(unsafe_code)]
                    // SAFETY: OpenProcess takes plain values and returns a handle or null.
                    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, id) };
                    (!handle.is_null()).then_some((pid, handle as usize))
                })
                .collect(),
        )
    }

    /// The ids of those that have not ended.
    fn left(&self) -> Vec<usize> {
        use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
        use windows_sys::Win32::System::Threading::WaitForSingleObject;
        self.0
            .iter()
            .filter(|(_, handle)| {
                #[allow(unsafe_code)]
                // SAFETY: the handle is open until this is dropped, with SYNCHRONIZE.
                let waited = unsafe { WaitForSingleObject(*handle as _, 0) };
                waited != WAIT_OBJECT_0
            })
            .map(|(pid, _)| *pid)
            .collect()
    }
}

#[cfg(windows)]
impl Drop for Held {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        for (_, handle) in &self.0 {
            #[allow(unsafe_code)]
            // SAFETY: each handle is open, and closed once, here.
            unsafe {
                CloseHandle(*handle as _);
            }
        }
    }
}

#[cfg(windows)]
impl Job {
    /// Ends every process of the job, and waits until each has ended, as its handle
    /// says, for `JOB_ENDS_WITHIN` at most (both reviewers' look at round 6: a process
    /// that still held a source file as it was put back). Why not, naming those that
    /// have not, where some have not.
    ///
    /// They are held before the job is ended: TerminateJobObject begins their end, and
    /// from then on the job counts and lists none of them while they are still ending
    /// (zeewin 2026-10-17: the call took 20 ms; the job then counted none and listed
    /// none, while 7 of 8 processes of 500 threads each had not ended, which all had
    /// 48 ms after it began); nor does a process's exit code say it has ended, which is
    /// set as its end begins. What is left: a process the job starts in the moment
    /// between the list and the end is ended and not waited for; and a process listed
    /// that ends in that moment, its id given to another, is waited for in its stead, as
    /// long as the wait allows. The list holds 256 processes at most.
    pub(crate) fn end(&self) -> Result<(), String> {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        let held = Held::open(self.processes());
        #[allow(unsafe_code)]
        // SAFETY: the handle is the job's, open until the job is dropped; the call
        // writes no memory of ours.
        unsafe {
            TerminateJobObject(self.handle as _, 1);
        }
        ended_within(JOB_ENDS_WITHIN, || held.left())
    }

    /// The ids of the processes the job holds, as far as the system says them.
    fn processes(&self) -> Vec<usize> {
        use windows_sys::Win32::System::JobObjects::{
            JobObjectBasicProcessIdList, QueryInformationJobObject,
        };
        /// JOBOBJECT_BASIC_PROCESS_ID_LIST, with room for 256 ids.
        #[repr(C)]
        struct Listed {
            _assigned: u32,
            listed: u32,
            ids: [usize; 256],
        }
        let mut list = Listed {
            _assigned: 0,
            listed: 0,
            ids: [0; 256],
        };
        let Ok(size) = u32::try_from(std::mem::size_of_val(&list)) else {
            return Vec::new();
        };
        #[allow(unsafe_code)]
        // SAFETY: the buffer is `list`, writable for the `size` bytes passed with it and
        // laid out as JOBOBJECT_BASIC_PROCESS_ID_LIST, and the call writes no more than
        // that.
        let asked = unsafe {
            QueryInformationJobObject(
                self.handle as _,
                JobObjectBasicProcessIdList,
                (&raw mut list).cast(),
                size,
                std::ptr::null_mut(),
            )
        };
        if asked == 0 {
            return Vec::new();
        }
        let listed = usize::try_from(list.listed)
            .unwrap_or(0)
            .min(list.ids.len());
        list.ids[..listed].to_vec()
    }
}

#[cfg(windows)]
impl Drop for Job {
    /// Every process of the job is ended, and waited for as `end` waits (none is left
    /// where the job was ended before), and its handle closed.
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        let _ = self.end();
        #[allow(unsafe_code)]
        // SAFETY: the handle is the job's, open until now, and closed once; the call
        // writes no memory of ours.
        unsafe {
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

    #[test]
    fn a_job_that_was_ended_is_waited_for_until_every_process_of_it_has_ended() {
        // Round 7, decision 7: TerminateJobObject returns before its processes have
        // ended, and a run said ended then had a process still holding a source file.
        let left = std::cell::RefCell::new(vec![vec![12, 34], vec![34], vec![]]);
        let read = || left.borrow_mut().remove(0);
        let within = std::time::Duration::from_secs(5);
        assert_eq!(ended_within(within, read), Ok(()));
        assert!(left.borrow().is_empty(), "asked until none was left");

        // One that does not end is said so, by name.
        let short = std::time::Duration::from_millis(50);
        assert_eq!(
            ended_within(short, || vec![12, 34]),
            Err("processes [12, 34] of its job had not ended 50ms after it was ended".to_owned())
        );
    }

    /// How many processes of `helper_holds_many_threads` the job test starts.
    #[cfg(windows)]
    const HELPERS: usize = 8;

    /// A program the job test starts, and no test of its own: 500 threads that wait, a
    /// file named by its pid in the folder it is given last that says they are there,
    /// and a minute's wait. Each thread is to end before the process has.
    #[cfg(windows)]
    #[test]
    #[ignore = "a program the job test starts"]
    fn helper_holds_many_threads() {
        use std::time::Duration;
        let folder = std::env::args().next_back().unwrap();
        for _ in 0..500 {
            std::thread::Builder::new()
                .stack_size(64 * 1024)
                .spawn(|| std::thread::sleep(Duration::from_secs(60)))
                .unwrap();
        }
        let ready = std::path::Path::new(&folder).join(format!("{}.ready", std::process::id()));
        std::fs::write(ready, "").unwrap();
        std::thread::sleep(Duration::from_secs(60));
    }

    #[cfg(windows)]
    #[test]
    #[allow(clippy::disallowed_methods)] // The test starts what it ends.
    fn every_process_of_a_job_that_was_ended_has_ended_once_its_end_has_returned() {
        // Round 7, decision 7: TerminateJobObject begins the end of a job's processes and
        // returns, and the job then counts and lists none of them, their exit codes set,
        // while they are still ending (zeewin 2026-10-17: 7 of 8 had not ended, 28 ms
        // more). A program held whole starts 8 that hold 500 threads each, and waits; the
        // test holds each by a handle of its own, opened before the end, which says when
        // it has ended.
        use std::time::{Duration, Instant};
        use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
        use windows_sys::Win32::System::Threading::{
            OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
        };
        let dir = tempfile::tempdir().unwrap();
        let helper = std::env::current_exe().unwrap();
        std::fs::write(
            dir.path().join("many.cmd"),
            format!(
                "@echo off\r\nfor /l %%i in (1,1,{HELPERS}) do start \"\" /b \"{}\" --exact job::tests::helper_holds_many_threads --ignored \"{}\"\r\nping -n 60 127.0.0.1 >NUL\r\n",
                helper.display(),
                dir.path().display()
            ),
        )
        .unwrap();
        let ready = || -> Vec<u32> {
            std::fs::read_dir(dir.path())
                .unwrap()
                .filter_map(|entry| {
                    let path = entry.ok()?.path();
                    if path.extension()? != "ready" {
                        return None;
                    }
                    path.file_stem()?.to_str()?.parse().ok()
                })
                .collect()
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let mut command = tokio::process::Command::new(r"C:\Windows\System32\cmd.exe");
            command
                .arg("/d")
                .arg("/c")
                .arg(dir.path().join("many.cmd"))
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .kill_on_drop(true);
            crate::group::lead_whole(&mut command);
            let child = command.spawn().unwrap();
            let job = Job::of(&child).unwrap();
            let until = Instant::now() + Duration::from_secs(60);
            while ready().len() < HELPERS {
                assert!(Instant::now() < until, "it started them: {:?}", ready());
                std::thread::sleep(Duration::from_millis(50));
            }
            let handles: Vec<_> = ready()
                .into_iter()
                .map(|pid| {
                    #[allow(unsafe_code)]
                    // SAFETY: OpenProcess takes plain values and returns a handle or null.
                    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
                    assert!(!handle.is_null(), "{pid} is there");
                    handle
                })
                .collect();
            assert_eq!(job.end(), Ok(()));
            let left = handles
                .iter()
                .filter(|handle| {
                    #[allow(unsafe_code)]
                    // SAFETY: the handle is open, with SYNCHRONIZE.
                    let waited = unsafe { WaitForSingleObject(**handle, 0) };
                    waited != WAIT_OBJECT_0
                })
                .count();
            for handle in handles {
                #[allow(unsafe_code)]
                // SAFETY: the handle is open, and closed once, here.
                unsafe {
                    CloseHandle(handle);
                }
            }
            assert_eq!(left, 0, "each had ended once its end returned");
        });
    }
}

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

/// What one ask of a job's list of processes, given room for some ids, came to.
#[cfg(any(windows, test))]
#[derive(Debug)]
enum Listed {
    /// How many processes the job has, and the ids of those that fitted.
    Ids(usize, Vec<usize>),
    /// The system said there was not room (`ERROR_MORE_DATA`).
    More,
    /// The ask failed: why.
    Failed(String),
}

/// How many times a job's list is asked for with more room before it is given up.
#[cfg(any(windows, test))]
const LISTED_WITHIN: usize = 16;

/// The ids of every process of a job, as `ask` says them given room for some (256 at
/// first): asked again with room for all it says it has, and more, until every one of
/// them fits (as Windows asks: both reviewers' look at round 7, the list held 256 ids
/// at most). Windows says there is not room rather than list some (zeewin 2026-10-21, a
/// job of 300 asked with room for 256), which round 7 took for a job of none. Why not,
/// where an ask fails, or the list outgrows each room it is given.
#[cfg(any(windows, test))]
fn listed_whole(mut ask: impl FnMut(usize) -> Listed) -> Result<Vec<usize>, String> {
    let mut room = 256;
    for _ in 0..LISTED_WITHIN {
        match ask(room) {
            Listed::Ids(assigned, ids) if ids.len() >= assigned => return Ok(ids),
            Listed::Ids(assigned, _) => room = room.max(assigned) + 64,
            Listed::More => room *= 2,
            Listed::Failed(why) => return Err(format!("its processes could not be listed: {why}")),
        }
    }
    Err(format!(
        "its processes could not be listed: the list outgrew room for {room} ids"
    ))
}

/// The processes of a job, each held by a handle that says when it has ended (it is
/// signalled), opened before the job is ended; closed when this is dropped. A process
/// that ended before it could be opened is not held, nor is one the job does not hold,
/// which has its id since the list was read (both reviewers' look at round 7: waited
/// for, a stranger failed the run).
#[cfg(windows)]
struct Held(Vec<(usize, usize)>);

#[cfg(windows)]
impl Held {
    /// The processes `pids` of the job `job`, those of them still there and in it.
    fn open(job: usize, pids: Vec<usize>) -> Self {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::JobObjects::IsProcessInJob;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        };
        Self(
            pids.into_iter()
                .filter_map(|pid| {
                    let id = u32::try_from(pid).ok()?;
                    #[allow(unsafe_code)]
                    // SAFETY: OpenProcess takes plain values and returns a handle or null;
                    // IsProcessInJob reads the two handles, open, and writes `inside`, ours;
                    // a handle not kept is closed once, here.
                    unsafe {
                        let handle = OpenProcess(
                            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                            0,
                            id,
                        );
                        if handle.is_null() {
                            return None;
                        }
                        let mut inside = 0;
                        if IsProcessInJob(handle, job as _, &mut inside) == 0 || inside == 0 {
                            CloseHandle(handle);
                            return None;
                        }
                        Some((pid, handle as usize))
                    }
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
    /// set as its end begins. Each is held only where the job holds it once it is
    /// opened: a process listed that ended before then, its id given to another, is
    /// not waited for in its stead (both reviewers' look at round 7). What is left: a
    /// process the job starts in the moment between the list and the end is ended and
    /// not waited for. A job whose processes cannot be listed whole is ended all the
    /// same, and has failed: none of them could be waited for.
    pub(crate) fn end(&self) -> Result<(), String> {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        let listed = self.processes();
        let held = Held::open(self.handle, listed.clone().unwrap_or_default());
        #[allow(unsafe_code)]
        // SAFETY: the handle is the job's, open until the job is dropped; the call
        // writes no memory of ours.
        unsafe {
            TerminateJobObject(self.handle as _, 1);
        }
        listed?;
        ended_within(JOB_ENDS_WITHIN, || held.left())
    }

    /// The ids of every process the job holds (`listed_whole`), or why not.
    fn processes(&self) -> Result<Vec<usize>, String> {
        listed_whole(|room| self.list(room))
    }

    /// One ask of the job's list of processes, with room for `room` ids.
    fn list(&self, room: usize) -> Listed {
        use windows_sys::Win32::Foundation::ERROR_MORE_DATA;
        use windows_sys::Win32::System::JobObjects::{
            JobObjectBasicProcessIdList, QueryInformationJobObject, JOBOBJECT_BASIC_PROCESS_ID_LIST,
        };
        // JOBOBJECT_BASIC_PROCESS_ID_LIST, its two counts and then the ids, laid out in
        // words of an id's size, so that the buffer is aligned as the list is.
        let header = std::mem::offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList)
            / std::mem::size_of::<usize>();
        let mut words = vec![0usize; header + room];
        let Ok(size) = u32::try_from(words.len() * std::mem::size_of::<usize>()) else {
            return Listed::Failed(format!("no room for {room} ids can be asked for"));
        };
        #[allow(unsafe_code)]
        // SAFETY: the buffer is `words`, writable for the `size` bytes passed with it,
        // aligned for and laid out as JOBOBJECT_BASIC_PROCESS_ID_LIST with room for
        // `room` ids, and the call writes no more than that; its counts are read from
        // it as that struct once it has.
        let (asked, assigned, listed) = unsafe {
            let asked = QueryInformationJobObject(
                self.handle as _,
                JobObjectBasicProcessIdList,
                words.as_mut_ptr().cast(),
                size,
                std::ptr::null_mut(),
            );
            let list = words.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
            (
                asked,
                (*list).NumberOfAssignedProcesses,
                (*list).NumberOfProcessIdsInList,
            )
        };
        if asked == 0 {
            let error = std::io::Error::last_os_error();
            return match u32::try_from(error.raw_os_error().unwrap_or(0)) {
                Ok(ERROR_MORE_DATA) => Listed::More,
                _ => Listed::Failed(error.to_string()),
            };
        }
        let count = |count: u32| usize::try_from(count).unwrap_or(usize::MAX);
        let listed = count(listed).min(room);
        Listed::Ids(count(assigned), words[header..header + listed].to_vec())
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

    #[test]
    fn a_job_s_list_is_asked_for_again_with_room_for_all_it_has_and_an_ask_that_fails_is_an_error()
    {
        // Both reviewers' look at round 7: the list held 256 ids at most, the rest of a
        // job's processes never waited for; and an ask that failed was an empty list, a
        // job with nothing to wait for.
        let job: Vec<usize> = (1..=300).collect();
        let mut rooms = Vec::new();
        let listed = listed_whole(|room| {
            rooms.push(room);
            Listed::Ids(job.len(), job[..room.min(job.len())].to_vec())
        });
        assert_eq!(listed, Ok(job.clone()));
        assert_eq!(rooms, [256, 364], "asked again with room for all it has");
        // Told only that there is not room, it is given more.
        let mut asks = 0;
        let listed = listed_whole(|room| {
            asks += 1;
            if room < job.len() {
                Listed::More
            } else {
                Listed::Ids(job.len(), job.clone())
            }
        });
        assert_eq!((listed, asks), (Ok(job.clone()), 2));
        assert_eq!(
            listed_whole(|_| Listed::Failed("Access is denied. (os error 5)".to_owned())),
            Err("its processes could not be listed: Access is denied. (os error 5)".to_owned())
        );
        // One that outgrows each room it is given is given up.
        let mut grows = 0;
        let listed = listed_whole(|room| {
            grows += 1;
            Listed::Ids(room * 3, Vec::new())
        });
        assert!(listed.is_err(), "{listed:?}");
        assert_eq!(grows, LISTED_WITHIN);
    }

    /// A job of its own for a test, every process of which is ended when it is dropped,
    /// and `count` programs put in it, each started suspended (a ping of a minute's,
    /// which never runs): their ids.
    #[cfg(windows)]
    #[allow(clippy::disallowed_methods)] // The test starts what the job ends.
    fn a_job_of(count: usize) -> (Job, Vec<usize>) {
        use std::os::windows::io::AsRawHandle;
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};
        #[allow(unsafe_code)]
        // SAFETY: CreateJobObjectW takes no attributes and no name and returns a handle,
        // closed by the job's drop; SetInformationJobObject reads `limits`, whole.
        let job = unsafe {
            let made = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            assert!(!made.is_null());
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let size = u32::try_from(std::mem::size_of_val(&limits)).unwrap();
            let set = SetInformationJobObject(
                made,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size,
            );
            assert_ne!(set, 0);
            Job {
                handle: made as usize,
            }
        };
        let ids = (0..count)
            .map(|_| {
                let child = std::process::Command::new(r"C:\Windows\System32\PING.EXE")
                    .args(["-n", "60", "127.0.0.1"])
                    .creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW)
                    .spawn()
                    .unwrap();
                #[allow(unsafe_code)]
                // SAFETY: both handles are open; the call writes no memory of ours.
                let put =
                    unsafe { AssignProcessToJobObject(job.handle as _, child.as_raw_handle()) };
                assert_ne!(put, 0, "{}", std::io::Error::last_os_error());
                usize::try_from(child.id()).unwrap()
            })
            .collect();
        (job, ids)
    }

    #[cfg(windows)]
    #[test]
    fn a_job_s_list_names_every_process_of_it_past_the_256_it_once_held() {
        let (job, mut started) = a_job_of(300);
        let mut listed = job.processes().unwrap();
        listed.sort_unstable();
        started.sort_unstable();
        assert_eq!(listed.len(), 300);
        assert_eq!(listed, started);
        // Each is ended with the job: 300 ended at once take longer than a job's end waits
        // for them on zeewin (4 were left at 2 s, 2026-10-21), so they are waited for here,
        // a while longer, each by a handle opened before the end.
        let held = Held::open(job.handle, listed);
        let _ = job.end();
        assert_eq!(
            ended_within(std::time::Duration::from_secs(60), || held.left()),
            Ok(())
        );
        // A job whose list cannot be asked for (the handle of an event here, no job: a
        // null one is this process's own job) has not ended: it had none of its processes
        // to wait for.
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::CreateEventW;
        #[allow(unsafe_code)]
        // SAFETY: CreateEventW takes no attributes and no name and returns a handle, closed
        // once, below.
        let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        assert!(!event.is_null());
        let none = Job {
            handle: event as usize,
        };
        let ended = none.end();
        std::mem::forget(none);
        #[allow(unsafe_code)]
        // SAFETY: the event's handle is open, and closed once, here.
        unsafe {
            CloseHandle(event);
        }
        assert!(
            ended
                .as_ref()
                .is_err_and(|why| why.starts_with("its processes could not be listed: ")),
            "{ended:?}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_process_listed_that_the_job_does_not_hold_once_it_is_opened_is_not_waited_for() {
        // Both reviewers' look at round 7: a process of the job's list that ended before it
        // was opened, its id given to another, was waited for in its stead, and failed the
        // run. This process, in no job of the test's, is such a one.
        let (job, ids) = a_job_of(1);
        let this = usize::try_from(std::process::id()).unwrap();
        let held = Held::open(job.handle, vec![ids[0], this]);
        assert_eq!(
            held.0.iter().map(|(pid, _)| *pid).collect::<Vec<_>>(),
            [ids[0]],
            "only the job's own is held"
        );
        drop(held);
        assert_eq!(job.end(), Ok(()));
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

//! The listener of a job's births (T-1145, T-1153's D2 and D3), on real jobs: a birth is kept
//! for as long as its process runs, however many short-lived ones come after it, and a job
//! that is dropped ends its listener and closes its port.
#![allow(clippy::disallowed_methods)] // A test's own programs.

use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::Weak;
use std::thread::sleep;
use std::time::{Duration, Instant};

use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};

use super::*;

/// No console window for a program the test starts.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The system's own folder of programs, by the name of one.
fn system(program: &str) -> String {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    format!(r"{root}\System32\{program}")
}

/// Waits up to `seconds` for `done`, looked at every 50 ms.
fn until(seconds: u64, what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while !done() {
        assert!(Instant::now() < deadline, "never: {what}");
        sleep(Duration::from_millis(50));
    }
}

/// How many handles this process has open.
fn handles() -> u32 {
    let mut count = 0u32;
    // SAFETY: the pseudo-handle of this process, and a writable count.
    let read = unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) };
    assert_ne!(read, 0, "{}", io::Error::last_os_error());
    count
}

/// A PowerShell that is put in `job` a moment after it starts, running `script` after it has
/// slept for two seconds (a child it started in the first instant would escape the job).
fn a_shell_in(job: &JobObject, script: &str) -> std::process::Child {
    let shell = Command::new(system(r"WindowsPowerShell\v1.0\powershell.exe"))
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(format!("Start-Sleep 2; {script}"))
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .unwrap();
    // SAFETY: both handles are open, the process's one until `shell` is dropped.
    let assigned =
        unsafe { AssignProcessToJobObject(job.handle(), shell.as_raw_handle() as HANDLE) };
    assert_ne!(assigned, 0, "{}", io::Error::last_os_error());
    shell
}

/// A server the agent leaves running keeps its birth while more processes than the bound of
/// the record come and go after it (T-1153's D2): the exits are told to the listener, which
/// lets go of them. Before, the oldest birth was let go of by age, a running server's
/// among them, and the server was no more proved the agent's.
#[test]
fn a_birth_of_a_server_that_runs_is_kept_while_more_short_lived_processes_than_the_bound_come_and_go(
) {
    let folder = std::env::temp_dir().join(format!("cf-panes-births-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let done = folder.join("spawned.txt");
    let _ = std::fs::remove_file(&done);
    let ping = system("PING.EXE");
    let cmd = system("cmd.exe");
    let job = JobObject::with_births(Births::with_cap(8)).unwrap();
    let mut shell = a_shell_in(
        &job,
        &format!(
            "Start-Process -WindowStyle Hidden -FilePath '{ping}' -ArgumentList '-n','1009','127.0.0.1'; \
             Start-Sleep 1; \
             1..40 | ForEach-Object {{ Start-Process -WindowStyle Hidden -FilePath '{cmd}' -ArgumentList '/c','exit' -Wait }}; \
             New-Item -ItemType File -Path '{}' | Out-Null; \
             Start-Sleep 1000",
            done.display()
        ),
    );

    until(
        120,
        "the shell to have started its short-lived processes",
        || done.exists(),
    );
    // The listener has read what the job told it; more than the bound has come and gone.
    sleep(Duration::from_secs(2));
    let members = job.members_with(&|_| None).unwrap();
    let server = members
        .iter()
        .find(|member| member.command.contains("-n 1009"))
        .unwrap_or_else(|| panic!("the server runs in the job: {members:?}"));
    assert!(
        job.started_by_a_shell(server.key),
        "the birth of a server that runs was let go of"
    );
    let _ = shell.kill();
    let _ = std::fs::remove_dir_all(&folder);
}

/// A job that is dropped ends the listener of its births and closes its port (T-1153's D3):
/// the listener holds the record, so that it has ended once nothing has the record but a weak
/// reference taken before the drop, and the process's handles are those it had.
#[test]
fn a_job_that_is_dropped_ends_its_listener_and_closes_its_port() {
    const JOBS: usize = 100;
    // The first job warms what the first use of a port allocates.
    drop(JobObject::new().unwrap());
    let before = handles();

    let jobs: Vec<JobObject> = (0..JOBS).map(|_| JobObject::new().unwrap()).collect();
    assert!(
        jobs.iter().all(|job| job.port.is_some()),
        "every job has its listener"
    );
    let listeners: Vec<Weak<Mutex<Births>>> =
        jobs.iter().map(|job| Arc::downgrade(&job.births)).collect();
    assert!(
        handles() >= before + JOBS as u32,
        "the jobs hold handles of their own"
    );
    drop(jobs);

    until(30, "every listener to have ended", || {
        listeners
            .iter()
            .all(|listener| listener.upgrade().is_none())
    });
    // The jobs' handles, and the ports the listeners closed as they ended: what is left is
    // other tests' noise, a few handles, never one for each job.
    until(30, "the jobs' handles to have been closed", || {
        handles() < before + (JOBS as u32) / 4
    });
}

//! What the tests of programs that start programs share: a program writes
//! the pids of the tree it made to a file, the test reads them and asks the
//! system whether each is still there, and whatever a failing test left
//! running is ended as the test goes, since nothing else would.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::{alive, terminate, Ending, Run};

/// How long a test waits for a program to write down what it started: how
/// soon it starts is not what is tested, and PowerShell started cold beside a
/// crate's other tests on a slow runner took longer than the ending is given
/// (CI's Windows, run 39055885804).
pub(crate) const STARTS_WITHIN: Duration = Duration::from_secs(120);

/// How long a test waits for the system to say that a process has gone.
pub(crate) const GONE_WITHIN: Duration = Duration::from_secs(20);

/// The `count` pids a program has written to `file`, if it has written them
/// all: white space between, and a line's end last, so that a line half
/// written is not read.
pub(crate) fn pids_in(file: &Path, count: usize) -> Option<Vec<u32>> {
    let written = std::fs::read_to_string(file).ok()?;
    let pids: Vec<u32> = written
        .split_whitespace()
        .filter_map(|word| word.parse().ok())
        .collect();
    (pids.len() == count && written.ends_with('\n')).then_some(pids)
}

/// The pids the program wrote to `file` (see [`pids_in`]), waited for on the
/// test's own thread: the program is running.
#[cfg(unix)]
pub(crate) fn pids_written_to(file: &Path, count: usize) -> Vec<u32> {
    let until = Instant::now() + STARTS_WITHIN;
    while Instant::now() < until {
        if let Some(pids) = pids_in(file, count) {
            return pids;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the program never wrote {count} pids to {}", file.display());
}

/// The pids the program wrote to `file`, waited for on the runtime that runs
/// the capture, which would stop were it waited for by the thread.
pub(crate) async fn pids_written_to_soon(file: &Path, count: usize) -> Vec<u32> {
    let until = Instant::now() + STARTS_WITHIN;
    while Instant::now() < until {
        if let Some(pids) = pids_in(file, count) {
            return pids;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the program never wrote {count} pids to {}", file.display());
}

/// Whether the process `pid` is gone within `within`: one the system reaps
/// as it ends (an orphan is its init's) is gone as soon as it is.
pub(crate) fn gone_within(pid: u32, within: Duration) -> bool {
    let until = Instant::now() + within;
    while alive(pid) {
        if Instant::now() >= until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    true
}

/// Whether the process `pid` is gone, given the time the system needs.
pub(crate) fn gone(pid: u32) -> bool {
    gone_within(pid, GONE_WITHIN)
}

/// Processes a test started, ended when it is dropped if they are still
/// there: a test that fails with them running must not leave them for as long
/// as they sleep. One that has passed has found them gone, and so sends
/// nothing to a pid that may be another's by now.
#[derive(Default)]
pub(crate) struct Survivors(pub(crate) Vec<u32>);

impl Drop for Survivors {
    fn drop(&mut self) {
        for &pid in &self.0 {
            if alive(pid) {
                terminate(pid, Ending::Forced);
            }
        }
    }
}

/// How many programs [`helper_keeps_starting_programs`] starts at most, and how long it
/// goes on starting them: a test that does not end it leaves this many, and no more
/// (zeewin, T-322: a loop with no bound reached 15,800 pings).
pub(crate) const KEEPS_STARTING: usize = 200;
const KEEPS_STARTING_FOR: Duration = Duration::from_secs(10);

/// How long each program the helper starts runs unless it is ended: far more than a test
/// waits for the end of what it started, and few enough seconds that a test that fails
/// leaves nothing for long.
const STARTED_RUN_FOR: u32 = 20;

/// A program that keeps starting programs, as an installer's script does, and writes the
/// pids to `folder/pids` as it goes: its own first, then each program's right after it
/// started. Every one is left its streams, as Windows gives a child every handle it may
/// inherit. It goes on for [`KEEPS_STARTING_FOR`] at most, and then waits to be ended.
pub(crate) fn keeps_starting_programs(folder: &Path) -> Run {
    Run {
        program: std::env::current_exe().unwrap(),
        args: [
            "--exact",
            "testing::helper_keeps_starting_programs",
            "--ignored",
        ]
        .into_iter()
        .map(Into::into)
        .chain([folder.as_os_str().to_owned()])
        .collect(),
        verbatim: false,
    }
}

/// The program [`keeps_starting_programs`] runs, and no test of its own.
#[test]
#[ignore = "a program the tests of ending a program start"]
// The helper starts what the test ends, and waits for none of it.
#[allow(clippy::disallowed_methods, clippy::zombie_processes)]
fn helper_keeps_starting_programs() {
    use std::io::Write;
    let folder = PathBuf::from(std::env::args().next_back().unwrap());
    let mut pids = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(folder.join("pids"))
        .unwrap();
    writeln!(pids, "{}", std::process::id()).unwrap();
    let until = Instant::now() + KEEPS_STARTING_FOR;
    for count in 0..KEEPS_STARTING {
        if Instant::now() >= until {
            break;
        }
        // Each is marked by an address of its own (127.0.53.x), so that a ping that a
        // test left is told from every other.
        #[cfg(windows)]
        let started = std::process::Command::new(r"C:\Windows\System32\PING.EXE")
            .args(["-n", &(STARTED_RUN_FOR + 1).to_string()])
            .arg(format!("127.0.53.{}", count % 250 + 1))
            .spawn();
        #[cfg(unix)]
        let started = {
            let _ = count;
            std::process::Command::new("/bin/sleep")
                .arg(STARTED_RUN_FOR.to_string())
                .spawn()
        };
        let started = started.unwrap();
        writeln!(pids, "{}", started.id()).unwrap();
        pids.flush().unwrap();
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(KEEPS_STARTING_FOR + Duration::from_secs(20));
}

/// Every pid the program wrote to `file` so far, whole lines only.
pub(crate) fn pids_so_far(file: &Path) -> Vec<u32> {
    let written = std::fs::read_to_string(file).unwrap_or_default();
    let Some((lines, _unfinished)) = written.rsplit_once('\n') else {
        return Vec::new();
    };
    lines
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

/// Which of the pids written to `file` are still running, asked again for `within` at
/// most until none is: the file is read each time, for a program may be writing it still.
/// Waited for on the runtime that ran the program, which reaps a child that was dropped
/// as it turns: a child not reaped is a zombie, and reads as running.
pub(crate) async fn left_running(file: &Path, within: Duration) -> Vec<u32> {
    let until = Instant::now() + within;
    loop {
        let left: Vec<u32> = pids_so_far(file)
            .into_iter()
            .filter(|&pid| alive(pid))
            .collect();
        if left.is_empty() || Instant::now() >= until {
            return left;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Ends, as the test ends, whatever is running of the pids written to `file`: a test that
/// fails with them running must not leave them. All in one `taskkill` on Windows: up to
/// [`KEEPS_STARTING`] of them one after another would take a minute.
pub(crate) struct Recorded<'a>(pub(crate) &'a Path);

impl Drop for Recorded<'_> {
    #[allow(clippy::disallowed_methods)] // The test ends what it started.
    fn drop(&mut self) {
        let left: Vec<u32> = pids_so_far(self.0)
            .into_iter()
            .filter(|&pid| alive(pid))
            .collect();
        if left.is_empty() {
            return;
        }
        #[cfg(windows)]
        {
            let mut command = std::process::Command::new("taskkill");
            command.arg("/F");
            for pid in &left {
                command.args(["/PID", &pid.to_string()]);
            }
            let _ = command.stdout(std::process::Stdio::null()).status();
        }
        #[cfg(unix)]
        for pid in left {
            terminate(pid, Ending::Forced);
        }
    }
}

/// A program that leaves a child running and ends at once: the child has none of
/// its streams, and writes `late` to the file `marked` once `after` has passed;
/// `pids` gets the program's pid and the child's, as `tree` writes them.
pub(crate) fn leaves_a_writer(pids: &Path, marked: &Path, after: u32) -> Run {
    #[cfg(unix)]
    return Run {
        program: PathBuf::from("/bin/sh"),
        args: [
            "-c".to_owned(),
            format!(
                "(sleep {after}; echo late > '{}') </dev/null >/dev/null 2>&1 & printf '%s %s\\n' \"$$\" \"$!\" > '{}'",
                marked.display(),
                pids.display()
            ),
        ]
        .map(OsString::from)
        .to_vec(),
        verbatim: false,
    };
    #[cfg(windows)]
    return Run {
        program: PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"),
        args: [
            "-NoProfile".to_owned(),
            "-Command".to_owned(),
            format!(
                "$p = Start-Process -PassThru -WindowStyle Hidden -FilePath 'C:\\Windows\\System32\\cmd.exe' -ArgumentList '/d','/c','ping -n {} 127.0.0.1 >NUL & echo late> \"{}\"'; \
                 Set-Content -Path '{}' -Value (@($PID, $p.Id) -join ' ')",
                after + 1,
                marked.display(),
                pids.display().to_string().replace('\'', "''")
            ),
        ]
        .map(OsString::from)
        .to_vec(),
        verbatim: false,
    };
}

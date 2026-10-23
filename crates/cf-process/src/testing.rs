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
    keeps_starting_every(folder, Duration::from_millis(5))
}

/// As [`keeps_starting_programs`], one every 40 ms: all [`KEEPS_STARTING`] take about nine
/// seconds, which a capture out of time in five (a slow runner's margin) still finds starting
/// them.
pub(crate) fn keeps_starting_programs_for_long(folder: &Path) -> Run {
    keeps_starting_every(folder, Duration::from_millis(40))
}

fn keeps_starting_every(folder: &Path, pace: Duration) -> Run {
    Run {
        program: std::env::current_exe().unwrap(),
        args: [
            "--exact",
            "testing::helper_keeps_starting_programs",
            "--ignored",
        ]
        .into_iter()
        .map(Into::into)
        .chain([
            pace.as_millis().to_string().into(),
            folder.as_os_str().to_owned(),
        ])
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
    let words: Vec<String> = std::env::args().collect();
    let [.., pace, folder] = &words[..] else {
        panic!("no pace and folder");
    };
    let (pace, folder) = (
        Duration::from_millis(pace.parse().unwrap()),
        PathBuf::from(folder),
    );
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
        std::thread::sleep(pace);
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

/// A program that starts two programs and writes what came of each, to `folder/pids` (its
/// own pid, then the first's, then the second's if it started) and to `folder/outcome` (one
/// word each): a ping that asks for nothing (`stays`), and a ping started as a program asks
/// that wants its child outside its job (`CREATE_BREAKAWAY_FROM_JOB`): `breaks` where it
/// started, `refused` and the system's code where the job would not have it. It then waits
/// for a half minute at most, to be ended. Windows only: a job is.
#[cfg(windows)]
pub(crate) fn starts_with_and_without_breakaway(folder: &Path) -> Run {
    Run {
        program: std::env::current_exe().unwrap(),
        args: [
            "--exact",
            "testing::helper_starts_with_and_without_breakaway",
            "--ignored",
        ]
        .into_iter()
        .map(Into::into)
        .chain([folder.as_os_str().to_owned()])
        .collect(),
        verbatim: false,
    }
}

/// The program [`starts_with_and_without_breakaway`] runs, and no test of its own.
#[cfg(windows)]
#[test]
#[ignore = "a program the breakaway tests start"]
// The helper starts what the test ends, and waits for none of it.
#[allow(clippy::disallowed_methods, clippy::zombie_processes)]
fn helper_starts_with_and_without_breakaway() {
    use std::io::Write;
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::{CREATE_BREAKAWAY_FROM_JOB, CREATE_NO_WINDOW};
    let folder = PathBuf::from(std::env::args().next_back().unwrap());
    let open = |name: &str| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(folder.join(name))
            .unwrap()
    };
    let (mut pids, mut outcome) = (open("pids"), open("outcome"));
    writeln!(pids, "{}", std::process::id()).unwrap();
    let ping = |address: &str, flags: u32| {
        std::process::Command::new(r"C:\Windows\System32\PING.EXE")
            .args(["-n", "121", address])
            .creation_flags(flags | CREATE_NO_WINDOW)
            .stdout(std::process::Stdio::null())
            .spawn()
    };
    // Each is marked by an address of its own, so that a ping a test left is told from the rest.
    // Each pings for two minutes: one that is gone a few seconds after the end was ended by it,
    // and not by running out (21 pings were 20 s, which a test that waited 10 s for the capture
    // and 5 s more for the ping could outlast).
    let stays = ping("127.0.55.1", 0).unwrap();
    writeln!(pids, "{}", stays.id()).unwrap();
    writeln!(outcome, "stays").unwrap();
    match ping("127.0.55.2", CREATE_BREAKAWAY_FROM_JOB) {
        Ok(breaks) => {
            writeln!(pids, "{}", breaks.id()).unwrap();
            writeln!(outcome, "breaks").unwrap();
        }
        Err(refused) => writeln!(outcome, "refused {:?}", refused.raw_os_error()).unwrap(),
    }
    pids.flush().unwrap();
    outcome.flush().unwrap();
    std::thread::sleep(Duration::from_secs(30));
}

/// How many programs [`starts_holders_and_says_too_much`] has hold a file.
#[cfg(windows)]
pub(crate) const HOLDERS: usize = 8;

/// A program that starts [`HOLDERS`] programs that each hold a file of their own open (`held-0`
/// and the rest, in `folder`, shared with no one) and 1000 threads, waits until all of them are
/// there, and says far more than a capture holds it to: the capture has it out of its limit,
/// and ends it. Every pid is written to `folder/pids`. Windows only: it asks what a job's end
/// is waited for.
#[cfg(windows)]
pub(crate) fn starts_holders_and_says_too_much(folder: &Path) -> Run {
    Run {
        program: std::env::current_exe().unwrap(),
        args: [
            "--exact",
            "testing::helper_starts_holders_and_says_too_much",
            "--ignored",
        ]
        .into_iter()
        .map(Into::into)
        .chain([folder.as_os_str().to_owned()])
        .collect(),
        verbatim: false,
    }
}

/// The program [`starts_holders_and_says_too_much`] runs, and no test of its own.
#[cfg(windows)]
#[test]
#[ignore = "a program the tests of a job's end start"]
// The helper starts what the test ends, and waits for none of it.
#[allow(clippy::disallowed_methods, clippy::zombie_processes)]
fn helper_starts_holders_and_says_too_much() {
    use std::io::Write;
    let folder = PathBuf::from(std::env::args().next_back().unwrap());
    let mut pids = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(folder.join("pids"))
        .unwrap();
    writeln!(pids, "{}", std::process::id()).unwrap();
    for index in 0..HOLDERS {
        let holder = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "testing::helper_holds_a_file_and_many_threads",
                "--ignored",
            ])
            .arg(&folder)
            .arg(index.to_string())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        writeln!(pids, "{}", holder.id()).unwrap();
    }
    pids.flush().unwrap();
    let until = Instant::now() + STARTS_WITHIN;
    while Instant::now() < until {
        let ready = std::fs::read_dir(&folder)
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .is_ok_and(|entry| entry.path().extension().is_some_and(|ext| ext == "ready"))
            })
            .count();
        if ready >= HOLDERS {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // Far more than a limit of a kilobyte, written to the stream itself: a `println!` of a test
    // is kept by the harness unless it is asked not to.
    let mut stdout = std::io::stdout();
    stdout.write_all("x".repeat(8 * 1024).as_bytes()).unwrap();
    stdout.flush().unwrap();
    std::thread::sleep(Duration::from_secs(60));
}

/// A program that [`starts_holders_and_says_too_much`] starts, and no test of its own: a file
/// of `folder` it holds open shared with no one, 1000 threads, a file that says it is there,
/// and a minute's wait. It is a minute at most from being ended by the test.
#[cfg(windows)]
#[test]
#[ignore = "a program the tests of a job's end start"]
fn helper_holds_a_file_and_many_threads() {
    use std::os::windows::fs::OpenOptionsExt;
    let words: Vec<String> = std::env::args().collect();
    let [.., folder, index] = &words[..] else {
        panic!("no folder and index");
    };
    let held = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .share_mode(0)
        .open(Path::new(folder).join(format!("held-{index}")))
        .unwrap();
    for _ in 0..1000 {
        std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(|| std::thread::sleep(Duration::from_secs(60)))
            .unwrap();
    }
    let ready = Path::new(folder).join(format!("{}.ready", std::process::id()));
    std::fs::write(ready, "").unwrap();
    std::thread::sleep(Duration::from_secs(60));
    drop(held);
}

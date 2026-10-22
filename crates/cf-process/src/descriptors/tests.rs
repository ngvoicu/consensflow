#![cfg(target_os = "macos")]

//! A program started here holds nothing of this one's but its standard streams, whatever
//! this one had open at that moment: on macOS std makes a new socket or pipe close-on-exec
//! in a second step, so a program started on another thread in between is left it, and
//! keeps it for as long as it runs (a listener that goes on taking connections after it
//! was closed, a pipe whose reader never reads the end of it).
//!
//! The moment is made to last here: a descriptor is opened and left as std leaves it for
//! that moment, open and not yet close-on-exec, while the program is started.

use std::ffi::OsString;
use std::fs::File;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cf_base::env::Env;
use nix::fcntl::{fcntl, FcntlArg, FdFlag};

use crate::{capture, hand_over, spawn, Limits, Run, Streams};

/// What the descriptors are: the three shapes of a descriptor that is open for a moment in
/// a program that starts others: a file, a listening socket and a pipe's end.
struct Open {
    _file: File,
    _listener: TcpListener,
    _pipe: [OwnedFd; 2],
}

impl Open {
    /// Each is open, and none is close-on-exec: the moment before std makes it so.
    fn not_yet_close_on_exec() -> (Self, Vec<i32>) {
        let file = File::open("/dev/null").unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let pipe = nix::unistd::pipe().unwrap();
        let open = Self {
            _file: file,
            _listener: listener,
            _pipe: [pipe.0, pipe.1],
        };
        let fds = [
            open._file.as_raw_fd(),
            open._listener.as_raw_fd(),
            open._pipe[0].as_raw_fd(),
            open._pipe[1].as_raw_fd(),
        ];
        for fd in fds {
            fcntl(fd, FcntlArg::F_SETFD(FdFlag::empty())).unwrap();
        }
        (open, fds.to_vec())
    }
}

/// A shell that says, for each descriptor, `held` or `clear`, by whether it is open in it:
/// `/dev/fd/<n>` is there for the descriptors the shell has open.
fn asking(fds: &[i32], then: &str) -> Run {
    let tests: String = fds
        .iter()
        .map(|fd| {
            format!("if [ -e /dev/fd/{fd} ]; then printf 'held '; else printf 'clear '; fi; ")
        })
        .collect();
    Run {
        program: PathBuf::from("/bin/sh"),
        args: vec![
            OsString::from("-c"),
            OsString::from(format!("{tests}{then}")),
        ],
        verbatim: false,
    }
}

fn system_env() -> Env {
    Env::from_vars([("PATH", "/usr/bin:/bin")])
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn all_clear(fds: &[i32]) -> String {
    "clear ".repeat(fds.len())
}

#[test]
fn a_program_run_beside_this_one_holds_no_descriptor_that_was_open_when_it_started() {
    let (_open, fds) = Open::not_yet_close_on_exec();
    let runtime = runtime();
    let _entered = runtime.enter();
    let child = spawn(
        &asking(&fds, "printf '\\n'"),
        None,
        &system_env(),
        Streams::Lines,
    )
    .unwrap();
    let line = runtime.block_on(child.read_line(1024)).unwrap().unwrap();
    runtime.block_on(child.closed());
    assert_eq!(line, all_clear(&fds));
}

#[test]
fn a_program_let_go_holds_no_descriptor_that_was_open_when_it_started() {
    let (_open, fds) = Open::not_yet_close_on_exec();
    let folder = tempfile::tempdir().unwrap();
    let said = folder.path().join("said");
    // A program let go has no stream to say it on: it writes a file, whole or not at all.
    let tests = asking(&fds, "true").args[1].to_string_lossy().into_owned();
    let run = Run {
        program: PathBuf::from("/bin/sh"),
        args: vec![
            OsString::from("-c"),
            OsString::from(format!(
                "({tests}) > '{part}' 2>&1; mv '{part}' '{said}'",
                part = folder.path().join("part").display(),
                said = said.display()
            )),
        ],
        verbatim: false,
    };
    runtime()
        .block_on(hand_over(&run, &system_env(), Duration::from_secs(60)))
        .unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    while !said.exists() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(std::fs::read_to_string(&said).unwrap(), all_clear(&fds));
}

#[test]
fn a_program_run_to_its_end_holds_no_descriptor_that_was_open_when_it_started() {
    let (_open, fds) = Open::not_yet_close_on_exec();
    let captured = runtime()
        .block_on(capture(
            &asking(&fds, "true"),
            None,
            &system_env(),
            Limits {
                timeout: Duration::from_secs(60),
                max_buffer: 1024,
            },
            |_| {},
        ))
        .unwrap();
    assert_eq!(captured.stdout, all_clear(&fds));
}

/// The first of the descriptors a crowded process leaves open: above whatever the test
/// harness, the runtime and the program asked have open of their own.
const CROWD_FROM: i32 = 100;

/// How many, from [`CROWD_FROM`] on: more than one listing holds.
const CROWD: i32 = 4200;

/// One more, far above the rest.
const FAR: i32 = 5000;

/// What a crowded process's limit on its descriptors is lowered to once they are open.
const LOWERED: u64 = 256;

/// Opens [`CROWD`] descriptors from [`CROWD_FROM`] and one at [`FAR`], none close-on-exec,
/// lowers the limit on descriptors to [`LOWERED`] (the hard one too when `hard`), and
/// starts a program that lists the descriptors it holds: it holds none of them.
fn crowded(hard: bool) {
    use nix::sys::resource::{getrlimit, setrlimit, Resource};
    if !crate::testing::is_alone() {
        return;
    }
    let (_, most) = getrlimit(Resource::RLIMIT_NOFILE).unwrap();
    setrlimit(Resource::RLIMIT_NOFILE, most.min(16_384), most).unwrap();
    let null = File::open("/dev/null").unwrap();
    let crowd: Vec<i32> = (CROWD_FROM..CROWD_FROM + CROWD)
        .chain([FAR])
        .map(|fd| nix::unistd::dup2(null.as_raw_fd(), fd).unwrap())
        .collect();
    for &fd in &crowd {
        assert!(!fcntl(fd, FcntlArg::F_GETFD)
            .map(|flags| flags & libc::FD_CLOEXEC != 0)
            .unwrap());
    }
    setrlimit(
        Resource::RLIMIT_NOFILE,
        LOWERED,
        if hard { LOWERED } else { most },
    )
    .unwrap();
    let runtime = runtime();
    let _entered = runtime.enter();
    let listing = Run {
        program: PathBuf::from("/bin/sh"),
        args: vec![
            OsString::from("-c"),
            OsString::from("echo $(/bin/ls /dev/fd)"),
        ],
        verbatim: false,
    };
    let child = spawn(&listing, None, &system_env(), Streams::Lines).unwrap();
    let line = runtime
        .block_on(child.read_line(64 * 1024))
        .unwrap()
        .unwrap();
    runtime.block_on(child.closed());
    let held: Vec<i32> = line
        .split_whitespace()
        .filter_map(|fd| fd.parse().ok())
        .filter(|fd| *fd >= CROWD_FROM)
        .collect();
    assert!(
        held.is_empty(),
        "the program held {} of the crowd: {held:?}",
        held.len()
    );
    // The program's own listing names its streams and the folder it reads: it was read.
    assert!(line.starts_with("0 1 2"), "{line}");
}

#[test]
fn a_program_holds_none_of_a_crowd_of_descriptors_whatever_the_limit_was_lowered_to() {
    crate::testing::alone(&[], "descriptors::tests::crowded_with_the_limit_lowered");
}

#[test]
fn a_program_holds_none_of_a_crowd_of_descriptors_whatever_the_hard_limit_was_lowered_to() {
    crate::testing::alone(
        &[],
        "descriptors::tests::crowded_with_the_hard_limit_lowered",
    );
}

#[test]
#[ignore = "run alone, by the test above it"]
fn crowded_with_the_limit_lowered() {
    crowded(false);
}

#[test]
#[ignore = "run alone, by the test above it"]
fn crowded_with_the_hard_limit_lowered() {
    crowded(true);
}

/// What a stress of a way of starting programs found.
struct Found {
    started: usize,
    listeners: usize,
    held: usize,
}

/// The race itself, not made to last: programs that run half a second are started on four
/// threads, by `starting` (which keeps going until it is told to stop, and counts what it
/// starts), while another opens a thousand listeners at a time, closes them all, and
/// `later` after connects to each port. A connect that goes through was answered by a
/// program that was left the closed listener's socket. A Mac's attention for fifteen
/// seconds, which is why the tests that use it are not run with the rest.
fn stress(
    starting: impl Fn(&AtomicBool, &AtomicUsize) + Send + Sync + 'static,
    later: Duration,
) -> Found {
    // One stress at a time: the programs of one, started as std starts them, would hold the
    // listeners of the other.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let stop = Arc::new(AtomicBool::new(false));
    let (started, listeners, held) = (
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
    );
    let starting = Arc::new(starting);
    let mut threads = Vec::new();
    for _ in 0..4 {
        let (stop, started, starting) = (stop.clone(), started.clone(), starting.clone());
        threads.push(std::thread::spawn(move || starting(&stop, &started)));
    }
    {
        let (stop, listeners, held) = (stop.clone(), listeners.clone(), held.clone());
        threads.push(std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let opened: Vec<TcpListener> = (0..1000)
                    .map(|_| TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap())
                    .collect();
                let ports: Vec<u16> = opened
                    .iter()
                    .map(|listener| listener.local_addr().unwrap().port())
                    .collect();
                drop(opened);
                listeners.fetch_add(ports.len(), Ordering::Relaxed);
                std::thread::sleep(later);
                for port in ports {
                    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
                    if TcpStream::connect_timeout(&address, Duration::from_secs(2)).is_ok() {
                        held.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }));
    }
    std::thread::sleep(Duration::from_secs(15));
    stop.store(true, Ordering::Relaxed);
    for thread in threads {
        thread.join().unwrap();
    }
    let found = Found {
        started: started.load(Ordering::Relaxed),
        listeners: listeners.load(Ordering::Relaxed),
        held: held.load(Ordering::Relaxed),
    };
    eprintln!(
        "{} programs started, {} listeners closed, {} still answered",
        found.started, found.listeners, found.held
    );
    found
}

/// A program that runs half a second.
fn sleeping() -> Run {
    Run {
        program: PathBuf::from("/bin/sleep"),
        args: vec![OsString::from("0.5")],
        verbatim: false,
    }
}

/// The window std leaves, shown: programs started as std starts them, with nothing done
/// about their descriptors, and a connect to each closed listener's port at once, which a
/// program being started holds for as long as it takes. This is the std of the toolchain
/// the repository is built with; when it passes no more, std has closed the window, and
/// the hook and the tests beside it can go.
/// `cargo test -p cf-process -- --ignored std_leaves`.
#[test]
#[ignore = "a stress of fifteen seconds that starts thousands of programs"]
#[allow(clippy::disallowed_methods)] // The window is std's: this is std, started as std starts.
fn std_leaves_a_listener_closed_here_to_a_program_started_while_it_was_made() {
    let found = stress(
        |stop, started| {
            let mut live = std::collections::VecDeque::new();
            while !stop.load(Ordering::Relaxed) {
                let mut bare = std::process::Command::new("/bin/sleep");
                bare.arg("0.5").stdin(Stdio::null());
                live.push_back(bare.spawn().unwrap());
                started.fetch_add(1, Ordering::Relaxed);
                if live.len() >= 48 {
                    live.pop_front().unwrap().wait().unwrap();
                }
            }
            for mut child in live {
                child.wait().unwrap();
            }
        },
        Duration::ZERO,
    );
    assert!(
        found.held > 0,
        "std left no program the socket of a listener: {} programs started, {} listeners closed",
        found.started,
        found.listeners
    );
}

/// What the hook is for, shown: programs started here, and a connect to each closed
/// listener's port a moment after, well past what a program being started takes.
/// `cargo test -p cf-process -- --ignored a_listener_closed_here`.
#[test]
#[ignore = "a stress of fifteen seconds that starts thousands of programs"]
fn a_listener_closed_here_is_not_held_by_a_program_started_while_it_was_made() {
    let found = stress(
        |stop, started| {
            let runtime = runtime();
            let _entered = runtime.enter();
            let mut live = std::collections::VecDeque::new();
            while !stop.load(Ordering::Relaxed) {
                live.push_back(spawn(&sleeping(), None, &system_env(), Streams::Quiet).unwrap());
                started.fetch_add(1, Ordering::Relaxed);
                if live.len() >= 48 {
                    runtime.block_on(live.pop_front().unwrap().closed());
                }
            }
            for child in live {
                runtime.block_on(child.closed());
            }
        },
        // Well past what a program being started takes to exec, on a loaded Mac, and well
        // within the half second the programs run.
        Duration::from_millis(150),
    );
    assert_eq!(found.held, 0);
}

/// A process about to end that leaves its starting of the next to another (Tauri's restart of the
/// app, a start that is not this crate's) first makes every descriptor it holds close-on-exec:
/// a crowd of them, more than one listing holds, none close-on-exec, and std's own start after.
#[test]
fn every_descriptor_is_made_close_on_exec_and_a_program_started_after_by_any_start_holds_none() {
    crate::testing::alone(&[], "descriptors::tests::swept");
}

#[test]
#[ignore = "run alone, by the test above it"]
#[allow(clippy::disallowed_methods)] // The start after is std's, not this crate's: the control.
fn swept() {
    use nix::sys::resource::{getrlimit, setrlimit, Resource};
    if !crate::testing::is_alone() {
        return;
    }
    let (_, most) = getrlimit(Resource::RLIMIT_NOFILE).unwrap();
    setrlimit(Resource::RLIMIT_NOFILE, most.min(16_384), most).unwrap();
    let null = File::open("/dev/null").unwrap();
    let crowd: Vec<i32> = (CROWD_FROM..CROWD_FROM + CROWD)
        .chain([FAR])
        .map(|fd| nix::unistd::dup2(null.as_raw_fd(), fd).unwrap())
        .collect();
    crate::close_all_on_exec().unwrap();
    let left: Vec<i32> = crowd
        .iter()
        .copied()
        .filter(|&fd| {
            !fcntl(fd, FcntlArg::F_GETFD)
                .map(|flags| flags & libc::FD_CLOEXEC != 0)
                .unwrap()
        })
        .collect();
    assert!(
        left.is_empty(),
        "{} left as they were: {left:?}",
        left.len()
    );
    let listed = std::process::Command::new("/bin/ls")
        .arg("/dev/fd")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let held: Vec<i32> = String::from_utf8_lossy(&listed.stdout)
        .split_whitespace()
        .filter_map(|fd| fd.parse().ok())
        .filter(|fd| *fd >= CROWD_FROM)
        .collect();
    assert!(
        held.is_empty(),
        "the program held {} of the crowd",
        held.len()
    );
}

/// The programs the tests start (and the tests [`crate::testing::alone`] runs again) hold
/// nothing of the tests' but their streams: the tests run side by side.
#[test]
fn a_program_a_test_starts_holds_no_descriptor_that_was_open_when_it_started() {
    let (_open, fds) = Open::not_yet_close_on_exec();
    let tests = asking(&fds, "true").args[1].clone();
    let said = crate::testing::command("/bin/sh")
        .arg("-c")
        .arg(tests)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&said.stdout), all_clear(&fds));
}

/// A sandbox in which the system will not say what this process holds: neither the listing of
/// its descriptors nor the size of its table. (A shell does not start in it with nothing in its
/// environment; `echo` does.)
const UNLISTED: &str = "(version 1)(allow default)(deny process-info*)";

/// A program whose start cannot tell which descriptors it was left, nor how far to look for
/// them, is not started: started, it might hold any of them.
#[test]
fn a_program_is_not_started_when_the_system_will_not_say_what_it_was_left() {
    crate::testing::alone(
        &["/usr/bin/sandbox-exec", "-p", UNLISTED],
        "descriptors::tests::unlisted",
    );
}

#[test]
#[ignore = "run alone, in a sandbox, by the test above it"]
fn unlisted() {
    if !crate::testing::is_alone() {
        return;
    }
    let (_open, _) = Open::not_yet_close_on_exec();
    let runtime = runtime();
    let _entered = runtime.enter();
    let echo = Run {
        program: PathBuf::from("/bin/echo"),
        args: vec![OsString::from("started")],
        verbatim: false,
    };
    let refused = spawn(&echo, None, &system_env(), Streams::Lines)
        .err()
        .expect("the program was started");
    assert_eq!(refused, "spawn /bin/echo EPERM");
}

/// What a start by fork and exec costs against one by `posix_spawn`, the way std starts a
/// program that has nothing to run first: how long `spawn` takes to return, which is how long
/// the new program holds every descriptor of this process before its exec lets the
/// close-on-exec ones go. Measured as this process is, with 256 MiB more of it resident, and
/// with 4,200 descriptors more open; the median, the 99th of 300 starts and the longest.
/// `cargo test -p cf-process --lib -- --ignored --nocapture what_a_start_by_fork_costs`.
#[test]
#[ignore = "a measurement, which says what it found and checks nothing"]
#[allow(clippy::disallowed_methods)] // std's own start is what the hook's is measured against.
fn what_a_start_by_fork_costs() {
    use nix::sys::resource::{getrlimit, setrlimit, Resource};

    fn measured(way: &str, hooked: bool) {
        let mut took: Vec<Duration> = (0..300)
            .map(|_| {
                let mut command = std::process::Command::new("/usr/bin/true");
                command
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                if hooked {
                    crate::hold_only_the_streams(&mut command);
                }
                let started = Instant::now();
                let mut child = command.spawn().unwrap();
                let took = started.elapsed();
                child.wait().unwrap();
                took
            })
            .collect();
        took.sort();
        eprintln!(
            "  {way}: median {:?}, 99th {:?}, longest {:?}",
            took[took.len() / 2],
            took[took.len() * 99 / 100],
            took[took.len() - 1]
        );
    }
    fn both(what: &str) {
        eprintln!("{what}:");
        measured("posix_spawn", false);
        measured("fork and exec, the hook", true);
    }

    both("this process");
    let resident = vec![1_u8; 256 << 20];
    both("256 MiB more resident");
    drop(resident);
    let (_, most) = getrlimit(Resource::RLIMIT_NOFILE).unwrap();
    setrlimit(Resource::RLIMIT_NOFILE, most.min(16_384), most).unwrap();
    let null = File::open("/dev/null").unwrap();
    let crowd: Vec<OwnedFd> = (0..CROWD)
        .map(|_| null.try_clone().unwrap().into())
        .collect();
    both("4,200 descriptors more open");
    drop(crowd);
}

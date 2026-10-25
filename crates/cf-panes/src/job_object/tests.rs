//! The job's reading of Git Bash's programs, on real ones: a script that Git Bash's `sh` runs
//! starts a `sleep` whose parent, as Windows names it, has gone (a forked copy of the shell,
//! which is there for some milliseconds after the `sleep` is: a look in those finds a parent
//! that is a process, and a chain whole without the repair, which is tried below as its own
//! state, T-375), and the job gives it the parent Git Bash says ([`repair_cygwin_parents`],
//! [`ps_w`]) (T-194, P3-7). Git Bash is where Git for Windows puts it; a machine without it
//! has nothing to try this on, and the tests say so and end. The program is put in the job
//! after it starts, here: how a process joined a job is not what the repair reads. A pane's,
//! which is in its job from its first instruction, is tried in `pty`'s `leaving_tests`.
#![allow(clippy::disallowed_methods)] // A test's own programs.

use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, sleep};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;

use super::*;

/// No console window for a program the test starts.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Git Bash's `bash.exe`, where Git for Windows puts it.
fn git_bash() -> Option<PathBuf> {
    [
        r"C:\Program Files\Git\bin\bash.exe",
        r"C:\Program Files (x86)\Git\bin\bash.exe",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|bash| bash.is_file())
}

/// A job whose window runs `sh ./s.sh` in a folder of its own, the script running `sleep 30`:
/// Git Bash's `bash` is the window's program, put in the job a moment before it starts the
/// script (a child it starts in the first instant would escape the job).
struct Window {
    job: JobObject,
    program: Child,
    folder: PathBuf,
}

impl Window {
    /// A window in a folder of the system's temporary one named for the test (`label`) and
    /// this process.
    fn open(bash: &Path, label: &str) -> Self {
        let folder = std::env::temp_dir().join(format!("cf-panes-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("s.sh"), "#!/bin/sh\nsleep 30\n").unwrap();
        let program = Command::new(bash)
            .args(["-c", "sleep 2; sh ./s.sh"])
            .current_dir(&folder)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap();
        let job = JobObject::new().unwrap();
        // SAFETY: both handles are open, the process's one until `program` is dropped.
        let assigned =
            unsafe { AssignProcessToJobObject(job.handle(), program.as_raw_handle() as HANDLE) };
        assert_ne!(assigned, 0, "{}", io::Error::last_os_error());
        Self {
            job,
            program,
            folder,
        }
    }

    /// The job's members, as Windows names the parents, once `wanted` says of one that runs
    /// `sleep` for 30 s that it is the state the test is about, looked at every [`LOOK`].
    fn until(&self, wanted: impl Fn(&Member) -> bool) -> Vec<Member> {
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            let members = self.job.members_with(&|_| None).unwrap();
            if members.iter().any(|one| is_the_sleep(one) && wanted(one)) {
                return members;
            }
            assert!(
                Instant::now() < until,
                "the script's sleep never was as the test needs it: {members:?}"
            );
            sleep(LOOK);
        }
    }

    /// The job's members once one runs `sleep` for 30 s: whether the forked shell that started
    /// it is still there or not.
    fn until_the_sleep(&self) -> Vec<Member> {
        self.until(|_| true)
    }

    /// The job's members once one runs `sleep` for 30 s and the process that started it has
    /// gone, which is what the repair is for. That process, a forked copy of the script's
    /// shell, is there for some milliseconds after the `sleep` is (Cygwin ends the old
    /// process once the new one has begun): on zeewin 13 ms (median; 20 ms at most) with no
    /// load, 64 ms (136 ms at most) beside five computing threads for each core. A look in
    /// those finds the sleep's parent a process of the job. These tests looked every 250 ms,
    /// which landed in them in 3 windows of 100 beside twice as many threads as cores, and
    /// they failed once on zeewin (T-348's whole-workspace run); looked at every few
    /// milliseconds, the look lands in them in every window, and waits.
    fn until_the_sleep_is_an_orphan(&self) -> Vec<Member> {
        self.until(|one| one.parent.is_none())
    }

    /// The host's own look at the job (`members`, which asks `ps`) that holds the sleep, and the
    /// sleep in it, with how many looks left it out first. A look leaves out a process it cannot
    /// look at, and one look in some 2,000 windows beside computing threads did not hold the
    /// sleep that the look before it had, which is not what these tests are about (T-375, not
    /// explained: `ps` was asked after the processes were held, and the sleep ran for 30 s).
    fn the_hosts_look(&self) -> (Vec<Member>, Member, usize) {
        for left_out in 0..5 {
            let members = self.job.members().unwrap();
            if let Some(one) = members.iter().find(|one| is_the_sleep(one)).cloned() {
                return (members, one, left_out);
            }
            sleep(LOOK);
        }
        panic!("five looks of the host's did not hold the sleep");
    }
}

/// How often a window's job is looked at while a test waits for its `sleep` to be as it needs
/// it: often enough to find the forked shell that started it (some milliseconds) before it
/// goes, which is a state of its own, not a chance the test takes.
const LOOK: Duration = Duration::from_millis(5);

impl Drop for Window {
    fn drop(&mut self) {
        let _ = self.program.kill();
        let _ = self.job.terminate();
        sleep(Duration::from_millis(500));
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

fn is_the_sleep(member: &Member) -> bool {
    member.program.eq_ignore_ascii_case("sleep.exe") && member.command.contains("30")
}

#[test]
fn the_sleep_of_a_script_has_the_script_for_its_parent_once_git_bash_has_said_so() {
    let Some(bash) = git_bash() else {
        eprintln!("no Git Bash here: nothing to try the repair on");
        return;
    };
    let window = Window::open(&bash, "repair");

    // As Windows names its parent, which has gone: the sleep has none among the job's members.
    let members = window.until_the_sleep_is_an_orphan();
    let sleep = members.iter().find(|one| is_the_sleep(one)).unwrap();
    assert_eq!(sleep.parent, None, "{members:?}");

    // With the parent Git Bash says: the script's shell, a member that runs `sh ./s.sh`.
    let (members, sleep, _) = window.the_hosts_look();
    let sleep = &sleep;
    let parent = members
        .iter()
        .find(|one| Some(one.key) == sleep.parent)
        .unwrap_or_else(|| panic!("the sleep has no parent among {members:?}"));
    assert!(
        parent.program.eq_ignore_ascii_case("sh.exe") && parent.command.contains("s.sh"),
        "{parent:?}"
    );
}

/// The chain from a script's sleep up to the window's program is whole at the first look that
/// finds the sleep, when the forked shell that started it is still there (some milliseconds,
/// every time on zeewin: the state the two tests above wait out) and the sleep's parent is a
/// process of the job, with no repair; and once that shell has gone, with the parent Git Bash
/// says. Both are what the host follows to name the sleep as the turn's work (T-375).
#[test]
fn the_chain_from_a_scripts_sleep_to_the_windows_program_is_whole_with_its_starter_there_and_without_it(
) {
    let Some(bash) = git_bash() else {
        eprintln!("no Git Bash here: nothing to try the repair on");
        return;
    };
    let window = Window::open(&bash, "chain");
    let program = window.program.id();

    window.until_the_sleep();
    let (members, sleep, _) = window.the_hosts_look();
    assert!(
        reaches(&members, &sleep, program),
        "at the first look that found the sleep: {members:?}"
    );

    window.until_the_sleep_is_an_orphan();
    let (members, sleep, _) = window.the_hosts_look();
    assert!(
        reaches(&members, &sleep, program),
        "once the sleep's starter has gone: {members:?}"
    );
}

// The forked shell of a script's sleep, under load (T-375).

/// Threads that compute until they are dropped: the load of a busy machine, bounded by the
/// test's own life.
struct Burners {
    stop: Arc<AtomicBool>,
    threads: Vec<thread::JoinHandle<()>>,
}

impl Burners {
    fn start(count: usize) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let threads = (0..count)
            .map(|_| {
                let stop = Arc::clone(&stop);
                thread::spawn(move || {
                    let mut x = 1u64;
                    while !stop.load(Ordering::Relaxed) {
                        for _ in 0..10_000 {
                            x = std::hint::black_box(
                                x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1),
                            );
                        }
                    }
                })
            })
            .collect();
        Self { stop, threads }
    }
}

impl Drop for Burners {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

/// The value of `name` as a number, or `default`.
fn size_from_the_environment(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|given| given.parse().ok())
        .unwrap_or(default)
}

/// Whether the chain of parents from `from` up, among `members`, ends at the process
/// `program`: the chain the host follows to name the sleep as the turn's work.
fn reaches(members: &[Member], from: &Member, program: u32) -> bool {
    let mut at = from;
    for _ in 0..=members.len() {
        let Some(parent) = at.parent else {
            return at.key.pid == program;
        };
        match members.iter().find(|one| one.key == parent) {
            Some(one) => at = one,
            None => return false,
        }
    }
    false
}

/// A look at the job as Windows names the parents, for the sleep: it, and whether the process
/// that started it is there.
fn the_sleep_raw(window: &Window) -> Option<Member> {
    let members = window.job.members_with(&|_| None).unwrap();
    members.into_iter().find(is_the_sleep)
}

/// How the sleep of a window stood when a look first found it.
struct Sighting {
    /// Whether the process that started it was still there, a member of the job.
    starter_there: bool,
    /// How long after that look that process went: none where it had gone already.
    starter_lived: Option<Duration>,
}

/// A script's `sleep` is started by a forked copy of the script's shell, which is there for a
/// moment after the `sleep` is (Cygwin ends the old process once the new one has begun), and
/// the tests above had a first look find the `sleep` with that process alive on zeewin once
/// (T-348's whole-workspace run, with Git's `usr\bin` first on the path): the parent it names
/// was a process, the chain up from it was whole without the repair, and `ps` was not asked.
/// This plays windows of Git Bash beside threads that compute on every core and looks at the
/// job as Windows names it every `CF_PANES_LOAD_POLL_MS` (5; the tests above looked every 250)
/// from the moment the window opens. It says how many windows had the sleep's starter alive
/// at the first look that found the sleep (on zeewin, 2026-10-23: all of 100 with no load,
/// all of 1,800 beside none or two to five computing threads for each core, and 3 of 100 looking
/// every 250 ms beside two), how long it lived after (in half of the windows; the median grew
/// from 13 ms to 64 ms with the load), and how many times the host's own look (`members`,
/// which asks `ps`) left the sleep out, which it did once in some 2,000 windows and is not
/// explained. In the other half of the windows the host's look is taken at once. The chain
/// from the sleep to the window's program must be whole at that look and, in all, once the
/// starter has gone, which is what the test asserts: it was, in every window. Opt-in, on
/// Windows with Git Bash, a few minutes:
/// `cargo test -p cf-panes --lib -- --ignored --nocapture under_load`, with
/// `CF_PANES_LOAD_WINDOWS` windows at once (4), `CF_PANES_LOAD_ROUNDS` of each (25),
/// `CF_PANES_LOAD_BURNERS` computing threads (twice the cores) and `CF_PANES_LOAD_POLL_MS`.
#[test]
#[ignore = "opt-in load reproduction of T-375, on Windows with Git Bash: a few minutes; cargo test -p cf-panes --lib -- --ignored --nocapture under_load"]
fn under_load_the_forked_shell_that_started_a_scripts_sleep_may_outlive_the_first_look_at_it_and_the_chain_is_whole_all_the_same(
) {
    let Some(bash) = git_bash() else {
        eprintln!("no Git Bash here: nothing to try the repair on");
        return;
    };
    let cores = thread::available_parallelism().map_or(4, std::num::NonZero::get);
    let _burners = Burners::start(size_from_the_environment(
        "CF_PANES_LOAD_BURNERS",
        2 * cores,
    ));
    let windows = size_from_the_environment("CF_PANES_LOAD_WINDOWS", 4);
    let rounds = size_from_the_environment("CF_PANES_LOAD_ROUNDS", 25);
    let poll = Duration::from_millis(size_from_the_environment("CF_PANES_LOAD_POLL_MS", 5) as u64);
    let sightings: Mutex<Vec<Sighting>> = Mutex::new(Vec::new());
    let broken: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let left_out_by_the_hosts_look = AtomicUsize::new(0);
    thread::scope(|scope| {
        for at in 0..windows {
            let (bash, sightings, broken) = (&bash, &sightings, &broken);
            let left_out_by_the_hosts_look = &left_out_by_the_hosts_look;
            scope.spawn(move || {
                for round in 0..rounds {
                    let window = Window::open(bash, &format!("load-{at}-{round}"));
                    let program = window.program.id();
                    // The first look that finds the sleep: is its starter there?
                    let opened = Instant::now();
                    let starter_there = loop {
                        if let Some(one) = the_sleep_raw(&window) {
                            break one.parent.is_some();
                        }
                        assert!(
                            opened.elapsed() < Duration::from_secs(30),
                            "the script's sleep never began"
                        );
                        sleep(poll);
                    };
                    let first = Instant::now();
                    let mut starter_lived = None;
                    if round % 2 == 0 {
                        // Half of the windows: the host's own look at once, with `ps`.
                        let (members, one, left_out) = window.the_hosts_look();
                        left_out_by_the_hosts_look.fetch_add(left_out, Ordering::Relaxed);
                        if !reaches(&members, &one, program) {
                            broken.lock().unwrap().push(format!(
                                "window {at}-{round}, starter there {starter_there}, at the first look: {members:?}"
                            ));
                        }
                    } else if starter_there {
                        // The other half: how long that process lives.
                        loop {
                            if the_sleep_raw(&window).is_some_and(|one| one.parent.is_none()) {
                                starter_lived = Some(first.elapsed());
                                break;
                            }
                            assert!(
                                first.elapsed() < Duration::from_secs(30),
                                "the sleep's starter never went"
                            );
                            sleep(poll);
                        }
                    }
                    // Once the starter has gone, the host's look.
                    window.until_the_sleep_is_an_orphan();
                    let (members, one, left_out) = window.the_hosts_look();
                    left_out_by_the_hosts_look.fetch_add(left_out, Ordering::Relaxed);
                    if !reaches(&members, &one, program) {
                        broken.lock().unwrap().push(format!(
                            "window {at}-{round}, once the starter had gone: {members:?}"
                        ));
                    }
                    sightings.lock().unwrap().push(Sighting {
                        starter_there,
                        starter_lived,
                    });
                }
            });
        }
    });
    let sightings = sightings.into_inner().unwrap();
    let mut lived: Vec<Duration> = sightings
        .iter()
        .filter_map(|one| one.starter_lived)
        .collect();
    lived.sort();
    eprintln!(
        "{} of {} windows had the sleep's starter alive at the first look that found the sleep \
         (a look every {poll:?}); it lived after that, median {:?}, longest {:?}",
        sightings.iter().filter(|one| one.starter_there).count(),
        sightings.len(),
        lived.get(lived.len() / 2),
        lived.last(),
    );
    eprintln!(
        "the host's look left the sleep out {} times",
        left_out_by_the_hosts_look.load(Ordering::Relaxed)
    );
    let broken = broken.into_inner().unwrap();
    assert!(
        broken.is_empty(),
        "the chain from the sleep to the window's program was broken {} times: {broken:#?}",
        broken.len()
    );
}

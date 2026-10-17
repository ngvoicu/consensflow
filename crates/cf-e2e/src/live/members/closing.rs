//! Ending a project's rig, and what is said when it will not end. The rig's own
//! close waits five seconds for the daemon and the pane host to end once their input
//! has, which suits the stand-in agents (they leave when asked). A real harness
//! takes its time (OpenCode's window needs a little over five seconds to leave once
//! the host asks it to, after a command of its was stopped), and the host gives a
//! program that does not leave ten before it kills it. So a project of real
//! harnesses is given [`EXIT`] to end in, and one that does not end in it says why:
//! "process did not exit" does not say which process it was, so the rig's processes
//! are looked at while it ends, and what was still there the moment it gave up (before
//! the rig is dropped, which kills what it could not end) goes into the failure.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use super::shown;
use crate::live::claude::Process;
use crate::process::Run;
use crate::rig::Rig;
use crate::{Error, Result};

/// How long the daemon and the pane host each have to end once their input has: the
/// host's own grace for a program that ignores the request to end, and the time it
/// takes to kill it, twice over.
pub const EXIT: Duration = Duration::from_secs(20);

/// How often the processes are looked at while the rig ends.
const LOOK: Duration = Duration::from_millis(500);

/// A process of the machine and the process group it is in, as `ps` lists them.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Listed {
    process: Process,
    group: u32,
}

/// The processes `ps -axo pid=,ppid=,pgid=,command=` listed, but for the listing's own.
fn parsed(listing: &str) -> Vec<Listed> {
    listing
        .lines()
        .filter_map(|line| {
            let (pid, rest) = line.trim_start().split_once(char::is_whitespace)?;
            let (ppid, rest) = rest.trim_start().split_once(char::is_whitespace)?;
            let (group, command) = rest.trim_start().split_once(char::is_whitespace)?;
            Some(Listed {
                process: Process {
                    pid: pid.parse().ok()?,
                    ppid: ppid.parse().ok()?,
                    command: command.trim_start().to_owned(),
                },
                group: group.parse().ok()?,
            })
        })
        .filter(|listed| !listed.process.command.contains("ps -axo"))
        .collect()
}

/// The processes of the machine, with their groups, or why they could not be listed.
type Listing = std::result::Result<Vec<Listed>, String>;

/// The processes of the machine now, with their groups, as `ps` lists them: why it
/// could not, on a machine with no `ps` or one that did not end well.
fn listing() -> Listing {
    let ran = Run::new("ps")
        .args(["-axo", "pid=,ppid=,pgid=,command="])
        .inheriting_env()
        .run()
        .map_err(|failed| failed.to_string())?;
    if ran.code == Some(0) {
        return Ok(parsed(&ran.stdout));
    }
    // The first line of what it ran to is how it ended (a code, a signal, its limit).
    let ended = ran.to_string();
    Err(format!(
        "ps: {}: {}",
        ended.lines().next().unwrap_or_default(),
        ran.stderr.trim()
    ))
}

/// The processes of `all` that descend from `root`, by the parent each was started
/// by: what a rig started, and what that started in turn.
fn descendants(all: &[Listed], root: u32) -> Vec<Listed> {
    let mut found: Vec<Listed> = Vec::new();
    let mut parents = vec![root];
    while let Some(parent) = parents.pop() {
        for child in all.iter().filter(|listed| listed.process.ppid == parent) {
            if !found
                .iter()
                .any(|known| known.process.pid == child.process.pid)
            {
                parents.push(child.process.pid);
                found.push(child.clone());
            }
        }
    }
    found.sort_by_key(|listed| listed.process.pid);
    found
}

/// What of `alive` the rig left: the processes it started, and whatever is in the
/// group of one of them. A window's program is the leader of a group of its own, and
/// the jobs it started stay in it after their parent has gone, which a look by the
/// parents alone does not find.
fn left_of(alive: &[Listed], started: &BTreeSet<u32>) -> Vec<Process> {
    alive
        .iter()
        .filter(|listed| started.contains(&listed.process.pid) || started.contains(&listed.group))
        .map(|listed| listed.process.clone())
        .collect()
}

/// What the looks at the rig's processes have seen.
#[derive(Default)]
struct Seen {
    /// Every process any look has seen.
    ever: BTreeSet<u32>,
    /// What the last look that could list the processes found: none before one could.
    last: Option<Vec<Process>>,
}

/// One look at what this process started, by `list`, written into `seen`: a look
/// that could not list the processes finds nothing.
fn look(seen: &Mutex<Seen>, list: &impl Fn() -> Listing) {
    let Ok(all) = list() else {
        return;
    };
    let tree = descendants(&all, std::process::id());
    let mut seen = seen.lock().unwrap_or_else(PoisonError::into_inner);
    seen.ever
        .extend(tree.iter().map(|listed| listed.process.pid));
    seen.last = Some(tree.into_iter().map(|listed| listed.process).collect());
}

/// Ends `rig`, giving its daemon and its pane host `within` each. One that does
/// not end is an error that says which of the processes it started were left.
pub fn end_within(mut rig: Rig, within: Duration) -> Result {
    let said = ended(|| rig.shut_down_within(within), listing, LOOK);
    // What was left is listed before the rig is dropped, which kills the daemon and
    // the pane host it could not end: a listing after that would not have the host
    // that was still waiting for its windows.
    drop(rig);
    said
}

/// The looker's stop: its flag is set when it goes, however that is. A shutdown or a
/// listing that panics would otherwise leave the looker looking, and the scope that
/// waits for its threads before the panic goes on waiting for ever.
struct Stop<'a>(&'a AtomicBool);

impl Drop for Stop<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// [`end_within`] with what it ends by (`shut_down`), what lists the machine's
/// processes (`list`) and how often they are looked at (`every`) given.
fn ended(
    shut_down: impl FnOnce() -> Result,
    list: impl Fn() -> Listing + Sync,
    every: Duration,
) -> Result {
    let seen = Mutex::new(Seen::default());
    let done = AtomicBool::new(false);
    // The first look is made before the rig begins to end: a window that leaves the
    // moment it is asked (a stand-in does) and the jobs it leaves to the machine are
    // out of reach of the parents by the time a look in a thread of its own is made.
    look(&seen, &list);
    thread::scope(|scope| {
        let looking = scope.spawn(|| loop {
            thread::sleep(every);
            if done.load(Ordering::Relaxed) {
                break;
            }
            look(&seen, &list);
        });
        let stop = Stop(&done);
        // What was left is listed the moment the shutdown gives up, on this thread: a
        // look in flight can take as long as `ps` is let run (half a minute, under
        // load), past the host's grace, after which the host has ended the window's
        // jobs itself, and a listing that waited for it would not have them.
        let failed = shut_down().err().map(|failed| (failed, list()));
        drop(stop);
        let _ = looking.join();
        let seen = seen.lock().unwrap_or_else(PoisonError::into_inner);
        match failed {
            None => Ok(()),
            Some((failed, now)) => Err(Error::Timeout(format!(
                "{failed}; {}",
                left_said(now, &seen)
            ))),
        }
    })
}

/// What is said of the processes a rig started that were left, by the listing
/// taken as it gave up (`now`) and what the looks saw: a listing that failed says
/// why, and what the last look that could list them found, as that look's.
fn left_said(now: Listing, seen: &Seen) -> String {
    let shown_or_none = |processes: &[Process]| {
        if processes.is_empty() {
            "none".to_owned()
        } else {
            shown(processes).join("; ")
        }
    };
    match now {
        Ok(alive) => {
            let mut left = left_of(&alive, &seen.ever);
            left.sort_by_key(|process| process.pid);
            format!(
                "the processes it started that were left: {}",
                shown_or_none(&left)
            )
        }
        Err(why) => format!(
            "the processes it started that were left could not be listed ({why}); {}",
            seen.last.as_deref().map_or_else(
                || "no look could list them either".to_owned(),
                |last| format!("the last look that could saw: {}", shown_or_none(last))
            )
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;

    use super::*;

    fn listed(pid: u32, ppid: u32, group: u32, command: &str) -> Listed {
        Listed {
            process: Process {
                pid,
                ppid,
                command: command.to_owned(),
            },
            group,
        }
    }

    #[test]
    fn a_listing_is_read_as_a_process_its_parent_its_group_and_the_command_as_it_is_spaced() {
        let text = "    1     0     1 /sbin/launchd\n  \
                    501   500   500 node  -e   \"x\"\n\
                    ps   -axo pid=,ppid=,pgid=,command=\n  \
                    7   1   7 ps -axo pid=,ppid=,pgid=,command=\n\
                    not a process\n";
        let all = parsed(text);
        assert_eq!(
            all,
            [
                listed(1, 0, 1, "/sbin/launchd"),
                listed(501, 500, 500, "node  -e   \"x\""),
            ]
        );
        assert!(parsed("").is_empty());
    }

    #[test]
    fn what_a_rig_started_is_what_descends_from_it_by_the_parents_whatever_their_depth() {
        let all = [
            listed(1, 0, 1, "launchd"),
            listed(10, 1, 10, "cargo test"),
            listed(20, 10, 10, "cf ui --json"),
            listed(21, 10, 10, "consensflow-bridge"),
            listed(30, 21, 30, "opencode"),
            listed(31, 30, 30, "sh ./script"),
            listed(32, 31, 30, "sleep 200"),
            // Another's, which no parent of this rig started.
            listed(40, 1, 40, "vim notes.md"),
            listed(41, 40, 40, "claude"),
        ];
        let found: Vec<u32> = descendants(&all, 10)
            .iter()
            .map(|listed| listed.process.pid)
            .collect();
        assert_eq!(found, [20, 21, 30, 31, 32]);
        assert!(descendants(&all, 999).is_empty());
        assert!(descendants(&[], 10).is_empty());
    }

    #[test]
    fn a_parent_that_is_its_own_child_does_not_loop() {
        let all = [listed(10, 10, 10, "odd"), listed(11, 10, 10, "child")];
        let found: Vec<u32> = descendants(&all, 10)
            .iter()
            .map(|listed| listed.process.pid)
            .collect();
        assert_eq!(found, [10, 11]);
    }

    #[test]
    fn what_is_left_of_a_rig_is_what_it_started_and_what_stayed_in_the_group_of_a_window() {
        // The window (30) went; its job (33) was left to launchd, in the window's group.
        let alive = [
            listed(21, 10, 10, "consensflow-bridge"),
            listed(33, 1, 30, "fake-agent --background-job 120 job --deaf"),
            listed(40, 1, 40, "vim notes.md"),
            listed(41, 40, 10, "a stranger in the test's own group"),
        ];
        let started: BTreeSet<u32> = [20, 21, 30, 31].into();
        let left: Vec<u32> = left_of(&alive, &started)
            .iter()
            .map(|process| process.pid)
            .collect();
        assert_eq!(left, [21, 33]);
        assert!(left_of(&alive, &BTreeSet::new()).is_empty());
    }

    /// A rig made up under this process: its pane host, the shell of a window's job
    /// and the job that ignores the request to end, in the window's group.
    fn a_rig_waiting_for_its_job() -> Vec<Listed> {
        let me = std::process::id();
        vec![
            listed(900_001, me, me, "consensflow-bridge"),
            listed(900_003, 900_001, 900_002, "sh -c job"),
            listed(
                900_004,
                900_003,
                900_002,
                "fake-agent --background-job 120 job --deaf",
            ),
        ]
    }

    fn did_not_exit() -> Result {
        Err(Error::Timeout("process did not exit".to_owned()))
    }

    #[test]
    fn what_was_left_is_listed_as_the_shutdown_gives_up_and_not_once_a_look_in_flight_has_ended() {
        // The looker's first look is in flight as the shutdown gives up, and ends only
        // once what was left is listed: a listing that waits for it never comes, and
        // the watchdog (time for nothing else) ends the look, as the host's grace would
        // end the job, and fails the case.
        const WATCHDOG: Duration = Duration::from_secs(20);
        let world = Mutex::new(a_rig_waiting_for_its_job());
        let here = thread::current().id();
        let listed_here = AtomicUsize::new(0);
        let first_look = AtomicBool::new(true);
        let held = AtomicBool::new(false);
        let (in_flight, a_look_is_in_flight) = mpsc::channel();
        let (let_go, let_go_of) = mpsc::channel();
        let let_go_of = Mutex::new(let_go_of);
        let list = || -> Listing {
            if thread::current().id() == here {
                let all = world.lock().unwrap().clone();
                // The first is the look before the rig ends, the second what was left.
                if listed_here.fetch_add(1, Ordering::SeqCst) == 1 {
                    let _ = let_go.send(());
                }
                return Ok(all);
            }
            // The looker's later looks are not held: only the first is in flight.
            if first_look.swap(false, Ordering::SeqCst) {
                let _ = in_flight.send(());
                if let_go_of.lock().unwrap().recv_timeout(WATCHDOG).is_err() {
                    held.store(true, Ordering::SeqCst);
                    world
                        .lock()
                        .unwrap()
                        .retain(|listed| listed.process.command == "consensflow-bridge");
                }
            }
            Ok(world.lock().unwrap().clone())
        };
        let shut_down = || {
            a_look_is_in_flight
                .recv_timeout(WATCHDOG)
                .expect("the looker made no look");
            did_not_exit()
        };
        let failed = ended(shut_down, list, Duration::from_millis(10))
            .unwrap_err()
            .to_string();
        let left = failed
            .split("the processes it started that were left: ")
            .nth(1)
            .unwrap_or_else(|| panic!("{failed}"));
        assert!(
            !held.load(Ordering::SeqCst),
            "the listing waited for the look in flight: {failed}"
        );
        for pid in [900_001, 900_003, 900_004] {
            assert!(left.contains(&format!("{pid} (")), "{pid}: {failed}");
        }
    }

    #[test]
    fn a_listing_that_fails_as_the_shutdown_gives_up_says_why_and_what_the_last_look_that_could_saw(
    ) {
        let here = thread::current().id();
        let listed_here = AtomicUsize::new(0);
        let list = || -> Listing {
            if thread::current().id() == here && listed_here.fetch_add(1, Ordering::SeqCst) == 1 {
                return Err("ps: Resource temporarily unavailable".to_owned());
            }
            Ok(a_rig_waiting_for_its_job())
        };
        let failed = ended(did_not_exit, list, Duration::from_millis(10))
            .unwrap_err()
            .to_string();
        assert!(
            failed.starts_with(
                "process did not exit; the processes it started that were left could not be \
                 listed (ps: Resource temporarily unavailable); the last look that could saw: "
            ),
            "{failed}"
        );
        for pid in [900_001, 900_003, 900_004] {
            assert!(failed.contains(&format!("{pid} (")), "{pid}: {failed}");
        }

        // No look could list them: none is said to have been seen.
        let never = || -> Listing { Err("ps: not found".to_owned()) };
        let failed = ended(did_not_exit, never, Duration::from_millis(10))
            .unwrap_err()
            .to_string();
        assert_eq!(
            failed,
            "process did not exit; the processes it started that were left could not be \
             listed (ps: not found); no look could list them either"
        );
    }

    /// What a panic in `ended` was, if it ended by one: the call is made on a thread of
    /// its own, so that one that never ends is said, after `watchdog`, and not waited
    /// for.
    fn panic_of(
        call: impl FnOnce() -> Result + Send + 'static,
        watchdog: Duration,
    ) -> std::result::Result<Option<String>, String> {
        let (ended, end) = mpsc::channel();
        thread::spawn(move || {
            let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(call));
            let _ = ended.send(caught.err().map(|payload| {
                payload.downcast_ref::<&str>().map_or_else(
                    || "a panic that says nothing".to_owned(),
                    |said| (*said).to_owned(),
                )
            }));
        });
        end.recv_timeout(watchdog).map_err(|_| {
            format!("the call had not ended after {watchdog:?}: the looker never stopped")
        })
    }

    #[test]
    fn a_shutdown_or_a_listing_that_panics_ends_the_call_with_its_panic_and_the_looker_with_it() {
        const WATCHDOG: Duration = Duration::from_secs(20);
        let shutdown = panic_of(
            || {
                ended(
                    || panic!("the shutdown panicked"),
                    || Ok(a_rig_waiting_for_its_job()),
                    Duration::from_millis(10),
                )
            },
            WATCHDOG,
        );
        assert_eq!(shutdown, Ok(Some("the shutdown panicked".to_owned())));

        let listing = panic_of(
            || {
                let here = thread::current().id();
                let listed_here = AtomicUsize::new(0);
                ended(
                    did_not_exit,
                    move || {
                        // The first is the look before the rig ends, the second what was left.
                        if thread::current().id() == here
                            && listed_here.fetch_add(1, Ordering::SeqCst) == 1
                        {
                            panic!("the listing panicked");
                        }
                        Ok(a_rig_waiting_for_its_job())
                    },
                    Duration::from_millis(10),
                )
            },
            WATCHDOG,
        );
        assert_eq!(listing, Ok(Some("the listing panicked".to_owned())));
    }
}

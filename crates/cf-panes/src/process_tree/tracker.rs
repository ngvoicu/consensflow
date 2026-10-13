//! What a pane's program has started, as it started it. A process that loses what the
//! tree finds it by is lost to a look made later: a job an agent detaches
//! (`nohup sleep 600 &`) from the shell a harness gave its command, once that shell
//! has gone, is no child of anything the tree has; it is in no session of the
//! pane's where the harness started the shell in one of its own (Node's `detached`,
//! which Pi's Bash tool sets, is a `setsid`); and it shows no environment where it
//! runs a system program. What is left to find it by is the shell that started it:
//! the system names the process that started a process by a unique identifier
//! ([`Identity`]) that no later process takes, and keeps naming it once that process
//! has gone. So what this keeps, of each process it saw start, is that identifier,
//! and a job whose shell it saw start is found by the shell's, whatever became of the
//! shell.
//!
//! The system does not follow forks (`kqueue` refuses `NOTE_TRACK` here), so this does
//! it: a process that is watched says when it forks (`NOTE_FORK`), and what it started
//! is listed, kept, and watched in its turn. A shell that is started and gone before
//! the list is made is not seen, and the job it started is found no better than
//! without this: the thread waits for the system and does nothing else, and a machine
//! with every core busy may not run it in time.
use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use super::native::process;
use super::Identity;

/// How long the thread waits for an event before it looks whether it is to stop.
const LOOK: libc::timespec = libc::timespec {
    tv_sec: 0,
    tv_nsec: 250_000_000,
};

/// How many events the thread takes from the system at a time.
const EVENTS: usize = 64;

/// How long a process that was seen is kept before [`Tracker::forget`] may let it go:
/// a look that was made before it was seen is not yet the look that says it is gone.
const KEPT: Duration = Duration::from_secs(5);

/// What was seen to start, by the unique identifier of each, and when it was seen.
type Journal = Arc<Mutex<HashMap<u64, (Identity, Instant)>>>;

/// The processes a program started and what they started, and the thread that
/// watches for them.
pub(super) struct Tracker {
    journal: Journal,
    stop: Arc<AtomicBool>,
}

impl Tracker {
    /// Follows `root`: what it has started, and from now on what it and its children
    /// start.
    pub(super) fn follow(root: i32) -> io::Result<Self> {
        // SAFETY: kqueue takes no arguments, and gives a descriptor of ours.
        let queue = unsafe { libc::kqueue() };
        if queue < 0 {
            return Err(io::Error::last_os_error());
        }
        let journal: Journal = Arc::default();
        let mut watcher = Watcher {
            queue,
            watched: HashSet::new(),
            journal: Arc::clone(&journal),
        };
        if !watcher.register(root) {
            let error = io::Error::last_os_error();
            // SAFETY: the descriptor was opened above, and is closed only here.
            unsafe { libc::close(queue) };
            return Err(error);
        }
        watcher.scan(root);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        thread::Builder::new()
            .name("pane-tracker".into())
            .spawn(move || watcher.run(&stopped))?;
        Ok(Self { journal, stop })
    }

    /// What was seen start, told by the unique identifier of each: the processes that
    /// still run and those that have gone, whose children are told by them.
    pub(super) fn seen(&self) -> HashMap<u64, Identity> {
        lock(&self.journal)
            .iter()
            .map(|(unique, (identity, _))| (*unique, *identity))
            .collect()
    }

    /// Lets go of what `keep` does not say matters any more, and was seen before the
    /// last [`KEPT`]: a process that has gone and started nothing that runs is
    /// nobody's parent.
    pub(super) fn forget(&self, keep: impl Fn(&Identity) -> bool) {
        lock(&self.journal).retain(|_, (identity, seen)| seen.elapsed() < KEPT || keep(identity));
    }
}

#[cfg(test)]
impl Tracker {
    /// A tracker that has seen `seen` start, each when it says, and follows nothing.
    pub(super) fn having_seen(seen: impl IntoIterator<Item = (Identity, Instant)>) -> Self {
        Self {
            journal: Arc::new(Mutex::new(
                seen.into_iter()
                    .map(|(identity, at)| (identity.unique, (identity, at)))
                    .collect(),
            )),
            stop: Arc::new(AtomicBool::new(true)),
        }
    }
}

impl Drop for Tracker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the thread knows: the queue it reads, the processes it has asked to be told
/// of, and what it has kept of them.
struct Watcher {
    queue: i32,
    watched: HashSet<i32>,
    journal: Journal,
}

impl Watcher {
    /// Asks to be told when `pid` forks and when it ends. False where it is gone.
    fn register(&mut self, pid: i32) -> bool {
        let change = libc::kevent {
            ident: pid as usize,
            filter: libc::EVFILT_PROC,
            flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_CLEAR,
            fflags: libc::NOTE_FORK | libc::NOTE_EXIT,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        // SAFETY: `change` is the one change passed with its count, and there is no
        // room for events: the call only registers.
        let registered = unsafe {
            libc::kevent(
                self.queue,
                &change,
                1,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
            )
        };
        if registered < 0 {
            return false;
        }
        self.watched.insert(pid);
        true
    }

    /// Keeps and watches the children of `parent` that are not yet watched, and theirs
    /// that were started before they were: a child is watched, and then listed again,
    /// so that none it started in between is missed.
    fn scan(&mut self, parent: i32) {
        let mut pending = vec![parent];
        while let Some(parent) = pending.pop() {
            for child in children_of(parent) {
                if self.watched.contains(&child) {
                    continue;
                }
                // One that has gone already started nothing that is not listed.
                let Some(found) = process(child) else {
                    continue;
                };
                lock(&self.journal).insert(found.identity.unique, (found.identity, Instant::now()));
                if self.register(child) {
                    pending.push(child);
                }
            }
        }
    }

    /// Reads what the system says until `stop` is set, then closes the queue, which
    /// ends the watching.
    fn run(mut self, stop: &AtomicBool) {
        // SAFETY: a `kevent` is plain numbers and a pointer, which zeros leave unused.
        let mut events: [libc::kevent; EVENTS] = unsafe { std::mem::zeroed() };
        while !stop.load(Ordering::Relaxed) {
            // SAFETY: `events` is writable for the `EVENTS` passed with it, and the
            // call writes no more than that.
            let count = unsafe {
                libc::kevent(
                    self.queue,
                    std::ptr::null(),
                    0,
                    events.as_mut_ptr(),
                    EVENTS as i32,
                    &LOOK,
                )
            };
            if count < 0 {
                if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                break;
            }
            for event in &events[..count as usize] {
                let (pid, said) = (event.ident as i32, event.fflags);
                if said & libc::NOTE_FORK != 0 {
                    self.scan(pid);
                }
                if said & libc::NOTE_EXIT != 0 {
                    self.watched.remove(&pid);
                }
            }
        }
        // SAFETY: the descriptor was opened by `follow`, and is closed only here.
        unsafe { libc::close(self.queue) };
    }
}

/// The numbers of the processes `parent` has started and that it still is the parent
/// of: what the system lists, with as much room as it needs.
fn children_of(parent: i32) -> Vec<i32> {
    let mut room = 128;
    loop {
        let mut pids = vec![0i32; room];
        // SAFETY: the buffer is `pids`, writable for the bytes passed with it, and
        // proc_listchildpids writes no more than that.
        let count = unsafe {
            libc::proc_listchildpids(
                parent,
                pids.as_mut_ptr().cast(),
                (room * std::mem::size_of::<i32>()) as i32,
            )
        };
        let Ok(count) = usize::try_from(count) else {
            return Vec::new();
        };
        if count >= room {
            room *= 4;
            continue;
        }
        pids.truncate(count);
        return pids;
    }
}

#[cfg(test)]
mod tests;

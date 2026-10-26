//! A first-come queue for each machine, for the runs of every project on this Mac (T-701 item 7).
//!
//! A machine takes one run at a time and refuses the next with 75, and a worker that was refused
//! asked again every few minutes for up to an hour: whoever asked as the machine freed went first,
//! the one that had waited longest or not. So a run that finds the machine busy takes a ticket and
//! waits for its turn, in the one folder every runner on this Mac shares (`~/.cf-windows-queues`, or
//! `CF_WINDOWS_QUEUES`), a folder for each machine, a file for each run in it. A run goes when
//! no ticket older than its own is alive and the machine says it is ready ([`super::machine::Machine::ready`]: no
//! poker-lab job, disk enough, no run on the copy, no one holding its lock), and says every minute
//! where it stands. It waits [`DEFAULT_WAIT_MINUTES`] at most (`--wait MINUTES`; 0 asks once and
//! refuses at once, as the runner did), and then ends with 75 saying how long it waited and what it
//! was waiting for.
//!
//! A ticket is a file named by the time the run came and its process (so that the names sort in
//! the order of coming), holding the tree and the command it is for. It is alive while its run
//! touches it: a thread beats every [`BEAT`] from the run's coming to its end, and a ticket not
//! touched for [`DEAD_AFTER_MS`] is a run that was killed, and is taken away by whoever looks. A
//! run holds its ticket while it goes on there, so the runs behind it wait for it; the ticket
//! goes with the run however it ends (the guard is dropped). Another project's runner that keeps
//! the same folder and the same names is served in the same order; one that does not is not in
//! the queue, and meets the machine's own lock, which refuses as before.
//!
//! A Mac that sleeps stops the beats: a run woken after [`DEAD_AFTER_MS`] finds its ticket gone
//! and makes it again under the same name, and may meanwhile have been gone past by one that did
//! not wait for it; the machine's own lock is the last word on who runs.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, UNIX_EPOCH};

use cf_base::env::Env;
use cf_base::time::{Clock, SystemClock};

use super::interrupt::Interrupt;
use super::machine::Ready;
use super::Error;

/// How long a run waits for its turn unless `--wait` says another, in minutes.
pub const DEFAULT_WAIT_MINUTES: u32 = 60;

/// How often a run touches its ticket while it lives.
const BEAT: Duration = Duration::from_secs(5);

/// How long a ticket may go untouched and be a run's: six beats.
const DEAD_AFTER_MS: i64 = 30_000;

/// How often a waiting run looks at the queue and asks the machine, in milliseconds.
const LOOK_MS: u64 = 10_000;

/// How often a waiting run says where it stands, in milliseconds.
const SAY_EVERY_MS: i64 = 60_000;

/// What a ticket says of its run, for the runs behind it to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Who {
    /// The tree the run is from: its folder's name.
    pub tree: String,
    /// The command, as the person typed it, cut short.
    pub command: String,
}

impl Who {
    /// The run of `command` from the checkout `root`.
    pub fn of(root: &Path, command: &[String]) -> Self {
        let tree = root.file_name().map_or_else(
            || root.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let words = command.join(" ");
        let command = if words.chars().count() > 80 {
            format!("{}…", words.chars().take(79).collect::<String>())
        } else {
            words
        };
        Self { tree, command }
    }
}

/// One run ahead of another in the queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ahead {
    pub who: Who,
    /// How long ago it came, in milliseconds.
    pub since_ms: i64,
}

/// The folder every runner on this Mac keeps its queues in.
#[derive(Debug, Clone)]
pub struct Queues {
    root: PathBuf,
    beat: Duration,
}

impl Queues {
    /// The folder `CF_WINDOWS_QUEUES` names, else `.cf-windows-queues` in the home folder; none
    /// where neither is known (the run then waits for the machine without a place in a queue).
    pub fn of(env: &Env) -> Option<Self> {
        let root = env
            .path("CF_WINDOWS_QUEUES")
            .map(Path::to_path_buf)
            .or_else(|| {
                env.path("HOME")
                    .or_else(|| env.path("USERPROFILE"))
                    .map(|home| home.join(".cf-windows-queues"))
            })?;
        Some(Self { root, beat: BEAT })
    }

    #[cfg(test)]
    pub fn at(root: &Path, beat: Duration) -> Self {
        Self {
            root: root.to_path_buf(),
            beat,
        }
    }

    fn folder(&self, machine: &str) -> PathBuf {
        self.root.join(machine)
    }

    /// How many runs are in `machine`'s queue now (alive), the one that goes on there included.
    pub fn depth(&self, machine: &str, now_ms: i64) -> usize {
        tickets(&self.folder(machine))
            .into_iter()
            .filter(|ticket| ticket.alive(now_ms))
            .count()
    }

    /// Takes a ticket for `machine` for the run `who`, which came `now_ms`.
    pub fn join(&self, machine: &str, who: &Who, now_ms: i64) -> io::Result<Ticket> {
        let folder = self.folder(machine);
        fs::create_dir_all(&folder)?;
        let mut at = now_ms;
        let (path, text) = loop {
            let path = folder.join(format!("{at:015}-{:010}.ticket", std::process::id()));
            let text = format!("{}\n{}\n", who.tree, who.command);
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    io::Write::write_all(&mut file, text.as_bytes())?;
                    break (path, text);
                }
                // A second run of this process in the same millisecond: after the first.
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => at += 1,
                Err(error) => return Err(error),
            }
        };
        touch(&path, &text, now_ms)?;
        let (stop, stopped) = mpsc::channel::<()>();
        let beating = {
            let (path, text, beat) = (path.clone(), text.clone(), self.beat);
            thread::spawn(move || {
                while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(beat) {
                    // A ticket that is gone (taken for a dead one while this Mac slept) is made
                    // again under its name, which is its place.
                    let _ = touch(&path, &text, SystemClock.now_ms());
                }
            })
        };
        Ok(Ticket {
            path,
            text,
            stop: Some(stop),
            beating: Some(beating),
        })
    }
}

/// Touches the ticket `path`, writing it again with `text` if it is gone.
fn touch(path: &Path, text: &str, now_ms: i64) -> io::Result<()> {
    if !path.exists() {
        fs::write(path, text)?;
    }
    let file = fs::OpenOptions::new().write(true).open(path)?;
    let since = u64::try_from(now_ms).unwrap_or(0);
    file.set_modified(UNIX_EPOCH + Duration::from_millis(since))
}

/// A ticket as it is on disk.
struct Entry {
    path: PathBuf,
    /// When the run came, from its name.
    came_ms: i64,
    /// When it was last touched.
    touched_ms: Option<i64>,
}

impl Entry {
    fn alive(&self, now_ms: i64) -> bool {
        self.touched_ms
            .is_some_and(|touched| now_ms - touched <= DEAD_AFTER_MS)
    }

    fn who(&self) -> Who {
        let text = fs::read_to_string(&self.path).unwrap_or_default();
        let mut lines = text.lines();
        Who {
            tree: lines.next().unwrap_or("a run").to_owned(),
            command: lines.next().unwrap_or("").to_owned(),
        }
    }
}

/// The tickets in `folder`, in the order the runs came.
fn tickets(folder: &Path) -> Vec<Entry> {
    let Ok(listing) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut found: Vec<Entry> = listing
        .filter_map(Result::ok)
        .filter_map(|file| {
            let name = file.file_name().to_string_lossy().into_owned();
            let came_ms = name
                .strip_suffix(".ticket")?
                .split('-')
                .next()?
                .parse()
                .ok()?;
            let touched_ms = file
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                .and_then(|since| i64::try_from(since.as_millis()).ok());
            Some(Entry {
                path: file.path(),
                came_ms,
                touched_ms,
            })
        })
        .collect();
    found.sort_by(|a, b| a.path.file_name().cmp(&b.path.file_name()));
    found
}

/// A run's place in a machine's queue: its file, kept alive by a thread that touches it, and
/// taken away when this is dropped, however the run ended.
pub struct Ticket {
    path: PathBuf,
    text: String,
    stop: Option<mpsc::Sender<()>>,
    beating: Option<JoinHandle<()>>,
}

impl Ticket {
    /// The live runs that came before this one, oldest first. A ticket not touched for
    /// [`DEAD_AFTER_MS`] is a run that was killed, and is taken away; this one's own, if it was
    /// taken so, is made again.
    pub fn ahead(&self, now_ms: i64) -> Vec<Ahead> {
        let _ = touch_if_gone(&self.path, &self.text, now_ms);
        let Some(folder) = self.path.parent() else {
            return Vec::new();
        };
        let mine = self.path.file_name();
        let mut ahead = Vec::new();
        for entry in tickets(folder) {
            if entry.path.file_name() >= mine {
                break;
            }
            if entry.alive(now_ms) {
                ahead.push(Ahead {
                    who: entry.who(),
                    since_ms: now_ms - entry.came_ms,
                });
            } else {
                let _ = fs::remove_file(&entry.path);
            }
        }
        ahead
    }
}

/// Makes `path` again if it is gone.
fn touch_if_gone(path: &Path, text: &str, now_ms: i64) -> io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    touch(path, text, now_ms)
}

impl Drop for Ticket {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(beating) = self.beating.take() {
            let _ = beating.join();
        }
        let _ = fs::remove_file(&self.path);
    }
}

/// What a wait is paced by: the time, and a pause that ends early on a stop.
pub trait Pace {
    fn now_ms(&mut self) -> i64;
    /// How long a waiting run pauses between two looks, in milliseconds.
    fn look_ms(&self) -> u64 {
        LOOK_MS
    }
    /// Waits `ms`; false if a stop was heard.
    fn pause(&mut self, ms: u64) -> bool;
    /// Whether a stop was heard, after something failed as a stop would make it fail.
    fn stopped_after_failure(&mut self) -> bool;
}

/// The system's clock, and a stop heard by `interrupt`; it looks every `look_ms`
/// ([`LOOK_MS`] unless the environment's `CF_WINDOWS_LOOK_MS` says another, which the tests of the
/// run use).
pub struct Real<'a> {
    interrupt: &'a Interrupt,
    look_ms: u64,
}

impl<'a> Real<'a> {
    pub fn new(interrupt: &'a Interrupt, env: &Env) -> Self {
        let look_ms = env
            .text("CF_WINDOWS_LOOK_MS")
            .and_then(|ms| ms.parse().ok())
            .unwrap_or(LOOK_MS);
        Self { interrupt, look_ms }
    }
}

impl Pace for Real<'_> {
    fn now_ms(&mut self) -> i64 {
        SystemClock.now_ms()
    }

    fn look_ms(&self) -> u64 {
        self.look_ms
    }

    fn pause(&mut self, ms: u64) -> bool {
        let until = SystemClock.now_ms() + i64::try_from(ms).unwrap_or(i64::MAX);
        while SystemClock.now_ms() < until {
            if self.interrupt.heard() {
                return false;
            }
            thread::sleep(Duration::from_millis(50.min(ms)));
        }
        !self.interrupt.heard()
    }

    fn stopped_after_failure(&mut self) -> bool {
        self.interrupt.heard_after_failure()
    }
}

/// How a wait for a machine's turn ended.
pub enum Waited {
    /// It is this run's turn, and the machine is ready; what is to be said of the machine, and the
    /// ticket the run holds while it goes on (none where it did not wait).
    Go(Vec<String>, Option<Ticket>),
    /// It is not, and why: the run ends with 75, having sent nothing.
    Refused(String),
    /// A stop was heard.
    Stopped,
}

/// Waits for this run's turn at `machine`, and for the machine to be ready (`ready`), up to
/// `minutes` (0: asks once). `say` is told where the run stands, every minute and when it changes.
pub fn wait(
    queues: Option<&Queues>,
    machine: &str,
    who: &Who,
    minutes: u32,
    ready: &mut dyn FnMut() -> Result<Ready, String>,
    pace: &mut dyn Pace,
    say: &mut dyn FnMut(&str) -> io::Result<()>,
) -> Result<Waited, Error> {
    let began = pace.now_ms();
    let limit_ms = i64::from(minutes) * 60_000;
    // A queue that cannot be kept (a folder that cannot be made, a disk that is full) does not stop
    // the run: it is said, and the run waits for the machine alone.
    let ticket = match queues.filter(|_| minutes > 0) {
        Some(queues) => match queues.join(machine, who, began) {
            Ok(ticket) => Some(ticket),
            Err(cause) => {
                say(&format!(
                    "no place could be kept in the queue for {machine} ({cause}): waiting for the machine alone"
                ))
                .map_err(Error::Output)?;
                None
            }
        },
        None => None,
    };
    let mut said: Option<(String, i64)> = None;
    loop {
        let now = pace.now_ms();
        let ahead = ticket
            .as_ref()
            .map(|ticket| ticket.ahead(now))
            .unwrap_or_default();
        let blocked = if let Some(first) = ahead.first() {
            let place = ordinal(ahead.len() + 1);
            format!(
                "{place} in the queue for {machine}; the run ahead is {} ({}), here for {}",
                first.who.tree,
                first.who.command,
                minutes_of(first.since_ms)
            )
        } else {
            let asked = ready();
            if !matches!(asked, Ok(Ready::Go(_))) && pace.stopped_after_failure() {
                return Ok(Waited::Stopped);
            }
            match asked.map_err(Error::Unready)? {
                Ready::Go(lines) => return Ok(Waited::Go(lines, ticket)),
                Ready::Refused(why) => why,
            }
        };
        if minutes == 0 {
            return Ok(Waited::Refused(blocked));
        }
        let waited = now - began;
        if waited >= limit_ms {
            return Ok(Waited::Refused(format!(
                "{machine} is still busy after waiting {} (as far as --wait {minutes} goes): {blocked}",
                minutes_of(waited)
            )));
        }
        let changed = said.as_ref().is_none_or(|(was, _)| *was != blocked);
        let due = said.as_ref().is_none_or(|(_, at)| now - at >= SAY_EVERY_MS);
        if changed || due {
            say(&format!(
                "waiting for {machine}, {} of up to {minutes} min: {blocked}",
                minutes_of(waited)
            ))
            .map_err(Error::Output)?;
            said = Some((blocked, now));
        }
        if !pace.pause(pace.look_ms()) {
            return Ok(Waited::Stopped);
        }
    }
}

/// `n` as "1st", "2nd", "3rd", "4th".
fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// `ms` as whole minutes, or seconds under one.
fn minutes_of(ms: i64) -> String {
    if ms < 60_000 {
        format!("{} s", ms.max(0) / 1000)
    } else {
        format!("{} min", ms / 60_000)
    }
}

#[cfg(test)]
mod tests;

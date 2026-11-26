//! The processes of a window's own tree, as a live check that opened the window reads
//! them to find and end what the window runs: what descends from the window's program,
//! by the process that started each, and nothing else of the machine's. A check that
//! ended every process whose command line held a mark ended the worker that ran it
//! (2026-10-16), and one that went by numbers alone could end a process that took a
//! number the tree had, or take for the tree's a process whose parent's number the
//! window's program had taken since (Astra and Sol on the B1 fix, their second look).
//! So a process is known by its number and when it began ([`Identity`]): a child began
//! no earlier than its parent, and the window's own process is the one the pane host
//! opened, read once as it answered the open and kept (the rig's
//! [`crate::rig::Rig::window_process`]): a window whose process has gone has nothing that
//! descends from it, whatever has its number since (Astra and Sol on round 6).
//!
//! A process is ended only while it is still the one chosen, checked on what holds it
//! ([`end_chosen`]): on Windows a handle to the process, opened first, on which its
//! creation time is read and through which it is ended, so that its number is no other's
//! in between; on macOS and Linux, which have no such handle, its number, when it began
//! read again right before the signal. What could not be ended is said, never dropped
//! ([`Ended`]).
//!
//! When a process began is read to the microsecond on macOS (`proc_pidinfo`), to the
//! 100 ns on Windows (the process's own handle), and to the second on Linux (`ps`'s
//! `lstart`, in the C locale). The machine's list is `ps` on macOS and Linux, and on
//! Windows, which has none, the system's own process table
//! (`NtQuerySystemInformation`, no program started: a PowerShell took 20 to 30 s to start on a
//! machine with many modules, T-2363); the image's path and the command line, which a command
//! line of many lines keeps as one string, are read on each process's own handle.
//! On macOS and Windows each process listed is read again where it
//! is held, for when it began ([`refined_by_bsd`], [`refined_by_handle`]).

use std::io;

#[cfg(unix)]
use crate::process::Run;
#[cfg(unix)]
use crate::Error;
use crate::Result;

mod native;
#[cfg(test)]
pub(crate) mod testing;

/// Which process a process is: its number and when it began, which no two processes
/// have together. `started` grows with the time it says, on the machine that said it:
/// microseconds since 1970 on macOS, Windows' 100 ns since 1601 of its creation time,
/// and `YYYYMMDDhhmmss` of `ps`'s `lstart` on Linux.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Identity {
    pub pid: u32,
    pub started: u64,
}

/// What a reader read, with the process, to tie it to the process that made it. A child is
/// the window's only where its link proves the particular parent that made it
/// ([`descending_from`]): a parent's number alone is no proof, for the number may be another
/// process's by now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Link {
    /// Its parent's number and the order they began in, which is all Linux's `ps` says (that
    /// reader is not changed here), and all a test's table says unless it says more.
    #[default]
    Number,
    /// The system would not say what made it (a process that is not the user's, or that changed
    /// while it was read): it proves no edge, as a child or as a parent.
    Unread,
    /// The Mac's: its own unique identifier, and the identifier of the process that forked it
    /// (or that was its parent at its last exec), which the system keeps after that process has
    /// gone. The parent's identifier is the one thing no process takes after it.
    Forked { own: u64, forker: u64 },
    /// Windows': the number of the process that created it, read on the process's own handle,
    /// which was held open with every other row's until the whole table was read, so that no
    /// number in it was another process's meanwhile. `described` is the parent Git Bash's `ps -W`
    /// said of it later, which describes it in a report and proves nothing by itself: `held` says
    /// that the process and the parent described were both held open, with the creation times they
    /// were read with, before the `ps -W` reading and again after it, so that the number Git Bash
    /// names is that very process (a held handle's number is no other process's).
    Created {
        by: u32,
        described: Option<u32>,
        held: bool,
    },
}

/// A process of the machine as a check reads it: which it is, the number of the process
/// that started it, and its command line, and, where the system says it, the path of the
/// image it runs (Windows' table does, and `ps` does not).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Running {
    pub identity: Identity,
    pub ppid: u32,
    pub command: String,
    pub exe: Option<String>,
    pub link: Link,
}

/// What a check reads of the machine's processes: the seam by which it finds what
/// descends from a window's own process. Nothing of it ends a process.
pub trait Machine {
    /// Every process of the machine.
    fn processes(&self) -> Result<Vec<Running>>;
    /// Which process has the number `pid` now: none where none has it.
    fn identity(&self, pid: u32) -> Result<Option<Identity>>;
}

/// The machine the check runs on.
#[derive(Debug, Clone, Copy)]
pub struct ThisMachine;

impl Machine for ThisMachine {
    fn processes(&self) -> Result<Vec<Running>> {
        native::processes()
    }

    fn identity(&self, pid: u32) -> Result<Option<Identity>> {
        native::identity(pid)
    }
}

/// What holds a process while a check makes sure it is the one chosen, and ends it: on
/// Windows a handle to it, on macOS and Linux its number.
pub(crate) trait Held {
    /// When the process held began: none where it has ended.
    fn started(&self) -> io::Result<Option<u64>>;
    /// The unique identifier the process held has now, where the machine gives one (a Mac's),
    /// which the process chosen was proved by: none where the machine gives none or says nothing.
    fn unique(&self) -> io::Result<Option<u64>>;
    /// Ends the process held.
    fn end(&self) -> io::Result<()>;
    /// Stops the process held where it is, or lets it go on ([`Pause`]).
    fn pause(&self, pause: Pause) -> io::Result<()>;
}

/// What a check does to a process it chose that it does not end: stops it where it is, as
/// a machine too loaded to run it does, or lets it go on (Unix only: Windows has no stop of
/// a process).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pause {
    Stop,
    Continue,
}

/// How a check takes hold of the process that has a number now.
pub(crate) trait Holds {
    /// What holds the process `pid`: none where no process has the number.
    fn hold(&self, pid: u32) -> io::Result<Option<Box<dyn Held>>>;
}

/// What came of ending the processes a check chose: each process ended, gone already
/// (ended, or its number another process's now), or not ended, with why.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Ended {
    pub ended: Vec<Running>,
    pub gone: Vec<Running>,
    /// What may still run: the process, and why it could not be ended.
    pub failed: Vec<(Running, String)>,
    /// What a say-only door said it would end, and left ([`crate::door`]).
    pub said: Vec<Running>,
}

impl Ended {
    /// What may still run, a problem for the case's verdict each: a process that could not
    /// be reached, read or ended.
    /// A process a say-only door said and left is one: a say-only run passes no case.
    pub fn problems(&self) -> Vec<String> {
        self.failed
            .iter()
            .map(|(process, why)| {
                format!(
                    "process {} ({}) may still run: {why}",
                    process.identity.pid,
                    crate::live::cut(&process.command, 90)
                )
            })
            .chain(self.said.iter().map(|process| {
                format!(
                    "process {} ({}) was said, not ended: the door only says (CF_LIVE_AUDIT)",
                    process.identity.pid,
                    crate::live::cut(&process.command, 90)
                )
            }))
            .collect()
    }
}

/// What came of ending one process.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Ended,
    Gone,
    Failed(String),
}

/// Ends each of `chosen` (chosen from a window's own tree) that is still the process
/// chosen, checked on what holds it right before its end ([`end_one`]): a number another
/// process took since is not ended, and what could not be reached, read or ended is said
/// ([`Ended::failed`]). This is the one way a live check ends a process: no check ends
/// one by what its command line says, or by its number alone (a guard test of the live
/// checks holds them to it).
pub fn end_chosen(chosen: &[Running]) -> Ended {
    end_chosen_by(&native::Native, chosen)
}

/// [`end_chosen`], each process held by `holds`.
pub(crate) fn end_chosen_by(holds: &dyn Holds, chosen: &[Running]) -> Ended {
    let mut said = Ended::default();
    for process in chosen {
        match end_one(holds, process) {
            Outcome::Ended => said.ended.push(process.clone()),
            Outcome::Gone => said.gone.push(process.clone()),
            Outcome::Failed(why) => said.failed.push((process.clone(), why)),
        }
    }
    said
}

/// Stops each of `chosen` (chosen from a window's own tree) where it is, or lets it go on,
/// only while it is still the process chosen, as [`end_chosen`] ends one: the one way a live
/// check stops a process (3.0.7 chief-meeting: a window stopped for longer than a paste's
/// wait, as a loaded machine stops one). `Ended::ended` holds the ones it reached.
pub fn pause_chosen(chosen: &[Running], pause: Pause) -> Ended {
    pause_chosen_by(&native::Native, chosen, pause)
}

/// [`pause_chosen`], each process held by `holds`.
pub(crate) fn pause_chosen_by(holds: &dyn Holds, chosen: &[Running], pause: Pause) -> Ended {
    let mut said = Ended::default();
    for process in chosen {
        match act_on_one(holds, process, &|held| held.pause(pause), "its pause") {
            Outcome::Ended => said.ended.push(process.clone()),
            Outcome::Gone => said.gone.push(process.clone()),
            Outcome::Failed(why) => said.failed.push((process.clone(), why)),
        }
    }
    said
}

/// Ends the process `process` names, only while what `holds` holds of its number began
/// when it did and has the identifier it was proved by: one that has ended, or whose number
/// another process has now, is gone.
fn end_one(holds: &dyn Holds, process: &Running) -> Outcome {
    act_on_one(holds, process, &|held| held.end(), "its end")
}

/// Does `act` to the process `process` names, only while what `holds` holds of its number began
/// when it did and, where it was proved by a unique identifier ([`Link::Forked`]), has that
/// identifier; `what` names the act where it failed.
fn act_on_one(
    holds: &dyn Holds,
    process: &Running,
    act: &dyn Fn(&dyn Held) -> io::Result<()>,
    what: &str,
) -> Outcome {
    let held = match holds.hold(process.identity.pid) {
        Ok(Some(held)) => held,
        Ok(None) => return Outcome::Gone,
        Err(failed) => return Outcome::Failed(format!("it could not be reached: {failed}")),
    };
    match held.started() {
        Ok(Some(started)) if started == process.identity.started => {}
        Ok(_) => return Outcome::Gone,
        Err(failed) => {
            return Outcome::Failed(format!("when it began could not be read again: {failed}"))
        }
    }
    if let Link::Forked { own, .. } = process.link {
        match held.unique() {
            Ok(Some(now)) if now == own => {}
            Ok(Some(_)) => return Outcome::Gone,
            Ok(None) => {
                return Outcome::Failed("its unique identifier could not be read again".to_owned())
            }
            Err(failed) => {
                return Outcome::Failed(format!(
                    "its unique identifier could not be read again: {failed}"
                ))
            }
        }
    }
    match act(&*held) {
        Ok(()) => Outcome::Ended,
        Err(failed) => Outcome::Failed(format!("{what} failed: {failed}")),
    }
}

/// What the links of two rows say of whether `parent` made `child`, which names its number as its
/// parent's.
enum Made {
    /// The parent made it.
    Yes,
    /// Another process made it, as the links read say: a child that began before the parent, or
    /// whose forker is not the parent's identifier, is no one's of the window by this number.
    No,
    /// The links prove neither, and why: it runs on, named ([`unproved_under`]).
    Unproved(String),
}

/// Whether `parent` is proved to be the process that made `child`, which names its number as its
/// parent's. The proof is the link both rows were read with:
/// on a Mac the child's forker is the parent's own unique identifier; on Windows the creator the
/// child's handle names is the parent's number, both held open in one reading, with the parent
/// born no later than the child. A link of any other kind than the parent's, a link the system
/// would not say, and a parent two rows could be (one number or one identifier twice in a
/// reading) prove nothing. Only a table with no better link (Linux's) goes by the number and the
/// order they began in.
fn proof(all: &[Running], parent: &Running, child: &Running) -> Made {
    if child.identity.started < parent.identity.started {
        return Made::No;
    }
    let twice = |same: &dyn Fn(&Running) -> bool| all.iter().filter(|row| same(row)).count() > 1;
    match (parent.link, child.link) {
        (Link::Number, Link::Number) => Made::Yes,
        (Link::Forked { own, .. }, Link::Forked { forker, .. }) => {
            if own == 0
                || twice(&|row| matches!(row.link, Link::Forked { own: same, .. } if same == own))
            {
                Made::Unproved(format!(
                    "two processes of one reading are process {}'s identifier",
                    parent.identity.pid
                ))
            } else if forker != own {
                Made::No
            } else {
                Made::Yes
            }
        }
        (
            Link::Created { .. },
            Link::Created {
                by,
                described,
                held,
            },
        ) => {
            if twice(&|row| row.identity.pid == parent.identity.pid) {
                Made::Unproved(format!(
                    "two processes of one reading are number {}",
                    parent.identity.pid
                ))
            } else if by == parent.identity.pid || (held && described == Some(parent.identity.pid))
            {
                Made::Yes
            } else {
                Made::No
            }
        }
        (Link::Unread, _) | (_, Link::Unread) => Made::Unproved(format!(
            "the system said nothing of what made it, or of process {}",
            parent.identity.pid
        )),
        _ => Made::Unproved(format!(
            "it and process {} were not read the same way",
            parent.identity.pid
        )),
    }
}

/// A process the window's tree names as its parent that is not proved its child, with why: it is
/// not ended, and runs on.
pub type Unproved = (Running, String);

/// The window's tree among `all`, as proved: what descends from the process `root` (the processes it
/// started, and what they started in turn, each a child [`proof`] proves of its particular
/// parent), and, apart, each process the tree names as its parent that is not proved, with what
/// descends from it by number alone, each with why. None of either where `root` is not among `all`:
/// the window's process has gone, and its number may be another's.
fn tree_of(all: &[Running], root: Identity) -> (Vec<Running>, Vec<Unproved>) {
    let Some(first) = all.iter().find(|process| process.identity == root) else {
        return (Vec::new(), Vec::new());
    };
    let mut found: Vec<Running> = Vec::new();
    let mut refused: Vec<Unproved> = Vec::new();
    let mut parents = vec![first.clone()];
    while let Some(parent) = parents.pop() {
        for process in all {
            let by_git_bash = matches!(
                process.link,
                Link::Created { described: Some(said), held: true, .. }
                    if said == parent.identity.pid
            );
            let named = (process.ppid == parent.identity.pid || by_git_bash)
                && process.identity != root
                && process.identity != parent.identity
                && !found.iter().any(|seen| seen.identity == process.identity);
            if !named {
                continue;
            }
            match proof(all, &parent, process) {
                Made::Yes => {
                    parents.push(process.clone());
                    found.push(process.clone());
                }
                Made::No => {}
                Made::Unproved(why) => {
                    if !refused
                        .iter()
                        .any(|(seen, _)| seen.identity == process.identity)
                    {
                        refused.push((process.clone(), why));
                    }
                }
            }
        }
    }
    let mut left: Vec<Unproved> = Vec::new();
    let add = |left: &mut Vec<Unproved>, process: &Running, why: String| {
        if !found.iter().any(|seen| seen.identity == process.identity)
            && !left
                .iter()
                .any(|(seen, _)| seen.identity == process.identity)
        {
            left.push((process.clone(), why));
        }
    };
    // What Git Bash described, later, as under a process of the window, and no table proved.
    for process in all {
        if let Link::Created {
            described: Some(parent),
            ..
        } = process.link
        {
            if parent != 0
                && (parent == first.identity.pid
                    || found.iter().any(|seen| seen.identity.pid == parent))
            {
                refused.push((
                    process.clone(),
                    format!(
                        "Git Bash said, in a later listing, that process {parent} of the window made it; no reading of the table proves it"
                    ),
                ));
            }
        }
    }
    for (process, why) in refused {
        add(&mut left, &process, why.clone());
        // What descends from it by number alone is named too, with the same why.
        let mut under = vec![process.identity];
        while let Some(parent) = under.pop() {
            for child in all.iter().filter(|row| {
                row.ppid == parent.pid
                    && row.identity.started >= parent.started
                    && row.identity != root
            }) {
                if !left.iter().any(|(seen, _)| seen.identity == child.identity) {
                    under.push(child.identity);
                }
                add(
                    &mut left,
                    child,
                    format!(
                        "it is under process {}, which is not proved the window's: {why}",
                        parent.pid
                    ),
                );
            }
        }
    }
    (found, left)
}

/// What descends from the process `root` among `all`, each child proved to be the one its particular
/// parent made ([`proof`]): a link that is missing or ambiguous leaves that process, and all that is
/// under it, out ([`unproved_under`] names them). None where `root` is not among them: the window's
/// process has gone, and its number may be another's.
pub fn descending_from(all: &[Running], root: Identity) -> Vec<Running> {
    tree_of(all, root).0
}

/// What the window's tree names as made by a process of it and does not prove: left running, with
/// why.
pub fn unproved_under(all: &[Running], root: Identity) -> Vec<Unproved> {
    tree_of(all, root).1
}

/// What descends from the window's process `window` among `all` and holds `text` in its
/// command line, with what each of those started, each once: what a check ends of a
/// command the window runs. A process outside the window is none of it, whatever its
/// command line says.
pub fn holding(all: &[Running], window: Identity, text: &str) -> Vec<Running> {
    let mut chosen: Vec<Running> = Vec::new();
    for process in descending_from(all, window)
        .into_iter()
        .filter(|process| process.command.contains(text))
    {
        let under = descending_from(all, process.identity);
        for one in std::iter::once(process).chain(under) {
            if !chosen.iter().any(|seen| seen.identity == one.identity) {
                chosen.push(one);
            }
        }
    }
    chosen
}

/// The folder the process `pid` runs in, as the system says it now (the real path, with
/// no link in it): none where it cannot be read, where no process has the number, and on
/// any machine but a Mac's (the one place a check needs it, [`orphans_in_folder`]).
pub fn working_folder(pid: u32) -> Option<std::path::PathBuf> {
    native::working_folder(pid)
}

/// A folder of the run's own: one under the root the rig made for the run (its temp folder, which
/// no other run has and nobody else works in), the one ground a path gives a rule of the kit's
/// ([`orphans_in_folder`], and the words of what the closed windows left, [`super::Watch`]). A
/// path outside the run's root is the person's or another run's: a live rig's home is the
/// person's own, and the close's sweep that took `/Users/…` for a word of the run ended 22
/// processes of the owner's and of other tasks (2026-11-12). It is refused here, whatever asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunFolder {
    /// The path as it was given, as a command line of what the kit asked writes it.
    given: std::path::PathBuf,
    /// Its real path, with no link in it, as the system says where a process runs.
    real: std::path::PathBuf,
}

impl RunFolder {
    /// `path` as a folder of the run whose root is `root`: none where its real path is not
    /// `root`'s or under it (compared by whole components), where either cannot be read, or
    /// where `root` holds the home of the person who runs the kit, as no run's root does.
    pub fn under(root: &std::path::Path, path: &std::path::Path) -> Option<Self> {
        let root = root.canonicalize().ok()?;
        let real = path.canonicalize().ok()?;
        let wide = crate::live::machine::home()
            .is_ok_and(|home| home.canonicalize().unwrap_or(home).starts_with(&root));
        (!wide && real.starts_with(&root)).then(|| Self {
            given: path.to_path_buf(),
            real,
        })
    }

    /// A folder of the unit tests' process tables, taken as the run's unread.
    #[cfg(test)]
    pub(crate) fn assumed(path: &std::path::Path) -> Self {
        Self {
            given: path.to_path_buf(),
            real: path.to_path_buf(),
        }
    }

    /// The path as it was given.
    pub fn given(&self) -> &std::path::Path {
        &self.given
    }

    /// Its real path.
    pub fn real(&self) -> &std::path::Path {
        &self.real
    }
}

/// The processes of `all` that a check can call its own although no window holds them any
/// longer, and what descends from them: each holds `text` in its command line, is an orphan
/// (its parent is the system's init, 1), began no earlier than `since` (when the check's window
/// began), and runs in `folder`, which is the run's own ([`RunFolder`]: under the run's temp root,
/// whose real path is compared), as `folder_of` says it. A process that began before the window,
/// that has a parent, whose folder is another or cannot be read, or whose command does not hold
/// `text` is none of them, whatever else it says: it is the parent, the start and the folder
/// together that make a process the run's, never the command line alone.
///
/// A job the agent detaches from a tool shell that leads a session of its own (`nohup sleep
/// 4711 &`) is one: the shell has ended, so the job's parent is init's; the session is the
/// shell's, not the window's; and a system binary shows no marker. What it keeps is the
/// folder it was started in, which is the worker's own, and when it began.
pub fn orphans_in_folder(
    all: &[Running],
    text: &str,
    folder: &RunFolder,
    since: u64,
    folder_of: impl Fn(u32) -> Option<std::path::PathBuf>,
) -> Vec<Running> {
    // An orphan has no parent among the processes, so nothing that descends from one orphan is
    // another orphan, and no process is found twice.
    all.iter()
        .filter(|process| {
            process.ppid == 1
                && process.identity.started >= since
                && process.command.contains(text)
                && folder_of(process.identity.pid).is_some_and(|there| there == folder.real)
        })
        .flat_map(|orphan| {
            std::iter::once(orphan.clone()).chain(descending_from(all, orphan.identity))
        })
        .collect()
}

/// What macOS says of a process, read where it is held (`proc_pidinfo`): the process that
/// started it, and when it began, in microseconds since 1970.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Bsd {
    pub ppid: u32,
    pub started: u64,
    /// What the same read says made it ([`Link::Forked`]), or [`Link::Unread`] where the system
    /// would not say.
    pub link: Link,
}

/// The processes `ps` listed (`listed`), each with when it began as macOS says it to the
/// microsecond (`read`): one the system says nothing of, whose parent it says is another than
/// `ps` listed, or whose start falls in another second than `ps` said (`second_of`, the second of a
/// start in microseconds as `ps` says it), has ended since, and its number is another's or may be:
/// it is not listed. The parent alone tells nothing where both have none (1): a number that went
/// from one orphan to another in between would put the first one's command line on the second
/// (T-1557).
#[cfg(any(target_os = "macos", test))]
pub(crate) fn refined_by_bsd(
    listed: Vec<Running>,
    read: impl Fn(u32) -> io::Result<Option<Bsd>>,
    second_of: impl Fn(u64) -> Option<u64>,
) -> Vec<Running> {
    listed
        .into_iter()
        .filter_map(|mut process| {
            let bsd = read(process.identity.pid).ok()??;
            if bsd.ppid != process.ppid || second_of(bsd.started)? != process.identity.started {
                return None;
            }
            process.identity.started = bsd.started;
            process.link = bsd.link;
            Some(process)
        })
        .collect()
}

/// How far, in 100 ns, the table's creation time of a process may be from its handle's:
/// the table says it to the microsecond.
#[cfg(any(windows, test))]
const TABLE_GRAIN: u64 = 10;

/// What a process's own handle says: when it was created, to the 100 ns, and the number of the
/// process that created it where the system says it.
#[cfg(any(windows, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Opened {
    pub created: u64,
    pub creator: Option<u32>,
}

/// The processes Windows' table listed (`listed`), each with its creation time and its creator
/// as its own handle says them (`read`), to the 100 ns. One the handle says has exited, or that
/// no process has the number of, is not listed; nor one whose handle says another creation time
/// than the table, give or take its grain ([`TABLE_GRAIN`]): its number is another's now. One
/// that cannot be opened (the system's own, another user's) keeps what the table said and proves
/// nothing ([`Link::Unread`]): no check can end it. The handles are read from one set held open
/// together by the caller, so that no number in the reading was another process's meanwhile
/// (a number is not given again while a handle to its process is open): the creator one handle
/// says and the creation time another says are of processes that all stood at once.
#[cfg(any(windows, test))]
pub(crate) fn refined_by_handle(
    listed: Vec<Running>,
    read: impl Fn(u32) -> io::Result<Option<Opened>>,
) -> Vec<Running> {
    listed
        .into_iter()
        .filter_map(|mut process| match read(process.identity.pid) {
            Ok(Some(opened)) if opened.created.abs_diff(process.identity.started) < TABLE_GRAIN => {
                process.identity.started = opened.created;
                // The table and the handle name one creator, or neither is believed.
                process.link = match opened.creator {
                    Some(by) if by == process.ppid => Link::Created {
                        by,
                        described: None,
                        held: false,
                    },
                    _ => Link::Unread,
                };
                Some(process)
            }
            Ok(_) => None,
            Err(_) => {
                process.link = Link::Unread;
                Some(process)
            }
        })
        .collect()
}

/// One row of Git Bash's `ps -W`: a process by its Cygwin number, its parent's, and its
/// Windows number.
#[derive(Debug, PartialEq, Eq)]
struct CygwinRow {
    pid: u32,
    ppid: u32,
    winpid: u32,
}

/// What Git Bash's `ps -W` says, read for the first four of its columns (`PID PPID PGID
/// WINPID`) of each row: the header and any line that is no row say nothing. Cygwin's `ps`
/// begins a row with a status letter (`S` stopped, `I` reading its terminal, `O` writing it) or
/// a space, then the number in seven columns, so a first word of one letter is the status and
/// the number is the next: a process that reads its terminal is a row all the same.
fn cygwin_table(listing: &str) -> Vec<CygwinRow> {
    listing
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let first = words.next()?;
            let first =
                if first.len() == 1 && first.chars().all(|letter| letter.is_ascii_alphabetic()) {
                    words.next()?
                } else {
                    first
                };
            let mut numbers = std::iter::once(first)
                .chain(words)
                .map(|word| word.parse::<u32>().ok());
            let (pid, ppid, _group, winpid) = (
                numbers.next()??,
                numbers.next()??,
                numbers.next()??,
                numbers.next()??,
            );
            Some(CygwinRow { pid, ppid, winpid })
        })
        .collect()
}

/// The folder an image is in, up to its last `\` or `/`; none where its path names none.
fn image_folder(exe: &str) -> Option<String> {
    exe.rsplit_once(['\\', '/'])
        .map(|(folder, _)| folder.to_owned())
}

/// `listed` (Windows' table) with the parent Git Bash says for each Cygwin program whose own
/// has gone ([`Link::Created::described`]). Git Bash's `sh` runs a program of its own kind from a
/// script by forking a copy of itself, which starts the program and is gone by the time anyone
/// looks (zeewin, 2026-10-21: the sleep of the `stuck` script, started by `sh ./script`, names a
/// parent that no process has). `ps -W` of the folder of the image the program runs (`ps`, asked
/// once for each folder: its text, or none where there is no `ps` there) says its Cygwin parent,
/// and which Windows process that is. That parent decides nothing by itself (T-2228): its numbers
/// are of the moment it ran. It becomes an edge only where `held` (the creation time a process's
/// handle, held open since before the reading, says now: none where it is not held or has exited)
/// says both the program and the parent described are the very processes of the table, to the
/// 100 ns, after the reading, and the parent began no later than the program: a held handle's
/// number is no other process's, so the number Git Bash names is that process. Where either end is
/// not held or a time differs, the program stays out of the window's tree and is named
/// ([`unproved_under`]). The folder is the image's, which Windows' table gives plain, and not the
/// command line's spelling of it (`Git\bin\..\usr\bin` and `Git\usr\bin` are one folder); a
/// process whose image the table could not say is left as it was.
pub fn with_cygwin_parents(
    mut listed: Vec<Running>,
    ps: impl Fn(&str) -> Option<String>,
    held: impl Fn(u32) -> Option<u64>,
) -> Vec<Running> {
    let began: std::collections::HashMap<u32, u64> = listed
        .iter()
        .map(|process| (process.identity.pid, process.identity.started))
        .collect();
    let mut tables: std::collections::HashMap<String, Vec<CygwinRow>> =
        std::collections::HashMap::new();
    for process in &mut listed {
        let Link::Created { by, .. } = process.link else {
            continue;
        };
        if process.ppid == 0 || began.contains_key(&process.ppid) {
            continue;
        }
        let Some(folder) = process.exe.as_deref().and_then(image_folder) else {
            continue;
        };
        let table = tables.entry(folder).or_insert_with_key(|folder| {
            ps(folder)
                .map(|text| cygwin_table(&text))
                .unwrap_or_default()
        });
        let parent = table
            .iter()
            .find(|row| row.winpid == process.identity.pid)
            .and_then(|own| table.iter().find(|row| row.pid == own.ppid))
            .map(|row| row.winpid);
        if let Some(parent) = parent {
            // Read after the `ps -W` text is in hand: both ends, with the times they were listed with.
            let both_held = began.get(&parent).is_some_and(|parent_began| {
                *parent_began <= process.identity.started
                    && held(process.identity.pid) == Some(process.identity.started)
                    && held(parent) == Some(*parent_began)
            });
            process.link = Link::Created {
                by,
                described: Some(parent),
                held: both_held,
            };
        }
    }
    listed
}

/// What `ps` says of every process, or of the process `pid` alone, in the C locale so
/// that its `lstart` is what [`ps_lines`] reads.
#[cfg(unix)]
fn ps(pid: Option<u32>) -> Result<Vec<Running>> {
    let words = match pid {
        Some(pid) => vec![
            "-o".to_owned(),
            "pid=,ppid=,lstart=,command=".to_owned(),
            "-p".to_owned(),
            pid.to_string(),
        ],
        None => vec!["-axo".to_owned(), "pid=,ppid=,lstart=,command=".to_owned()],
    };
    let ran = Run::new("ps").args(words).var("LC_ALL", "C").run()?;
    // `ps -p` of a number no process has says nothing, and ends with 1.
    let nothing = pid.is_some() && ran.stdout.trim().is_empty();
    if ran.code != Some(0) && !nothing {
        return Err(Error::Program {
            action: "list the processes with",
            program: "ps".to_owned(),
            source: io::Error::other(ran.to_string()),
        });
    }
    Ok(ps_lines(&ran.stdout))
}

/// The processes of a listing `ps -o pid=,ppid=,lstart=,command=` made: on each line the
/// number, the parent's, five words of `lstart` (`Fri Oct  6 04:44:58 2026`) and the
/// command line, as it is spaced.
#[cfg(any(unix, test))]
fn ps_lines(listing: &str) -> Vec<Running> {
    listing
        .lines()
        .filter_map(|line| {
            let mut rest = line.trim_start();
            let mut words = Vec::new();
            for _ in 0..7 {
                let (word, after) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                words.push(word);
                rest = after.trim_start();
            }
            Some(Running {
                identity: Identity {
                    pid: words[0].parse().ok()?,
                    started: started(&words[2..7])?,
                },
                ppid: words[1].parse().ok()?,
                command: rest.to_owned(),
                exe: None,
                link: Link::Number,
            })
        })
        .collect()
}

/// The months as `lstart` names them in the C locale.
#[cfg(any(unix, test))]
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The five words of an `lstart` (`Fri Oct 16 04:44:58 2026`) as `YYYYMMDDhhmmss`.
#[cfg(any(unix, test))]
fn started(lstart: &[&str]) -> Option<u64> {
    let month = MONTHS.iter().position(|name| *name == lstart[1])?;
    let month = u64::try_from(month).ok()? + 1;
    let day: u64 = lstart[2].parse().ok()?;
    let mut clock = lstart[3].split(':').map(|part| part.parse::<u64>().ok());
    let (hour, minute, second) = (clock.next()??, clock.next()??, clock.next()??);
    let year: u64 = lstart[4].parse().ok()?;
    Some(((((year * 100 + month) * 100 + day) * 100 + hour) * 100 + minute) * 100 + second)
}

/// Where the fields of one entry of what `NtQuerySystemInformation(SystemProcessInformation)`
/// writes are, on a 64-bit Windows (x64 and arm64 alike; the layout `SYSTEM_PROCESS_INFORMATION`
/// has had since Windows 7): the offset of the next entry (0 at the last), the creation time (the
/// 100 ns since 1601 that `GetProcessTimes` says, 0 for the system's own Idle), the number of the
/// process and the number of the one that created it.
#[cfg(any(windows, test))]
mod entry {
    pub const NEXT: usize = 0;
    pub const CREATED: usize = 32;
    pub const PID: usize = 80;
    pub const CREATOR: usize = 88;
}

/// The processes of what `NtQuerySystemInformation(SystemProcessInformation)` wrote into `buffer`:
/// each by its number, its creator's and when it began, which is all that call says (the path of
/// its image and its command line are read where a process is held, [`refined_by_handle`] and
/// the caller's). An entry that is cut short, or that points outside the buffer, ends the table:
/// what was read before it is kept, and nothing is made up of what is not there. (A next offset
/// is at least 1, so each entry is further on than the last and the read ends.)
#[cfg(any(windows, test))]
fn table_of_system(buffer: &[u8]) -> Vec<Running> {
    let word = |at: usize, size: usize| -> Option<u64> {
        let bytes = buffer.get(at..at.checked_add(size)?)?;
        let mut whole = [0u8; 8];
        whole[..size].copy_from_slice(bytes);
        Some(u64::from_le_bytes(whole))
    };
    let mut listed = Vec::new();
    let mut at = 0usize;
    loop {
        let row = (|| {
            Some(Running {
                identity: Identity {
                    pid: u32::try_from(word(at + entry::PID, 8)?).ok()?,
                    started: word(at + entry::CREATED, 8)?,
                },
                ppid: u32::try_from(word(at + entry::CREATOR, 8)?).ok()?,
                command: String::new(),
                exe: None,
                link: Link::Number,
            })
        })();
        listed.extend(row);
        let Some(next) = word(at + entry::NEXT, 4).filter(|next| *next != 0) else {
            break;
        };
        let Some(after) = usize::try_from(next)
            .ok()
            .and_then(|next| at.checked_add(next))
        else {
            break;
        };
        at = after;
    }
    listed
}

#[cfg(test)]
mod ownership;
#[cfg(test)]
mod tests;

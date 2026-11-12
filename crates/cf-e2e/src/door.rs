//! The one door of the kit's endings and deletions: every process the kit ends or stops, every
//! window it closes, every own program it kills, and every file or folder it takes away goes
//! through here, each with the ground that makes it the run's own (T-1561). At 18:58 on
//! 2026-11-12 a live run of a kit branch ended 22 processes that were none of its own (the
//! owner's app helpers, two Codex daemons, a git ssh connection, another task's benchmark), and
//! the review after found the same in what the kit deletes (T-1557): another run's rig folders, a
//! login copy in use, the owner's own records by a name's end. A rule test holds the kit to the
//! door (`tests/door_rule.rs`): no ending or deletion is written anywhere else.
//!
//! The grounds, each the proof the door asks:
//!
//! - **A process** ([`Door::end`], [`Door::pause`]) only as a [`Grounded`] set holds it, which the
//!   descent alone makes: what descends from a window's or the test's own process, an orphan
//!   that holds the command and runs in the run's own folder, a stand-in job by its file, or a
//!   decoy the test started itself. Each is ended only while it is still the process chosen.
//! - **An own program** ([`Door::end_child`]): a child this process started and still holds.
//! - **A window** ([`Door::close_pane`]): a pane of the rig's own pane host.
//! - **A folder the door made** ([`made_in`], [`Made`]), and **a path under one**
//!   ([`Door::remove_in_made`]): a folder of a random name made by this process, known here.
//! - **A folder nobody uses** ([`Door::remove_unused`]): a login copy of a run that is gone, or
//!   an older rig folder, taken only while no process runs in it or names it in its command line
//!   or its environment, as the machine says now; where that cannot be read, it is left.
//! - **A record of the renewal's** ([`Door::remove_record`]): in the harness's records folder,
//!   the one entry named for the renewal's own folder, by its whole name.
//!
//! **Say only.** With `CF_LIVE_AUDIT=1` the door does nothing: it writes what it would do, one
//! line each (the act, the process or the path, the ground), to the file `CF_LIVE_AUDIT_FILE`
//! names (`cargo xtask live` names one) or one of its own in the temp folder, and says each on
//! the error output. A case that needs an ending to have happened stops there and fails, so
//! that no say-only run passes. The tests make a door of their own ([`Door::saying`]) and never
//! set the variable.
//!
//! This is the kit's door. The product's door (`live::members::door`, what a member's lane
//! shows) is another thing.

use std::fmt;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};

use crate::live::members::{
    descending_from, end_chosen, holding, orphans_in_folder, pause_chosen, Ended, Identity,
    Machine, Pause, RunFolder, Running, ThisMachine,
};

mod usage;

/// The variable that makes the door say only.
pub const AUDIT: &str = "CF_LIVE_AUDIT";

/// The variable that names the file the door writes what it would do in.
pub const AUDIT_FILE: &str = "CF_LIVE_AUDIT_FILE";

/// The file a folder kept for a person holds: an older rig folder that holds it is never swept.
pub const KEPT: &str = "consensflow-kept.txt";

/// How a rig names its folder, by the start of its name.
pub const RIG_PREFIX: &str = "consensflow-integration-";

/// The door: one that acts, or one that only says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Door {
    /// The file what would be done is written in, when the door only says.
    said: Option<PathBuf>,
}

impl Door {
    /// The run's door, as its variables ask: one that acts unless [`AUDIT`] says to say only.
    // The kit reads its own switch, as the run was started with it.
    #[allow(clippy::disallowed_methods)]
    pub fn of_run() -> &'static Door {
        static DOOR: OnceLock<Door> = OnceLock::new();
        DOOR.get_or_init(|| {
            Door::asked(
                std::env::var_os(AUDIT).as_deref(),
                std::env::var_os(AUDIT_FILE).as_deref(),
            )
        })
    }

    /// The door the variables `audit` and `file` ask for: say-only where `audit` is set to
    /// anything but `0` or nothing, writing to `file` or to a file of its own in the temp folder.
    pub fn asked(audit: Option<&std::ffi::OsStr>, file: Option<&std::ffi::OsStr>) -> Door {
        let saying = audit.is_some_and(|audit| !audit.is_empty() && audit != "0");
        if !saying {
            return Door::acting();
        }
        let file = file
            .filter(|file| !file.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!("live-audit-{}.txt", std::process::id()))
            });
        Door::saying(file)
    }

    /// A door that acts.
    pub fn acting() -> Door {
        Door { said: None }
    }

    /// A door that only says, in `file`.
    pub fn saying(file: PathBuf) -> Door {
        Door { said: Some(file) }
    }

    /// The file it says in, where it only says.
    pub fn says_in(&self) -> Option<&Path> {
        self.said.as_deref()
    }

    /// Whether it only says.
    pub fn says_only(&self) -> bool {
        self.said.is_some()
    }

    /// Writes that it would `act` on `what` by `ground`, where it only says: a line in its file
    /// and on the error output.
    #[allow(clippy::print_stderr)] // Said in the run's output as well as in its file.
    fn say(&self, act: &str, what: &str, ground: &str) {
        let Some(file) = &self.said else {
            return;
        };
        let line = format!("{act}\t{what}\t{ground}");
        eprintln!("[say only] {line}");
        let written = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file)
            .and_then(|mut open| writeln!(open, "{line}"));
        if let Err(failed) = written {
            eprintln!(
                "[say only] {} could not be written: {failed}",
                file.display()
            );
        }
    }

    /// Ends each process of `chosen` that is still the process chosen.
    pub fn end(&self, chosen: &Grounded) -> Ended {
        if self.says_only() {
            for (process, ground) in chosen.each() {
                self.say("end", &said_process(process), &ground.to_string());
            }
            return said_only(chosen);
        }
        end_chosen(&chosen.processes())
    }

    /// Stops each process of `chosen` where it is, or lets it go on, while it is still the
    /// process chosen.
    pub fn pause(&self, chosen: &Grounded, pause: Pause) -> Ended {
        if self.says_only() {
            let act = match pause {
                Pause::Stop => "stop",
                Pause::Continue => "continue",
            };
            for (process, ground) in chosen.each() {
                self.say(act, &said_process(process), &ground.to_string());
            }
            return said_only(chosen);
        }
        pause_chosen(&chosen.processes(), pause)
    }

    /// Kills `child`, a program this process started and holds (`name` says which): whether
    /// it was killed (a door that only says kills none).
    pub fn end_child(&self, name: &str, child: &mut std::process::Child) -> bool {
        if self.says_only() {
            self.say(
                "kill",
                &format!("process {} ({name})", child.id()),
                "a child this process started and holds",
            );
            return false;
        }
        // One that has ended already is no matter.
        let _ = child.kill();
        true
    }

    /// Closes the pane `pane` of the rig's own pane host, by asking `host` (the rig's
    /// `pane.kill`): what the host answered, or what the door says it would have.
    pub fn close_pane(
        &self,
        pane: &serde_json::Value,
        host: impl FnOnce(&str, serde_json::Value) -> crate::Result<serde_json::Value>,
    ) -> crate::Result<serde_json::Value> {
        if self.says_only() {
            self.say(
                "close",
                &format!("pane {}", pane["id"]),
                "a pane of the rig's own pane host",
            );
            return Ok(serde_json::json!({ "ok": true, "said": true }));
        }
        host("pane.kill", pane.clone())
    }

    /// Closes `name`, a terminal this test opened itself and holds (a pane of its own pane table,
    /// a pseudo-terminal's child), by `close`: whether it did (a say-only door says it and does
    /// not).
    pub fn close_own_terminal(&self, name: &str, close: impl FnOnce()) -> bool {
        if self.says_only() {
            self.say("close", name, "a terminal this test opened and holds");
            return false;
        }
        close();
        true
    }

    /// Stops `child`, a program this process started and holds (`name` says which), where it is,
    /// and lets it go on after `span` ([`crate::process::stall`]): whether it was stopped (a
    /// say-only door says it and stops nothing).
    #[cfg(unix)]
    pub fn stall_child(
        &self,
        name: &str,
        child: &std::process::Child,
        span: std::time::Duration,
    ) -> bool {
        if self.says_only() {
            self.say(
                &format!("stall {span:?}"),
                &format!("process {} ({name})", child.id()),
                "a child this process started and holds",
            );
            return false;
        }
        crate::process::stall(child.id(), span).is_ok()
    }

    /// Sends `signal` to `child`, a program this process started and holds (`name` says which):
    /// whether it was sent (a say-only door says it and sends none).
    pub fn signal_child(
        &self,
        name: &str,
        child: &std::process::Child,
        signal: crate::process::Signal,
    ) -> bool {
        if self.says_only() {
            self.say(
                &format!("signal {signal:?}"),
                &format!("process {} ({name})", child.id()),
                "a child this process started and holds",
            );
            return false;
        }
        // A program that is gone is no error.
        let _ = crate::process::signal(child.id(), signal);
        true
    }

    /// Takes away `path`, a folder this process made through the door ([`made_in`]): left, and
    /// said, where it is none.
    pub fn remove_made(&self, path: &Path) -> Removed {
        let known = canonical(path).is_some_and(|real| registry().contains(&real));
        if !known {
            return Removed::Left(path.to_path_buf(), "it is no folder this run made".into());
        }
        if self.says_only() {
            self.say(
                "remove",
                &said_path(path),
                "a folder this run made (kept, and marked kept)",
            );
            let _ = fs::write(path.join(KEPT), "kept by a say-only run\n");
            return Removed::Said(path.to_path_buf());
        }
        let removed = remove_whole(path);
        forget_made(path);
        removed
    }

    /// Takes away `path`, under a folder this process made through the door: left, and said,
    /// where it is under none (compared by the real path of what holds it, by whole components).
    pub fn remove_in_made(&self, path: &Path) -> Removed {
        let Some(real) = path
            .parent()
            .and_then(canonical)
            .zip(path.file_name())
            .map(|(parent, name)| parent.join(name))
        else {
            return Removed::Absent(path.to_path_buf());
        };
        let under = registry()
            .iter()
            .any(|made| real.starts_with(made) && real != *made);
        if !under {
            return Removed::Left(
                path.to_path_buf(),
                "it is under no folder this run made".into(),
            );
        }
        if fs::symlink_metadata(path).is_err() {
            return Removed::Absent(path.to_path_buf());
        }
        if self.says_only() {
            self.say("remove", &said_path(path), "under a folder this run made");
            return Removed::Said(path.to_path_buf());
        }
        remove_whole(path)
    }

    /// Takes away `path`, a folder named for the process `owner`, once that process is gone and
    /// no process uses the folder, as the machine says now ([`usage`]): where either is not so,
    /// or cannot be read, it is left and the reason said.
    pub fn remove_unused(&self, path: &Path, owner: u32) -> Removed {
        match ThisMachine.identity(owner) {
            Ok(None) => {}
            Ok(Some(_)) => return Removed::Left(path.to_path_buf(), "its run is there".into()),
            Err(failed) => {
                return Removed::Left(
                    path.to_path_buf(),
                    format!("whether its run is there could not be read: {failed}"),
                )
            }
        }
        self.remove_if_unused(path, Some(owner), "its run is gone and no process uses it")
    }

    /// Takes away `path`, a rig's folder older than `older` that no person kept ([`KEPT`]) and
    /// no process uses, as the machine says now: what `cargo xtask live sweep` takes. Anything
    /// else is left and the reason said. On Windows every one is left: the kit cannot ask there
    /// which processes work in a folder or hold a file in it, and nothing in Temp is taken for
    /// what command lines say (T-1368).
    pub fn remove_stale_rig(
        &self,
        path: &Path,
        older: std::time::Duration,
        now: std::time::SystemTime,
    ) -> Removed {
        let named = path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(RIG_PREFIX));
        let Ok(found) = fs::symlink_metadata(path) else {
            return Removed::Absent(path.to_path_buf());
        };
        if !named || !found.is_dir() {
            return Removed::Left(path.to_path_buf(), "it is no rig's folder".into());
        }
        if path.join(KEPT).exists() {
            return Removed::Left(path.to_path_buf(), "it was kept".into());
        }
        if cfg!(windows) {
            return Removed::Left(
                path.to_path_buf(),
                "on Windows the kit cannot read which processes work in a folder".into(),
            );
        }
        let age = found
            .modified()
            .ok()
            .and_then(|changed| now.duration_since(changed).ok());
        if age.is_none_or(|age| age <= older) {
            return Removed::Left(path.to_path_buf(), "it is not old enough".into());
        }
        self.remove_if_unused(path, None, "an older rig folder no process uses")
    }

    fn remove_if_unused(&self, path: &Path, owner: Option<u32>, ground: &str) -> Removed {
        match usage::in_use(path, owner) {
            Ok(false) => {}
            Ok(true) => {
                return Removed::Left(path.to_path_buf(), "a process uses it".into());
            }
            Err(why) => {
                return Removed::Left(
                    path.to_path_buf(),
                    format!("whether a process uses it could not be read: {why}"),
                )
            }
        }
        if self.says_only() {
            self.say("remove", &said_path(path), ground);
            return Removed::Said(path.to_path_buf());
        }
        remove_whole(path)
    }

    /// Takes away `path`, below the kit's own live folder of `home`
    /// ([`crate::live::machine::live_root`], `.consensflow-candidate/live`), which only the kit
    /// writes in: a fixed file or folder a live case makes there by its own name. Left, and
    /// said, where it is not below that folder.
    pub fn remove_in_live_root(&self, home: &Path, path: &Path) -> Removed {
        let Some(live) = canonical(&crate::live::machine::live_root(home)) else {
            return Removed::Left(
                path.to_path_buf(),
                "there is no live folder in that home".into(),
            );
        };
        let Some(real) = path
            .parent()
            .and_then(canonical)
            .zip(path.file_name())
            .map(|(parent, name)| parent.join(name))
        else {
            return Removed::Absent(path.to_path_buf());
        };
        if !real.starts_with(&live) || real == live {
            return Removed::Left(
                path.to_path_buf(),
                "it is not below the kit's own live folder".into(),
            );
        }
        if fs::symlink_metadata(path).is_err() {
            return Removed::Absent(path.to_path_buf());
        }
        if self.says_only() {
            self.say(
                "remove",
                &said_path(path),
                "below the kit's own live folder",
            );
            return Removed::Said(path.to_path_buf());
        }
        remove_whole(path)
    }

    /// Takes away `path`, a file or a folder whose name this process made ([`own_name`]): left,
    /// and said, where its name is none of this process's.
    pub fn remove_own_named(&self, path: &Path) -> Removed {
        let own = path
            .file_name()
            .is_some_and(|name| own_names().iter().any(|known| name == known.as_str()));
        if !own {
            return Removed::Left(path.to_path_buf(), "its name is none this run made".into());
        }
        if fs::symlink_metadata(path).is_err() {
            return Removed::Absent(path.to_path_buf());
        }
        if self.says_only() {
            self.say("remove", &said_path(path), "named by this run's own name");
            return Removed::Said(path.to_path_buf());
        }
        remove_whole(path)
    }

    /// Takes away `record`, the one entry a harness wrote for the renewal's own folder.
    pub fn remove_record(&self, record: &Record) -> Removed {
        let path = record.path();
        if fs::symlink_metadata(&path).is_err() {
            return Removed::Absent(path);
        }
        if self.says_only() {
            self.say(
                "remove",
                &said_path(&path),
                "the record named for the renewal's own folder",
            );
            return Removed::Said(path);
        }
        remove_whole(&path)
    }
}

/// What a say-only door answers of an ending: every process said, none ended.
fn said_only(chosen: &Grounded) -> Ended {
    Ended {
        said: chosen.processes(),
        ..Ended::default()
    }
}

/// A process as the door says it: its number, when it began, and its command line.
fn said_process(process: &Running) -> String {
    format!(
        "process {} begun {} ({})",
        process.identity.pid,
        process.identity.started,
        crate::live::cut(&process.command, 160)
    )
}

/// A path as the door says it, and whether it is there.
fn said_path(path: &Path) -> String {
    let there = if fs::symlink_metadata(path).is_ok() {
        "there"
    } else {
        "not there"
    };
    format!("{} ({there})", path.display())
}

/// The real path of `path`, with Windows' `\\?\` taken off: none where it cannot be read.
fn canonical(path: &Path) -> Option<PathBuf> {
    let real = path.canonicalize().ok()?;
    let said = real.to_string_lossy();
    Some(match said.strip_prefix(r"\\?\") {
        Some(plain) => PathBuf::from(plain),
        None => real,
    })
}

/// Takes `path` away, whatever it is: a folder with what it holds, or a file or a link itself.
fn remove_whole(path: &Path) -> Removed {
    let Ok(found) = fs::symlink_metadata(path) else {
        return Removed::Absent(path.to_path_buf());
    };
    let removed = if found.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    match removed {
        Ok(()) => Removed::Removed(path.to_path_buf()),
        Err(failed) => Removed::Left(
            path.to_path_buf(),
            format!("it could not be taken: {failed}"),
        ),
    }
}

/// What came of a removal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removed {
    /// It was taken away.
    Removed(PathBuf),
    /// A say-only door said it, and left it.
    Said(PathBuf),
    /// It was left, and why.
    Left(PathBuf, String),
    /// It was not there.
    Absent(PathBuf),
}

impl Removed {
    /// Whether it is gone, or was not there.
    pub fn is_gone(&self) -> bool {
        matches!(self, Removed::Removed(_) | Removed::Absent(_))
    }
}

impl fmt::Display for Removed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Removed::Removed(path) => write!(formatter, "{} taken away", path.display()),
            Removed::Said(path) => write!(formatter, "{} said, not taken", path.display()),
            Removed::Left(path, why) => write!(formatter, "{} left: {why}", path.display()),
            Removed::Absent(path) => write!(formatter, "{} not there", path.display()),
        }
    }
}

/// The file names this process made ([`own_name`]).
fn own_names() -> std::sync::MutexGuard<'static, Vec<String>> {
    static NAMES: Mutex<Vec<String>> = Mutex::new(Vec::new());
    NAMES.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A file name that is this process's alone, `<stem>-<pid>-<ms>-<n>.<extension>`, known to the
/// door, which takes what has that name away ([`Door::remove_own_named`]): no file of another
/// run, or of another case of this run, can have it.
pub fn own_name(stem: &str, extension: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let millis = jiff::Timestamp::now().as_millisecond();
    let name = format!(
        "{stem}-{}-{millis}-{}.{extension}",
        std::process::id(),
        COUNT.fetch_add(1, Ordering::Relaxed)
    );
    own_names().push(name.clone());
    name
}

/// The folders this process made through the door, by their real paths.
fn registry() -> std::sync::MutexGuard<'static, Vec<PathBuf>> {
    static MADE: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
    MADE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Forgets `path` as a folder this process made.
fn forget_made(path: &Path) {
    let real = canonical(path);
    registry().retain(|made| Some(made) != real.as_ref() && made != path);
}

/// A folder of a random name this process made through the door, in `parent`, its name begun
/// with `prefix`: taken away through the door when it is dropped, unless it is kept.
pub fn made_in(parent: &Path, prefix: &str) -> io::Result<Made> {
    let folder = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(parent)?
        .keep();
    let real = canonical(&folder).unwrap_or_else(|| folder.clone());
    registry().push(real);
    Ok(Made {
        path: folder,
        kept: false,
    })
}

/// [`made_in`], in the machine's temp folder.
pub fn made(prefix: &str) -> io::Result<Made> {
    made_in(&std::env::temp_dir(), prefix)
}

/// A folder this process made through the door ([`made_in`]).
#[derive(Debug)]
pub struct Made {
    path: PathBuf,
    kept: bool,
}

impl Made {
    /// Where it is.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Keeps it for a person to look at: it is not taken away when it is dropped, it holds the
    /// mark [`KEPT`], and no sweep takes it. Where it is.
    pub fn keep(&mut self) -> PathBuf {
        self.kept = true;
        let _ = fs::write(self.path.join(KEPT), "kept for a person to look at\n");
        self.path.clone()
    }

    /// Hands it on as a path: it is not taken away when this is dropped, and is still a folder
    /// this run made ([`Door::remove_made`] takes it later, as a rig's root goes with its rig).
    pub fn hand_on(mut self) -> PathBuf {
        self.kept = true;
        self.path.clone()
    }
}

impl AsRef<Path> for Made {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl Drop for Made {
    fn drop(&mut self) {
        if !self.kept {
            let _ = Door::of_run().remove_made(&self.path);
        }
    }
}

/// The record a harness writes of the folder `own` in its records folder: Claude Code names it
/// by the folder's real path, every character that is no letter or digit a dash
/// ([`crate::rig::project_name`]), as it is told it, with Windows' `\\?\` taken off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    records: PathBuf,
    name: String,
}

impl Record {
    /// The record of `own` in `records`: none where `own`'s real path cannot be read.
    pub fn of(records: &Path, own: &Path) -> Option<Record> {
        let real = canonical(own)?;
        Some(Record {
            records: records.to_path_buf(),
            name: crate::rig::project_name(&real.to_string_lossy()),
        })
    }

    /// Where it is.
    pub fn path(&self) -> PathBuf {
        self.records.join(&self.name)
    }
}

/// Why a process is the run's own to end.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Ground {
    /// It descends from the process `root` (a window's, or the test's own).
    Descends(Identity),
    /// It is the process `root` itself, a window's or the test's own, whose tree is chosen.
    Root(Identity),
    /// It descends from the window `window` and holds `text`, or descends from one that does.
    Holding(Identity, String),
    /// An orphan that holds `text`, began no earlier than `since`, and runs in the run's own
    /// folder, or one that descends from it.
    Orphan(PathBuf, u64),
    /// The rig's stand-in job of `file`, begun no earlier than `since`.
    Job(String, u64),
    /// A process this test started, begun no earlier than it, with the number and command line
    /// the test read of it.
    StartedHere(u64),
}

impl fmt::Display for Ground {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ground::Descends(root) => write!(formatter, "descends from process {}", root.pid),
            Ground::Root(root) => write!(formatter, "the root process {} itself", root.pid),
            Ground::Holding(window, text) => write!(
                formatter,
                "in window process {}'s tree, holding {}",
                window.pid,
                crate::live::cut(text, 60)
            ),
            Ground::Orphan(folder, since) => write!(
                formatter,
                "an orphan begun at {since} or after, in the run's own folder {}",
                folder.display()
            ),
            Ground::Job(file, since) => {
                write!(
                    formatter,
                    "the stand-in's job of {file}, begun at {since} or after"
                )
            }
            Ground::StartedHere(since) => {
                write!(formatter, "started by this test, begun at {since} or after")
            }
        }
    }
}

/// Processes the run may end, each with the ground that makes it the run's own: made only by the
/// descent, so that no case ends a process by its number or its command line alone. It reads as
/// the processes it holds, in the order chosen.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Grounded {
    processes: Vec<Running>,
    grounds: Vec<Ground>,
}

impl std::ops::Deref for Grounded {
    type Target = [Running];

    fn deref(&self) -> &[Running] {
        &self.processes
    }
}

impl Grounded {
    /// None.
    pub fn none() -> Grounded {
        Grounded::default()
    }

    fn of(processes: Vec<Running>, ground: &Ground) -> Grounded {
        let mut grounded = Grounded::none();
        for process in processes {
            grounded.push(process, ground.clone());
        }
        grounded
    }

    fn push(&mut self, process: Running, ground: Ground) {
        if !self.contains(process.identity) {
            self.processes.push(process);
            self.grounds.push(ground);
        }
    }

    /// Each process with its ground.
    fn each(&self) -> impl Iterator<Item = (&Running, &Ground)> {
        self.processes.iter().zip(&self.grounds)
    }

    /// What descends from `root` among `all` ([`descending_from`]).
    pub fn descending_from(all: &[Running], root: Identity) -> Grounded {
        Grounded::of(descending_from(all, root), &Ground::Descends(root))
    }

    /// The process `root` and what descends from it: none where it is not among `all`.
    pub fn tree(all: &[Running], root: Identity) -> Grounded {
        let mut grounded = Grounded::none();
        if let Some(process) = all.iter().find(|process| process.identity == root) {
            grounded.push(process.clone(), Ground::Root(root));
            grounded.add(Grounded::descending_from(all, root));
        }
        grounded
    }

    /// What descends from `window` among `all` and holds `text`, with what it started
    /// ([`holding`]).
    pub fn holding(all: &[Running], window: Identity, text: &str) -> Grounded {
        Grounded::of(
            holding(all, window, text),
            &Ground::Holding(window, text.to_owned()),
        )
    }

    /// What descends from `window` among `all` whose command line `runs` says is the one
    /// (`said` names it), with what each of those started.
    pub fn in_window_where(
        all: &[Running],
        window: Identity,
        runs: impl Fn(&str) -> bool,
        said: &str,
    ) -> Grounded {
        let mut grounded = Grounded::none();
        let ground = Ground::Holding(window, said.to_owned());
        for process in descending_from(all, window)
            .into_iter()
            .filter(|process| runs(&process.command))
        {
            let under = descending_from(all, process.identity);
            for one in std::iter::once(process).chain(under) {
                grounded.push(one, ground.clone());
            }
        }
        grounded
    }

    /// The orphans of the run's own `folder` that hold `text` and began no earlier than `since`,
    /// with what they started ([`orphans_in_folder`]).
    pub fn orphans_in_folder(
        all: &[Running],
        text: &str,
        folder: &RunFolder,
        since: u64,
        folder_of: impl Fn(u32) -> Option<PathBuf>,
    ) -> Grounded {
        Grounded::of(
            orphans_in_folder(all, text, folder, since, folder_of),
            &Ground::Orphan(folder.real().to_path_buf(), since),
        )
    }

    /// `process` as the rig's stand-in job of `file` (the job's own file, which no process but
    /// the job has in its command line), begun no earlier than `since`: none where it is not.
    pub fn stand_in_job(process: &Running, file: &str, since: u64) -> Option<Grounded> {
        (!file.is_empty() && process.command.contains(file) && process.identity.started >= since)
            .then(|| Grounded::of(vec![process.clone()], &Ground::Job(file.to_owned(), since)))
    }

    /// The process numbered `pid` with the command line `command` among `all`, begun no earlier
    /// than this test's own process: a decoy the test started itself. None where there is none.
    pub fn started_here(all: &[Running], pid: u32, command: &str) -> Option<Grounded> {
        let since = ThisMachine.identity(std::process::id()).ok()??.started;
        all.iter()
            .find(|one| {
                one.identity.pid == pid && one.command == command && one.identity.started >= since
            })
            .map(|one| Grounded::of(vec![one.clone()], &Ground::StartedHere(since)))
    }

    /// `process`, read by this test as one it started, begun no earlier than it: none where it
    /// began before.
    pub fn read_here(process: &Running) -> Option<Grounded> {
        let since = ThisMachine.identity(std::process::id()).ok()??.started;
        (process.identity.started >= since)
            .then(|| Grounded::of(vec![process.clone()], &Ground::StartedHere(since)))
    }

    /// Adds what `other` holds, each process once.
    pub fn add(&mut self, other: Grounded) {
        for (process, ground) in other.processes.into_iter().zip(other.grounds) {
            self.push(process, ground);
        }
    }

    /// Keeps of it only the processes `keep` says: a set is made smaller, never larger, here.
    pub fn retain_where(&mut self, keep: impl Fn(&Running) -> bool) {
        let kept: Vec<(Running, Ground)> = std::mem::take(&mut self.processes)
            .into_iter()
            .zip(std::mem::take(&mut self.grounds))
            .filter(|(process, _)| keep(process))
            .collect();
        for (process, ground) in kept {
            self.push(process, ground);
        }
    }

    /// The processes, in the order chosen.
    pub fn processes(&self) -> Vec<Running> {
        self.processes.clone()
    }

    /// Whether it holds the process `identity`.
    pub fn contains(&self, identity: Identity) -> bool {
        self.processes
            .iter()
            .any(|process| process.identity == identity)
    }
}

#[cfg(test)]
mod rule;
#[cfg(test)]
mod tests;

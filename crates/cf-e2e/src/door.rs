//! The one door of the kit's endings and deletions: every process the kit ends or stops, every
//! window it closes, every own program it kills, and every file or folder it takes away goes
//! through here, each with the ground that makes it the run's own (T-1561, T-1592). At 18:58 on
//! 2026-11-12 a live run of a kit branch ended 22 processes that were none of its own (the
//! owner's app helpers, two Codex daemons, a git ssh connection, another task's benchmark), and
//! the review after found the same in what the kit deletes (T-1557): another run's rig folders, a
//! login copy in use, the owner's own records by a name's end. A rule test reads the kit's code
//! for the plain ways around the door (`door/rule.rs`, with what it cannot see).
//!
//! The rule the grounds serve: a live run ends no process and removes or overwrites no path that
//! this same run did not itself make. What an earlier run left is a person's to clean up
//! (`cargo xtask live sweep` lists it and takes none).
//!
//! The grounds, each the proof the door asks:
//!
//! - **A process** ([`Door::end`], [`Door::pause`]) only as a [`Grounded`] set holds it, which the
//!   descent alone makes: what descends from a window's or the test's own process, an orphan
//!   that holds the command and runs in the run's own folder, a stand-in job by its file, or a
//!   decoy the test started itself. Each is ended only while it is still the process chosen.
//! - **An own program** ([`Door::end_child`], [`Door::end_group_of_child`],
//!   [`Door::signal_child`], [`Door::stall_child`]): a child this process started and still holds.
//! - **A window** ([`Door::close_pane`]): a pane of the rig's own pane host; a terminal the test
//!   opened itself ([`Door::close_own_terminal`], [`Door::quit_own_terminal`]).
//! - **A folder the door made** ([`made_in`], [`Made`], [`Door::remove_made`]), and **a path under
//!   one** ([`Door::remove_in_made`]): a folder of a random name made by this process, or handed
//!   down to it by the process that started it ([`MADE_ROOTS`]), in the registry here with its real
//!   path. A stand-in takes away its own status file (`sessions/<its process id>.json` in the
//!   config folder of the run) on this ground: the folder is the run's. No removal is grounded on
//!   a name alone.
//! - **A record of the renewal's** ([`Door::remove_record`]): in the harness's records folder,
//!   the one entry named for the renewal's own folder, by its whole name.
//!
//! **Writes** have a guard where the kit's own writers are ([`crate::files`], [`writable`]): in
//! the live folder every run on the machine shares (`~/.consensflow-candidate/live`) a file or a
//! folder is made only below a folder this run made through [`made_in`], by its real path. The
//! last link of a path is refused, and a path through an earlier link is taken only where its real
//! place is inside what the run made. Outside it the kit writes in folders of its own (a rig's, a login copy's)
//! that it made through the door.
//!
//! **Say only.** With `CF_LIVE_AUDIT=1` the door does nothing: it writes what it would do, one
//! line each (the act, the process or the path, the ground; the whole command line, with a tab or
//! a line break written `\t`, `\n`), to the file `CF_LIVE_AUDIT_FILE` names (`cargo xtask live`
//! names one) or one of its own in the temp folder, and says each on the error output. The run
//! makes that file as a new one of its own, with first lines (`# …`) that say what the run still
//! does ([`STILL_ACTS`]); a file that is there is never added to, and a run whose file cannot be
//! made or written stops the run when it first uses its door, which is after the builds and the compile of the suite ([`STILL_ACTS`] says what is done by then). The programs the run starts are given the same file
//! ([`Door::child_vars`]) and add to it. A case that needs an ending to have happened stops there
//! and fails, so that no say-only run passes. The tests make a door of their own
//! ([`Door::asked`]) and never set the variable in their own process: a test of what a caller names
//! for the run to write is run again in a process that has it set ([`again_under_audit`]).
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

mod again;
mod writes;

pub use again::again_under_audit;
#[cfg(unix)]
pub use writes::symlink;
pub use writes::{
    append, copy, copy_cloning, copy_keeping_mode, copy_tree, create, create_new,
    create_new_private, made_dir, made_file, make_dir, make_dir_private, rename, say_its_process,
    set_modified, set_permissions, write, write_executable, write_whole,
};

/// The variable that makes the door say only.
pub const AUDIT: &str = "CF_LIVE_AUDIT";

/// The variable that names the file the door writes what it would do in.
pub const AUDIT_FILE: &str = "CF_LIVE_AUDIT_FILE";

/// The variable a say-only run gives the programs it starts to say that the file is made already
/// (by it): they open it to add lines, and stop where it is not there. Never set by a person.
pub const AUDIT_MADE: &str = "CF_LIVE_AUDIT_MADE";

/// What a say-only run still does, each as the first lines of the audit file say it: the door
/// says what the kit would end or take away, and these are what the kit does not do through it.
pub const STILL_ACTS: &[&str] = &[
    "before the run's audit file is made, `cargo xtask live` builds in the checkout, in a say-only run as in an acting one: four release builds (`cf`, the pane host `consensflow-bridge`, and the test programs `harness-ask` and `engine-ask`; on Windows `cf-shim` as well) into `app/src-tauri/target/release`, and then `cargo test` compiles the live suite and the stand-in programs into `app/src-tauri/target/debug`; they write and replace files in the checkout's build folder, which every run of the checkout shares",
    "the product's own shutdown at the rig's close still runs: the daemon ends, the pane host ends, and every pane of the rig and what descends from it (a Codex session's children among them) is ended by the product's own code, not the kit's door",
    "the product's own operations while the case runs go on, each by the product's own code and not the kit's door: the daemon closes a window at a task's end, ends a pane it restarts or kills, and moves what it is asked to",
    "the product's own clean-up of files goes on: a Codex session removes the private socket folder it made, and the daemon takes the files it wrote",
    "keys the run types into the rig's own panes (an Escape, a Ctrl-C, /exit, a dialog's answer, a paste) are typed",
    "a terminal the run opened itself (a pane of its own test, Muse's window) is hung up by the system when the run's process ends, with what runs in it",
    "the programs the run starts (a harness's window and what it runs, its tools, git, a stand-in) run, and what they do or end is theirs, not the kit's door; among them the machine's own program of each harness the run seats, asked for its version, and on a Mac `security`, which reads the Keychain item of Claude Code's login (the first read asks the person once to allow it) and lists the names of its items",
    "the files and folders the run writes below its own folders (its rig's folder in the temp folder, a project folder of its own in the live folder, the config folder of its login copy, its wrappers, what it exports into a new folder of its own in a folder a caller names) are written; besides them the run makes new files of its own outside its folders and never writes over one that is there (the audit file, a fixture or a report file a caller names, a failure report), and makes a folder that is missing on the way to them",
    "an acting run on a test machine (CONSENSFLOW_TEST_MACHINE, which the Windows runner sets) may renew that machine's own Claude Code or Pi login by running the harness itself, which writes that login; a say-only run never starts that renewal: where a login would need it, the rig that needed it does not start and the run ends with one line that says so, which is in the audit file too; what was done by then stays done: the builds, the questions to the installed harnesses for their versions, the reads of the logins, and the run's first folders (the folder of its shims in the temp folder)",
    "a say-only run uses the real model's quota, and keeps what it made for reading until a person removes it: its folders, its transcripts, the whole command lines in the audit file, and its copies of the logins",
    "what a run's copy of a login holds, harness by harness, and a say-only run removes none of them: Claude Code's (in a config folder of the run's own) the access token and no refresh token, and on a Mac its window may move that into a Keychain item named for the folder, which outlives the run and which the kit never deletes; Codex's the whole `auth.json` as the machine holds it, so a refresh token that is in it is copied; Pi's the entries of the providers the run's model needs, each with its access token (a key entry, its key) and no refresh token, with the files of a proxy provider the model names, copied whole; Devin's `credentials.toml`, which holds an API key, and its `config.json`; Muse's `auth.json` with every key that names a refresh token taken out (on a Mac that file holds metadata and the secret stays in the Keychain, which the run's Muse reads)",
];

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
    /// The run's door, as its variables ask: one that acts unless [`AUDIT`] says to say only. A
    /// say-only run whose audit file cannot be made, opened or written stops here, with the
    /// reason, before anything it does acts ([`Door::asked`]).
    // The kit reads its own switch, as the run was started with it.
    #[allow(clippy::disallowed_methods)]
    pub fn of_run() -> &'static Door {
        static DOOR: OnceLock<Door> = OnceLock::new();
        DOOR.get_or_init(|| {
            Door::asked(
                std::env::var_os(AUDIT).as_deref(),
                std::env::var_os(AUDIT_FILE).as_deref(),
                std::env::var_os(AUDIT_MADE).as_deref(),
            )
            .unwrap_or_else(|why| panic!("the say-only run stops before it acts: {why}"))
        })
    }

    /// Makes the run's door now: a run that starts a log, a rig or a program asks it first, so
    /// that an audit file that cannot be used stops the run before any act.
    pub fn init() {
        let _ = Door::of_run();
    }

    /// The door the variables `audit`, `file` and `made` ask for: say-only where `audit` is set to
    /// anything but `0` or nothing. A say-only door has an audit file of its own: `file`, or one
    /// in the temp folder named for this process. The process that begins the run makes it as a
    /// new file, which it fails where one is there (a file of an older run is never added to), and
    /// writes the first lines of ([`STILL_ACTS`]); a program the run started (`made` set) opens
    /// that file to add lines and fails where it is not there. Anything else is an error, and the
    /// run stops.
    pub fn asked(
        audit: Option<&std::ffi::OsStr>,
        file: Option<&std::ffi::OsStr>,
        made: Option<&std::ffi::OsStr>,
    ) -> Result<Door, String> {
        if !is_on(audit) {
            return Ok(Door::acting());
        }
        let file = file
            .filter(|file| !file.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir()
                    .join(format!("consensflow-live-audit-{}.txt", std::process::id()))
            });
        if is_on(made) {
            let open = fs::OpenOptions::new().append(true).open(&file);
            open.map_err(|failed| {
                format!(
                    "the audit file {} the run began is not there to add to: {failed}",
                    file.display()
                )
            })?;
        } else {
            let mut open = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&file)
                .map_err(|failed| {
                    format!(
                        "the audit file {} cannot be made as a new file of this run's own ({failed}); a file that is there is never added to, so name another with {AUDIT_FILE}",
                        file.display()
                    )
                })?;
            let header = format!(
                "# say-only run ({AUDIT}) of process {}: the kit's door ends no process, closes no window, signals none and takes no file away, and writes down each such act below instead; the run is no pass\n{}\n",
                std::process::id(),
                STILL_ACTS
                    .iter()
                    .map(|act| format!("# still acts: {act}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            open.write_all(header.as_bytes()).map_err(|failed| {
                format!(
                    "the audit file {} cannot be written: {failed}",
                    file.display()
                )
            })?;
        }
        Ok(Door::saying(file))
    }

    /// Whether the variables ask for a say-only run ([`AUDIT`]), read and nothing more: a program
    /// that has no door to make (`live sweep`) says what a say-only run is no pass by.
    // The kit reads its own switch, as the run was started with it.
    #[allow(clippy::disallowed_methods)]
    pub fn audit_asked() -> bool {
        is_on(std::env::var_os(AUDIT).as_deref())
    }

    /// The variables that make a program this run starts say only into the same file, as this
    /// run does: none where this run acts.
    pub fn audit_vars() -> Vec<(String, String)> {
        match Door::of_run().says_in() {
            Some(file) => vec![
                (AUDIT.to_owned(), "1".to_owned()),
                (AUDIT_FILE.to_owned(), file.to_string_lossy().into_owned()),
                (AUDIT_MADE.to_owned(), "1".to_owned()),
            ],
            None => Vec::new(),
        }
    }

    /// What a program this run starts is given of the door: the variables that make it say only
    /// into the same file as this run does where this run says only ([`Door::audit_vars`]), and the
    /// folders this run made so far, which it may write below ([`handed_down`]). The second is what
    /// a program that is a stand-in for a harness (it writes its status file, its records and its
    /// sockets) is given of the run's folders: it writes nowhere else.
    pub fn child_vars() -> Vec<(String, String)> {
        let mut vars = Door::audit_vars();
        vars.extend(handed_down());
        vars
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
    /// (the file is added to, never made here) and on the error output. A line that cannot be
    /// written stops the run: a say-only run whose audit does not say is no audit.
    #[allow(clippy::print_stderr)] // Said in the run's output as well as in its file.
    fn say(&self, act: &str, what: &str, ground: &str) {
        let Some(file) = &self.said else {
            return;
        };
        let line = format!("{act}\t{}\t{}", escaped(what), escaped(ground));
        eprintln!("[say only] {line}");
        self.append(file, &line);
    }

    /// Writes `line`, a sentence of the run's own and no act, as a comment line of the audit file
    /// (the file's first lines are comment lines too: a line that starts with `#` is no ending and
    /// no removal), and on the error output; nothing where the door acts. A run that stops because
    /// it would act where it only says (a login it would renew) leaves the reason there.
    #[allow(clippy::print_stderr)] // Said in the run's output as well as in its file.
    pub fn note(&self, line: &str) {
        let Some(file) = &self.said else {
            return;
        };
        eprintln!("[say only] {line}");
        self.append(file, &format!("# {}", escaped(line)));
    }

    /// Adds `line` and its break to `file` in one write: programs of the run add to the same file,
    /// and two writes could be cut between by another's.
    #[allow(clippy::print_stderr)]
    fn append(&self, file: &Path, line: &str) {
        let written = fs::OpenOptions::new()
            .append(true)
            .open(file)
            .and_then(|mut open| open.write_all(format!("{line}\n").as_bytes()));
        if let Err(failed) = written {
            let why = format!(
                "[say only] {} could not be written: {failed}: the run stops, for what it would do is no longer said",
                file.display()
            );
            if std::thread::panicking() {
                eprintln!("{why}");
            } else {
                panic!("{why}");
            }
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

    /// Types the keys of a quit into `name`, a terminal this test opened itself and holds (the two
    /// Ctrl-Cs a person quits an app at rest with), by `quit`: whether it did (a say-only door
    /// says it and types none).
    pub fn quit_own_terminal(&self, name: &str, quit: impl FnOnce()) -> bool {
        if self.says_only() {
            self.say(
                "quit",
                name,
                "keys typed into a terminal this test opened and holds",
            );
            return false;
        }
        quit();
        true
    }

    /// Ends the group `child` leads, a program this process started and holds in a group of its
    /// own (`name` says which): on Unix the group, on Windows the tree. Whether it did (a
    /// say-only door says it and ends none).
    pub fn end_group_of_child(&self, name: &str, child: &std::process::Child) -> bool {
        if self.says_only() {
            self.say(
                "kill group",
                &format!("process {} ({name})", child.id()),
                "the group of a child this process started and holds",
            );
            return false;
        }
        #[cfg(unix)]
        {
            use nix::sys::signal::{killpg, Signal};
            use nix::unistd::Pid;

            if let Ok(leader) = i32::try_from(child.id()) {
                let _ = killpg(Pid::from_raw(leader), Signal::SIGKILL);
            }
        }
        #[cfg(windows)]
        {
            let root =
                crate::process::own_var("SystemRoot").unwrap_or_else(|| r"C:\Windows".to_owned());
            let _ = crate::process::Run::new(format!(r"{root}\System32\taskkill.exe"))
                .args(["/T", "/F", "/PID"])
                .arg(child.id().to_string())
                .run();
        }
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

    /// Marks `path`, a folder this process made through the door ([`made_in`]), kept for a person
    /// to look at ([`KEPT`]): no sweep takes it. Whether it is one, and was marked.
    pub fn keep_made(&self, path: &Path) -> bool {
        if !is_made(path) {
            return false;
        }
        fs::write(path.join(KEPT), "kept for a person to look at\n").is_ok()
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

/// Whether a switch's value says yes: anything but nothing or `0`.
fn is_on(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|value| !value.is_empty() && value != "0")
}

/// What a say-only door answers of an ending: every process said, none ended.
fn said_only(chosen: &Grounded) -> Ended {
    Ended {
        said: chosen.processes(),
        ..Ended::default()
    }
}

/// A process as the door says it: its number, when it began, and its command line, whole.
fn said_process(process: &Running) -> String {
    format!(
        "process {} begun {} ({})",
        process.identity.pid, process.identity.started, process.command
    )
}

/// `text` as one field of a line of the audit file: a backslash, a tab and a line break written
/// as `\\`, `\t` and `\n`, so that a command line that holds them is one field of one line.
fn escaped(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
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

/// A file name that is this process's alone, `<stem>-<pid>-<ms>-<n>.<extension>`: no file of
/// another run, or of another case of this run, can have it, so that a process list that shows
/// it (a command line a member is given) shows this command's. It is a name and no ground: a
/// file of it is taken only where it is below a folder the run made ([`Door::remove_in_made`]).
pub fn own_name(stem: &str, extension: &str) -> String {
    format!("{}.{extension}", own_stem(stem))
}

/// [`own_name`] with no extension: `<stem>-<pid>-<ms>-<n>`, the name of a folder.
pub fn own_stem(stem: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let millis = jiff::Timestamp::now().as_millisecond();
    format!(
        "{stem}-{}-{millis}-{}",
        std::process::id(),
        COUNT.fetch_add(1, Ordering::Relaxed)
    )
}

/// The variable that hands the folders this run made to a program it starts, which is another
/// process with a registry of its own: the real paths, joined as a `PATH` is (a wrapper that hands a
/// program a folder as it wrote it is read by the real path it leads to: [`roots_handed_down`]). Never set by a
/// person; a program that does not find it has made nothing and may write nowhere. `cargo xtask
/// live`, the rig's start ([`crate::rig`]) and the kit's own program starts clear it before they
/// set their own; a person who sets it by hand and runs a test with `cargo test` alone gives that
/// run those folders to write below, which is that person's own act (a known limit: the first
/// process of a run cannot tell a person's value from its parent's).
pub const MADE_ROOTS: &str = "CF_LIVE_MADE_ROOTS";

/// The folders and files this process made through the door, by their real paths, and those its
/// parent handed to it ([`MADE_ROOTS`]).
#[allow(clippy::disallowed_methods)] // the door is read before any `main` could hand an Env on
fn registry() -> std::sync::MutexGuard<'static, Vec<PathBuf>> {
    static MADE: OnceLock<Mutex<Vec<PathBuf>>> = OnceLock::new();
    MADE.get_or_init(|| Mutex::new(roots_handed_down(std::env::var_os(MADE_ROOTS).as_deref())))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// The paths `value` hands down ([`MADE_ROOTS`]), each by its real path where it is there: the
/// registry is by real paths, and a program that is handed a folder as its wrapper wrote it (a
/// shell's `$PWD`, the temp folder of a Mac, where `/var` leads to `/private/var`) writes below it.
fn roots_handed_down(value: Option<&std::ffi::OsStr>) -> Vec<PathBuf> {
    value
        .map(|value| {
            std::env::split_paths(value)
                .map(|root| canonical(&root).unwrap_or(root))
                .collect()
        })
        .unwrap_or_default()
}

/// The variable that hands this run's folders to a program it starts: none where it made none.
/// The program's own writers ask the same registry as this process's ([`writable`]); what it
/// makes itself it cannot hand back, and what this process makes after it started it is not
/// there.
pub fn handed_down() -> Vec<(String, String)> {
    let roots = registry().clone();
    if roots.is_empty() {
        return Vec::new();
    }
    match std::env::join_paths(roots) {
        Ok(joined) => vec![(MADE_ROOTS.to_owned(), joined.to_string_lossy().into_owned())],
        Err(_) => Vec::new(),
    }
}

/// Registers the file or folder `path` as one this run made.
fn register(path: &Path) {
    let real = canonical(path).unwrap_or_else(|| path.to_path_buf());
    let mut made = registry();
    if !made.contains(&real) {
        made.push(real);
    }
}

/// Forgets `path` as a folder this process made.
fn forget_made(path: &Path) {
    let real = canonical(path);
    registry().retain(|made| Some(made) != real.as_ref() && made != path);
}

/// Whether `path` is a folder this process made through the door ([`made_in`]) and has not taken.
pub fn is_made(path: &Path) -> bool {
    canonical(path).is_some_and(|real| registry().contains(&real))
}

/// Whether `path` is a folder this process made through the door, or is below one, by real paths.
pub fn is_made_or_below(path: &Path) -> bool {
    real_beneath(path).is_some_and(|real| registry().iter().any(|made| real.starts_with(made)))
}

/// The real path of `path` where it is not there yet: its deepest folder that is there by its
/// real path, and the rest of the names after it. None where a name is `..`.
fn real_beneath(path: &Path) -> Option<PathBuf> {
    let mut rest = Vec::new();
    let mut at = path.to_path_buf();
    loop {
        if let Some(real) = canonical(&at) {
            return Some(rest.iter().rev().fold(real, |real, name| real.join(name)));
        }
        let name = at.file_name()?.to_os_string();
        if name == ".." {
            return None;
        }
        rest.push(name);
        at = at.parent()?.to_path_buf();
    }
}

/// Whether `path` names a place in the kit's live zone: the folder every run puts its projects
/// in (`<home>/.consensflow-candidate/live`) or anything below it, by the path as given or by the
/// real path it leads to, where a Windows path is the same in any case.
pub fn in_live_zone(path: &Path) -> bool {
    let zone = |path: &Path| {
        let names: Vec<String> = path
            .components()
            .map(|part| {
                let name = part.as_os_str().to_string_lossy();
                if cfg!(windows) {
                    name.to_lowercase()
                } else {
                    name.into_owned()
                }
            })
            .collect();
        names
            .windows(2)
            .any(|pair| pair[0] == ".consensflow-candidate" && pair[1] == "live")
    };
    zone(path) || real_beneath(path).is_some_and(|real| zone(&real))
}

/// Whether a file or folder may be made at `path`: only below a folder this run made through the
/// door ([`made_in`]) or handed down to this program ([`MADE_ROOTS`]), or as a file this run made
/// ([`made_file`]), by the real path; the last link of a path is refused, and a path through an earlier
/// link is taken only where its real place is inside what the run made. Everywhere else is
/// another run's, a person's, or nobody's: the live folder, the temp folder, a config folder a caller
/// named. A folder (`folder`) may also be the run's own folder itself, made again.
pub fn writable(path: &Path, folder: bool) -> Result<(), String> {
    let Some(real) = real_beneath(path) else {
        return Err("it leads up out of the folder it names".into());
    };
    let allowed = registry().iter().any(|made| {
        if real == *made {
            // The folder itself may be made again; a file this run made may be written.
            folder == made.is_dir()
        } else {
            real.starts_with(made) && made.is_dir()
        }
    });
    if !allowed {
        return Err(format!(
            "{} is below no folder this run made, so it is another run's, a person's, or nobody's",
            real.display()
        ));
    }
    if fs::symlink_metadata(path).is_ok_and(|found| found.file_type().is_symlink()) {
        return Err("it is a link, and a write goes through a link to what it leads to".into());
    }
    Ok(())
}

/// Makes the place an allocation is made in where it is not there (the live folder of a machine
/// nobody has run on, the folder of a build's reports before a first run): folders made empty,
/// and none that is there is changed. It is the one thing the door makes that is no allocation of
/// the run's, and it is no place to write: [`writable`] says no to it until a folder is made in it.
pub fn make_place(place: &Path) -> io::Result<()> {
    if place.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(place)
}

/// A folder of a random name this process made through the door, in `parent`, its name begun
/// with `prefix`: taken away through the door when it is dropped, unless it is kept.
pub fn made_in(parent: &Path, prefix: &str) -> io::Result<Made> {
    // A say-only run whose audit file cannot be used stops before it makes a folder.
    Door::init();
    make_place(parent)?;
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
                "in window process {}'s tree, holding {text}",
                window.pid
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

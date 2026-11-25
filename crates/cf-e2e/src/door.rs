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

pub mod account;
mod again;
pub mod cargo;
pub mod database;
pub mod identity;
pub mod keychain;
pub mod place;
pub mod reads;
pub mod renewal;
pub(crate) mod shells;
pub mod source;
pub mod spawn;
pub mod terminal;
mod windows_registry;
mod writes;

pub use again::again_under_audit;
#[cfg(unix)]
pub use writes::symlink;
pub use writes::{
    append, copy, copy_cloning, copy_keeping_mode, copy_keeping_mode_from, copy_tree,
    copy_tree_from, create, create_new, create_new_private, made_dir, made_file, make_dir,
    make_dir_private, remove, rename, say_its_process, set_modified, set_permissions, write,
    write_executable, write_whole,
};
#[cfg(windows)]
pub use writes::{symlink_dir, symlink_file};

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
    "an acting run on a test machine (CONSENSFLOW_TEST_MACHINE, which the Windows runner sets on a machine whose account is a test account of its own, and not on zeewin2, whose account is its owner's) may renew that machine's own Claude Code or Pi login by running the harness itself, which writes that login; a say-only run never starts that renewal: where a login would need it, the rig that needed it does not start and the run ends with one line that says so, which is in the audit file too; what was done by then stays done: the builds, the questions to the installed harnesses for their versions, the reads of the logins, and the run's first folders (the folder of its shims in the temp folder)",
    "a say-only run uses the real model's quota, and keeps what it made for reading until a person removes it: its folders, its transcripts, the whole command lines in the audit file, and its copies of the logins",
    "what a run's copy of a login holds, and a say-only run removes none of them: Claude Code's access token, Pi's discovery copy and the providers the run needs, and Muse's auth.json have every key whose name contains refresh removed at every depth; Codex's auth.json keeps its other fields, including its access token, with refresh-named keys removed at every depth; a token-backed copy then has an empty tokens.refresh_token and last_refresh set to the time of the copy, so no refresh-token value is copied; Devin's credentials.toml is copied only if it parses and has no such key, otherwise the case refuses to run; a Claude window on a Mac may move its access token into a Keychain item named for the run's folder, which outlives the run and which the kit never deletes; on a Mac Muse's file holds metadata and its secret stays in the Keychain",
    "to check that Codex's owner setup stayed unchanged, the kit compares auth.json by size, modification time and file identity, and reads and hashes config.toml before and after; it keeps the config's hash and TOML table names in memory and on a changed config reports added or removed table names and those also in the run's own config, never values",
    "when a run seats Pi, to check its owner setup stayed unchanged the kit compares auth.json, models.json and mcp-auth.json by size, modification time and file identity, and reads and hashes settings.json before and after; it names a changed path and prints no login values",
    "for a Pi proxy provider, the kit compares its config, which may hold a key, by size, modification time and file identity; it reads and hashes the model cache, npm manifests and package files and lists package names, before and after; it skips linked files and folders in the package walk and linked provider files, and names changed paths",
    "to check that Pi wrote no owner session for the run, the kit lists the immediate names in the owner's sessions folder before and after; it keeps matching run names in memory and reports any gained name, without reading session contents",
    "to check that Devin's owner setup stayed unchanged, the kit compares credentials.toml by size, modification time and file identity and reads and hashes its config and trusted-workspaces files before and after; it names a changed path",
    "to check that Muse's owner setup stayed unchanged, the kit compares auth.json by size, modification time and file identity and reads and hashes its settings, trust and session-index database files before and after; it names a changed path, without printing file or database contents; every live matrix run and replay also compares the owner's Muse auth.json by size, modification time and file identity, hashes its settings, trust and session index and Claude's settings, and reads the Claude launcher link before and after; its evidence holds metadata and hashes, never login contents",
    "to check that Claude Code's owner login stayed unchanged, the kit compares a file-backed login by size, modification time and file identity without reading its bytes; a Keychain login has no file metadata, so the kit reads it again and compares its accessToken, refreshToken and expiresAt with those held in memory, never printing the login",
    "except in Claude's parity case, to check Claude Code's owner setup, the kit reads and hashes its settings, reads its project configuration and hashes the run-named project entries, and lists immediate project-folder names before and after; it keeps hashes and matching names in memory, prints the first 12 settings-hash digits when unchanged, and reports changed run paths or names",
    "except in Claude's parity case, to check Claude Code's history, the kit records its size before the run, then reads and parses the appended bytes after it; it holds the text only while checking and prints only the count of prompts naming the run on failure",
    "to check the installed harness launchers stayed unchanged, the kit stats and hashes their binaries, reads link targets and lists installed version names before and after, including Muse's version file; it keeps these in memory, prints the first 12 hash digits when unchanged and reports changed paths, links or version names otherwise",
    "Muse's window cases additionally read and fingerprint the owner's settings, trust, launcher and version file to check they stayed unchanged; a background case prints the path and fingerprint maps on change, while the posture case prints only that they changed",
    "Muse's reads case recursively lists the owner's Muse config before and after; it compares every non-directory entry by size, modification time and file identity only, skips links, refuses a linked root or a folder it cannot list, and prints changed paths, not contents",
    "Claude's dialog case reads the owner's .claude.json before and after to check it stayed unchanged; it keeps a hash and four LSP and hints settings in memory and prints the complete hashes and those settings in its report",
    "Claude's parity case does not read or check the owner's Claude settings, project configuration, history or project folders; it checks the login and its copy, and refuses a login that needs renewal, even on a test machine",
    "for setup, Claude's dialog case recursively copies the owner's official plugin marketplace without links; its parity case uses the kit's copy of the owner's Claude login without its refresh token in a config of the run's own, with a skill and an MCP server of the run's own, and reads no other owner Claude configuration; the run's settings ask Claude not to sync account skills or plugins or fetch account connectors; the login copy stays in a say-only run",
    "for setup, only when Pi is seated and no Pi model is explicitly named, the kit reads the owner's auth.json once in memory for the run's seed, makes a private discovery copy with refresh-named keys removed, and reads provider names from that copy; the first matching run home reuses the seed after checking that the owner's file metadata stayed unchanged; an explicitly named Pi model skips discovery and its run home reads auth.json in memory to seed its sanitized copy; a later Pi home may also read it again, and a test-machine renewal rereads the login after renewal; a say-only run keeps its sanitized discovery copy as well as its run copies; a named proxy's package and model cache are copied and its config is copied without refresh-named keys; Muse's window cases copy feature config; a live Muse home and its reads and direct TUI cases copy the model catalog where present, and seeding its home reads the owner's login in memory and writes only a sanitized copy; ordinary tree files are copied as they are, links outside their source tree and dangling links are skipped, and links inside it point to the copied target; linked Pi provider files and model cache are skipped or refused; Devin's config is copied and its compaction case reads it to make a run config; Devin's Git gets a generated config, and the kit's seed copies no owner Git file",
    "the Muse window case checks whether the owner's Claude rules file exists and may print its path; the Claude launcher case reads and prints the owner's Claude launcher link target",
];

/// The file a folder kept for a person holds: an older rig folder that holds it is never swept.
pub const KEPT: &str = "consensflow-kept.txt";

/// How a rig names its folder, by the start of its name.
pub const RIG_PREFIX: &str = "consensflow-integration-";

/// The variable a say-only run gives the programs it starts with the identity of the audit file
/// it made ([`identity`]): a program opens the file by its path once and checks it is that one, so
/// that a file put at the path since is never added to (T-2002 finding 5). Never set by a person.
pub const AUDIT_ID: &str = "CF_LIVE_AUDIT_ID";

/// The door: one that acts, or one that only says.
#[derive(Debug, Clone)]
pub struct Door {
    /// The file what would be done is written in, when the door only says.
    said: Option<PathBuf>,
    /// The audit file, open: every line is added through the handle it was made or checked with,
    /// never by opening the path again.
    open: Option<std::sync::Arc<Mutex<fs::File>>>,
}

impl PartialEq for Door {
    fn eq(&self, other: &Door) -> bool {
        self.said == other.said
    }
}

impl Eq for Door {}

impl Door {
    /// The run's door, as its variables ask: one that acts unless [`AUDIT`] says to say only. A
    /// say-only run whose audit file cannot be made, opened or written stops here, with the
    /// reason, before anything it does acts ([`Door::asked`]).
    // The kit reads its own switch, as the run was started with it.
    #[allow(clippy::disallowed_methods)]
    pub fn of_run() -> &'static Door {
        static DOOR: OnceLock<Door> = OnceLock::new();
        DOOR.get_or_init(|| {
            Door::asked_of(
                std::env::var_os(AUDIT).as_deref(),
                std::env::var_os(AUDIT_FILE).as_deref(),
                std::env::var_os(AUDIT_MADE).as_deref(),
                std::env::var_os(AUDIT_ID).as_deref(),
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
        Door::asked_of(audit, file, made, None)
    }

    /// [`Door::asked`], where a program the run started is also told the identity of the audit
    /// file the run made (`identity`, [`AUDIT_ID`]): the file it opens must be that one.
    pub fn asked_of(
        audit: Option<&std::ffi::OsStr>,
        file: Option<&std::ffi::OsStr>,
        made: Option<&std::ffi::OsStr>,
        identity: Option<&std::ffi::OsStr>,
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
        let opened = if is_on(made) {
            let open = fs::OpenOptions::new()
                .append(true)
                .open(&file)
                .map_err(|failed| {
                    format!(
                        "the audit file {} the run began is not there to add to: {failed}",
                        file.display()
                    )
                })?;
            if let Some(expected) = identity.filter(|identity| !identity.is_empty()) {
                let found = self::identity::of_file(&open).map_err(|failed| failed.to_string())?;
                if std::ffi::OsStr::new(&found) != expected {
                    return Err(format!(
                        "the audit file {} is not the one the run made: another file is at its path",
                        file.display()
                    ));
                }
            }
            open
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
            open
        };
        Ok(Door {
            said: Some(file),
            open: Some(std::sync::Arc::new(Mutex::new(opened))),
        })
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
        let door = Door::of_run();
        match door.says_in() {
            Some(file) => {
                let mut vars = vec![
                    (AUDIT.to_owned(), "1".to_owned()),
                    (AUDIT_FILE.to_owned(), file.to_string_lossy().into_owned()),
                    (AUDIT_MADE.to_owned(), "1".to_owned()),
                ];
                if let Some(found) = door.open.as_ref().and_then(|open| {
                    self::identity::of_file(&open.lock().unwrap_or_else(PoisonError::into_inner))
                        .ok()
                }) {
                    vars.push((AUDIT_ID.to_owned(), found));
                }
                vars
            }
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
        // The roots the child must not write in, which it reads from the run's file and never
        // builds from its own environment ([`account`]); a run that cannot write them starts
        // nothing.
        vars.extend(
            account::child_vars().unwrap_or_else(|why| panic!("the run starts no program: {why}")),
        );
        vars
    }

    /// A door that acts.
    pub fn acting() -> Door {
        Door {
            said: None,
            open: None,
        }
    }

    /// A door that only says, in `file`, which it opens to add to each line (a door the run's
    /// variables ask for holds the file it made or checked open instead, [`Door::asked`]).
    pub fn saying(file: PathBuf) -> Door {
        Door {
            said: Some(file),
            open: None,
        }
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
        let written = match &self.open {
            // The handle the file was made or checked with: a file put at its path since is never
            // added to, and one taken away stops the run as a file that cannot be written does.
            Some(open) => {
                let open = open.lock().unwrap_or_else(PoisonError::into_inner);
                let same = match (
                    self::identity::of_file(&open),
                    self::identity::of_path(file),
                ) {
                    (Ok(held), Ok(now)) => held == now,
                    _ => false,
                };
                if same {
                    (&*open).write_all(format!("{line}\n").as_bytes())
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "the audit file at its path is no longer the one the run made",
                    ))
                }
            }
            None => fs::OpenOptions::new()
                .append(true)
                .open(file)
                .and_then(|mut open| open.write_all(format!("{line}\n").as_bytes())),
        };
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
        if !is_made(path) {
            return Removed::Left(path.to_path_buf(), "it is no folder this run made".into());
        }
        if let Err(refused) = judge(path, place::Act::Remove) {
            return Removed::Left(path.to_path_buf(), refused.to_string());
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
        if path.parent().and_then(canonical).is_none() {
            return Removed::Absent(path.to_path_buf());
        }
        let judged = judge(path, place::Act::Remove);
        let under = judged.as_ref().is_ok_and(|real| {
            registry().iter().any(|made| {
                made.file.is_none() && real.starts_with(&made.real) && *real != made.real
            })
        });
        if !under {
            let why = match judged {
                Err(refused) => refused.to_string(),
                Ok(_) => "it is under no folder this run made".into(),
            };
            return Removed::Left(path.to_path_buf(), why);
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
        self.remove_record_on(record, crate::live::renew::TestMachine::detect().as_ref())
    }

    /// [`Door::remove_record`] on the machine `marked` says this account is, if any.
    pub(crate) fn remove_record_on(
        &self,
        record: &Record,
        marked: Option<&crate::live::renew::TestMachine>,
    ) -> Removed {
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
        // The record is in the account's own records folder: only an account that is a test
        // machine's own (T-2007 round 3) has one the kit may take away.
        if marked.is_none() {
            return Removed::Left(
                path,
                format!(
                    "the records folder is the account's own, and this account is no test machine ({}), so nothing of it is taken away",
                    crate::live::renew::TEST_MACHINE
                ),
            );
        }
        // The record is taken whole: the tree it is must hold no root (a folder of a store that
        // was linked in, say), judged as every removal is (T-2002 finding 3).
        if let Err(refused) = place::tree_holds_no_root(&path, &account::protected()) {
            return Removed::Left(path, refused.to_string());
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

/// What the run made: a folder, or a file with the identity it had when it was made (a file put
/// at that path since is another, and none of the run's: T-2002 finding 5).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    real: PathBuf,
    file: Option<String>,
}

/// The folders and files this process made through the door, by their real paths, and those its
/// parent handed to it ([`MADE_ROOTS`]).
#[allow(clippy::disallowed_methods)] // the door is read before any `main` could hand an Env on
fn registry() -> std::sync::MutexGuard<'static, Vec<Entry>> {
    static MADE: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();
    MADE.get_or_init(|| {
        Mutex::new(
            roots_handed_down(std::env::var_os(MADE_ROOTS).as_deref())
                .into_iter()
                .map(|real| Entry { real, file: None })
                .collect(),
        )
    })
    .lock()
    .unwrap_or_else(PoisonError::into_inner)
}

impl place::Registry for Vec<Entry> {
    fn is_made_folder(&self, real: &Path, any_case: bool) -> bool {
        self.iter().any(|made| {
            made.file.is_none()
                && place::within(real, &made.real, any_case)
                && place::within(&made.real, real, any_case)
                && made.real.is_dir()
        })
    }

    fn is_below_made(&self, real: &Path, any_case: bool) -> bool {
        self.iter().any(|made| {
            made.file.is_none()
                && place::within(real, &made.real, any_case)
                && !place::within(&made.real, real, any_case)
                && made.real.is_dir()
        })
    }

    fn is_made_file(&self, real: &Path, any_case: bool) -> bool {
        self.iter().any(|made| {
            made.file.as_ref().is_some_and(|identity| {
                place::within(real, &made.real, any_case)
                    && place::within(&made.real, real, any_case)
                    && identity::of_path(real).is_ok_and(|now| now == *identity)
            })
        })
    }
}

/// Judges `path` for `act` against the account's protected roots and what this run made: the
/// place to act on, resolved, or why not ([`place`]).
pub fn judge(path: &Path, act: place::Act) -> io::Result<PathBuf> {
    let protected = account::protected();
    let made = registry().clone();
    place::judge_with(path, act, &protected, &made)
}

/// The protected roots as they resolve now, each also as spelled: resolved once for a seal or a
/// filter that asks of many paths ([`Roots::holds`]), never kept between calls.
pub struct Roots {
    roots: Vec<PathBuf>,
}

impl Roots {
    /// The account's protected roots, resolved now.
    pub fn now() -> Roots {
        let mut roots = Vec::new();
        for root in account::protected() {
            if let Ok(found) = place::resolve(&root) {
                roots.extend(found.iter().map(place::Resolved::whole));
            }
            roots.push(root);
        }
        Roots { roots }
    }

    /// Whether `path` resolves inside one of them, by any way it resolves, or is spelled inside
    /// one.
    pub fn holds(&self, path: &Path) -> bool {
        let ways = place::resolve(path)
            .map(|ways| ways.iter().map(place::Resolved::whole).collect::<Vec<_>>())
            .unwrap_or_default();
        let any_case = cfg!(any(windows, target_os = "macos"));
        self.roots.iter().any(|root| {
            place::within(path, root, any_case)
                || ways.iter().any(|way| place::within(way, root, any_case))
        })
    }
}

/// Whether `path` resolves inside a protected root, by any way it resolves, or is spelled inside
/// one: what the seal asks of every variable but the search path and the homes.
pub fn inside_protected(path: &Path) -> bool {
    Roots::now().holds(path)
}

/// Seals `env`, a program's whole environment, for this run ([`spawn::seal`]): its homes must
/// be folders this run made, no other variable but the search path names a protected root, and
/// the door's own variables (`kit`) are as the door gave them.
pub fn sealed(
    env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    kit: &[(String, String)],
    windows: bool,
) -> Result<Vec<(std::ffi::OsString, std::ffi::OsString)>, String> {
    let roots = Roots::now();
    spawn::seal(
        env,
        &spawn::Judges {
            windows,
            is_runs: &is_made_or_below,
            in_protected: &|path| roots.holds(path),
            kit,
        },
    )
}

/// Whether a window the rig's pane host is asked to open (`body`, a `pane.open` request) may
/// open, on the host's own environment `host` (T-2000 finding 1, T-2002 correction 3): the
/// environment it will really have holds the seal, and its program, by the file it resolves to on
/// that environment's search path, is no program with a door of its own and, where it is a
/// harness's, has the switches that keep it from updating itself. Why not, where it may not.
pub fn window_admitted(
    host: &[(std::ffi::OsString, std::ffi::OsString)],
    body: &serde_json::Value,
) -> Result<(), String> {
    let windows = cfg!(windows);
    let env = spawn::pane_environment(host, body, windows)?;
    let kit = spawn::kit_vars(host);
    let env = sealed(env, &kit, windows)?;
    let argv = body["argv"]
        .as_array()
        .ok_or_else(|| "the window names no program".to_owned())?;
    let words = argv
        .iter()
        .map(|word| {
            word.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("a word of the window's command is no text: {word}"))
        })
        .collect::<Result<Vec<String>, String>>()?;
    let (program, args) = words
        .split_first()
        .ok_or_else(|| "the window names no program".to_owned())?;
    let program = program_started(Path::new(program), args, &env)?;
    match program {
        spawn::Program::Keychain => {
            Err("the window's program is the Keychain's utility, which only its door starts".into())
        }
        spawn::Program::Cargo => {
            Err("the window's program is cargo, which only its door starts".into())
        }
        program => spawn::has_update_switches(&program, &env),
    }
}

/// The names of the variables the account's registry stores (Windows: the machine's
/// `Session Manager\Environment` and the user's `Environment`), which the pane host's builder puts
/// over its own environment in every window; none elsewhere. Names only: no value is read. A key
/// that is not there holds none; any other failure to read is the error, so the window it would
/// have is refused (T-2119 B1-a).
pub fn stored_names() -> Result<Vec<std::ffi::OsString>, String> {
    #[cfg(windows)]
    {
        windows_registry::stored_names()
    }
    #[cfg(not(windows))]
    {
        Ok(Vec::new())
    }
}

/// The window `body` asks for, as the rig sends it to its pane host on `host`, where it may open
/// ([`window_admitted`]); why not, where not (T-2086 B1). On Windows the pane host starts a window
/// on its own environment with every variable the account's registry stores put over it
/// (portable-pty's builder), then the request's `env`, then its `dropEnv` taken out. So the
/// request sent names each of `stored`, the names the registry holds: with the value the window
/// was judged on where it has that name (a request's value wins over the registry's), and in
/// `dropEnv` where it has not. The window's environment is then the one judged, whatever the
/// registry holds; a value of the judged one that is not Unicode, which a request cannot carry,
/// refuses the window.
pub fn window_to_send(
    host: &[(std::ffi::OsString, std::ffi::OsString)],
    body: &serde_json::Value,
    stored: &[std::ffi::OsString],
) -> Result<serde_json::Value, String> {
    window_admitted(host, body)?;
    if stored.is_empty() {
        return Ok(body.clone());
    }
    let judged = spawn::pane_environment(host, body, cfg!(windows))?;
    let same =
        |left: &str, right: &std::ffi::OsStr| right.to_string_lossy().eq_ignore_ascii_case(left);
    let mut sent = body.clone();
    let mut env = sent["env"].as_object().cloned().unwrap_or_default();
    let mut dropped = sent["dropEnv"].as_array().cloned().unwrap_or_default();
    for name in stored {
        let Some(text) = name.to_str() else {
            return Err(format!(
                "the account stores a variable whose name is not Unicode ({}), which a window's request cannot name",
                name.to_string_lossy()
            ));
        };
        if env.keys().any(|asked| asked.eq_ignore_ascii_case(text)) {
            continue;
        }
        match judged.iter().find(|(held, _)| same(text, held)) {
            Some((held, value)) => {
                let value = value.to_str().ok_or_else(|| {
                    format!(
                        "the window's {} is not Unicode, so it cannot be named over the one the account stores",
                        held.to_string_lossy()
                    )
                })?;
                env.insert(held.to_string_lossy().into_owned(), value.into());
            }
            None => {
                if !dropped.iter().any(|out| {
                    out.as_str()
                        .is_some_and(|out| out.eq_ignore_ascii_case(text))
                }) {
                    dropped.push(text.into());
                }
            }
        }
    }
    sent["env"] = serde_json::Value::Object(env);
    sent["dropEnv"] = serde_json::Value::Array(dropped);
    Ok(sent)
}

/// What a file the run made starts, where it starts another program (T-2086 C2): a window is
/// judged by that program, not by the file the run made.
#[derive(Debug, Clone)]
enum Starts {
    /// A rig's shim: the program its variable names in the window's environment, or the one it
    /// was written for where it reads no variable (Windows' holds the path) or the window has none.
    Shim {
        var: Option<String>,
        program: PathBuf,
    },
    /// The kit's wrapper of a harness (`claude-isolated`, `claude-shimmed`), which starts the
    /// harness's own program.
    Wrapper(&'static str),
}

/// The files the run made that start another program, by their real paths.
static STARTERS: Mutex<Vec<(PathBuf, Starts)>> = Mutex::new(Vec::new());

fn register_starter(file: &Path, starts: Starts) {
    let real = canonical(file).unwrap_or_else(|| file.to_path_buf());
    let mut held = STARTERS.lock().unwrap_or_else(PoisonError::into_inner);
    held.retain(|(path, _)| *path != real);
    held.push((real, starts));
}

fn starter_of(real: &Path) -> Option<Starts> {
    STARTERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .find(|(path, _)| path == real)
        .map(|(_, starts)| starts.clone())
}

/// Registers `wrapper`, a file the run made, as the kit's wrapper of `harness`: a window that
/// starts it starts `harness` ([`program_in`]).
pub fn register_wrapper(wrapper: &Path, harness: &'static str) {
    register_starter(wrapper, Starts::Wrapper(harness));
}

/// Registers `shim`, a file the run made, as a rig's shim: a window that starts it starts the
/// program its variable `var` names in the window's environment, or `program` ([`program_in`]).
pub fn register_shim(shim: &Path, var: Option<&str>, program: &Path) {
    register_starter(
        shim,
        Starts::Shim {
            var: var.map(str::to_owned),
            program: program.to_path_buf(),
        },
    );
}

/// The value of the variable `wanted` in `env`, the name in any case on Windows.
fn env_value(env: &[(std::ffi::OsString, std::ffi::OsString)], wanted: &str) -> Option<String> {
    let windows = cfg!(windows);
    env.iter()
        .find(|(name, _)| {
            if windows {
                name.to_string_lossy().eq_ignore_ascii_case(wanted)
            } else {
                name == wanted
            }
        })
        .map(|(_, value)| value.to_string_lossy().into_owned())
}

/// The file `program` is, as a start with the environment `env` finds it: its own path where it
/// has a folder, a bare name's first place on the search path otherwise.
fn found_on(program: &Path, env: &[(std::ffi::OsString, std::ffi::OsString)]) -> Option<PathBuf> {
    if program.components().count() > 1 || program.is_absolute() {
        return Some(program.to_path_buf());
    }
    crate::process::first_on_path(
        &program.to_string_lossy(),
        &env_value(env, "PATH").unwrap_or_default(),
        &env_value(env, "PATHEXT").unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_owned()),
    )
}

/// What a program started with `args` is: [`program_in`], and where the program is a shell told
/// to run a command, the program that command starts (T-2219). A command the shell would read for
/// itself (an operator, a substitution, a variable) cannot be judged, and is the error: the start
/// is refused.
pub fn program_started(
    program: &Path,
    args: &[String],
    env: &[(std::ffi::OsString, std::ffi::OsString)],
) -> Result<spawn::Program, String> {
    started_within(program, args, env, 0)
}

fn started_within(
    program: &Path,
    args: &[String],
    env: &[(std::ffi::OsString, std::ffi::OsString)],
    depth: usize,
) -> Result<spawn::Program, String> {
    let judged = program_within(program, env, 0);
    if judged != spawn::Program::Plain {
        return Ok(judged);
    }
    let file = found_on(program, env).unwrap_or_else(|| program.to_path_buf());
    let real = canonical(&file);
    let names = [Some(file.as_path()), real.as_deref()];
    let shell = names
        .into_iter()
        .flatten()
        .filter_map(|path| path.file_name())
        .find_map(|name| shells::shell_named(&name.to_string_lossy()));
    let Some(shell) = shell else {
        return Ok(judged);
    };
    match shells::told(shell, args)? {
        shells::Told::Nothing => Ok(judged),
        shells::Told::Command(words) => {
            if depth >= STARTERS_DEEP {
                return Err(
                    "the shell's command starts shells inside shells too deep to judge".into(),
                );
            }
            started_within(Path::new(&words[0]), &words[1..], env, depth + 1)
        }
    }
}

/// What `program` is ([`spawn::program_of`]), found on the search path `env` gives it where it
/// is a bare name, by the file it resolves to.
pub fn program_in(
    program: &Path,
    env: &[(std::ffi::OsString, std::ffi::OsString)],
) -> spawn::Program {
    program_within(program, env, 0)
}

/// How many files that start another program a window's program is followed through.
const STARTERS_DEEP: usize = 8;

/// [`program_in`], `depth` files that start another program in: a rig's shim is followed to what
/// it starts, and the kit's wrapper of a harness is that harness (T-2086 C2).
fn program_within(
    program: &Path,
    env: &[(std::ffi::OsString, std::ffi::OsString)],
    depth: usize,
) -> spawn::Program {
    let get = |wanted: &str| env_value(env, wanted);
    let found = found_on(program, env);
    let real = found.as_deref().and_then(canonical);
    let judged = real.as_deref().or(found.as_deref()).unwrap_or(program);
    if depth < STARTERS_DEEP {
        match starter_of(judged) {
            Some(Starts::Wrapper(harness)) => return spawn::Program::Harness(harness),
            Some(Starts::Shim {
                var,
                program: written,
            }) => {
                let next = var
                    .as_deref()
                    .and_then(get)
                    .filter(|value| !value.is_empty())
                    .map_or(written, PathBuf::from);
                return program_within(&next, env, depth + 1);
            }
            None => {}
        }
    }
    let in_store = inside_protected(judged);
    let made_by_run = is_made_or_below(judged);
    spawn::program_of(
        found.as_deref().unwrap_or(program),
        real.as_deref(),
        in_store,
        made_by_run,
    )
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
/// there. Only folders are handed down: a file the run made is written by the process that made
/// it.
pub fn handed_down() -> Vec<(String, String)> {
    let roots: Vec<PathBuf> = registry()
        .iter()
        .filter(|made| made.file.is_none())
        .map(|made| made.real.clone())
        .collect();
    if roots.is_empty() {
        return Vec::new();
    }
    match std::env::join_paths(roots) {
        Ok(joined) => vec![(MADE_ROOTS.to_owned(), joined.to_string_lossy().into_owned())],
        Err(_) => Vec::new(),
    }
}

/// Registers the folder `path` as one this run made.
fn register_folder(path: &Path) {
    let real = canonical(path).unwrap_or_else(|| path.to_path_buf());
    let mut made = registry();
    let entry = Entry { real, file: None };
    if !made.contains(&entry) {
        made.push(entry);
    }
}

/// Registers `file`, open at `path`, as a file this run made, by its identity.
fn register_file(path: &Path, file: &fs::File) -> io::Result<()> {
    let real = canonical(path).unwrap_or_else(|| path.to_path_buf());
    let identity = identity::of_file(file)?;
    registry().push(Entry {
        real,
        file: Some(identity),
    });
    Ok(())
}

/// Forgets `path` as a folder this process made.
fn forget_made(path: &Path) {
    let real = canonical(path);
    registry().retain(|made| Some(&made.real) != real.as_ref() && made.real != path);
}

/// Whether `path` is a folder this process made through the door ([`made_in`]) and has not taken.
pub fn is_made(path: &Path) -> bool {
    canonical(path).is_some_and(|real| {
        registry()
            .iter()
            .any(|made| made.file.is_none() && made.real == real)
    })
}

/// Whether `path` is a folder this process made through the door, or is below one, by real paths
/// (a file this run made counts as below the folder it is in only where that folder is the run's).
pub fn is_made_or_below(path: &Path) -> bool {
    real_beneath(path).is_some_and(|real| {
        registry()
            .iter()
            .any(|made| made.file.is_none() && real.starts_with(&made.real))
    })
}

/// The real path of `path` where it is not there yet: its deepest folder that is there by its
/// real path, and the rest of the names after it ([`place::resolve`]). None where it cannot be
/// resolved, or where a name is `..`.
fn real_beneath(path: &Path) -> Option<PathBuf> {
    place::resolve(path)
        .ok()?
        .first()
        .map(place::Resolved::whole)
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

/// Whether a file or folder may be made or written at `path` ([`place::Act::Write`]): outside
/// every protected root, and below a folder this run made through the door ([`made_in`]) or
/// handed down to this program ([`MADE_ROOTS`]), or a file this run made and is still the same
/// file, by the real path; the last link of a path is refused. A folder may also be the run's own
/// folder itself, made again, where `folder` says the caller makes a folder ([`place::Act::Folder`]).
pub fn writable(path: &Path, folder: bool) -> Result<(), String> {
    let act = if folder {
        place::Act::Folder
    } else {
        place::Act::Write
    };
    judge(path, act)
        .map(drop)
        .map_err(|refused| refused.to_string())
}

/// Makes the place an allocation is made in where it is not there (the live folder of a machine
/// nobody has run on, the folder of a build's reports before a first run), judged as an
/// allocation ([`place::Act::Allocate`]): refused inside a protected root; each missing folder
/// made new, one at a time, and none that is there changed. No folder of it is the run's:
/// [`writable`] says no to it until a folder is made in it.
pub fn allocation_place(place: &Path) -> io::Result<()> {
    judge(place, place::Act::Allocate)?;
    make_missing(place)
}

/// Makes each folder of `place` that is not there, from the top, each new: a folder that is
/// there already (made since by another) is taken only where it is a folder and no link.
fn make_missing(place: &Path) -> io::Result<()> {
    let mut missing = Vec::new();
    let mut at = place.to_path_buf();
    while fs::symlink_metadata(&at).is_err() {
        missing.push(at.clone());
        match at.parent() {
            Some(parent) => at = parent.to_path_buf(),
            None => break,
        }
    }
    for folder in missing.into_iter().rev() {
        match fs::create_dir(&folder) {
            Ok(()) => {}
            Err(failed) if failed.kind() == io::ErrorKind::AlreadyExists => {
                let found = fs::symlink_metadata(&folder)?;
                if !found.is_dir() {
                    return Err(failed);
                }
            }
            Err(failed) => return Err(failed),
        }
    }
    Ok(())
}

/// A folder of a random name this process made through the door, in `parent`, its name begun
/// with `prefix`: taken away through the door when it is dropped, unless it is kept.
pub fn made_in(parent: &Path, prefix: &str) -> io::Result<Made> {
    // A say-only run whose audit file cannot be used stops before it makes a folder.
    Door::init();
    // Judged by where the parent resolves, before anything is made there.
    allocation_place(parent)?;
    let folder = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(parent)?
        .keep();
    register_folder(&folder);
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
mod k3_tests;
#[cfg(test)]
mod rule;
#[cfg(test)]
mod tests;

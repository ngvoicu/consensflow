//! What the daemon does when its ledger cannot save any more (CLN-65): a
//! rollback did not take and a transaction is open still, so every later write
//! would join it and never be committed. The owner's decision, 2026-11-22: show
//! an error. The daemon ends as at a quit, every project's windows close with it,
//! the app says why once, in [`words`], and does not start it again by itself.
//! At the next start each open project is suspended until the person chooses
//! Resume, and its members go on from their records.
//!
//! This is the one place that reaction is made: [`reaction`] is what the ledger
//! tells when it breaks. In this order: the latch is tripped (the halt first), no new
//! work is admitted (the bridge is fenced) and nothing more is asked of the app, an
//! effect under way off this thread is waited for ([`SETTLE`] at most), the app is told,
//! and only then is the cause written down, for as long as the files let it
//! ([`SETTLE`]): a log that another process holds must not keep the daemon from ending.

use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use cf_bridge::local::Bridge;
use serde_json::json;

use crate::errors::Errors;
use crate::files::SETTLE;
use crate::stop::Latch;

/// What the app is told, once, before the daemon ends: `{"words": …, "pid": …}`,
/// the words to show and the process that says them. The app knows the end from
/// the process's [`EXIT`] status first; this only gives it the words.
pub const EVENT: &str = "ledger.broken";

/// Why the latch was tripped.
pub const WHY: &str = "the ledger cannot save";

/// How the daemon's process ends for a ledger that cannot save, a stop or a start
/// that was refused for it: the app reads this at the end of the process, before
/// it decides anything, and does not start the daemon again.
pub const EXIT: i32 = cf_base::home::CANNOT_SAVE_EXIT;

/// What the person is told, for every user on either platform.
pub fn words(file: &Path) -> String {
    cf_base::home::cannot_save_words(file)
}

/// What the person is told when the home had a board and its ledger is not there as it was.
pub fn lost_words(file: &Path) -> String {
    cf_base::home::lost_ledger_words(file)
}

/// What the ledger tells when it breaks (`Ledger::on_broken`). It runs inside the
/// operation that broke the ledger, which holds it: it uses nothing of it.
pub fn reaction(
    file: &Path,
    errors: &Rc<Errors>,
    bridge: &Bridge,
    latch: &Rc<Latch>,
) -> Box<dyn FnMut(&str)> {
    let (file, errors, bridge, latch) = (
        file.to_path_buf(),
        Rc::clone(errors),
        bridge.clone(),
        Rc::clone(latch),
    );
    Box::new(move |cause| {
        // The break is declared first: from here on no effect acts (the trace appends no
        // event, no page answer says it worked, no file is written) and every log line
        // goes off the runtime.
        latch.trip_broken();
        bridge.fence();
        // From here no ordinary event or request goes: the daemon's last word is the only one.
        bridge.ask_no_more();
        // An act under way off this thread (a spec's copy put at its name) is over before the
        // app hears, within the bound; none begins after the flags.
        let settled = latch.halt().settled(SETTLE);
        // The app hears it before the bridge closes with the stop.
        bridge.final_event(
            EVENT,
            json!({ "words": words(&file), "pid": std::process::id() }),
        );
        errors
            .trace()
            .error(&format!("the ledger cannot save any more: {cause}"));
        let said = if settled {
            "the ledger cannot save any more"
        } else {
            "the ledger cannot save any more (an effect was still under way)"
        };
        errors.log().error_within(said, Some(cause), SETTLE);
    })
}

/// The file that says a home has a board: written once a ledger is held and known to be
/// ConsensFlow's, before the first write to it, and again at every start that finds a home
/// without it, so a home that an earlier build made is marked by its first start under this
/// one. It names the ledger. It is a witness: a home that holds it is used, and is never
/// started with an empty board.
pub const MARKER: &str = "consensflow.board";

/// The ledger's file in a home.
pub const LEDGER: &str = "consensflow.db";

/// Says in `home` that its board is `LEDGER`. It is written only once a ledger is held and
/// known to be ConsensFlow's, so a home that holds it had one.
pub fn mark(home: &Path) -> std::io::Result<()> {
    let marker = home.join(MARKER);
    if marker.exists() {
        return Ok(());
    }
    let said = format!("{{\"board\":true,\"ledger\":\"{LEDGER}\"}}\n");
    cf_base::file::write_whole(&marker, said.as_bytes())
        .map_err(|failed| std::io::Error::other(failed.to_string()))
}

/// What the daemon writes in a home once it has a ledger, each by its name in the home: a
/// name that is there, whatever it is (a link included, dangling or not), shows that the
/// home has been run in, whichever build wrote it. A ledger's file is one too, as anything.
const WITNESSES: [&str; 9] = [
    MARKER,
    LEDGER,
    "consensflow.db-wal",
    "consensflow.db-shm",
    "daemon.log",
    "daemon.log.1",
    "daemon.log.lock",
    "events.jsonl",
    "events.jsonl.1",
];

/// What a home is, judged before anything is written in it.
#[derive(Debug)]
pub enum Home {
    /// It holds a witness: a board was kept here, and the start never makes it empty.
    Used,
    /// Every name asked was not there, and the folder was read whole or is not there.
    Unused,
    /// A look failed for a reason other than "not there", and no witness was found: the home
    /// is refused, for its refusal to say whether a board is kept there is not given.
    Unknown(PathBuf, std::io::Error),
}

/// The names in a folder, as the system lists them (a stand-in lists what a test says).
pub type Lister = dyn Fn(&Path) -> std::io::Result<Vec<std::io::Result<String>>>;

/// [`judge_with`] over the folder as the system lists it.
pub fn judge(home: &Path) -> Home {
    judge_with(home, &|folder| {
        Ok(fs::read_dir(folder)?
            .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
            .collect())
    })
}

/// Says what `home` is. It only reads. **The app's own files are no witness:** the app makes
/// `app/`, opens `app/app.log` and may keep `app/updates.json` before the daemon's first start,
/// the page keeps `agents.json`, and a first start's staging file is its own: a home with
/// only these is a first use. Where the whole folder is replaced by another, empty one (a
/// mount point whose disk is not there) nothing is left to show it: a known limit (39).
pub fn judge_with(home: &Path, list: &Lister) -> Home {
    let mut unread = None;
    for name in WITNESSES {
        match fs::symlink_metadata(home.join(name)) {
            Ok(_) => return Home::Used,
            Err(failed) if failed.kind() == std::io::ErrorKind::NotFound => {}
            Err(failed) => unread = unread.or(Some(failed)),
        }
    }
    match list(home) {
        Ok(names) => {
            for name in names {
                match name {
                    Ok(name) if name.ends_with(".jsonl") => return Home::Used,
                    Ok(_) => {}
                    Err(failed) => unread = unread.or(Some(failed)),
                }
            }
        }
        Err(failed) if failed.kind() == std::io::ErrorKind::NotFound => {}
        Err(failed) => unread = unread.or(Some(failed)),
    }
    match unread {
        Some(failed) => Home::Unknown(home.to_path_buf(), failed),
        None => Home::Unused,
    }
}

/// What a first start does to the disk, each a file operation of its own: the real ones, or
/// the ones a test puts in their place to refuse one or to be cut at it.
#[derive(Clone, Copy)]
pub struct Steps {
    /// The ledger made whole in a new file ([`cf_ledger::make_ledger`]).
    pub make: fn(&Path) -> std::io::Result<()>,
    /// The closed file flushed.
    pub flush: fn(&Path) -> std::io::Result<()>,
    /// The file put at a name only if it is free, and the folder flushed.
    pub put: fn(&Path, &Path) -> std::io::Result<()>,
    /// A file removed.
    pub remove: fn(&Path) -> std::io::Result<()>,
}

impl Steps {
    /// What a start does.
    pub const REAL: Steps = Steps {
        make: |staging| {
            cf_ledger::make_ledger(staging)
                .map_err(|failed| std::io::Error::other(failed.to_string()))
        },
        flush: cf_base::file::flush_path,
        put: cf_base::file::put_new,
        remove: |file| fs::remove_file(file),
    };
}

#[cfg(test)]
thread_local! {
    /// What a test puts in place of the first start's file operations, for the start of its thread.
    pub(crate) static STEPS: std::cell::Cell<Steps> = const { std::cell::Cell::new(Steps::REAL) };
}

/// The file operations of this start's first ledger.
pub fn steps() -> Steps {
    #[cfg(test)]
    {
        STEPS.with(std::cell::Cell::get)
    }
    #[cfg(not(test))]
    {
        Steps::REAL
    }
}

/// What a leftover of a first start that was cut is called: the ledger's name, `.new-`, and
/// sixteen hex digits of the system's entropy. Only a first start makes such a name.
const STAGING: &str = "consensflow.db.new-";

fn is_staging(name: &str) -> bool {
    name.strip_prefix(STAGING)
        .is_some_and(|rest| rest.len() == 16 && rest.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The first ledger of an unused home, made all or nothing (T-2241 findings 11, 12): whole
/// under a name that is no witness, flushed, and put at [`LEDGER`] only if that name is free.
/// A failure or a cut before the put leaves a staging file and nothing that counts; a name
/// that was taken meanwhile is left to the start that goes on (it holds the ledger, or is
/// refused as `ledger-locked`). A failure is the one that stopped it, with nothing left
/// for the caller to clean up.
pub fn first_ledger(home: &Path, steps: &Steps) -> std::io::Result<()> {
    let name = home.join(LEDGER);
    let random = crate::api::credentials::random_hex(&cf_harness::seams::SystemEntropy, 8)
        .map_err(std::io::Error::other)?;
    let staging = home.join(format!("{STAGING}{random}"));
    let put = (steps.make)(&staging)
        .and_then(|()| (steps.flush)(&staging))
        .and_then(|()| (steps.put)(&staging, &name));
    let Err(failed) = put else {
        return Ok(());
    };
    // Where the name is there now, another start took it, or this one did and failed to flush
    // the folder: either way it goes on as a start on a ledger that is there.
    let taken = fs::symlink_metadata(&name).is_ok();
    let _ = (steps.remove)(&staging);
    if taken {
        Ok(())
    } else {
        Err(failed)
    }
}

/// Removes what a first start that was cut left, by its name: the staging files of this home.
/// Only the daemon that holds the ledger asks, after its open, so a start that builds its
/// own is never beneath it (on Windows its removal is refused; on Unix that start finds its
/// file gone at the put, the name taken, and goes on as a start that is refused as locked).
pub fn remove_leftovers(home: &Path) {
    let Ok(names) = fs::read_dir(home) else {
        return;
    };
    for entry in names.flatten() {
        if is_staging(&entry.file_name().to_string_lossy()) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// What a home's ledger file is, looked at where SQLite will open it (a link is followed:
/// a dangling one is a ledger that is not there).
#[derive(Debug, PartialEq, Eq)]
pub enum Ledgerfile {
    /// A file with something in it.
    Held,
    /// No file, or a link whose target is none.
    Missing,
    /// A file of no bytes.
    Empty,
    /// There, and not to be read or not a plain file.
    Unreadable,
}

/// Looks at the ledger of a home.
pub fn ledger_file(file: &Path) -> Ledgerfile {
    match fs::metadata(file) {
        Err(failed) if failed.kind() == std::io::ErrorKind::NotFound => Ledgerfile::Missing,
        Err(_) => Ledgerfile::Unreadable,
        Ok(found) if !found.is_file() => Ledgerfile::Unreadable,
        Ok(found) if found.len() == 0 => Ledgerfile::Empty,
        Ok(_) => match fs::File::open(file) {
            Ok(_) => Ledgerfile::Held,
            Err(_) => Ledgerfile::Unreadable,
        },
    }
}

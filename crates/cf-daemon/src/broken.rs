//! What the daemon does when its ledger cannot save any more (CLN-65): a
//! rollback did not take and a transaction is open still, so every later write
//! would join it and never be committed. The owner's decision, 2026-11-22: show
//! an error. The daemon ends as at a quit, every project's windows close with it,
//! the app says why once, in [`words`], and does not start it again by itself.
//! At the next start each open project is suspended until the person chooses
//! Resume, and its members go on from their records.
//!
//! This is the one place that reaction is made: [`reaction`] is what the ledger
//! tells when it breaks. In this order: no new work is admitted (the bridge is
//! fenced), the app is told and the latch is tripped, and only then is the cause
//! written down, for as long as the files let it ([`SETTLE`]): a log that another
//! process holds must not keep the daemon from ending.

use std::fs;
use std::path::Path;
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
        // The app hears it before the bridge closes with the stop.
        bridge.final_event(
            EVENT,
            json!({ "words": words(&file), "pid": std::process::id() }),
        );
        errors
            .trace()
            .error(&format!("the ledger cannot save any more: {cause}"));
        errors
            .log()
            .error_within("the ledger cannot save any more", Some(cause), SETTLE);
    })
}

/// The file that says a home has a board: written beside the ledger when the ledger is
/// first made, and again at every start that opens one that has none, so a home that an
/// earlier build made is marked by its first start under this one. It names the ledger.
pub const MARKER: &str = "consensflow.board";

/// The ledger's file in a home.
pub const LEDGER: &str = "consensflow.db";

/// Says in `home` that its board is `LEDGER`. A marker that cannot be written is a loss of
/// this one witness only: the other signs ([`home_was_used`]) stand.
pub fn mark(home: &Path) -> std::io::Result<()> {
    let marker = home.join(MARKER);
    if marker.exists() {
        return Ok(());
    }
    let said = format!("{{\"board\":true,\"ledger\":\"{LEDGER}\"}}\n");
    cf_base::file::write_whole(&marker, said.as_bytes())
        .map_err(|failed| std::io::Error::other(failed.to_string()))
}

/// What ConsensFlow writes in a home beside its ledger, each by its name in the home: a
/// file that is there shows the home has been run in, whichever build wrote it.
const WRITTEN: [&str; 10] = [
    MARKER,
    "consensflow.db-wal",
    "consensflow.db-shm",
    "daemon.log",
    "daemon.log.1",
    "daemon.log.lock",
    "events.jsonl",
    "events.jsonl.1",
    "app/updates.json",
    "app/app.log",
];

/// Whether `home` has been run in: it holds any file ConsensFlow writes in a home (the
/// marker, the ledger's side files, the log or its lock, the trace, the app's own files), a
/// project trace (any other `*.jsonl`), or a `consensflow.db` that is a link, dangling or
/// not. Looked at as names, not followed. A home moved before its first start on the build
/// that writes the marker is used all the same. Where the whole folder is replaced by
/// another, empty one (a mount point whose disk is not there) nothing is left to show it:
/// a known limit (39), and it starts as a first use.
pub fn home_was_used(home: &Path) -> bool {
    let named = |name: &str| fs::symlink_metadata(home.join(name)).is_ok();
    if WRITTEN.iter().any(|name| named(name)) || named(LEDGER) {
        return true;
    }
    fs::read_dir(home).is_ok_and(|names| {
        names.flatten().any(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "jsonl")
        })
    })
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

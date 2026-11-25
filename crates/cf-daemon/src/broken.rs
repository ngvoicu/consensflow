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

/// The file that says a home has a board: written beside the ledger when the ledger is
/// first made, and again at every start that opens one that has none, so a home that an
/// earlier build made is marked by its first start under this one. It names the ledger.
/// A home that holds it is used, and is never started with an empty board.
pub const MARKER: &str = "consensflow.board";

/// The ledger's file in a home.
pub const LEDGER: &str = "consensflow.db";

/// Says in `home` that its board is `LEDGER`. It is written only once a ledger has been
/// opened whole, so a home that holds it had one.
pub fn mark(home: &Path) -> std::io::Result<()> {
    let marker = home.join(MARKER);
    if marker.exists() {
        return Ok(());
    }
    let said = format!("{{\"board\":true,\"ledger\":\"{LEDGER}\"}}\n");
    cf_base::file::write_whole(&marker, said.as_bytes())
        .map_err(|failed| std::io::Error::other(failed.to_string()))
}

/// Whether `home` holds the marker, as a name that is there (a link included): the one sign,
/// until a first start is all or nothing (P23-A2), that a home has had a board.
pub fn marked(home: &Path) -> bool {
    fs::symlink_metadata(home.join(MARKER)).is_ok()
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

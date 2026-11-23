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
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::rc::Rc;

use cf_base::file::aside;
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
        bridge.fence();
        // The app hears it before the bridge closes with the stop.
        bridge.event(
            EVENT,
            json!({ "words": words(&file), "pid": std::process::id() }),
        );
        latch.trip(WHY);
        errors
            .trace()
            .error(&format!("the ledger cannot save any more: {cause}"));
        errors
            .log()
            .error_within("the ledger cannot save any more", Some(cause), SETTLE);
    })
}

/// Whether `home` shows that ConsensFlow has been used in it: a ledger's own side
/// files (`-wal`, `-shm`), or a trace that holds a line of the board (an event that is not
/// the daemon's own `daemon.*`). A daemon that started and ended before it made its
/// ledger leaves only its own lines, which is a first use. Where the whole folder is
/// another, empty one (a mount point whose disk is not there) nothing says it was
/// used: that is a known limit, and it starts as a first use.
pub fn home_was_used(home: &Path) -> bool {
    let ledger = home.join("consensflow.db");
    let beside = |suffix: &str| {
        let mut name = ledger.clone().into_os_string();
        name.push(suffix);
        std::path::PathBuf::from(name)
    };
    if beside("-wal").exists() || beside("-shm").exists() {
        return true;
    }
    let trace = home.join("events.jsonl");
    [trace.clone(), aside(&trace)].iter().any(|file| {
        fs::File::open(file).is_ok_and(|file| {
            BufReader::new(file)
                .lines()
                .map_while(Result::ok)
                .any(|line| line.contains("\"kind\":\"") && !line.contains("\"kind\":\"daemon."))
        })
    })
}

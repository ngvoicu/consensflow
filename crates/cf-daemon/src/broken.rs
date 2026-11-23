//! What the daemon does when its ledger cannot save any more (CLN-65): a
//! rollback did not take and a transaction is open still, so every later write
//! would join it and never be committed. The owner's decision, 2026-11-22: show
//! an error. The daemon ends as at a quit, every project's windows close with it,
//! the app says why once, in [`words`], and does not start it again by itself.
//! At the next start each member goes on from its record.
//!
//! This is the one place that reaction is made: [`reaction`] is what the ledger
//! tells when it breaks, and it trips the latch the stop is armed on.

use std::path::Path;
use std::rc::Rc;

use cf_bridge::local::Bridge;
use serde_json::json;

use crate::errors::Errors;
use crate::stop::Latch;

/// What the app is told, once, before the daemon ends: `{"words": …}`. The app
/// that hears it keeps the words and does not start the daemon again.
pub const EVENT: &str = "ledger.broken";

/// Why the latch was tripped.
pub const WHY: &str = "the ledger cannot save";

/// What the person is told, for every user on either platform: the ledger by its
/// real path, the likely causes, what to do, and that a change that was not
/// confirmed may still show after the restart.
pub fn words(file: &Path) -> String {
    format!(
        "ConsensFlow can't save to its ledger, {}, so it has stopped. \
         The likely causes are a full disk, a failing disk, a disk that went away, or a lost \
         permission. Fix that, then start ConsensFlow again: each member goes on from its \
         record. A change that wasn't confirmed may still show after the restart, so check the \
         board before you try it again.",
        file.display()
    )
}

/// What the ledger tells when it breaks (`Ledger::on_broken`): the cause is in
/// the log and the trace, the app is told in [`words`], and the daemon stops.
/// It runs inside the operation that broke the ledger, which holds it: it uses
/// nothing of it.
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
        errors
            .log()
            .error("the ledger cannot save any more", Some(cause));
        errors
            .trace()
            .error(&format!("the ledger cannot save any more: {cause}"));
        // The app hears it before the bridge closes with the stop.
        let _ = bridge.event(EVENT, json!({ "words": words(&file) }));
        latch.trip(WHY);
    })
}

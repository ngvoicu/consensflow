//! What of a line of the transcript is built into a value: the members
//! `project` and `ancestry` read of a record, and no others. The rest of the
//! line (a message's usage, its thinking, a tool's input and what it returned
//! besides its text, a snapshot, a rendering) is read to its end and never
//! built, which is most of a big transcript.
//!
//! A line fails exactly where it did when it was built whole (`Keep`). A
//! member listed here that nothing reads costs a value built and dropped; one
//! that something reads and is not listed here is read as absent, so the
//! tests hold what `project` makes of a kept record to what it makes of the
//! whole.

use cf_base::json::Keep;

/// A block of a message's content: its type, the text it says, the ids that
/// open and close a call, and what a result holds (all of it: it is the
/// item's text).
static BLOCK: Keep = Keep::Members(&[
    ("type", &Keep::Scalar),
    ("text", &Keep::Scalar),
    ("id", &Keep::Scalar),
    ("tool_use_id", &Keep::Scalar),
    ("content", &Keep::All),
]);

static MESSAGE: Keep = Keep::Members(&[
    ("id", &Keep::Scalar),
    ("role", &Keep::Scalar),
    ("stop_reason", &Keep::Scalar),
    ("content", &Keep::Items(&BLOCK)),
]);

/// An attachment's type and hook, and the parts of a hook's context, which
/// are texts.
static ATTACHMENT: Keep = Keep::Members(&[
    ("type", &Keep::Scalar),
    ("hookEvent", &Keep::Scalar),
    ("content", &Keep::Items(&Keep::Scalar)),
]);

/// A record.
pub(super) static RECORD: Keep = Keep::Members(&[
    ("type", &Keep::Scalar),
    ("subtype", &Keep::Scalar),
    ("uuid", &Keep::Scalar),
    ("parentUuid", &Keep::Scalar),
    ("sessionId", &Keep::Scalar),
    ("isSidechain", &Keep::Scalar),
    // The item's time, whatever it is.
    ("timestamp", &Keep::All),
    ("message", &MESSAGE),
    ("attachment", &ATTACHMENT),
    ("operation", &Keep::Scalar),
    // What a queue operation names, whatever it is, and a `/clear`'s output.
    ("content", &Keep::All),
    ("isApiErrorMessage", &Keep::Scalar),
    ("apiErrorStatus", &Keep::Scalar),
    ("error", &Keep::Scalar),
    ("interruptedMessageId", &Keep::Scalar),
    ("promptSource", &Keep::Scalar),
    ("isMeta", &Keep::Scalar),
    ("level", &Keep::Scalar),
    ("durationMs", &Keep::Scalar),
    ("messageCount", &Keep::Scalar),
    ("pendingBackgroundAgentCount", &Keep::Scalar),
    ("pendingWorkflowCount", &Keep::Scalar),
    ("preventedContinuation", &Keep::Scalar),
]);

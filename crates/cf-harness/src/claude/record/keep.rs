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

/// What a call's input says that is read: a Monitor's, what it is called and what it
/// watches, a command or a socket (an object, kept as an empty one: that it is one).
static INPUT: Keep = Keep::Members(&[
    ("command", &Keep::Scalar),
    ("description", &Keep::Scalar),
    ("ws", &Keep::Scalar),
]);

/// The model a `fallback` block names on either side (`from`, `to`).
static SIDE: Keep = Keep::Members(&[("model", &Keep::Scalar)]);

/// A block of a message's content: its type, the text it says, the ids that
/// open and close a call, the tool a call is of and what of its input is read, whether a
/// result is an error, what a result holds (all of it: it is the item's text), and the
/// two models of a `fallback` block (T-816).
static BLOCK: Keep = Keep::Members(&[
    ("type", &Keep::Scalar),
    ("text", &Keep::Scalar),
    ("id", &Keep::Scalar),
    ("name", &Keep::Scalar),
    ("input", &INPUT),
    ("tool_use_id", &Keep::Scalar),
    ("is_error", &Keep::Scalar),
    ("content", &Keep::All),
    ("from", &SIDE),
    ("to", &SIDE),
]);

static MESSAGE: Keep = Keep::Members(&[
    ("id", &Keep::Scalar),
    ("role", &Keep::Scalar),
    ("model", &Keep::Scalar),
    ("stop_reason", &Keep::Scalar),
    ("content", &Keep::Items(&BLOCK)),
]);

/// An attachment's type and hook, and the parts of a hook's context, which
/// are texts; and of a message brought into a turn under way, what it was
/// (`commandMode`) and its text.
static ATTACHMENT: Keep = Keep::Members(&[
    ("type", &Keep::Scalar),
    ("hookEvent", &Keep::Scalar),
    ("content", &Keep::Items(&Keep::Scalar)),
    ("commandMode", &Keep::Scalar),
    ("prompt", &Keep::Scalar),
]);

/// What a tool's result says besides its text: the id of the job a call that ran in
/// the background started, the monitor a Monitor call armed and its time (which says it
/// is one), and the job a TaskStop call stopped. A tool whose result is text, or a list,
/// says none.
static TOOL_USE_RESULT: Keep = Keep::Members(&[
    ("backgroundTaskId", &Keep::Scalar),
    ("taskId", &Keep::Scalar),
    ("timeoutMs", &Keep::Scalar),
    ("task_id", &Keep::Scalar),
]);

/// Where a user's record came from, by its kind (`task-notification`).
static ORIGIN: Keep = Keep::Members(&[("kind", &Keep::Scalar)]);

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
    ("toolUseResult", &TOOL_USE_RESULT),
    ("operation", &Keep::Scalar),
    // What a queue operation names, whatever it is, and what a command printed.
    ("content", &Keep::All),
    ("isApiErrorMessage", &Keep::Scalar),
    ("apiErrorStatus", &Keep::Scalar),
    ("error", &Keep::Scalar),
    ("promptSource", &Keep::Scalar),
    ("turnOrigin", &Keep::Scalar),
    ("origin", &ORIGIN),
    ("isMeta", &Keep::Scalar),
    ("turnCompanion", &Keep::Scalar),
    ("isCompactSummary", &Keep::Scalar),
    ("queueTranscriptOnly", &Keep::Scalar),
    ("level", &Keep::Scalar),
    ("durationMs", &Keep::Scalar),
    ("messageCount", &Keep::Scalar),
    ("pendingBackgroundAgentCount", &Keep::Scalar),
    ("pendingWorkflowCount", &Keep::Scalar),
    ("preventedContinuation", &Keep::Scalar),
]);

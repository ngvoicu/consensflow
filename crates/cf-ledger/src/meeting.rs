//! A meeting with the chief (3.0.7 chief-meeting, CMT-11 to CMT-16): the human
//! talks with the chief, and what the board has for it waits. The meeting is the
//! project's `meeting_at`, when it began, and no message state of its own: a held
//! message is a message queued for the chief while the column is set. It is not
//! the head of the chief's queue (`next_delivery`), so it is not typed in and opens
//! no closed chief window; the chief's `cf` does not read it (the daemon's
//! `Waits`); and the board lists it as held, in the order it will go.
//!
//! What a launch owes the chief's window is not held: the role it is to say (a
//! replacement of a role, known by its role text), the handoff a chief that takes
//! over is given, and the note that its turn was cut. Nor is the note that asks the
//! chief where things stand before a Switch chief, which the human asked for: a
//! Switch chief keeps the meeting, and the new chief's handoff goes in. These are
//! ConsensFlow's notes about no task, known by the words they open with, which are
//! here so that the engine that writes them and the ledger that knows them by them
//! say them alike.
//!
//! The human's Stop ends a meeting, and so do the project's Close and the daemon's
//! start after a quit, each logged with why (`project.meeting`). What was held then
//! goes as any queued message does: at once after a Stop, at the Resume after a
//! Close, paced by the engine, a member's question first.

use cf_proto::ledger::{MessageView, ProjectView};
use rusqlite::{params, OptionalExtension};
use serde_json::json;

use crate::model::LedgerError;
use crate::projects::known_project;
use crate::store::Store;
use crate::views::{message_view, MESSAGE_SELECT};

/// What the handoff a chief that takes over is given opens with.
pub const HANDOFF_TITLE: &str = "You are the chief now";

/// What the handoff opened with before the chief was called the chief, which a
/// ledger may still hold queued for a chief that never came up.
pub const LEAD_HANDOFF_TITLE: &str = "You are the lead now";

/// What the note that tells the chief its turn was cut opens with.
pub const CHIEF_CUT: &str = "ConsensFlow stopped in the middle of your turn, which was cut";

/// What the note that asks the chief where things stand before a Switch chief
/// opens with.
pub const SWITCH_ASKED: &str = "The human is moving this project's chief to";

/// The words ConsensFlow's notes to a chief open with that a meeting does not hold.
const NOT_HELD: [&str; 4] = [HANDOFF_TITLE, LEAD_HANDOFF_TITLE, CHIEF_CUT, SWITCH_ASKED];

/// Why a meeting ended, as its event says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ended {
    /// The human's Stop.
    Stopped,
    /// The project's Close.
    Closed,
    /// The daemon's start, after the app quit or stopped.
    Quit,
}

impl Ended {
    fn word(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Closed => "closed",
            Self::Quit => "quit",
        }
    }
}

/// The SQL condition that the message `alias` is held by its project's meeting:
/// queued for the chief while a meeting is on, and none of what a launch owes the
/// chief's window or a Switch chief asks of it.
pub(crate) fn held_by_meeting(alias: &str) -> String {
    let opens = NOT_HELD
        .iter()
        .map(|words| {
            format!(
                "substr({alias}.body, 1, {}) = '{}'",
                words.chars().count(),
                words.replace('\'', "''")
            )
        })
        .collect::<Vec<_>>()
        .join(" OR ");
    format!(
        "({alias}.state = 'queued'
          AND EXISTS (SELECT 1 FROM project meeting
            WHERE meeting.id = {alias}.project_id AND meeting.meeting_at IS NOT NULL)
          AND EXISTS (SELECT 1 FROM participant seat
            WHERE seat.id = {alias}.recipient_id AND seat.role = 'chief')
          AND {alias}.role_text IS NULL
          AND NOT ({alias}.sender_id IS NULL AND {alias}.kind = 'note'
            AND {alias}.task_id IS NULL AND ({opens})))"
    )
}

/// The messages the project's meeting holds, in the order they will go: a
/// member's question first, then the oldest. None where no meeting is on.
pub(crate) fn held(store: &Store, project_id: i64) -> Result<Vec<MessageView>, LedgerError> {
    Ok(store
        .db
        .prepare(&format!(
            "{MESSAGE_SELECT} WHERE m.project_id = ? AND {}
             ORDER BY NOT (m.kind = 'question' AND m.urgent = 0), m.id",
            held_by_meeting("m")
        ))?
        .query_map([project_id], message_view)?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The human starts (`on`) or stops a meeting with the project's chief. A
/// meeting is had with an open project's chief: a closed project's is refused.
/// Starting one that is on, or stopping none, changes nothing and logs nothing.
pub(crate) fn set_meeting(
    store: &mut Store,
    project_id: i64,
    on: bool,
) -> Result<ProjectView, LedgerError> {
    store.write(|store| {
        let project = store.project_row(project_id)?;
        if on {
            if project.state != "open" {
                return Err(LedgerError::refused_with(
                    "project-closed",
                    format!("{} is closed: resume it first", project.name),
                    409,
                ));
            }
            if project.meeting_at.is_none() {
                let at = store.at();
                store.db.execute(
                    "UPDATE project SET meeting_at = ?, updated_at = ? WHERE id = ?",
                    params![at, at, project_id],
                )?;
                store.log(project_id, "project.meeting", json!({ "on": true }))?;
            }
        } else {
            end_meeting(store, project_id, Ended::Stopped)?;
        }
        known_project(store, project_id)
    })
}

/// Ends the project's meeting, if one is on, for `why`: logged with how many
/// messages it held, which go from now as any queued message does. Inside the
/// caller's write.
pub(crate) fn end_meeting(
    store: &mut Store,
    project_id: i64,
    why: Ended,
) -> Result<(), LedgerError> {
    let on = store
        .db
        .query_row(
            "SELECT 1 FROM project WHERE id = ? AND meeting_at IS NOT NULL",
            [project_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !on {
        return Ok(());
    }
    let held = held(store, project_id)?.len();
    let at = store.at();
    store.db.execute(
        "UPDATE project SET meeting_at = NULL, updated_at = ? WHERE id = ?",
        params![at, project_id],
    )?;
    store.log(
        project_id,
        "project.meeting",
        json!({ "on": false, "ended": why.word(), "held": held }),
    )
}

/// At the daemon's start: every meeting ended with the app's quit.
pub(crate) fn end_every_meeting(store: &mut Store) -> Result<(), LedgerError> {
    let meeting = store
        .db
        .prepare("SELECT id FROM project WHERE meeting_at IS NOT NULL ORDER BY id")?
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in meeting {
        end_meeting(store, id, Ended::Quit)?;
    }
    Ok(())
}

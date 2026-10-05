//! A line of the daemon's trace, which the daemon writes as JSON, one line
//! each (`src/core/trace.js`): what happened at a window, or a project
//! deleted. The engine says each as it happens; the daemon writes it.

use serde::ser::{Serialize, SerializeMap, Serializer};

use crate::ledger::DeletedProject;

/// A line of the trace: when it happened, and what.
#[derive(Debug, Clone, PartialEq)]
pub struct TraceLine {
    /// As JavaScript's `toISOString` writes the time.
    pub at: String,
    pub what: Traced,
}

/// What a line says.
#[derive(Debug, Clone, PartialEq)]
pub enum Traced {
    /// Something at a window, by its project and participant while the
    /// ledger still knows them.
    Window {
        project: Option<i64>,
        participant: Option<String>,
        event: WindowEvent,
    },
    /// A project deleted: the line names no project, its data says which
    /// one went.
    ProjectDeleted(DeletedProject),
}

/// What happened at a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowEvent {
    /// `window.activity`: what the window does now, and why where it says.
    Activity {
        state: String,
        reason: Option<String>,
    },
    /// `window.kill_failed`: the pane host would not end it, in its words.
    KillFailed { error: Option<String> },
    /// `delivery.held`: a message waits for the window, and why.
    DeliveryHeld { message: i64, reason: String },
    /// `delivery.enter_again`: Enter pressed once more for a paste.
    EnterAgain { message: i64 },
}

impl WindowEvent {
    /// The line's `kind`.
    pub fn kind(&self) -> &'static str {
        match self {
            WindowEvent::Activity { .. } => "window.activity",
            WindowEvent::KillFailed { .. } => "window.kill_failed",
            WindowEvent::DeliveryHeld { .. } => "delivery.held",
            WindowEvent::EnterAgain { .. } => "delivery.enter_again",
        }
    }
}

/// In the order JavaScript wrote the keys: `at`, `kind`, `project`,
/// `participant`, then what the event says.
impl Serialize for TraceLine {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut line = serializer.serialize_map(None)?;
        line.serialize_entry("at", &self.at)?;
        match &self.what {
            Traced::ProjectDeleted(deleted) => {
                line.serialize_entry("kind", "project.deleted")?;
                line.serialize_entry("project", &None::<i64>)?;
                line.serialize_entry("data", deleted)?;
            }
            Traced::Window {
                project,
                participant,
                event,
            } => {
                line.serialize_entry("kind", event.kind())?;
                line.serialize_entry("project", project)?;
                line.serialize_entry("participant", participant)?;
                match event {
                    WindowEvent::Activity { state, reason } => {
                        line.serialize_entry("state", state)?;
                        line.serialize_entry("reason", reason)?;
                    }
                    WindowEvent::KillFailed { error } => line.serialize_entry("error", error)?,
                    WindowEvent::DeliveryHeld { message, reason } => {
                        line.serialize_entry("message", message)?;
                        line.serialize_entry("reason", reason)?;
                    }
                    WindowEvent::EnterAgain { message } => {
                        line.serialize_entry("message", message)?;
                    }
                }
            }
        }
        line.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(line: &TraceLine) -> String {
        serde_json::to_string(line).unwrap_or_default()
    }

    fn at(project: Option<i64>, participant: Option<&str>, event: WindowEvent) -> TraceLine {
        TraceLine {
            at: "2026-09-19T12:00:00.000Z".to_owned(),
            what: Traced::Window {
                project,
                participant: participant.map(str::to_owned),
                event,
            },
        }
    }

    #[test]
    fn a_window_s_line_is_written_as_javascript_wrote_its_keys() {
        let activity = WindowEvent::Activity {
            state: "waiting".to_owned(),
            reason: Some("a question".to_owned()),
        };
        assert_eq!(
            written(&at(Some(1), Some("chief"), activity)),
            r#"{"at":"2026-09-19T12:00:00.000Z","kind":"window.activity","project":1,"participant":"chief","state":"waiting","reason":"a question"}"#
        );
        let killed = WindowEvent::KillFailed { error: None };
        assert_eq!(
            written(&at(None, None, killed)),
            r#"{"at":"2026-09-19T12:00:00.000Z","kind":"window.kill_failed","project":null,"participant":null,"error":null}"#
        );
        let held = WindowEvent::DeliveryHeld {
            message: 7,
            reason: "the window is not ready for a paste: a paste is on its way".to_owned(),
        };
        assert_eq!(
            written(&at(Some(2), Some("zeus"), held)),
            r#"{"at":"2026-09-19T12:00:00.000Z","kind":"delivery.held","project":2,"participant":"zeus","message":7,"reason":"the window is not ready for a paste: a paste is on its way"}"#
        );
        assert_eq!(
            written(&at(
                Some(2),
                Some("zeus"),
                WindowEvent::EnterAgain { message: 7 }
            )),
            r#"{"at":"2026-09-19T12:00:00.000Z","kind":"delivery.enter_again","project":2,"participant":"zeus","message":7}"#
        );
    }

    #[test]
    fn a_deleted_project_s_line_names_no_project_and_carries_what_went() {
        let line = TraceLine {
            at: "2026-09-19T12:00:00.000Z".to_owned(),
            what: Traced::ProjectDeleted(DeletedProject {
                id: 3,
                name: "Parser".to_owned(),
                directory: "/work/parser".to_owned(),
                created_at: "2026-09-19T11:00:00.000Z".to_owned(),
                members: 1,
                sessions: 2,
                tasks: 4,
                messages: 9,
            }),
        };
        assert_eq!(
            written(&line),
            r#"{"at":"2026-09-19T12:00:00.000Z","kind":"project.deleted","project":null,"data":{"id":3,"name":"Parser","directory":"/work/parser","createdAt":"2026-09-19T11:00:00.000Z","members":1,"sessions":2,"tasks":4,"messages":9}}"#
        );
    }
}

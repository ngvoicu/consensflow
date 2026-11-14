//! The ledger's board, with durable staff marks added to its participant lanes.

use std::collections::HashMap;

use cf_proto::ledger::{Lane, MessageView, ParticipantView, ProjectView, Resume, TaskCard};
use serde::{Serialize, Serializer};

use crate::HoldKind;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StaffMark {
    pub(crate) backup_of: Option<i64>,
    pub(crate) vacation_since: Option<String>,
    pub(crate) out_kind: Option<HoldKind>,
}

/// The board's ordinary fields, with staff marks sent in each lane's participant.
/// The existing Rust lane and project types stay usable by the engine.
#[derive(Debug, Clone, PartialEq)]
pub struct Board {
    pub project: ProjectView,
    pub open: Vec<TaskCard>,
    pub lanes: Vec<Lane>,
    pub gated: Vec<MessageView>,
    pub held: Vec<MessageView>,
    pub resume: Option<Resume>,
    pub(crate) marks: HashMap<i64, StaffMark>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ParticipantOnBoard<'a> {
    #[serde(flatten)]
    participant: &'a ParticipantView,
    #[serde(skip_serializing_if = "Option::is_none")]
    backup_of: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vacation_since: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    out_kind: Option<HoldKind>,
}

#[derive(Serialize)]
struct LaneOnBoard<'a> {
    participant: ParticipantOnBoard<'a>,
    tasks: &'a [TaskCard],
}

#[derive(Serialize)]
struct BoardOnPage<'a> {
    project: &'a ProjectView,
    open: &'a [TaskCard],
    lanes: Vec<LaneOnBoard<'a>>,
    gated: &'a [MessageView],
    #[serde(skip_serializing_if = "no_messages")]
    held: &'a [MessageView],
    #[serde(skip_serializing_if = "Option::is_none")]
    resume: Option<&'a Resume>,
}

fn no_messages(messages: &&[MessageView]) -> bool {
    messages.is_empty()
}

impl Serialize for Board {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let lanes = self
            .lanes
            .iter()
            .map(|lane| {
                let mark = self.marks.get(&lane.participant.id);
                LaneOnBoard {
                    participant: ParticipantOnBoard {
                        participant: &lane.participant,
                        backup_of: mark.and_then(|mark| mark.backup_of),
                        vacation_since: mark.and_then(|mark| mark.vacation_since.as_deref()),
                        out_kind: mark.and_then(|mark| mark.out_kind),
                    },
                    tasks: &lane.tasks,
                }
            })
            .collect();
        BoardOnPage {
            project: &self.project,
            open: &self.open,
            lanes,
            gated: &self.gated,
            held: &self.held,
            resume: self.resume.as_ref(),
        }
        .serialize(serializer)
    }
}

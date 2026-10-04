//! Claude's records in the conversation's tree, for the decision whether a
//! user record is a late ancestor.
//!
//! Claude can flush a turn's user record and its ancestors after the answer
//! they started, so whether a user record is such a late ancestor is decided
//! from every record read, not only those before it. The tree holds each
//! record's place by its uuid (none when two records claim one uuid), and
//! remembers every uuid a decision looked up, since a record read later under
//! one of them may decide it otherwise, and then the transcript is read
//! again from its start.
//!
//! Where Node compared two places by identity (`record === user`), a place
//! here is its index in the order the records were read, and two places are
//! one when their indices are.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::Value;

use super::kind;
use crate::shared::record::jsonl::Stop;

/// A record's place in the conversation's tree.
struct Place {
    uuid: Arc<str>,
    /// Its parent's uuid, when the record names one as text.
    parent_uuid: Option<Arc<str>>,
    /// The record is of the session read.
    own: bool,
    /// The record is of the main conversation, not a sidechain.
    main: bool,
    /// The record is an attachment.
    attachment: bool,
}

/// Every record read: its place, who claims each uuid, and which uuids a
/// decision looked up.
#[derive(Default)]
pub(super) struct Ancestry {
    places: Vec<Place>,
    /// Each uuid's place, or none for a uuid two records claim: a third claim
    /// does not give it one again.
    parents: HashMap<Arc<str>, Option<usize>>,
    /// The uuids a decision looked up.
    watched: HashSet<Arc<str>>,
}

impl Ancestry {
    /// A record's place, when it has a uuid as text. Fails, to have the
    /// transcript read again, where a decision already looked its uuid up.
    pub(super) fn place(&mut self, record: &Value, session: &str) -> Result<Option<usize>, Stop> {
        let uuid = record
            .get("uuid")
            .and_then(Value::as_str)
            .filter(|uuid| !uuid.is_empty());
        let Some(uuid) = uuid else {
            return Ok(None);
        };
        if self.watched.contains(uuid) {
            return Err(Stop::Reread);
        }
        let uuid: Arc<str> = Arc::from(uuid);
        let at = self.places.len();
        self.places.push(Place {
            uuid: Arc::clone(&uuid),
            parent_uuid: record
                .get("parentUuid")
                .and_then(Value::as_str)
                .map(Arc::from),
            own: record.get("sessionId").and_then(Value::as_str) == Some(session),
            main: record.get("isSidechain") == Some(&Value::Bool(false)),
            attachment: kind(record) == Some("attachment"),
        });
        let claimed = self.parents.contains_key(&uuid);
        self.parents.insert(uuid, (!claimed).then_some(at));
        Ok(Some(at))
    }

    /// `lookup`: the place of the record that claims `uuid`, and the uuid
    /// watched. None for a uuid that is no text, an empty one, one nobody
    /// claims and one two records do: no decision tells those apart.
    fn lookup(&mut self, uuid: Option<&str>) -> Option<usize> {
        let uuid = uuid.filter(|uuid| !uuid.is_empty())?;
        if !self.watched.contains(uuid) {
            self.watched.insert(Arc::from(uuid));
        }
        self.parents.get(uuid).copied().flatten()
    }

    /// `lateAncestor`, once the turn is known to have ended by a boundary
    /// record (`end`, by its uuid) after the answer `candidate`: whether
    /// `user` is an ancestor of that answer, the answer itself and
    /// attachments the only records between, all of this session's main
    /// conversation, and the boundary record the answer's child.
    pub(super) fn is_late(
        &mut self,
        user: Option<usize>,
        end: Option<&str>,
        candidate: &str,
    ) -> bool {
        let Some(end) = self.lookup(end) else {
            return false;
        };
        let end = &self.places[end];
        if !end.own || !end.main || end.parent_uuid.as_deref() != Some(candidate) {
            return false;
        }
        let mut record = self.lookup(Some(candidate));
        let mut seen = HashSet::new();
        while let Some(at) = record {
            if seen.contains(&at) {
                return false;
            }
            let place = &self.places[at];
            if !place.own || !place.main {
                return false;
            }
            if Some(at) == user {
                return true;
            }
            if *place.uuid != *candidate && !place.attachment {
                return false;
            }
            seen.insert(at);
            let parent = place.parent_uuid.clone();
            record = self.lookup(parent.as_deref());
        }
        false
    }
}

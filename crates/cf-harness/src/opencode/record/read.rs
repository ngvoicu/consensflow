//! What looks read of OpenCode's store (`readWhole`, `readEvents` and
//! `readOnward`, `hosts/lib/completion/opencode.js`): the session's messages
//! and parts by id, and the events that name them, numbered in order.

use std::borrow::Cow;
use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};

use cf_base::js;
use rusqlite::types::Value as Bound;
use serde_json::Value;

use crate::shared::record::key::{Key, Keys};
use crate::shared::record::sqlite::{self, Cell, Reads};

/// The event types a look reads on past; any other has the store read whole.
const FOLLOWED: [&str; 4] = [
    "message.updated.1",
    "message.part.updated.1",
    "session.created.1",
    "session.updated.1",
];

/// What the looks read so far.
pub(super) struct Read {
    /// The seq of the last event read: -1 before any.
    last: f64,
    /// How many events were read.
    events: usize,
    pub(super) messages: Rows,
    pub(super) parts: Rows,
    /// The seq of the first event that named each message.
    pub(super) message_positions: HashMap<Key, f64>,
    /// The seq of the first event that named each message completed.
    pub(super) completion_positions: HashMap<Key, f64>,
    /// The seq of the last event that named each part.
    pub(super) part_positions: HashMap<Key, f64>,
}

/// What a look past the events read before found.
pub(super) enum Onward {
    /// The store is to be read whole.
    Whole,
    /// Read on; whether any event was new.
    Read { changed: bool },
}

/// The messages and parts the events read name, and whether every event's
/// type is one followed here.
struct Named {
    messages: Ids,
    parts: Ids,
    followed: bool,
}

/// Ids, each once, in the order first named (a JavaScript `Set`).
#[derive(Default)]
struct Ids {
    named: Vec<(Key, Value)>,
    held: HashSet<Key>,
}

impl Ids {
    /// `set.add(id)`: an id named before is not added again.
    fn add(&mut self, key: Key, id: &Value) {
        if self.held.insert(key.clone()) {
            self.named.push((key, id.clone()));
        }
    }
}

impl Read {
    /// `readWhole`'s first read: the session's messages, by their ids. Its
    /// parts and events are read by [`Read::parts_and_events`].
    pub(super) fn messages(
        reads: &Reads<'_>,
        session: &str,
        keys: &mut Keys,
    ) -> Result<Self, String> {
        let mut read = Self {
            last: -1.0,
            events: 0,
            messages: Rows::default(),
            parts: Rows::default(),
            message_positions: HashMap::new(),
            completion_positions: HashMap::new(),
            part_positions: HashMap::new(),
        };
        let all = "select * from message where session_id = ? order by time_created, id";
        for row in reads.all(all, [session])? {
            let row = Stored::new(row, keys);
            read.messages.set(row.id.clone(), row);
        }
        Ok(read)
    }

    /// The rest of `readWhole`: the session's parts by their ids, then its
    /// events.
    pub(super) fn parts_and_events(
        &mut self,
        reads: &Reads<'_>,
        session: &str,
        keys: &mut Keys,
    ) -> Result<(), String> {
        let all = "select * from part where session_id = ? order by time_created, id";
        for row in reads.all(all, [session])? {
            let row = Stored::new(row, keys);
            self.parts.set(row.id.clone(), row);
        }
        let events = reads.all(
            "select * from event where aggregate_id = ? order by seq",
            [session],
        )?;
        self.events(events, session, keys).map(|_| ())
    }

    /// `readOnward`: on from the last event read, and the rows the events
    /// after it name; whether the store is to be read whole instead.
    pub(super) fn onward(
        &mut self,
        reads: &Reads<'_>,
        session: &str,
        keys: &mut Keys,
    ) -> Result<Onward, String> {
        let before = self.events;
        let events = reads.all(
            "select * from event where aggregate_id = ? and seq > ? order by seq",
            (session, Bound::Real(self.last)),
        )?;
        let named = self.events(events, session, keys)?;
        if !named.followed {
            return Ok(Onward::Whole);
        }
        let session_value = Value::from(session);
        for (table, ids) in [("message", named.messages), ("part", named.parts)] {
            let sql = format!("select * from {table} where id = ? and session_id = ?");
            for (key, id) in ids.named {
                let found = reads.get_js(&sql, &[&id, &session_value])?;
                let rows = if table == "message" {
                    &mut self.messages
                } else {
                    &mut self.parts
                };
                match found {
                    Some(row) => rows.set(key, Stored::new(row, keys)),
                    None => rows.delete(&key),
                }
            }
        }
        let counts = reads.get(
            "select (select count(*) from message where session_id = ?) as messages,
              (select count(*) from part where session_id = ?) as parts,
              (select count(*) from event where aggregate_id = ?) as events",
            [session, session, session],
        )?;
        // A count is a whole number within 2^53, so a double holds it.
        #[allow(clippy::cast_precision_loss)]
        let agrees = |name: &str, held: usize| {
            let count = counts.as_ref().and_then(|counts| counts.get(name));
            count.is_some_and(|count| count.same(&Cell::Number(held as f64)))
        };
        let whole = !agrees("messages", self.messages.len())
            || !agrees("parts", self.parts.len())
            || !agrees("events", self.events);
        Ok(if whole {
            Onward::Whole
        } else {
            Onward::Read {
                changed: self.events > before,
            }
        })
    }

    /// `readEvents`: the conversation's events in `rows`, each after the
    /// last read, and what they name.
    fn events(
        &mut self,
        rows: Vec<sqlite::Row>,
        session: &str,
        keys: &mut Keys,
    ) -> Result<Named, String> {
        let mut named = Named {
            messages: Ids::default(),
            parts: Ids::default(),
            followed: true,
        };
        for row in rows {
            let written = row.get("seq");
            let seq = written.map_or(f64::NAN, Cell::number);
            if !(seq.is_finite() && seq.fract() == 0.0) || seq < 0.0 || seq <= self.last {
                return Err(format!(
                    "malformed OpenCode event sequence at {}",
                    text(written)
                ));
            }
            self.last = seq;
            self.events += 1;
            let event = parse(row.get("data"), &format!("event {}", text(row.get("id"))))?;
            let kind = match row.get("type") {
                Some(Cell::Text(kind)) => Some(kind.as_str()),
                _ => None,
            };
            let info = if kind == Some("message.updated.1") {
                property(&event, "info")?
            } else {
                None
            };
            if let (Some(info), Some(id)) = (info, truthy(optional(info, "id"))) {
                let key = keys.of(Some(id));
                named.messages.add(key.clone(), id);
                self.message_positions.entry(key.clone()).or_insert(seq);
                let completed = optional(optional(Some(info), "time"), "completed");
                if completed.is_some_and(|completed| !completed.is_null()) {
                    self.completion_positions.entry(key).or_insert(seq);
                }
                continue;
            }
            let part = if kind == Some("message.part.updated.1") {
                property(&event, "part")?
            } else {
                None
            };
            if let Some(id) = truthy(optional(part, "id")) {
                let key = keys.of(Some(id));
                named.parts.add(key.clone(), id);
                self.part_positions.insert(key, seq);
                continue;
            }
            if !kind.is_some_and(|kind| FOLLOWED.contains(&kind)) {
                named.followed = false;
            }
        }
        if self.last < 0.0 {
            return Err(format!("missing OpenCode event sequence for {session}"));
        }
        Ok(named)
    }
}

/// Rows by id, in the order each id was first set: a JavaScript `Map`, where
/// setting an id held keeps its place, and one deleted and set again comes
/// last.
#[derive(Default)]
pub(super) struct Rows {
    order: Vec<Option<Stored>>,
    places: HashMap<Key, usize>,
}

impl Rows {
    fn set(&mut self, key: Key, row: Stored) {
        match self.places.get(&key) {
            Some(&at) => self.order[at] = Some(row),
            None => {
                self.places.insert(key, self.order.len());
                self.order.push(Some(row));
            }
        }
    }

    fn delete(&mut self, key: &Key) {
        if let Some(at) = self.places.remove(key) {
            self.order[at] = None;
        }
    }

    /// How many rows it holds (`size`).
    pub(super) fn len(&self) -> usize {
        self.places.len()
    }

    /// The rows, in the order of their ids.
    pub(super) fn values(&self) -> impl Iterator<Item = &Stored> {
        self.order.iter().flatten()
    }
}

/// A message or a part as a query read it, with the ids a `Map` keys it by,
/// and its data parsed the first time it is read (`parsed`).
pub(super) struct Stored {
    row: sqlite::Row,
    /// `row.id`.
    pub(super) id: Key,
    /// `row.message_id`: a part's message.
    pub(super) message: Key,
    data: OnceCell<Value>,
}

impl Stored {
    fn new(row: sqlite::Row, keys: &mut Keys) -> Self {
        let mut key = |name: &str| row.get(name).map_or(Key::Undefined, |cell| cell.key(keys));
        let (id, message) = (key("id"), key("message_id"));
        Self {
            row,
            id,
            message,
            data: OnceCell::new(),
        }
    }

    /// The column `name`, none where the row has no such column.
    pub(super) fn cell(&self, name: &str) -> Option<&Cell> {
        self.row.get(name)
    }

    /// Its data (`JSON.parse(row.data)`), parsed once: `what` says which row
    /// it is in the failure.
    pub(super) fn data(&self, what: &str) -> Result<&Value, String> {
        if let Some(data) = self.data.get() {
            return Ok(data);
        }
        let data = parse(self.cell("data"), what)?;
        Ok(self.data.get_or_init(|| data))
    }
}

/// `parseStoredJson(raw, description)`: `malformed OpenCode <description>`
/// where it is no JSON.
fn parse(raw: Option<&Cell>, description: &str) -> Result<Value, String> {
    let malformed = || format!("malformed OpenCode {description}");
    // `JSON.parse(undefined)` reads the text "undefined".
    let raw = raw.ok_or_else(malformed)?;
    raw.parse()
        .map_err(|unparsed| unparsed.said(&format!("OpenCode {description}"), malformed()))
}

/// `${value}` of a column, `undefined` where the row has none.
pub(super) fn text(cell: Option<&Cell>) -> Cow<'_, str> {
    cell.map_or(Cow::Borrowed("undefined"), Cell::text)
}

/// `value.name`, where JavaScript reads a field of a value it holds: none
/// for a value of no fields, and V8's `TypeError` for null.
pub(super) fn property<'a>(value: &'a Value, name: &str) -> Result<Option<&'a Value>, String> {
    if value.is_null() {
        return Err(format!(
            "OpenCode data is null, where its field {name} was read"
        ));
    }
    Ok(value.get(name))
}

/// `value?.name`: none for none, and for null.
pub(super) fn optional<'a>(value: Option<&'a Value>, name: &str) -> Option<&'a Value> {
    value.filter(|value| !value.is_null())?.get(name)
}

/// `value` where JavaScript takes it for true.
fn truthy(value: Option<&Value>) -> Option<&Value> {
    value.filter(|value| js::truthy(Some(value)))
}

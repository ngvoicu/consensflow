//! Devin's store of a session's messages (`readStore`,
//! `hosts/lib/completion/devin.js`): its `sessions.db`, read as
//! `node:sqlite` read it, its rows' columns JavaScript's values.
//!
//! The store gains message rows (a revision is a new node), so a look reads
//! the rows after the last one it saw, and reads the store whole when the
//! conversation's row count disagrees. Its last few rows are read again too:
//! a message Devin rewrote in place would otherwise stay as first seen, and
//! a reply never read whole never settles.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::path::Path;

use cf_base::env::Env;
use cf_base::js;
use cf_base::json::{from_slice_lossy, is_json_lossy, DEEPEST};
use rusqlite::types::Value as Bound;
use serde_json::{Map, Value};

use super::chain::Chain;
use crate::devin::paths;
use crate::shared::record::key::{Key, Keys};
use crate::shared::record::sqlite::{self, bound, Reads};

/// The last rows of a conversation each look reads again, in case one was rewritten.
const RECHECKED: usize = 8;

/// The columns of a row, as each query reads them.
const COLUMNS: &str = "select row_id, node_id, parent_node_id, chat_message, created_at";

/// What a session's store said so far.
pub(super) struct Store {
    /// Each node's row, in the order the node was first read.
    rows: Vec<Row>,
    /// Where each node's row is among them.
    nodes: HashMap<Key, usize>,
    /// How many rows were read since the store was last read whole.
    count: usize,
    /// The id of the row read last: null before any.
    last: Value,
    /// The main chain's head, as the last look read it.
    head: Key,
    /// What the main chain says.
    pub(super) chain: Chain,
}

/// A row of `message_nodes`, each column as JavaScript held it.
pub(super) struct Row {
    pub(super) id: Value,
    pub(super) node: Key,
    pub(super) parent: Key,
    message: Value,
    pub(super) created_at: Value,
    /// Its message's JSON, parsed the first time it is read (`parsed`).
    parsed: OnceCell<Value>,
}

impl Store {
    /// A look at the store of `session`: what it holds now, and whether that
    /// changed since the look that left `kept`. A look that fails leaves
    /// none, so the next reads the store whole.
    pub(super) fn read<'a>(
        kept: &'a mut Option<Store>,
        session: &str,
        env: &Env,
    ) -> Result<(bool, &'a Store), String> {
        let file = paths::store(env)?;
        let opened = sqlite::Store::open(Path::new(&file))?;
        let previous = kept.take();
        let (store, changed) = opened.read(|reads| Store::look(previous, reads, session))?;
        Ok((changed, kept.insert(store)))
    }

    /// One look's queries, in one transaction, on from `previous`.
    fn look(
        previous: Option<Store>,
        reads: &Reads<'_>,
        session: &str,
    ) -> Result<(Store, bool), String> {
        let found = reads
            .get("select main_chain_id from sessions where id = ?", [session])?
            .ok_or_else(|| "missing Devin session".to_owned())?;
        let mut keys = Keys::default();
        let head = keys.of(found.get("main_chain_id"));
        let all = || {
            reads.all(
                &format!("{COLUMNS} from message_nodes where session_id = ? order by row_id"),
                [session],
            )
        };
        let mut rewritten = false;
        let (kept, rows) = match previous {
            None => (None, all()?),
            Some(mut store) => {
                let at_most = if store.last.is_null() {
                    Bound::Real(-1.0)
                } else {
                    bound(&store.last)
                };
                let recheck = format!(
                    "{COLUMNS} from message_nodes where session_id = ? and row_id <= ?
                     order by row_id desc limit {RECHECKED}"
                );
                for row in reads.all(&recheck, (session, at_most))? {
                    let row = Row::new(row, &mut keys);
                    if !store.holds(&row) {
                        store.set(row);
                        rewritten = true;
                    }
                }
                let rows = if store.last.is_null() {
                    all()?
                } else {
                    reads.all(
                        &format!(
                            "{COLUMNS} from message_nodes where session_id = ? and row_id > ? order by row_id"
                        ),
                        (session, bound(&store.last)),
                    )?
                };
                let count = reads
                    .get(
                        "select count(*) as count from message_nodes where session_id = ?",
                        [session],
                    )?
                    .and_then(|counted| counted.get("count").and_then(Value::as_u64));
                if count == u64::try_from(store.count + rows.len()).ok() {
                    (Some(store), rows)
                } else {
                    (None, all()?)
                }
            }
        };
        let fresh = kept.is_none();
        let mut store = kept.unwrap_or_else(Store::empty);
        let read_rows = !rows.is_empty();
        for row in rows {
            let row = Row::new(row, &mut keys);
            store.count += 1;
            store.last = row.id.clone();
            store.set(row);
        }
        let changed = fresh || rewritten || read_rows || head != store.head;
        if changed {
            store.head = head;
            store.chain = Chain::of(&store)?;
        }
        Ok((store, changed))
    }

    /// A store of no rows, its head `undefined` (`store ??= …`).
    fn empty() -> Self {
        Self {
            rows: Vec::new(),
            nodes: HashMap::new(),
            count: 0,
            last: Value::Null,
            head: Key::Undefined,
            chain: Chain::default(),
        }
    }

    /// Whether the store holds `row`'s node with the same message
    /// (`store.nodes.get(row.node_id)?.chat_message === row.chat_message`).
    fn holds(&self, row: &Row) -> bool {
        self.nodes
            .get(&row.node)
            .is_some_and(|&at| same(&self.rows[at].message, &row.message))
    }

    /// `row` as its node's row (`store.nodes.set`): in the place of the
    /// node's row before it, else after every row.
    fn set(&mut self, row: Row) {
        match self.nodes.get(&row.node) {
            Some(&at) => self.rows[at] = row,
            None => {
                self.nodes.insert(row.node.clone(), self.rows.len());
                self.rows.push(row);
            }
        }
    }

    /// The main chain's head.
    pub(super) fn head(&self) -> &Key {
        &self.head
    }

    /// The row of `node`, if the store holds one.
    pub(super) fn row(&self, node: &Key) -> Option<&Row> {
        self.nodes.get(node).map(|&at| &self.rows[at])
    }

    /// The row written last below `parent`, if any (`newestChild`): the one
    /// of the greatest row id, the first read of equals.
    pub(super) fn newest_child(&self, parent: &Key) -> Option<&Row> {
        let mut newest: Option<&Row> = None;
        for row in &self.rows {
            if row.parent == *parent && newest.is_none_or(|newest| greater(&row.id, &newest.id)) {
                newest = Some(row);
            }
        }
        newest
    }
}

impl Row {
    /// A row as a query read it. A query names each column, so none is
    /// missing.
    fn new(mut columns: Map<String, Value>, keys: &mut Keys) -> Self {
        let mut column = |name: &str| columns.remove(name).unwrap_or(Value::Null);
        let (id, node, parent) = (
            column("row_id"),
            column("node_id"),
            column("parent_node_id"),
        );
        Self {
            id,
            node: keys.of(Some(&node)),
            parent: keys.of(Some(&parent)),
            message: column("chat_message"),
            created_at: column("created_at"),
            parsed: OnceCell::new(),
        }
    }

    /// Its message (`JSON.parse(row.chat_message)`, the column made text as
    /// `String` makes it), parsed once.
    pub(super) fn message(&self) -> Result<&Value, String> {
        if let Some(message) = self.parsed.get() {
            return Ok(message);
        }
        let text = js::text(Some(&self.message));
        let message = from_slice_lossy(text.as_bytes()).map_err(|_| {
            let at = js::text(Some(&self.id));
            if is_json_lossy(text.as_bytes()) {
                format!(
                    "Devin's message at row {at} is JSON this build cannot hold: nested past {DEEPEST} levels, or a number past a double's range"
                )
            } else {
                format!("Devin's message at row {at} is no JSON")
            }
        })?;
        Ok(self.parsed.get_or_init(|| message))
    }
}

/// `left === right` for a column's values: null, a number (an integer and
/// the same double alike) or text.
fn same(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => left.as_f64() == right.as_f64(),
        _ => left == right,
    }
}

/// `left > right` for a column's values, as JavaScript compares them: two
/// texts by their UTF-16 code units, anything else as numbers (null as 0,
/// text that is no number as `NaN`, never greater).
fn greater(left: &Value, right: &Value) -> bool {
    if let (Value::String(left), Value::String(right)) = (left, right) {
        return left.encode_utf16().gt(right.encode_utf16());
    }
    let number = |value: &Value| js::to_number(Some(value)).unwrap_or(f64::NAN);
    number(left) > number(right)
}

#[cfg(test)]
mod tests;

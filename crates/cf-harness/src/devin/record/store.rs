//! Devin's store of a session's messages: its `sessions.db`, read as
//! `node:sqlite` read it, its rows' columns JavaScript's values.
//!
//! The store gains message rows (a revision is a new node), so a look reads
//! the rows after the last one it saw, and reads the store whole when the
//! conversation's row count disagrees. Its last few rows are read again too:
//! a message Devin rewrote in place would otherwise stay as first seen, and
//! a reply never read whole never settles.
//!
//! A column is held as it was read, and made what Devin's JavaScript made of
//! it where it made it: a blob or an infinity fails no look it plays no part
//! in. A blob is an object to JavaScript: the node a blob names is no node
//! another names.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::path::Path;

use cf_base::env::Env;
use rusqlite::types::Value as Bound;
use serde_json::Value;

use super::chain::Chain;
use crate::devin::paths;
use crate::shared::record::key::{Key, Keys};
use crate::shared::record::sqlite::{
    self, greater_of, key_of, same_of, text_of, Cell, Reads, Unparsed,
};

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
    last: Option<Cell>,
    /// The main chain's head, as the last look read it.
    head: Key,
    /// What the main chain says.
    pub(super) chain: Chain,
}

/// A row of `message_nodes`, each column as JavaScript read it, its node
/// and its parent as a `Map` keys them. A column is none where the row has
/// none by the name asked (`undefined`): a table may declare it in another
/// case, which names the row's column.
pub(super) struct Row {
    pub(super) id: Option<Cell>,
    pub(super) node: Key,
    pub(super) parent: Key,
    message: Option<Cell>,
    pub(super) created_at: Option<Cell>,
    /// Its message's JSON, parsed the first time it is read (`parsed`).
    parsed: OnceCell<Value>,
}

impl Store {
    /// A look at the store of `session`: what it holds now, and whether that
    /// changed since the look that left `kept`. A look that fails leaves
    /// none, so the next reads the store whole. `keys` makes every key the
    /// reader keeps, so that no two blobs it read are one.
    pub(super) fn read<'a>(
        kept: &'a mut Option<Store>,
        keys: &mut Keys,
        session: &str,
        env: &Env,
    ) -> Result<(bool, &'a Store), String> {
        let file = paths::store(env)?;
        let opened = sqlite::Store::open(Path::new(&file))?;
        let previous = kept.take();
        let (store, changed) = opened.read(|reads| Store::look(previous, reads, keys, session))?;
        Ok((changed, kept.insert(store)))
    }

    /// One look's queries, in one transaction, on from `previous`.
    fn look(
        previous: Option<Store>,
        reads: &Reads<'_>,
        keys: &mut Keys,
        session: &str,
    ) -> Result<(Store, bool), String> {
        let found = reads
            .get("select main_chain_id from sessions where id = ?", [session])?
            .ok_or_else(|| "missing Devin session".to_owned())?;
        let head = found
            .get("main_chain_id")
            .map_or(Key::Undefined, |head| head.key(keys));
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
                let at_most = match &store.last {
                    None | Some(Cell::Null) => Bound::Real(-1.0),
                    Some(last) => last.bound(),
                };
                let recheck = format!(
                    "{COLUMNS} from message_nodes where session_id = ? and row_id <= ?
                     order by row_id desc limit {RECHECKED}"
                );
                for row in reads.all(&recheck, (session, at_most))? {
                    let row = Row::new(row, keys);
                    if !store.holds(&row) {
                        store.set(row);
                        rewritten = true;
                    }
                }
                let rows = if let Some(Cell::Null) = store.last {
                    all()?
                } else {
                    // Node bound `undefined`, and threw.
                    let Some(last) = &store.last else {
                        return Err(
                            "provided value cannot be bound to SQLite parameter 2".to_owned()
                        );
                    };
                    reads.all(
                        &format!(
                            "{COLUMNS} from message_nodes where session_id = ? and row_id > ? order by row_id"
                        ),
                        (session, last.bound()),
                    )?
                };
                let counted = reads.get(
                    "select count(*) as count from message_nodes where session_id = ?",
                    [session],
                )?;
                // A count is a whole number within 2^53, so a double holds it.
                #[allow(clippy::cast_precision_loss)]
                let expected = (store.count + rows.len()) as f64;
                let count = counted.as_ref().and_then(|counted| counted.get("count"));
                if count.is_some_and(|count| count.same(&Cell::Number(expected))) {
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
            let row = Row::new(row, keys);
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
            last: Some(Cell::Null),
            head: Key::Undefined,
            chain: Chain::default(),
        }
    }

    /// Whether the store holds `row`'s node with the same message
    /// (`store.nodes.get(row.node_id)?.chat_message === row.chat_message`).
    fn holds(&self, row: &Row) -> bool {
        self.nodes
            .get(&row.node)
            .is_some_and(|&at| same_of(self.rows[at].message.as_ref(), row.message.as_ref()))
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
            if row.parent == *parent
                && newest.is_none_or(|newest| greater_of(row.id.as_ref(), newest.id.as_ref()))
            {
                newest = Some(row);
            }
        }
        newest
    }
}

impl Row {
    /// A row as a query read it.
    fn new(mut columns: sqlite::Row, keys: &mut Keys) -> Self {
        let mut column = |name: &str| columns.take(name);
        Self::of(
            column("row_id"),
            column("node_id").as_ref(),
            column("parent_node_id").as_ref(),
            column("chat_message"),
            column("created_at"),
            keys,
        )
    }

    /// A row of these columns.
    pub(super) fn of(
        id: Option<Cell>,
        node: Option<&Cell>,
        parent: Option<&Cell>,
        message: Option<Cell>,
        created_at: Option<Cell>,
        keys: &mut Keys,
    ) -> Self {
        Self {
            id,
            node: key_of(node, keys),
            parent: key_of(parent, keys),
            message,
            created_at,
            parsed: OnceCell::new(),
        }
    }

    /// Its message (`JSON.parse(row.chat_message)`, the column made text as
    /// `String` makes it), parsed once.
    pub(super) fn message(&self) -> Result<&Value, String> {
        if let Some(message) = self.parsed.get() {
            return Ok(message);
        }
        // `JSON.parse(undefined)` reads the text "undefined", which is no JSON.
        let parsed = self
            .message
            .as_ref()
            .map_or(Err(Unparsed::NoJson), Cell::parse);
        let message = parsed.map_err(|unparsed| {
            let what = format!("Devin's message at row {}", text_of(self.id.as_ref()));
            let no_json = format!("{what} is no JSON");
            unparsed.said(&what, no_json)
        })?;
        Ok(self.parsed.get_or_init(|| message))
    }
}

#[cfg(test)]
mod tests;

//! The items a transcript's records made, in the order they were pushed.
//!
//! An assistant's message is one item however many records it was written
//! in: each record is a fragment of its text, by the record's uuid. A tool's
//! result is one item by the call it answers. Each item keeps the place
//! (`seq`) of the latest record that said anything of it, which orders the
//! items in an answer; neither that nor the fragments are in what it says.

use std::collections::HashMap;
use std::sync::Arc;

use cf_base::js;
use serde_json::Value;

use crate::shared::record::reading::{Item, Role};
use crate::shared::record::sort::sort;

#[derive(Default)]
pub(super) struct Items {
    list: Vec<Entry>,
    /// Each assistant's message, by its native id.
    assistants: HashMap<Arc<str>, usize>,
    /// Each tool's result, by the call's native id.
    tools: HashMap<Arc<str>, usize>,
}

struct Entry {
    item: Item,
    seq: usize,
    fragments: Fragments,
}

/// An assistant's text in the pieces its records said it in.
#[derive(Default)]
struct Fragments {
    /// The records' uuids, as each first said something.
    order: Vec<Arc<str>>,
    text: HashMap<Arc<str>, String>,
}

impl Items {
    /// An item that is no assistant's message or tool's result.
    pub(super) fn push(&mut self, id: Arc<str>, role: Role, text: &str, at: &Value, seq: usize) {
        self.list.push(Entry::new(id, role, text, true, at, seq));
    }

    /// `addAssistant`: the message `id` names, made when it is the first
    /// record of it; its time and place are the latest record's. Its place in
    /// the list is where it was first pushed.
    pub(super) fn assistant(&mut self, id: Arc<str>, at: &Value, seq: usize) -> usize {
        let place = match self.assistants.get(&id) {
            Some(&place) => place,
            None => {
                let place = self.list.len();
                self.assistants.insert(Arc::clone(&id), place);
                self.list
                    .push(Entry::new(id, Role::Assistant, "", false, at, seq));
                place
            }
        };
        let entry = &mut self.list[place];
        entry.item.at = at.clone();
        entry.seq = seq;
        place
    }

    /// `updateNativeFragment`: what the record `identity` says of the
    /// message at `place` is its text, now; the message's text is every
    /// record's, a line each, in the order each first said something. A
    /// record that says nothing changes nothing.
    pub(super) fn fragment(&mut self, place: usize, identity: Arc<str>, text: &str) {
        if text.is_empty() {
            return;
        }
        let fragments = &mut self.list[place].fragments;
        if !fragments.text.contains_key(&identity) {
            fragments.order.push(Arc::clone(&identity));
        }
        fragments.text.insert(identity, text.to_owned());
        let joined = fragments
            .order
            .iter()
            .filter_map(|identity| fragments.text.get(identity).map(String::as_str))
            .collect::<Vec<_>>()
            .join("\n");
        self.list[place].item.text = Arc::from(joined);
    }

    /// `addTool`: the result of the call `id` names, made at the first
    /// record of it and said anew by a later one.
    pub(super) fn tool(&mut self, id: Arc<str>, text: &str, at: &Value, seq: usize) {
        if let Some(&place) = self.tools.get(&id) {
            let entry = &mut self.list[place];
            entry.item.text = Arc::from(text);
            entry.item.at = at.clone();
            entry.seq = seq;
            return;
        }
        self.tools.insert(Arc::clone(&id), self.list.len());
        self.list
            .push(Entry::new(id, Role::Tool, text, true, at, seq));
    }

    /// The item pushed last, in the order pushed (`list.at(-1)`).
    pub(super) fn last(&self) -> Option<&Item> {
        self.list.last().map(|entry| &entry.item)
    }

    /// The assistant's message `id` names is complete, when there is one.
    pub(super) fn complete(&mut self, id: &str) {
        if let Some(&place) = self.assistants.get(id) {
            self.list[place].item.complete = true;
        }
    }

    /// The items in the record's order: an item's `seq` is its line's place,
    /// and those of one line are ordered by id, as `localeCompare` orders.
    pub(super) fn sorted(&self) -> Result<Vec<Item>, String> {
        let entries: Vec<&Entry> = self.list.iter().collect();
        let sorted = sort(entries, |left, right| {
            let order = left
                .seq
                .cmp(&right.seq)
                .then_with(|| js::locale_compare(&left.item.id, &right.item.id));
            Ok(match order {
                std::cmp::Ordering::Less => -1.0,
                std::cmp::Ordering::Equal => 0.0,
                std::cmp::Ordering::Greater => 1.0,
            })
        })?;
        Ok(sorted.into_iter().map(|entry| entry.item.clone()).collect())
    }
}

impl Entry {
    fn new(id: Arc<str>, role: Role, text: &str, complete: bool, at: &Value, seq: usize) -> Self {
        Self {
            item: Item {
                id,
                role,
                text: Arc::from(text),
                complete,
                at: at.clone(),
                commentary: false,
            },
            seq,
            fragments: Fragments::default(),
        }
    }
}

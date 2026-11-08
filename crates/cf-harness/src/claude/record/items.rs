//! The items a transcript's records made, in the order they were pushed.
//!
//! An assistant's message is one item however many records it was written
//! in: each record is a fragment of its text, by the record's uuid. A tool's
//! result is one item by the call it answers. Each item keeps the place
//! (`seq`) of the latest record that said anything of it, which orders the
//! items in an answer; neither that nor the fragments are in what it says.
//! A message that holds a tool's result itself (the advisor's: of the blocks a
//! message holds, only a `tool_result` or an `advisor_tool_result` is one, so no
//! other server tool's) goes on after it in an item of its own, as a message after
//! a tool does: what it said before stopped for the tool (T-593's result, m-4165).
//! A record that holds a result and words of its own both does not split, and its
//! words stay with what the message said before: Claude Code writes a block a
//! record, and no transcript read has held one (T-670's P3).
//!
//! Beside them are the items only the history shows ([`Record::beside`]): a
//! record's thinking, which stands before the message it is of, and a call with
//! no result yet, which stands after it until its result is an item.
//!
//! [`Record::beside`]: crate::shared::record::reading::Record::beside

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cf_base::js;
use serde_json::Value;

use crate::shared::record::drawn::{self, Detail, Kind};
use crate::shared::record::key::Key;
use crate::shared::record::reading::{Item, Role};
use crate::shared::record::sort::sort;

#[derive(Default)]
pub(super) struct Items {
    list: Vec<Entry>,
    /// Each assistant's message, by its native id: its latest item.
    assistants: HashMap<Arc<str>, usize>,
    /// The messages that hold a tool's result and have written nothing since:
    /// the next record of one begins its next item.
    after_a_tool: HashSet<Arc<str>>,
    /// Each tool's result, by the call's native id.
    tools: HashMap<Arc<str>, usize>,
    /// The items only the history shows, in the order they were pushed.
    beside: Vec<Beside>,
    /// Each call with no result yet among them, by its native id: its place among them.
    running: HashMap<Arc<str>, usize>,
}

struct Entry {
    item: Item,
    seq: usize,
    /// The place of the record it began in, which no later record moves.
    began: usize,
    fragments: Fragments,
    /// What its words are, as the records that said something of it say ([`Words`]).
    words: Words,
}

/// Whose words an assistant's message holds, by the records that said something of it.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Words {
    /// None yet.
    #[default]
    None,
    /// An error of the API's alone, which Claude Code writes in the assistant's place
    /// (`isApiErrorMessage`): the harness's own.
    Refusal,
    /// The model's, whatever else it holds.
    Model,
}

/// An item only the history shows, the place of the record that said it, and where it
/// stands among the items of that record ([`Stands`]).
struct Beside {
    item: Item,
    seq: usize,
    stands: Stands,
    /// The call it is, where it is one: no longer running once the turn is over without
    /// its result ([`Items::timeline`]).
    call: Option<Key>,
    /// Its result came, and is an item of its own.
    answered: bool,
}

/// The items in their order, the place of the record each began in, and those only the
/// history shows, each with the place among the items it stands before.
type Laid = (Vec<Item>, Vec<usize>, Vec<(usize, Item)>);

/// The items in their order, the place of the record each began in, and the place of the
/// latest record that said anything of each.
type Ordered = (Vec<Item>, Vec<usize>, Vec<usize>);

/// Where an item only the history shows stands among the items of the record that said it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Stands {
    /// Before them: a message's thinking, before its words.
    Before,
    /// After them: a call, after the words of the message that made it.
    After,
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

    /// A notification the harness wrote in the user's place (`<task-notification>`): a
    /// user's item, as Node read it, of the harness's own words (`synthetic`).
    pub(super) fn notification(&mut self, id: Arc<str>, text: &str, at: &Value, seq: usize) {
        let mut entry = Entry::new(id, Role::User, text, true, at, seq);
        entry.item.synthetic = true;
        self.list.push(entry);
    }

    /// The summary a compaction wrote in the user's place: a user's item, as Node read
    /// it, that says it is the harness's own; and what the history draws of it, the tokens
    /// the compaction's boundary said it compacted from.
    pub(super) fn summary(
        &mut self,
        id: Arc<str>,
        text: &str,
        at: &Value,
        seq: usize,
        tokens: Option<u64>,
    ) {
        let mut entry = Entry::new(id, Role::User, text, true, at, seq);
        entry.item.compaction = true;
        entry.item.detail = Some(Arc::new(Detail {
            tokens,
            ..Detail::of(Kind::Compaction)
        }));
        self.list.push(entry);
    }

    /// What a record said its message's model thought (`thinking`): an item only the
    /// history shows, by the record's uuid and its place among the record's thinking
    /// blocks, which stands before the words of the record's message.
    pub(super) fn thinking(&mut self, record: &str, text: &str, at: &Value, seq: usize) {
        let place = self
            .beside
            .iter()
            .filter(|beside| beside.seq == seq)
            .count();
        let id = Arc::from(format!("{record}:thinking:{place}"));
        self.beside.push(Beside {
            item: drawn::beside_item(
                id,
                Role::Custom,
                text,
                Some(at.clone()),
                Detail::of(Kind::Thinking),
            ),
            seq,
            stands: Stands::Before,
            call: None,
            answered: false,
        });
    }

    /// A call a record made (`call`, by the native id `id` its result will be the item
    /// of): an item only the history shows until its result is one, running, with what the
    /// history draws of it. Said again, it is the same call.
    pub(super) fn called(
        &mut self,
        id: Arc<str>,
        call: Key,
        detail: Detail,
        at: &Value,
        seq: usize,
    ) {
        if self.running.contains_key(&id) || self.tools.contains_key(&id) {
            return;
        }
        self.running.insert(Arc::clone(&id), self.beside.len());
        self.beside.push(Beside {
            item: drawn::beside_item(id, Role::Tool, "", Some(at.clone()), detail),
            seq,
            stands: Stands::After,
            call: Some(call),
            answered: false,
        });
    }

    /// `addAssistant`: the message `id` names, made when it is the first
    /// record of it; its time and place are the latest record's. Its place in
    /// the list is where it was first pushed.
    ///
    /// A message that held a tool's result goes on in an item of its own, by
    /// the uuid of the record that goes on with it (`record`). A record that is
    /// no place of the message's words (`placed` false: it holds a tool's result
    /// alone) leaves the message's item where it was.
    pub(super) fn assistant(
        &mut self,
        id: Arc<str>,
        record: &Arc<str>,
        at: &Value,
        seq: usize,
        placed: bool,
    ) -> usize {
        if !placed {
            if let Some(&place) = self.assistants.get(&id) {
                return place;
            }
        }
        let after_a_tool = self.after_a_tool.remove(&id);
        let place = match self.assistants.get(&id) {
            Some(&place) if !after_a_tool => place,
            _ => {
                let place = self.list.len();
                let item = if after_a_tool {
                    Arc::clone(record)
                } else {
                    Arc::clone(&id)
                };
                self.assistants.insert(id, place);
                self.list
                    .push(Entry::new(item, Role::Assistant, "", false, at, seq));
                place
            }
        };
        let entry = &mut self.list[place];
        entry.item.at = Some(at.clone());
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
    /// record of it and said anew by a later one; and what the history draws of it
    /// (`detail`), which the call it answers no longer stands beside the items as.
    pub(super) fn tool(
        &mut self,
        id: Arc<str>,
        text: &str,
        at: &Value,
        seq: usize,
        detail: Detail,
    ) {
        if let Some(place) = self.running.remove(&id) {
            self.beside[place].answered = true;
        }
        let detail = Some(Arc::new(detail));
        if let Some(&place) = self.tools.get(&id) {
            let entry = &mut self.list[place];
            entry.item.text = Arc::from(text);
            entry.item.at = Some(at.clone());
            entry.item.detail = detail;
            entry.seq = seq;
            return;
        }
        self.tools.insert(Arc::clone(&id), self.list.len());
        let mut entry = Entry::new(id, Role::Tool, text, true, at, seq);
        entry.item.detail = detail;
        self.list.push(entry);
    }

    /// The call `id` names while it ran, as the history drew it, where it did: what its
    /// result's item starts from.
    pub(super) fn call_of(&self, id: &str) -> Option<&Item> {
        let place = *self.running.get(id)?;
        Some(&self.beside[place].item)
    }

    /// The item pushed last, in the order pushed (`list.at(-1)`).
    pub(super) fn last(&self) -> Option<&Item> {
        self.list.last().map(|entry| &entry.item)
    }

    /// The message at `place` is no one's words of the conversation: the
    /// harness's own (`commentary`), which no answer holds, written with no
    /// model behind it (`synthetic`).
    pub(super) fn synthetic(&mut self, place: usize) {
        let item = &mut self.list[place].item;
        item.commentary = true;
        item.synthetic = true;
    }

    /// A record that said something of the message at `place`, an error of the API's
    /// (`refused`) or the model's words: a message of errors alone is the harness's own words
    /// (`synthetic`), which no answer takes the place of the agent's words with (T-1310's
    /// review of T-878, T-1311). It is no progress note (`commentary`): a reading says that,
    /// and Node's held these words as the message's. Words of the model's in the same message,
    /// before or after the error, make it the model's again.
    pub(super) fn said(&mut self, place: usize, refused: bool) {
        let entry = &mut self.list[place];
        entry.words = match (entry.words, refused) {
            (Words::None | Words::Refusal, true) => Words::Refusal,
            _ => Words::Model,
        };
        if refused && entry.words == Words::Refusal {
            entry.item.synthetic = true;
        } else if entry.words == Words::Model && !entry.item.commentary {
            // Claude Code's close of a cut turn is its own whatever follows (`synthetic`).
            entry.item.synthetic = false;
        }
    }

    /// The message `id` holds a tool's result: what it writes next is after it.
    pub(super) fn after_a_tool(&mut self, id: &Arc<str>) {
        self.after_a_tool.insert(Arc::clone(id));
    }

    /// The assistant's message `id` names is complete, when there is one.
    pub(super) fn complete(&mut self, id: &str) {
        if let Some(&place) = self.assistants.get(id) {
            self.list[place].item.complete = true;
        }
    }

    /// The items in the record's order (an item's `seq` is its line's place,
    /// and those of one line are ordered by id, as `localeCompare` orders
    /// under ICU's root collation, whatever the machine's locale), with the place of
    /// the record each began in, and those only the history shows, each
    /// with the place among the items it stands before: a record's beside the items of
    /// the same record, before or after them as it [`Stands`], and after those of the
    /// records before it. A call whose turn is over without its result (`open`, the calls
    /// open, does not hold it) says no more that it runs.
    pub(super) fn timeline(&self, open: &std::collections::HashSet<Key>) -> Result<Laid, String> {
        let (items, places, seqs) = self.ordered()?;
        let mut beside: Vec<&Beside> = self
            .beside
            .iter()
            .filter(|beside| !beside.answered)
            .collect();
        // Stable: those of one record and one standing keep the order they were pushed in.
        beside.sort_by_key(|beside| (beside.seq, beside.stands));
        let mut shown = Vec::with_capacity(beside.len());
        let mut next = 0;
        for one in beside {
            // The first item it stands before: the first of a later record, or of its own
            // where it stands before them.
            while next < seqs.len()
                && (seqs[next] < one.seq || (seqs[next] == one.seq && one.stands == Stands::After))
            {
                next += 1;
            }
            let mut item = one.item.clone();
            if one.call.as_ref().is_some_and(|call| !open.contains(call)) {
                if let Some(detail) = &item.detail {
                    item.detail = Some(Arc::new(Detail {
                        status: None,
                        ..Detail::clone(detail)
                    }));
                }
            }
            shown.push((next, item));
        }
        Ok((items, places, shown))
    }

    /// The items in the record's order, the place of the record each began in, and the
    /// place of the latest record that said anything of each.
    fn ordered(&self) -> Result<Ordered, String> {
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
        let mut items = Vec::with_capacity(sorted.len());
        let mut places = Vec::with_capacity(sorted.len());
        let mut seqs = Vec::with_capacity(sorted.len());
        for entry in sorted {
            items.push(entry.item.clone());
            places.push(entry.began);
            seqs.push(entry.seq);
        }
        Ok((items, places, seqs))
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
                // `record.timestamp ?? seq`: a Claude item's time is never undefined.
                at: Some(at.clone()),
                commentary: false,
                synthetic: false,
                compaction: false,
                detail: None,
            },
            seq,
            began: seq,
            fragments: Fragments::default(),
            words: Words::None,
        }
    }
}

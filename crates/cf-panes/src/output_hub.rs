//! Where the panes' output goes: to one destination at a time, the window's
//! page or the headless helper's peer, and what a pane prints while there is
//! none waits for the next.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::pty::{PaneKey, PaneOutput};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneOutputMessage {
    id: String,
    generation: u64,
    seq: u64,
    /// The offset of the first byte, where the window keeps a journal ([`crate::journal`]):
    /// the count of bytes the program had printed before it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) offset: Option<u64>,
    pub(crate) bytes: Vec<u8>,
}

impl PaneOutputMessage {
    /// The message's sequence, which the page acknowledges.
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// The bytes the message carries.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// A message of bytes the window's journal holds from `offset` on.
    pub(crate) fn at_offset(output: PaneOutput, offset: u64) -> Self {
        Self {
            offset: Some(offset),
            ..Self::from(output)
        }
    }

    fn taken(&self) -> (PaneKey, Taken) {
        let taken = Taken {
            seq: self.seq,
            end: self.offset.map(|offset| offset + self.bytes.len() as u64),
        };
        (PaneKey::new(&self.id, self.generation), taken)
    }
}

impl From<PaneOutput> for PaneOutputMessage {
    fn from(output: PaneOutput) -> Self {
        Self {
            id: output.key.id,
            generation: output.key.generation,
            seq: output.seq,
            offset: None,
            bytes: output.bytes,
        }
    }
}

struct OutputHubState {
    sink: Option<OutputSink>,
    pending: VecDeque<PaneOutputMessage>,
    /// For each window, the last message a sink took: what a new subscription settles the room
    /// up to, and, where the window keeps a journal, the offset its history ends at
    /// ([`OutputHub::attach_page`]). Set under the lock the sink is called under, so a byte is
    /// handed to a page or it is not.
    handed: HashMap<PaneKey, Taken>,
}

/// How far a window's output has been handed to a sink that took it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Taken {
    /// The sequence of the last message taken.
    pub(crate) seq: u64,
    /// The offset past its last byte, where the window keeps a journal.
    pub(crate) end: Option<u64>,
}

/// Where one pane's bytes go. `false` means the destination is gone, and the
/// hub parks what follows until a new one arrives. The sink runs under the
/// hub's lock and the headless one waits while its peer is busy, so publish
/// only from a pane's own output thread, never from a bridge handler.
pub type OutputSink = Arc<dyn Fn(PaneOutputMessage) -> bool + Send + Sync>;

pub struct OutputHub {
    state: Mutex<OutputHubState>,
}

impl OutputHub {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(OutputHubState {
                sink: None,
                pending: VecDeque::new(),
                handed: HashMap::new(),
            }),
        }
    }

    /// A destination for the panes' bytes: the window's, a webview channel the
    /// page reads (the app wraps it in a sink), or the headless one, back over
    /// the bridge the request came in on.
    ///
    /// The hub exists so `register_pane_handlers` need not know which of the
    /// two it is feeding — that is what lets the window and the helper share
    /// one set of handlers instead of two that drift.
    pub(crate) fn register_sink(&self, sink: OutputSink) {
        self.attach(sink);
    }

    fn attach(&self, sink: OutputSink) {
        self.attach_page(sink);
    }

    /// [`OutputHub::register_sink`], and what had been handed to a page for each window, read
    /// in the one hold of the lock that swaps the sink and before what was parked is delivered
    /// to the new one: the new page's history ends there, what was parked is the first of its
    /// live messages, and the room is settled up to there and no further.
    pub(crate) fn attach_page(&self, sink: OutputSink) -> HashMap<PaneKey, Taken> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let handed = state.handed.clone();
        state.sink = Some(sink);
        let pending = std::mem::take(&mut state.pending);
        for message in pending {
            Self::deliver_or_park(&mut state, message);
        }
        handed
    }

    /// A window with no journal has ended and all it printed has been handed on or parked: no
    /// page needs what was handed of it (no history, and no room left to settle). One with a
    /// journal keeps its entry for as long as its journal is kept ([`OutputHub::retain_handed`]).
    pub(crate) fn forget(&self, key: &PaneKey) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .handed
            .remove(key);
    }

    /// How many windows the hub keeps what it handed for.
    #[cfg(test)]
    pub(crate) fn handed_windows(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .handed
            .len()
    }

    /// Forget the windows `keep` does not name: a window that is gone has no boundary.
    pub(crate) fn retain_handed(&self, keep: impl Fn(&PaneKey) -> bool) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .handed
            .retain(|key, _| keep(key));
    }

    fn deliver_or_park(state: &mut OutputHubState, message: PaneOutputMessage) {
        if state
            .sink
            .as_ref()
            .is_some_and(|sink| sink(message.clone()))
        {
            let (key, taken) = message.taken();
            state.handed.insert(key, taken);
            return;
        }
        state.sink = None;
        state.pending.push_back(message);
    }

    pub(crate) fn publish(&self, message: PaneOutputMessage) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        Self::deliver_or_park(&mut state, message);
    }
}

impl Default for OutputHub {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_output_includes_a_members_window() {
        let hub = OutputHub::new();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let copy = Arc::clone(&seen);
        hub.register_sink(Arc::new(move |message| {
            copy.lock().unwrap().push(message.id);
            true
        }));
        hub.publish(PaneOutputMessage {
            id: "p1-zeus".into(),
            generation: 1,
            seq: 1,
            offset: None,
            bytes: vec![65],
        });
        assert_eq!(*seen.lock().unwrap(), vec!["p1-zeus"]);
    }

    #[test]
    fn main_subscription_receives_the_chief_and_a_member_without_replacing_either() {
        let hub = OutputHub::new();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let copy = Arc::clone(&seen);
        hub.attach(Arc::new(move |message| {
            copy.lock().unwrap().push(message.id);
            true
        }));
        for id in ["p1-zeus", "p1-chief", "p1-zeus"] {
            hub.publish(PaneOutputMessage {
                id: id.into(),
                generation: 1,
                seq: 1,
                offset: None,
                bytes: vec![65],
            });
        }
        assert_eq!(
            *seen.lock().unwrap(),
            vec!["p1-zeus", "p1-chief", "p1-zeus"]
        );
    }

    /// A destination that has gone (a page that reloaded, its channel with
    /// it) parks what follows, and the next one gets all of it, in order,
    /// before anything newer.
    #[test]
    fn output_waits_for_the_next_destination_when_one_goes() {
        let hub = OutputHub::new();
        let message = |seq| PaneOutputMessage {
            id: "p1-chief".into(),
            generation: 1,
            seq,
            offset: None,
            bytes: vec![65],
        };
        let gone = Arc::new(Mutex::new(Vec::new()));
        let seen_by_gone = Arc::clone(&gone);
        hub.register_sink(Arc::new(move |message: PaneOutputMessage| {
            seen_by_gone.lock().unwrap().push(message.seq);
            message.seq == 1
        }));
        for seq in 1..=3 {
            hub.publish(message(seq));
        }
        let next = Arc::new(Mutex::new(Vec::new()));
        let seen_by_next = Arc::clone(&next);
        hub.register_sink(Arc::new(move |message: PaneOutputMessage| {
            seen_by_next.lock().unwrap().push(message.seq);
            true
        }));
        hub.publish(message(4));

        assert_eq!(
            *gone.lock().unwrap(),
            vec![1, 2],
            "a destination that had gone was asked again"
        );
        assert_eq!(*next.lock().unwrap(), vec![2, 3, 4]);
    }
}

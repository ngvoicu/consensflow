//! Where the panes' output goes: to one destination at a time, the window's
//! page or the headless helper's peer, and what a pane prints while there is
//! none waits for the next.

use std::collections::{HashMap, HashSet, VecDeque};
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
    /// The windows that ended while some of their output was still parked: their entry in
    /// `handed` goes once the last of it has been taken, not before and not after.
    finished: HashSet<PaneKey>,
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
                finished: HashSet::new(),
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
        Self::forget_finished(&mut state);
        handed
    }

    /// The entry of each window that ended and has none of its output left parked goes: what
    /// delivering its parked messages wrote there is no page's to settle.
    fn forget_finished(state: &mut OutputHubState) {
        let OutputHubState {
            pending,
            handed,
            finished,
            ..
        } = state;
        finished.retain(|key| {
            let parked = pending.iter().any(|message| message.taken().0 == *key);
            if !parked {
                handed.remove(key);
            }
            parked
        });
    }

    /// A window with no journal has ended and all it printed has been handed on or parked: no
    /// page needs what was handed of it (no history, and no room left to settle). One with a
    /// journal keeps its entry for as long as its journal is kept ([`OutputHub::retain_handed`]).
    pub(crate) fn forget(&self, key: &PaneKey) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.handed.remove(key);
        // Some of its output may still be parked, for a sink that is not there yet: delivering
        // it would write the entry again, so it is forgotten again once the last is taken.
        if state
            .pending
            .iter()
            .any(|message| message.taken().0 == *key)
        {
            state.finished.insert(key.clone());
        }
    }

    /// How many windows the hub keeps what it handed for.
    #[cfg(all(test, unix))]
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

    /// A window that ended before the first sink registered (its output parked, the hub told it
    /// had ended) leaves nothing behind when that sink takes the parked bytes: the helper's
    /// bridge reader starts before the sink is installed, so this is every start (T-2284's
    /// finding 12).
    #[cfg(unix)]
    #[test]
    fn windows_that_ended_before_the_first_sink_are_not_remembered_when_it_takes_their_output() {
        let hub = OutputHub::new();
        let message = |id: &str, seq| PaneOutputMessage {
            id: id.into(),
            generation: 1,
            seq,
            offset: None,
            bytes: vec![65],
        };
        for id in ["a", "b", "c"] {
            hub.publish(message(id, 1));
            hub.forget(&PaneKey::new(id, 1));
        }
        // One that is still running is kept, as ever.
        hub.publish(message("live", 1));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let copy = Arc::clone(&seen);
        hub.register_sink(Arc::new(move |message: PaneOutputMessage| {
            copy.lock().unwrap().push(message.id);
            true
        }));
        assert_eq!(
            seen.lock().unwrap().len(),
            4,
            "every parked byte is delivered"
        );
        assert_eq!(
            hub.handed_windows(),
            1,
            "only the running window is remembered"
        );
    }

    /// A sink that takes the parked output of a window that ended only in part leaves what it
    /// refused parked, and the window forgotten once the last of it has been taken.
    #[cfg(unix)]
    #[test]
    fn an_ended_window_is_forgotten_when_the_last_of_its_parked_output_is_taken() {
        let hub = OutputHub::new();
        let message = |seq| PaneOutputMessage {
            id: "gone".into(),
            generation: 1,
            seq,
            offset: None,
            bytes: vec![65],
        };
        hub.publish(message(1));
        hub.publish(message(2));
        hub.forget(&PaneKey::new("gone", 1));
        hub.register_sink(Arc::new(|message: PaneOutputMessage| message.seq == 1));
        assert_eq!(
            hub.handed_windows(),
            1,
            "its second message is still parked"
        );
        hub.register_sink(Arc::new(|_| true));
        assert_eq!(hub.handed_windows(), 0);
    }

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

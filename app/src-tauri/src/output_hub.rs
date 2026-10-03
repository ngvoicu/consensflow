//! Where the panes' output goes: to one destination at a time, the window's
//! page or the headless helper's peer, and what a pane prints while there is
//! none waits for the next.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::ipc::Channel;

use crate::pty::PaneOutput;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneOutputMessage {
    id: String,
    generation: u64,
    seq: u64,
    pub(crate) bytes: Vec<u8>,
}

impl From<PaneOutput> for PaneOutputMessage {
    fn from(output: PaneOutput) -> Self {
        Self {
            id: output.key.id,
            generation: output.key.generation,
            seq: output.seq,
            bytes: output.bytes,
        }
    }
}

struct OutputHubState {
    sink: Option<OutputSink>,
    pending: VecDeque<PaneOutputMessage>,
}

/// Where one pane's bytes go. `false` means the destination is gone, and the
/// hub parks what follows until a new one arrives. The sink runs under the
/// hub's lock and the headless one waits while its peer is busy, so publish
/// only from a pane's own output thread, never from a bridge handler.
pub(crate) type OutputSink = Arc<dyn Fn(PaneOutputMessage) -> bool + Send + Sync>;

pub(crate) struct OutputHub {
    state: Mutex<OutputHubState>,
}

impl OutputHub {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(OutputHubState {
                sink: None,
                pending: VecDeque::new(),
            }),
        }
    }

    /// The window's destination: a webview channel the page reads.
    pub(crate) fn register(&self, channel: Channel<PaneOutputMessage>) {
        self.attach(Arc::new(move |message| channel.send(message).is_ok()));
    }

    /// The headless destination: back over the bridge the request came in on.
    ///
    /// The hub exists so `register_pane_handlers` need not know which of the
    /// two it is feeding — that is what lets the window and the helper share
    /// one set of handlers instead of two that drift.
    pub(crate) fn register_sink(&self, sink: OutputSink) {
        self.attach(sink);
    }

    fn attach(&self, sink: OutputSink) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.sink = Some(sink);
        let pending = std::mem::take(&mut state.pending);
        for message in pending {
            Self::deliver_or_park(&mut state, message);
        }
    }

    fn deliver_or_park(state: &mut OutputHubState, message: PaneOutputMessage) {
        if state
            .sink
            .as_ref()
            .is_some_and(|sink| sink(message.clone()))
        {
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

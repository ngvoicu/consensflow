use std::sync::Arc;

use serde_json::{json, Value};

use super::*;
use crate::arbiter::{EnterTiming, InputArbiter};
use crate::journal::{Journal, Resize};
use crate::pty::PaneKey;

/// A frame's header, its seed and its bytes, as the page reads one.
fn read_frame(frame: &[u8]) -> (Value, Vec<u8>, Vec<u8>) {
    let length = u32::from_le_bytes(frame[..4].try_into().unwrap()) as usize;
    let header: Value = serde_json::from_slice(&frame[4..4 + length]).unwrap();
    let seed_end = 4 + length + header["seedLength"].as_u64().unwrap_or(0) as usize;
    (
        header,
        frame[4 + length..seed_end].to_vec(),
        frame[seed_end..].to_vec(),
    )
}

/// The frame the page reads (`historyFrame` in terminal-link.js, which the page suite holds to
/// these same bytes).
#[test]
fn a_piece_of_history_is_framed_as_the_page_reads_it() {
    let piece = Piece {
        reset: true,
        truncated: false,
        from: 5,
        size: (8, 30),
        seed: vec![0x1b, 0x5b],
        resizes: vec![Resize {
            at: 6,
            rows: 9,
            cols: 31,
        }],
        bytes: vec![b'h', b'i'],
        boundary: 7,
        done: true,
    };
    let (header, seed, bytes) = read_frame(&history_frame(&piece));
    // The same header, bytes and seed the page suite's frame holds.
    assert_eq!(
        header,
        json!({"ok":true,"reset":true,"truncated":false,"from":5,"size":{"rows":8,"cols":30},
               "seedLength":2,"resizes":[{"at":6,"rows":9,"cols":31}],"boundary":7,"done":true})
    );
    assert_eq!(seed, [0x1b, 0x5b]);
    assert_eq!(bytes, b"hi");
}

struct Host {
    panes: Arc<PaneTable>,
    inputs: InputQueue,
    hub: OutputHub,
}

fn host() -> Host {
    let panes = Arc::new(PaneTable::new());
    let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
    let inputs = InputQueue::new(Arc::clone(&panes), arbiter);
    Host {
        panes,
        inputs,
        hub: OutputHub::new(),
    }
}

impl Host {
    fn pulled(&self, page: &str, id: &str) -> Value {
        read_frame(&history(&self.inputs, &self.panes, page, id, 1, None)).0
    }

    fn subscribe(&self, page: &str) -> Value {
        subscribe(
            &self.inputs,
            &self.panes,
            &self.hub,
            page,
            Arc::new(|_| true),
        )
    }

    /// An ended window of `bytes` bytes, its journal kept.
    fn ended(&self, id: &str, bytes: usize) -> PaneKey {
        let key = PaneKey::new(id, 1);
        let journal = Arc::new(Journal::new((24, 80)));
        journal.append(&vec![b'a'; bytes]);
        journal.finish();
        self.panes.keep_ended_journal(key.clone(), journal);
        key
    }
}

/// A pull from a page that is not the current one is refused (a page that reloaded, or one a new
/// page is replacing), and one from the current page before it has subscribed too: it would be
/// answered against the boundary of the page before (T-2229 finding 1).
#[test]
fn a_pull_is_answered_only_to_the_page_that_subscribed() {
    let host = host();
    host.ended("w", 10);
    host.inputs.begin_page("old").unwrap();
    assert_eq!(host.subscribe("old"), json!({"ok":true}));
    host.inputs.begin_page("new").unwrap();
    assert_eq!(host.pulled("old", "w")["error"], "pane-input-stale-page");
    assert_eq!(host.pulled("new", "w")["error"], "pane-not-subscribed");
    assert_eq!(host.subscribe("new"), json!({"ok":true}));
    assert_eq!(host.pulled("new", "w")["ok"], true);
    assert_eq!(host.pulled("new", "x")["error"], "pane-not-found");
}

/// The host forgets an ended window's journal only where the page is the current one and its
/// emulator has taken all of it (T-2229 finding 3).
#[test]
fn an_ended_windows_history_is_forgotten_only_where_the_page_took_all_of_it() {
    let host = host();
    let key = host.ended("w", 10);
    host.inputs.begin_page("page").unwrap();
    host.subscribe("page");
    let done =
        |page: &str, consumed| history_done(&host.inputs, &host.panes, page, "w", 1, consumed);
    assert_eq!(done("page", 9), json!({"ok":false}));
    assert_eq!(done("gone", 10)["error"], "pane-input-stale-page");
    assert!(
        host.panes.history("page", &key, None).is_ok(),
        "what was refused is still there"
    );
    assert_eq!(done("page", 10), json!({"ok":true}));
    assert!(host.panes.history("page", &key, None).is_err());
}

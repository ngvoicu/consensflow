//! The headless pane host, and what the window's shares with it: the pane
//! handlers (here over stdin and stdout instead of a webview), the drain on the
//! way out, and the frame limit and paste timing both are built with.

use std::sync::Arc;

use cf_bridge::BridgeBuilder;
use cf_proto::bridge::Role;
use serde_json::json;

use crate::arbiter::{EnterTiming, InputArbiter};
use crate::input_queue::InputQueue;
use crate::output_hub::{OutputHub, PaneOutputMessage};
use crate::pane_handlers::register_pane_handlers;
use crate::pty::{PaneKey, PaneTable};

/// The largest frame the pane host's bridge carries; one over it is refused.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
/// A paste's Enter goes once the window has drawn it and then printed
/// nothing for 120 ms, never sooner than 10 ms, at 2 s whatever it drew. A
/// fixed 10 ms was a Mac's speed: through Windows' ConPTY the paste was still
/// going in when its Enter came, and Devin took the Enter into it; and 120 ms
/// of silence was not enough either, since Devin reads a long paste silently
/// before it draws it.
pub const ENTER: EnterTiming = EnterTiming {
    least_ms: 10,
    quiet_ms: 120,
    most_ms: 2_000,
};

/// Ends every pane the table still holds.
pub fn reap_all(panes: &PaneTable) {
    if let Ok(open) = panes.list() {
        for pane in open {
            let _ = panes.kill(&PaneKey::new(pane.id, pane.generation));
        }
    }
}

/// The headless pane helper, running the WINDOW's handlers.
///
/// `consensflow-bridge` used to carry its own copy of the pane operations, and
/// a copy is a contract that drifts: it had no pane id or generation on
/// `pane.open`, and it never reported a natural `pane.exit`. The real Node
/// side speaks to the window, so against the helper it could only be refused.
/// There is nothing to keep in step here: this is `register_pane_handlers`,
/// the same `InputQueue` and the same shutdown drain the window uses, over
/// stdin and stdout instead of a webview.
///
/// Serves until the peer closes the transport, then reaps what it opened.
// Its errors go to its stderr: the helper's own log, which the integration rig reads.
#[allow(clippy::print_stderr)]
pub fn run_headless() -> Result<(), String> {
    let panes = Arc::new(PaneTable::new());
    let output = Arc::new(OutputHub::new());
    let arbiter = Arc::new(InputArbiter::new(ENTER));
    let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));

    let mut builder = BridgeBuilder::new(Role::Host, MAX_FRAME_BYTES);
    register_pane_handlers(
        &mut builder,
        Arc::clone(&panes),
        Arc::clone(&arbiter),
        Arc::clone(&output),
        Arc::clone(&inputs),
    );
    builder.on_error(|error| eprintln!("consensflow-bridge: {error}"));

    let bridge = builder
        .serve(
            std::io::stdin(),
            std::io::stdout(),
            &json!({"v":1,"kind":"consensflow-bridge"}),
        )
        .map_err(|error| error.to_string())?;

    // No page to draw into, so a pane's bytes go back over the same bridge, as
    // a stream: a burst waits for the peer to read instead of closing the
    // bridge. Registered after `serve` on purpose: whatever a pane produced in
    // between is parked in the hub and drains into this sink the moment it
    // attaches.
    let sink = bridge.clone();
    output.register_sink(Arc::new(move |message: PaneOutputMessage| {
        sink.stream_event("pane.output", json!(message)).is_ok()
    }));

    // The same order the window shuts down in, and for the same reason: the
    // peer's EOF is what closes admission, so the drain can only run after it.
    bridge
        .wait_launches_closed()
        .map_err(|error| error.to_string())?;
    reap_all(&panes);
    inputs.close_and_drain();
    bridge.wait_closed().map_err(|error| error.to_string())
}

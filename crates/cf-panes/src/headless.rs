//! The headless pane host, and what the window's shares with it: the pane
//! handlers (here over stdin and stdout instead of a webview), the drain on the
//! way out, and the frame limit and paste timing both are built with. The headless
//! host tells the exits of the windows it ends as it stops; the window's quit does not.

use std::sync::Arc;
use std::time::Duration;

use cf_bridge::{Bridge, BridgeBuilder};
use cf_proto::bridge::Role;
use serde_json::json;

use crate::arbiter::{EnterTiming, InputArbiter};
use crate::input_queue::InputQueue;
use crate::output_hub::{OutputHub, PaneOutputMessage};
use crate::pane_handlers::{register_pane_handlers, register_typed};
use crate::pty::{PaneError, PaneKey, PaneTable};

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

/// How long a host that has ended every window waits for their exits to be told, before
/// it closes its output. An exit is told a moment after the close that ended its window:
/// when the window's output has ended (the terminal's last bytes are read for a second at
/// most) and its program's status is read (a second at most), which is a fraction of a
/// second when the program has gone. The bound is those two and a second more. A rig
/// gives a host five seconds to stop, and a close that looks at what a window left runs
/// two seconds of that; an exit not told by then is not told (the host is gone), for
/// what holds it back (a program that cannot be proven gone, whose proof is waited for up
/// to thirty seconds; a peer that reads nothing) will not end within a stop's time.
pub const FAREWELL: Duration = Duration::from_secs(3);

/// How long what the host told is given to be written, once it has nothing more to say: a
/// peer that reads, or is gone, takes it at once; one that holds its pipe and reads nothing
/// is not waited for, and the output is cut off (on Windows, where a blocked write cannot be
/// cut off, the host ends without it).
const FLUSH: Duration = Duration::from_secs(2);

/// Ends every pane the table still holds, all at once, each by `close`, and a window
/// whose programs ignore the request does not make the others wait for it.
fn close_all(panes: &PaneTable, close: fn(&PaneTable, &PaneKey) -> Result<(), PaneError>) {
    if let Ok(open) = panes.list() {
        std::thread::scope(|scope| {
            for pane in open {
                scope.spawn(move || {
                    let _ = close(panes, &PaneKey::new(pane.id, pane.generation));
                });
            }
        });
    }
}

/// Ends every pane the table still holds, all at once: each is closed as
/// `PaneTable::kill` closes one (asked to end and given its grace; on Windows
/// ended at once). A quit's: no one is told what a window left running, and its
/// tree is not looked at, so the quit does not wait for the second look.
pub fn reap_all(panes: &PaneTable) {
    close_all(panes, PaneTable::reap);
}

/// [`reap_all`] for a stop that has a peer to hear it: each window is closed as the
/// daemon's `pane.kill` closes one, with a look at what it leaves running, which its
/// exit (`pane.exit`) names. A window with work in it makes the stop wait for the
/// second look (`SETTLE`); the windows are looked at together.
pub fn end_all(panes: &PaneTable) {
    close_all(panes, PaneTable::kill);
}

/// The pane host's end once its peer has ended its input: the launches admitted before
/// are waited for, every window is ended, and the exit of each (`pane.exit`: how it
/// ended, what its screen showed, what it left running) is told before the host closes
/// its own output, [`FAREWELL`] at most; then the input queue is drained and the output
/// is ended, after what was told is written. The peer reads it to the output's end: the
/// rig's last word is every exit the host told. (The window's quit tells no one, for the
/// daemon is gone before it reaps: `reap_all`.)
///
/// # Errors
///
/// The bridge failed in a way that left it unable to say when it closed.
pub fn shut_down(bridge: &Bridge, panes: &PaneTable, inputs: &InputQueue) -> Result<(), String> {
    bridge
        .wait_launches_closed()
        .map_err(|error| error.to_string())?;
    end_all(panes);
    let _ = panes.exits_told_within(FAREWELL);
    inputs.close_and_drain();
    if !bridge.end_output(FLUSH) {
        // A peer that reads nothing leaves the writer blocked in a write that only Unix can
        // interrupt: the host's own end ends it, and nothing is left to wait for.
        return Ok(());
    }
    bridge.wait_closed().map_err(|error| error.to_string())
}

/// The variable that has the headless host take the human's keys (`pane.typed`)
/// where it is `1`: the rig sets it, to type into a window as the app's page does.
/// The bundled host is started without it, and takes no keys as the human's.
pub const HUMAN_KEYS: &str = "CONSENSFLOW_TEST_HUMAN_KEYS";

/// Whether the host takes the human's keys, [`HUMAN_KEYS`] being `value`.
fn takes_human_keys(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|value| value == "1")
}

/// The headless pane helper, running the WINDOW's handlers.
///
/// `consensflow-bridge` used to carry its own copy of the pane operations, and
/// a copy is a contract that drifts: it had no pane id or generation on
/// `pane.open`, and it never reported a natural `pane.exit`. The real Node
/// side speaks to the window, so against the helper it could only be refused.
/// There is nothing to keep in step here: this is `register_pane_handlers`,
/// the same `InputQueue` and the same drain the window uses, over stdin and
/// stdout instead of a webview.
///
/// Serves until the peer closes the transport, then ends what it opened and tells how
/// each ended ([`shut_down`]). `human_keys` is what [`HUMAN_KEYS`] is in the
/// environment, if anything.
// Its errors go to its stderr: the helper's own log, which the integration rig reads.
#[allow(clippy::print_stderr)]
pub fn run_headless(human_keys: Option<&std::ffi::OsStr>) -> Result<(), String> {
    let panes = Arc::new(PaneTable::new());
    let output = Arc::new(OutputHub::new());
    let arbiter = Arc::new(InputArbiter::new(ENTER));
    let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));

    let mut builder = BridgeBuilder::new(Role::Host, MAX_FRAME_BYTES);
    // The peer ends the input to stop the host, which has the exits of its windows to tell.
    builder.keep_output_after_eof();
    register_pane_handlers(
        &mut builder,
        Arc::clone(&panes),
        Arc::clone(&arbiter),
        Arc::clone(&output),
        Arc::clone(&inputs),
    );
    if takes_human_keys(human_keys) {
        register_typed(&mut builder, Arc::clone(&inputs));
    }
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
    shut_down(&bridge, &panes, &inputs)
}

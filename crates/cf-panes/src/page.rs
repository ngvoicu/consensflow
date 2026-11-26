//! What the page asks of the pane host about its terminals, as the app's commands answer it: its
//! subscription to the output, the pulls of a window's history and their end, its
//! acknowledgements and the sizes its emulators take. The app's commands only call these (and
//! the gate's host does, `examples/gate_bridge.rs`), so that what is checked is what runs.
//!
//! Each answer is the JSON the page reads; [`history`]'s is raw bytes: four bytes (little endian)
//! for the length of a header of JSON, the header, the seed's bytes, then the journal's. A
//! refusal is a header alone with `ok` false. A page that is not the current one is refused
//! `pane-input-stale-page`, one that has not subscribed `pane-not-subscribed`.

use serde_json::{json, Value};

use crate::input_queue::InputQueue;
use crate::journal::Piece;
use crate::output_hub::{OutputHub, OutputSink};
use crate::pty::{PaneError, PaneTable};
use crate::validation::{pane_key, validate_seq, validate_size};

fn ok() -> Value {
    json!({"ok":true})
}

fn refused(error: &str) -> Value {
    json!({"ok":false,"error":error})
}

/// The page `page_id` subscribes to the panes' output ([`PaneTable::attach_page`]), under its
/// page's identity, so a late subscription of a page that has gone replaces nothing.
pub fn subscribe(
    inputs: &InputQueue,
    panes: &PaneTable,
    hub: &OutputHub,
    page_id: &str,
    sink: OutputSink,
) -> Value {
    match inputs.with_current_page(page_id, || panes.attach_page(page_id, hub, sink)) {
        Ok(_) => ok(),
        Err(error) => refused(&error),
    }
}

/// One piece of a window's history for the page `page_id`, from `from` (none: the page has
/// nothing): at most 1 MiB, never past the boundary the page's subscription fixed
/// ([`PaneTable::history`]).
pub fn history(
    inputs: &InputQueue,
    panes: &PaneTable,
    page_id: &str,
    id: &str,
    generation: u64,
    from: Option<u64>,
) -> Vec<u8> {
    let key = match pane_key(id, generation) {
        Ok(key) => key,
        Err(error) => return history_refusal(&error),
    };
    // Under the page's identity: a page that is not the current one, or that a new one is
    // replacing, is answered no piece.
    match inputs.with_current_page(page_id, || panes.history(page_id, &key, from)) {
        Err(error) => history_refusal(&error),
        Ok(Err(PaneError::NotFound(_))) => history_refusal("pane-not-found"),
        Ok(Err(error)) => history_refusal(&error.to_string()),
        Ok(Ok(piece)) => history_frame(&piece),
    }
}

/// The page's emulator has taken `consumed` bytes of a window's output, the last piece of its
/// history written: the journal of an ended window is forgotten where that is its end, and only
/// for the current page. Otherwise it is kept, for whoever asks next.
pub fn history_done(
    inputs: &InputQueue,
    panes: &PaneTable,
    page_id: &str,
    id: &str,
    generation: u64,
    consumed: u64,
) -> Value {
    let key = match pane_key(id, generation) {
        Ok(key) => key,
        Err(error) => return refused(&error),
    };
    match inputs.with_current_page(page_id, || panes.forget_consumed_journal(&key, consumed)) {
        Ok(true) => ok(),
        Ok(false) => json!({"ok":false}),
        Err(error) => refused(&error),
    }
}

/// The page has drawn (or let go of) a window's output up to the message numbered `seq`.
pub fn ack(panes: &PaneTable, id: &str, generation: u64, seq: u64) -> Value {
    if let Err(error) = validate_seq(seq) {
        return refused(&error);
    }
    let key = match pane_key(id, generation) {
        Ok(key) => key,
        Err(error) => return refused(&error),
    };
    match panes.ack(&key, seq) {
        Ok(()) => ok(),
        // `gone`: the window is not there to acknowledge; any other refusal is tried again.
        Err(error) => json!({
            "ok":false,
            "error":error.to_string(),
            "gone":matches!(error, PaneError::NotFound(_) | PaneError::NoOutputStream(_)),
        }),
    }
}

/// A window takes the size `cols` by `rows`, which the page's emulator took once it had parsed
/// `at` bytes of the window's output (`None`: the journal's end). Only the current page's size:
/// one from a page that has gone, or that names none, is refused `pane-input-stale-page` and the
/// terminal keeps its size (that page's emulator is gone; the page after rebuilds at the journal's
/// last size, and sends its own where its layout differs). Taken, it would leave the terminal at a
/// size no emulator has. Held under the page's identity, so that no page replaces this one
/// between the check and the size.
pub fn resize(
    inputs: &InputQueue,
    panes: &PaneTable,
    page_id: Option<&str>,
    id: &str,
    generation: u64,
    size: (u16, u16),
    at: Option<u64>,
) -> Value {
    let (cols, rows) = size;
    if let Err(error) = validate_size(cols, rows) {
        return refused(&error);
    }
    let key = match pane_key(id, generation) {
        Ok(key) => key,
        Err(error) => return refused(&error),
    };
    let Some(page) = page_id else {
        return refused(crate::input_queue::INPUT_STALE_PAGE);
    };
    // The page's lock is held to read who the page is and no longer: the window's terminal may
    // be slow to resize, and nothing but that window's next size waits on it. A page that is
    // replaced meanwhile finds its size undone when it is recorded (`resize_while`).
    let epoch = match inputs.current_page_epoch(page) {
        Ok(epoch) => epoch,
        Err(error) => return refused(&error),
    };
    match panes.resize_while(&key, rows, cols, at, &|| inputs.page_unchanged(epoch)) {
        Ok(()) => ok(),
        Err(error) => refused(&error.to_string()),
    }
}

/// `header` as JSON, framed with `seed` and `bytes` ([`history`]).
fn history_frame_of(header: &Value, seed: &[u8], bytes: &[u8]) -> Vec<u8> {
    let header = header.to_string();
    let mut frame = Vec::with_capacity(4 + header.len() + seed.len() + bytes.len());
    frame.extend_from_slice(&(header.len() as u32).to_le_bytes());
    frame.extend_from_slice(header.as_bytes());
    frame.extend_from_slice(seed);
    frame.extend_from_slice(bytes);
    frame
}

/// The frame of a refusal: what a command that panicked answers too.
pub fn history_refusal(error: &str) -> Vec<u8> {
    history_frame_of(&refused(error), &[], &[])
}

fn history_frame(piece: &Piece) -> Vec<u8> {
    let header = json!({
        "ok": true,
        "reset": piece.reset,
        "truncated": piece.truncated,
        "from": piece.from,
        "size": {"rows": piece.size.0, "cols": piece.size.1},
        "seedLength": piece.seed.len(),
        "resizes": piece.resizes.iter()
            .map(|size| json!({"at": size.at, "rows": size.rows, "cols": size.cols}))
            .collect::<Vec<_>>(),
        "boundary": piece.boundary,
        "done": piece.done,
    });
    history_frame_of(&header, &piece.seed, &piece.bytes)
}

#[cfg(test)]
mod tests;

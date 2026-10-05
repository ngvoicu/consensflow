//! The input: read, split into lines, and dispatched one frame at a time, in
//! the order the peer wrote them. Dispatching is synchronous and never waits
//! for a handler, so a frame is dealt with before the next is read. The
//! frames of one read are handled together, and `after_read` is told when
//! they all are.

use std::rc::Rc;

use cf_proto::bridge::{too_large_body, Frame};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt};

use super::lines::{Line, Lines};
use super::state::Inner;
use super::Bridge;
use crate::BridgeError;

/// How much the reader asks the input for at once.
const READ_BYTES: usize = 64 * 1024;

/// What is told of a line or a frame that is over the limit.
const OVER_THE_LIMIT: &str = "bridge frame over maxFrameBytes";

/// How much of a malformed frame its report quotes.
const QUOTED_CHARS: usize = 80;

/// Closes the bridge when the reader ends, whatever ended it: a reader that
/// stopped must not leave a bridge that looks alive and hears nothing.
struct ReaderEnded(Rc<Inner>);

impl Drop for ReaderEnded {
    fn drop(&mut self) {
        self.0.eof();
    }
}

/// Reads `input` until it ends, the bridge closes, or reading fails.
pub(super) async fn read_loop<R: AsyncRead + Unpin>(inner: Rc<Inner>, mut input: R) {
    let bridge = Bridge {
        inner: Rc::clone(&inner),
    };
    let _ended = ReaderEnded(inner);
    let mut lines = Lines::new(bridge.inner.max_frame_bytes);
    let mut chunk = vec![0; READ_BYTES];
    loop {
        let read = tokio::select! {
            biased;
            () = bridge.inner.stop.notified() => return,
            read = input.read(&mut chunk) => read,
        };
        match read {
            Ok(0) => {
                bridge.inner.eof();
                return;
            }
            Err(error) => {
                bridge.inner.fail(error.into());
                return;
            }
            Ok(count) => lines.push(&chunk[..count]),
        }
        let closed = bridge.dispatch_lines(&mut lines);
        bridge.inner.read_handled();
        if closed {
            return;
        }
    }
}

impl Bridge {
    /// Dispatches each whole line `lines` holds, in order: whether the bridge
    /// closed meanwhile, which leaves the lines after it unread.
    fn dispatch_lines(&self, lines: &mut Lines) -> bool {
        while let Some(line) = lines.next_line() {
            match line {
                Line::Complete(bytes) => self.dispatch_bytes(&bytes),
                Line::Overflow => self.report_over_the_limit(),
            }
            if self.inner.is_closed() {
                return true;
            }
        }
        false
    }

    fn report_over_the_limit(&self) {
        self.inner
            .report(BridgeError::MalformedFrame(OVER_THE_LIMIT.to_owned()));
    }

    /// One complete line. Text is read as Node read it: bytes that are not
    /// UTF-8 become U+FFFD, which may make the line longer.
    fn dispatch_bytes(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let text = String::from_utf8_lossy(bytes);
        if bytes.len() > self.inner.max_frame_bytes || text.len() > self.inner.max_frame_bytes {
            self.refuse_oversized(&text);
            return;
        }
        let frame = match serde_json::from_str::<Frame>(&text) {
            Ok(frame) if frame.is_well_formed() => frame,
            Ok(_) => {
                let quoted = text.chars().take(QUOTED_CHARS).collect();
                self.inner.report(BridgeError::MalformedFrame(quoted));
                return;
            }
            Err(error) => {
                self.inner
                    .report(BridgeError::MalformedFrame(error.to_string()));
                return;
            }
        };
        if !self.inner.accepts(&frame) {
            self.inner.report(BridgeError::MalformedFrame(format!(
                "id {} has the wrong namespace for {}",
                frame.id, frame.kind
            )));
            return;
        }
        match frame.kind.as_str() {
            "res" => self.inner.settle(&frame.id, &frame.op, frame.body),
            "evt" => self.emit(&frame.op, &frame.body),
            _ => self.serve(frame),
        }
    }

    /// A line that does not fit is refused without being trusted: only a
    /// frame that is well formed all the way gets an answer, a request as
    /// `too-large` to the peer, a response as `too-large` to the request
    /// waiting for it. Anything else is reported and dropped.
    fn refuse_oversized(&self, text: &str) {
        let frame = serde_json::from_str::<Frame>(text)
            .ok()
            .filter(|frame| frame.is_well_formed() && self.inner.accepts(frame));
        match frame {
            Some(frame) if frame.kind == "req" => {
                self.respond(frame.id, frame.op, too_large_body());
            }
            Some(frame) if frame.kind == "res" => {
                self.inner.settle(&frame.id, &frame.op, too_large_body());
            }
            _ => self.report_over_the_limit(),
        }
    }

    /// Calls the handlers of an event, in place and one after another.
    fn emit(&self, op: &str, body: &Value) {
        for handler in self.inner.registry.events(op) {
            handler(body);
        }
    }
}

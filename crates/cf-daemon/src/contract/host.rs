//! The app's end of the bridge as bytes. The test is the pane host: it writes
//! the lines the host would, in the reads it chooses (what is written
//! together is one read, up to the 64 KiB the reader asks for at once), and
//! reads the lines the daemon wrote. It answers what the daemon asks as the
//! real host does, by the request's id, but for what the test holds.

use cf_proto::bridge::{Frame, PROTOCOL_VERSION};
use futures_util::FutureExt;
use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::settle;

/// How much the bridge's reader asks its input for at once.
pub const READ_BYTES: usize = 64 * 1024;

/// The pane host, at the other end of the pipes the daemon's bridge reads and
/// writes.
pub struct Host {
    to_daemon: Box<dyn AsyncWrite + Unpin>,
    from_daemon: Box<dyn AsyncRead + Unpin>,
    /// What the daemon wrote that is not a whole line yet.
    unread: Vec<u8>,
    /// The ids of the events the host sends: `r-1`, `r-2`...
    sent: u64,
}

impl Host {
    /// The host that reads what the daemon writes from `from_daemon` and
    /// writes what it says to `to_daemon`.
    pub fn new(
        from_daemon: impl AsyncRead + Unpin + 'static,
        to_daemon: impl AsyncWrite + Unpin + 'static,
    ) -> Self {
        Self {
            to_daemon: Box::new(to_daemon),
            from_daemon: Box::new(from_daemon),
            unread: Vec::new(),
            sent: 0,
        }
    }

    /// Writes `bytes` as they are: one read of the daemon's, or the start of
    /// the next one, whatever the reader finds there when it is polled.
    pub async fn write(&mut self, bytes: &[u8]) {
        self.to_daemon
            .write_all(bytes)
            .await
            .expect("the daemon's input takes what the host writes");
    }

    /// The frames the daemon wrote since the last call, once what is ready
    /// has run.
    pub async fn frames(&mut self) -> Vec<Frame> {
        settle().await;
        let mut chunk = vec![0; READ_BYTES];
        while let Some(read) = self.from_daemon.read(&mut chunk).now_or_never() {
            match read {
                Ok(0) | Err(_) => break,
                Ok(count) => self.unread.extend_from_slice(&chunk[..count]),
            }
        }
        let mut frames = Vec::new();
        while let Some(end) = self.unread.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.unread.drain(..=end).collect();
            frames.push(serde_json::from_slice(&line).expect("the daemon wrote a frame"));
        }
        frames
    }

    /// What the daemon asked since the last call, answered as the host
    /// answers: all of it, but the requests `held` picks, which are given
    /// back unanswered.
    pub async fn serve(&mut self, held: impl Fn(&Frame) -> bool) -> Vec<Frame> {
        let mut kept = Vec::new();
        for frame in self.frames().await {
            if frame.kind != "req" {
                continue;
            }
            if held(&frame) {
                kept.push(frame);
            } else {
                let line = answer(&frame);
                self.write(&line).await;
            }
        }
        kept
    }

    /// A `pane.exit` event, as a line: the host's own ids are `r-` and count.
    pub fn exit(&mut self, pane: &Value) -> Vec<u8> {
        self.sent += 1;
        line(&Frame {
            v: PROTOCOL_VERSION,
            id: format!("r-{}", self.sent),
            kind: "evt".to_owned(),
            op: "pane.exit".to_owned(),
            body: pane.clone(),
        })
    }
}

/// A frame as one line, newline last.
pub fn line(frame: &Frame) -> Vec<u8> {
    let mut line = serde_json::to_vec(frame).expect("a frame is JSON");
    line.push(b'\n');
    line
}

/// The answer of a host that does what it is asked to `request`, as a line.
pub fn answer(request: &Frame) -> Vec<u8> {
    let body = match request.op.as_str() {
        "pane.open" => json!({
            "ok": true,
            "id": request.body["id"],
            "generation": request.body["generation"],
        }),
        _ => json!({ "ok": true }),
    };
    answer_with(request, body)
}

/// The answer to `request` with `body`, as a line.
pub fn answer_with(request: &Frame, body: Value) -> Vec<u8> {
    line(&Frame {
        v: PROTOCOL_VERSION,
        id: request.id.clone(),
        kind: "res".to_owned(),
        op: request.op.clone(),
        body,
    })
}

/// The pane a `pane.open` request names, as an exit says it.
pub fn pane_of(open: &Frame) -> Value {
    json!({ "id": open.body["id"], "generation": open.body["generation"] })
}

/// An answer for a request that was never made, padded to `bytes` bytes as a
/// line: the daemon drops it, having nothing waiting for it, and it takes up
/// the room of a read. The padding of what comes before an exit that is to be
/// in a read of its own.
pub fn padding(bytes: usize) -> Vec<u8> {
    let padded = |room: usize| {
        line(&Frame {
            v: PROTOCOL_VERSION,
            id: "n-0".to_owned(),
            kind: "res".to_owned(),
            op: "pane.padding".to_owned(),
            body: json!({ "padding": "x".repeat(room) }),
        })
    };
    let room = bytes
        .checked_sub(padded(0).len())
        .expect("the padding is room for its own frame");
    padded(room)
}

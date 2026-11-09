//! The daemon's input, read on a thread of its own. The daemon runs on one
//! thread, and a read that waits for that thread to poll it leaves the pipe
//! unread while the thread is busy: a pass that holds the thread for seconds (a
//! ledger write waiting on the disk of a loaded Mac) leaves the pane host's
//! writes in a pipe that holds 64 KiB, and the host's writer waits behind it.
//! Here the bytes are taken off the pipe as they come, whatever the thread is
//! doing, and wait in order, in memory, until it reads them: the host's
//! frames are not held up by the daemon's busy thread, and the daemon's reader
//! sees exactly what it saw when it read the pipe itself.

use std::io::{self, Read};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::thread;

use tokio::io::{AsyncRead, ReadBuf};
use tokio::sync::mpsc;

/// How much the thread asks the pipe for at once: what the reader asks its
/// input for ([`super::reader`]).
const CHUNK_BYTES: usize = 64 * 1024;

/// How many chunks wait for the reader before the thread stops taking more off
/// the pipe: 16 MiB at the most, and a chunk holds what one read of the pipe
/// gave, far less when the host writes frames of a few hundred bytes. Past it
/// the pipe fills again and the host's writer waits, as it does for any busy
/// reader, and nothing is lost or closed: the bound is the daemon's memory
/// against a peer that writes without end, not a limit on the peer.
pub const READ_AHEAD_CHUNKS: usize = 256;

/// What the thread read: a chunk, or the way the read failed. The end of the
/// input is the channel's.
type Chunk = io::Result<Vec<u8>>;

/// An input that is read ahead on its own thread ([`read_ahead`]).
pub struct ReadAhead {
    chunks: mpsc::Receiver<Chunk>,
    /// The chunk the reader is partway through, and where it is in it.
    pending: Vec<u8>,
    at: usize,
}

/// `reader` read on a thread of its own, from now on, and given to the
/// bridge as an `AsyncRead`. The thread ends with the input, or when the
/// `ReadAhead` is dropped (at the next chunk: a read that waits for a pipe
/// cannot be cancelled, and the process that stopped has ended it).
pub fn read_ahead<R: Read + Send + 'static>(reader: R) -> ReadAhead {
    let (chunks, taken) = mpsc::channel(READ_AHEAD_CHUNKS);
    let started = thread::Builder::new()
        .name("consensflow-daemon-input".to_owned())
        .spawn(move || take(reader, &chunks));
    if let Err(error) = started {
        // No thread to read on: the input fails, which fails the bridge, which
        // stops the daemon with the reason in its log.
        let (failed, taken) = mpsc::channel(1);
        let _ = failed.try_send(Err(error));
        return ReadAhead {
            chunks: taken,
            pending: Vec::new(),
            at: 0,
        };
    }
    ReadAhead {
        chunks: taken,
        pending: Vec::new(),
        at: 0,
    }
}

/// Reads `reader` to its end, a chunk at a time, in order.
fn take<R: Read>(mut reader: R, chunks: &mpsc::Sender<Chunk>) {
    let mut buffer = vec![0; CHUNK_BYTES];
    loop {
        let sent = match reader.read(&mut buffer) {
            Ok(0) => return,
            Ok(count) => chunks.blocking_send(Ok(buffer[..count].to_vec())),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                let _ = chunks.blocking_send(Err(error));
                return;
            }
        };
        if sent.is_err() {
            // Nobody reads any more.
            return;
        }
    }
}

impl AsyncRead for ReadAhead {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.at == self.pending.len() {
            match self.chunks.poll_recv(context) {
                Poll::Pending => return Poll::Pending,
                // The end of the input: nothing is added to the buffer.
                Poll::Ready(None) => return Poll::Ready(Ok(())),
                Poll::Ready(Some(Err(error))) => return Poll::Ready(Err(error)),
                Poll::Ready(Some(Ok(chunk))) => {
                    self.pending = chunk;
                    self.at = 0;
                }
            }
        }
        let this = &mut *self;
        let count = buffer.remaining().min(this.pending.len() - this.at);
        buffer.put_slice(&this.pending[this.at..this.at + count]);
        this.at += count;
        Poll::Ready(Ok(()))
    }
}

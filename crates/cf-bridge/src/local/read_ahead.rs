//! The daemon's input, read on a thread of its own. The daemon runs on one
//! thread, and a read that waits for that thread to poll it leaves the pipe
//! unread while the thread is busy: a pass that holds the thread for seconds (a
//! ledger write waiting on the disk of a loaded Mac) leaves the pane host's
//! writes in a pipe that holds 64 KiB (4 KiB on Windows), and the host's writer
//! waits behind it. Here the bytes are taken off the pipe as they come, whatever
//! the thread is doing, and wait in order, in memory, until it reads them: the
//! host's frames are not held up by the daemon's busy thread, and the daemon's
//! reader sees exactly what it saw when it read the pipe itself.

use std::io::{self, Read};
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::task::{Context, Poll};
use std::thread;
use std::time::Duration;

use tokio::io::{AsyncRead, ReadBuf};
use tokio::sync::mpsc;

/// How much the thread asks the pipe for at once: what the reader asks its
/// input for ([`super::reader`]).
const CHUNK_BYTES: usize = 64 * 1024;

/// How many bytes wait for the reader before the thread stops taking more off
/// the pipe: 16 MiB, counted in bytes and not in reads, since a pipe gives what
/// was written as it was written (a Windows pipe gives a frame at a time, a few
/// dozen bytes of it). Past it the pipe fills again and the host's writer
/// waits, as it does for any busy reader, and nothing is lost or closed: the
/// bound is the daemon's memory against a peer that writes without end, not a
/// limit on the peer.
pub const READ_AHEAD_BYTES: usize = 16 * 1024 * 1024;

/// How often the thread looks whether anyone still reads, while it waits for room.
const LOOK: Duration = Duration::from_millis(50);

/// What the thread read: a chunk, or the way the read failed. The end of the
/// input is the channel's.
type Chunk = io::Result<Vec<u8>>;

/// The bytes that wait, and what tells the thread there is room again.
#[derive(Default)]
struct Waiting {
    bytes: Mutex<usize>,
    room: Condvar,
}

/// An input that is read ahead on its own thread ([`read_ahead`]).
pub struct ReadAhead {
    chunks: mpsc::UnboundedReceiver<Chunk>,
    waiting: Arc<Waiting>,
    /// The chunk the reader is partway through, and where it is in it.
    pending: Vec<u8>,
    at: usize,
}

/// `reader` read on a thread of its own, from now on, and given to the
/// bridge as an `AsyncRead`. The thread ends with the input, or when the
/// `ReadAhead` is dropped (at the next chunk: a read that waits for a pipe
/// cannot be cancelled, and the process that stopped has ended it).
pub fn read_ahead<R: Read + Send + 'static>(reader: R) -> ReadAhead {
    let (sender, chunks) = mpsc::unbounded_channel();
    let waiting = Arc::new(Waiting::default());
    let held = Arc::clone(&waiting);
    let started = thread::Builder::new()
        .name("consensflow-daemon-input".to_owned())
        .spawn(move || take(reader, &sender, &held));
    if let Err(error) = started {
        // No thread to read on: the input fails, which fails the bridge, which
        // stops the daemon with the reason in its log.
        let (failed, chunks) = mpsc::unbounded_channel();
        let _ = failed.send(Err(error));
        return ReadAhead {
            chunks,
            waiting,
            pending: Vec::new(),
            at: 0,
        };
    }
    ReadAhead {
        chunks,
        waiting,
        pending: Vec::new(),
        at: 0,
    }
}

/// Reads `reader` to its end, a chunk at a time, in order.
fn take<R: Read>(mut reader: R, chunks: &mpsc::UnboundedSender<Chunk>, waiting: &Waiting) {
    let mut buffer = vec![0; CHUNK_BYTES];
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) => return,
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                let _ = chunks.send(Err(error));
                return;
            }
        };
        // Room for it first: what waits is never more than the bound and this chunk.
        let mut queued = waiting.bytes.lock().unwrap_or_else(PoisonError::into_inner);
        while *queued >= READ_AHEAD_BYTES {
            if chunks.is_closed() {
                return;
            }
            queued = waiting
                .room
                .wait_timeout(queued, LOOK)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        *queued += count;
        drop(queued);
        if chunks.send(Ok(buffer[..count].to_vec())).is_err() {
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
                    {
                        let mut queued = self
                            .waiting
                            .bytes
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner);
                        *queued = queued.saturating_sub(chunk.len());
                    }
                    self.waiting.room.notify_one();
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

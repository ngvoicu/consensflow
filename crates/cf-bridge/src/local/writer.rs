//! The output: the frames queued for it, written one whole line at a time in
//! the order they were queued, and nothing else.

use std::rc::Weak;

use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;

use super::state::Inner;

/// Writes what is queued until the queue is closed, then ends the output so
/// the peer sees it end. A failed write fails the bridge.
pub(super) async fn write_loop<W: AsyncWrite + Unpin>(
    bridge: Weak<Inner>,
    mut output: W,
    mut queue: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    while let Some(line) = queue.recv().await {
        let written = async {
            output.write_all(&line).await?;
            output.flush().await
        };
        if let Err(error) = written.await {
            if let Some(inner) = bridge.upgrade() {
                inner.fail(error.into());
            }
            return;
        }
    }
    let _ = output.shutdown().await;
}

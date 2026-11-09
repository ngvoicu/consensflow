//! The input read ahead on its own thread: what it takes off the pipe while
//! nobody reads, how much it keeps, and how it ends.

use std::io::{self, Read};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncReadExt;

use crate::local::{read_ahead, READ_AHEAD_CHUNKS};

/// An input that never ends, and counts what it was asked for.
struct Endless(Arc<AtomicUsize>);

impl Read for Endless {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.0.fetch_add(1, Ordering::SeqCst);
        bytes.fill(b'x');
        Ok(bytes.len())
    }
}

#[tokio::test]
async fn it_stops_taking_bytes_at_its_bound_and_goes_on_as_they_are_read() {
    let reads = Arc::new(AtomicUsize::new(0));
    let mut input = read_ahead(Endless(Arc::clone(&reads)));
    tokio::time::sleep(Duration::from_millis(300)).await;
    let at_rest = reads.load(Ordering::SeqCst);
    assert!(
        at_rest <= READ_AHEAD_CHUNKS + 2,
        "{at_rest} chunks were taken with nobody reading"
    );
    assert!(at_rest >= READ_AHEAD_CHUNKS, "{at_rest} chunks were taken");

    let mut chunk = vec![0; 64 * 1024];
    for _ in 0..10 {
        input.read_exact(&mut chunk).await.expect("a chunk");
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let after = reads.load(Ordering::SeqCst);
    assert!(
        after > at_rest,
        "reading what waited did not make room for more"
    );
}

#[tokio::test]
async fn it_gives_the_bytes_in_order_across_chunks_and_ends_with_its_input() {
    let (reader, mut writer) = std::io::pipe().expect("a pipe");
    let mut input = read_ahead(reader);
    let said = std::thread::spawn(move || {
        use std::io::Write;
        for n in 0..1000 {
            writeln!(writer, "line {n}").expect("a line");
        }
    });
    said.join().expect("the writer");
    let mut all = String::new();
    input.read_to_string(&mut all).await.expect("the input");
    let want: String = (0..1000).map(|n| format!("line {n}\n")).collect();
    assert_eq!(all, want);
}

#[tokio::test]
async fn a_failed_read_is_told_once_and_then_the_input_has_ended() {
    struct Fails(bool);
    impl Read for Fails {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            if std::mem::replace(&mut self.0, true) {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "gone"));
            }
            bytes[..3].copy_from_slice(b"abc");
            Ok(3)
        }
    }
    let mut input = read_ahead(Fails(false));
    let mut first = [0; 3];
    input.read_exact(&mut first).await.expect("what came first");
    assert_eq!(&first, b"abc");
    let error = input.read_u8().await.expect_err("the failure");
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert_eq!(input.read(&mut [0; 4]).await.expect("then its end"), 0);
}

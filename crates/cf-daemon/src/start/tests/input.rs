//! The daemon's standard input as the process makes it: taken off its pipe as
//! it comes, whatever the daemon's one thread is doing, so the pane host's
//! frames are never held up behind a pass.

use std::io::{pipe, Write};
use std::time::{Duration, Instant};

use tokio::io::AsyncReadExt;

use super::*;

#[tokio::test]
async fn the_process_input_is_taken_off_its_pipe_while_nothing_reads_it() {
    let (reader, mut writer) = pipe().expect("a pipe");
    let mut options =
        Options::process_reading(Box::new(|_| Ok(())), reader).expect("the process's options");
    // A mebibyte, sixteen pipes' worth: an input read only when the daemon's
    // thread polls it leaves the writer waiting at the first 64 KiB.
    let wrote = std::thread::spawn(move || {
        let chunk = vec![b'x'; 64 * 1024];
        for _ in 0..16 {
            writer.write_all(&chunk).expect("a chunk");
        }
    });
    let until = Instant::now() + Duration::from_secs(5);
    while !wrote.is_finished() && Instant::now() < until {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        wrote.is_finished(),
        "the host's writes waited for the daemon to read its input"
    );
    wrote.join().expect("the writer");

    let mut all = Vec::new();
    options
        .input
        .read_to_end(&mut all)
        .await
        .expect("what was written");
    assert_eq!(all.len(), 16 * 64 * 1024);
}

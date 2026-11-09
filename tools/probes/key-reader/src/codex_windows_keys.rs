//! What Codex 0.167.0's own key reader makes of the bytes a pane writes on Windows (T-1348): the
//! console's key events, read by crossterm's `EventStream` (the pinned fork) inside Codex's
//! `WindowsKeySequence`, which rebuilds the seven key events of the kitty protocol's Shift+Enter
//! (`ESC [ 1 3 ; 2 u`) as Shift+Enter within 50 ms. Codex's file is copied as it is at `rust-v0.167.0`
//! (`windows_key_sequence.rs`; only its `#[cfg(test)] mod tests;` line is not here), wired as Codex
//! wires it (`tui/event_stream.rs`: `WindowsKeySequence<crossterm::event::EventStream>`).
//!
//! Says READY, waits `--wait-ms` (200 by default: a window busy drawing does not read at once, so
//! the keys are all in the console's input when it does; 0: it reads as they come), then reads its
//! raw console until nothing has come for 600 ms, and prints each key press that is not an `x`
//! as `KEY<n>=[<Code>/<Modifiers>]#` (`n` counting those, in the order read; the modifiers `SHIFT`,
//! `CONTROL`, `ALT` joined by `+`, or `NONE`), then `RESULT chars=<n> releases=<n>#` (how many `x`
//! keys it read, and how many key releases came: the console makes one per press, and the adapter
//! takes the final one of a sequence in with it), then `DONE`.

use std::io::{self, Write};
use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal;
use tokio::time::{Instant, sleep, sleep_until};
use tokio_stream::StreamExt;

/// Where Codex's file finds the result type of its event source (`tui/event_stream.rs`).
mod event_stream {
    pub type EventResult = std::io::Result<crossterm::event::Event>;
}
mod windows_key_sequence;

use windows_key_sequence::WindowsKeySequence;

/// How long nothing may come before the probe says what it read.
const QUIET: Duration = Duration::from_millis(600);
/// The modifiers as words: `SHIFT`, `CONTROL` and `ALT` joined by `+`, or `NONE`.
fn modifiers(held: KeyModifiers) -> String {
    let words: Vec<&str> = [
        (KeyModifiers::SHIFT, "SHIFT"),
        (KeyModifiers::CONTROL, "CONTROL"),
        (KeyModifiers::ALT, "ALT"),
    ]
    .into_iter()
    .filter(|(modifier, _)| held.contains(*modifier))
    .map(|(_, word)| word)
    .collect();
    if words.is_empty() {
        "NONE".to_owned()
    } else {
        words.join("+")
    }
}

/// How long the first key may take to come after READY and the wait.
const FIRST: Duration = Duration::from_secs(4);

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let wait = args
        .windows(2)
        .find(|pair| pair[0] == "--wait-ms")
        .and_then(|pair| pair[1].parse().ok())
        .unwrap_or(200);
    terminal::enable_raw_mode()?;
    println!("READY");
    io::stdout().flush()?;
    sleep(Duration::from_millis(wait)).await;
    let mut events = WindowsKeySequence::new(EventStream::new());
    let mut chars = 0_usize;
    let mut releases = 0_usize;
    let mut other = Vec::new();
    let mut until = Instant::now() + FIRST;
    loop {
        tokio::select! {
            next = events.next() => match next {
                Some(Ok(Event::Key(key))) => {
                    until = Instant::now() + QUIET;
                    if key.kind == KeyEventKind::Release {
                        releases += 1;
                    } else if matches!(key.code, KeyCode::Char('x')) {
                        chars += 1;
                    } else {
                        other.push(format!("{:?}/{}", key.code, modifiers(key.modifiers)));
                    }
                }
                Some(Ok(event)) => {
                    until = Instant::now() + QUIET;
                    other.push(format!("{event:?}"));
                }
                Some(Err(_)) | None => break,
            },
            () = sleep_until(until) => break,
        }
    }
    terminal::disable_raw_mode()?;
    for (at, key) in other.iter().enumerate() {
        println!("KEY{}=[{key}]#", at + 1);
    }
    println!("RESULT chars={chars} releases={releases}#");
    println!("DONE");
    Ok(())
}

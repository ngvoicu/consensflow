//! Says READY, waits 200 ms as a window busy drawing would before it reads, then reads its raw
//! terminal for 700 ms with crossterm's own event reader and prints `RESULT chars=<n> keys=[...]`:
//! how many `x` keys it read, and every other key as `Code/Modifiers`, in the order read.

use std::io::{self, Write};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal;

fn main() -> io::Result<()> {
    terminal::enable_raw_mode()?;
    println!("READY");
    io::stdout().flush()?;
    std::thread::sleep(Duration::from_millis(200));
    let start = Instant::now();
    let mut chars = 0;
    let mut other = Vec::new();
    while start.elapsed() < Duration::from_millis(700) {
        if event::poll(Duration::from_millis(30))? {
            if let Event::Key(key) = event::read()? {
                if matches!(key.code, KeyCode::Char('x')) {
                    chars += 1;
                } else {
                    other.push(format!("{:?}/{:?}", key.code, key.modifiers));
                }
            }
        }
    }
    terminal::disable_raw_mode()?;
    println!("RESULT chars={chars} keys={other:?}");
    Ok(())
}

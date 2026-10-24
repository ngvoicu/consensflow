//! A window that prints as fast as it can for as long as it is asked to, as a harness's
//! screen does that repaints itself for hours: what tells how much of a window's output
//! the rig holds (the long run's two hours, zeewin's 7 GB). It is this program started
//! with [`NOISE`] first: `fake-agent --noise <bytes> <done file>`. It prints lines of
//! text until it has printed `<bytes>`, then [`DONE`] on a line of its own, writes
//! `<done file>`, and waits to be ended.

use std::fs;
use std::io::{self, Write};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

/// What the noise is started with, first: no window is launched with it.
pub const NOISE: &str = "--noise";

/// The line it prints once it has printed all it was asked to.
pub const DONE: &str = "the noise is over";

/// Prints what `args` (`<bytes> <done file>`) asks, and waits to be ended.
pub fn run(args: &[String]) -> ExitCode {
    let (Some(bytes), Some(done)) = (
        args.first().and_then(|bytes| bytes.parse::<u64>().ok()),
        args.get(1),
    ) else {
        let _ = writeln!(io::stderr(), "fake-agent: {NOISE} <bytes> <done file>");
        return ExitCode::FAILURE;
    };
    let mut out = io::stdout().lock();
    let mut printed = 0;
    let mut line = 0u64;
    while printed < bytes {
        line += 1;
        let text = format!("noise line {line:010} of a window that prints as fast as it can\r\n");
        if out.write_all(text.as_bytes()).is_err() {
            return ExitCode::FAILURE;
        }
        printed += text.len() as u64;
    }
    if write!(out, "{DONE}\r\n")
        .and_then(|()| out.flush())
        .is_err()
        || fs::write(done, "").is_err()
    {
        return ExitCode::FAILURE;
    }
    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}

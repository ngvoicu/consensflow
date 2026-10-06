//! What the tests of programs that start programs share: a program writes
//! the pids of the tree it made to a file, the test reads them and asks the
//! system whether each is still there, and whatever a failing test left
//! running is ended as the test goes, since nothing else would.

use std::path::Path;
use std::time::{Duration, Instant};

use crate::{alive, terminate, Ending};

/// How long a test waits for a program to write down what it started, and
/// for the system to say that a process has gone.
const WAIT: Duration = Duration::from_secs(20);

/// The `count` pids a program has written to `file`, if it has written them
/// all: white space between, and a line's end last, so that a line half
/// written is not read.
pub(crate) fn pids_in(file: &Path, count: usize) -> Option<Vec<u32>> {
    let written = std::fs::read_to_string(file).ok()?;
    let pids: Vec<u32> = written
        .split_whitespace()
        .filter_map(|word| word.parse().ok())
        .collect();
    (pids.len() == count && written.ends_with('\n')).then_some(pids)
}

/// The pids the program wrote to `file` (see [`pids_in`]), waited for on the
/// test's own thread: the program is running.
#[cfg(unix)]
pub(crate) fn pids_written_to(file: &Path, count: usize) -> Vec<u32> {
    let until = Instant::now() + WAIT;
    while Instant::now() < until {
        if let Some(pids) = pids_in(file, count) {
            return pids;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the program never wrote {count} pids to {}", file.display());
}

/// The pids the program wrote to `file`, waited for on the runtime that runs
/// the capture, which would stop were it waited for by the thread.
pub(crate) async fn pids_written_to_soon(file: &Path, count: usize) -> Vec<u32> {
    let until = Instant::now() + WAIT;
    while Instant::now() < until {
        if let Some(pids) = pids_in(file, count) {
            return pids;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the program never wrote {count} pids to {}", file.display());
}

/// Whether the process `pid` is gone within `within`: one the system reaps
/// as it ends (an orphan is its init's) is gone as soon as it is.
pub(crate) fn gone_within(pid: u32, within: Duration) -> bool {
    let until = Instant::now() + within;
    while alive(pid) {
        if Instant::now() >= until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    true
}

/// Whether the process `pid` is gone, given the time the system needs.
pub(crate) fn gone(pid: u32) -> bool {
    gone_within(pid, WAIT)
}

/// Processes a test started, ended when it is dropped if they are still
/// there: a test that fails with them running must not leave them for as long
/// as they sleep. One that has passed has found them gone, and so sends
/// nothing to a pid that may be another's by now.
#[derive(Default)]
pub(crate) struct Survivors(pub(crate) Vec<u32>);

impl Drop for Survivors {
    fn drop(&mut self) {
        for &pid in &self.0 {
            if alive(pid) {
                terminate(pid, Ending::Forced);
            }
        }
    }
}

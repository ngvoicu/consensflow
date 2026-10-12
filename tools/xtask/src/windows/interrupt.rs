//! Whether this program was asked to stop: Ctrl-C, and where the system has them a
//! closed terminal and a `kill`. A run on the machine goes on there when this side
//! goes (Windows' SSH server does not end what ssh started), so a stop is not the
//! end of this program: it is heard, the run is ended there, and then this ends.
//!
//! The stops are the ones `plants` listens for, heard on a thread of their own
//! that sets a flag, so that the code that waits for a program, a pipe or a file
//! need not be a task of a runtime: it asks [`Interrupt::heard`] as it waits. Once
//! one is heard nothing listens for the next, and the handler stays installed, so
//! a second Ctrl-C does not end this program: what this starts (ssh, tar, scp) is
//! in the terminal's group and the second Ctrl-C ends that. The ssh that is asking
//! the machine to end the run is cut short (and the run says it may still be going
//! there), and this ends as it would have, with the status of a stop.

use std::future::poll_fn;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::plants::stops::Stops;

/// How often a wait for a stop looks again.
const LOOK: Duration = Duration::from_millis(10);

/// How long what failed is waited on for a stop it may have been the doing of.
/// A program the terminal gave the same Ctrl-C (ssh, tar, scp) may be gone, and
/// seen to be, before this program has heard it.
const GRACE: Duration = Duration::from_millis(500);

/// A stop that has been heard, or not yet. It may be cloned: every copy says the
/// same. The default has heard nothing, listens to no one, and waits for no stop
/// that a failure may have been the doing of.
#[derive(Debug, Clone, Default)]
pub struct Interrupt {
    heard: Arc<AtomicBool>,
    grace: Duration,
}

impl Interrupt {
    /// Starts listening for what stops this program, which is heard from here on.
    pub fn listen() -> io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let mut stops = {
            let _inside = runtime.enter();
            Stops::listen()?
        };
        let interrupt = Self {
            grace: GRACE,
            ..Self::default()
        };
        let heard = Arc::clone(&interrupt.heard);
        thread::Builder::new()
            .name("windows-stops".into())
            .spawn(move || {
                runtime.block_on(poll_fn(|cx| stops.poll(cx)));
                heard.store(true, Ordering::SeqCst);
            })?;
        Ok(interrupt)
    }

    /// Whether a stop has been heard.
    pub fn heard(&self) -> bool {
        self.heard.load(Ordering::SeqCst)
    }

    /// Whether a stop has been heard, or is within the grace: for what failed as a
    /// stop would make it (a program the terminal gave the same Ctrl-C) just
    /// before this program was told of it.
    pub fn heard_after_failure(&self) -> bool {
        let until = Instant::now() + self.grace;
        while !self.heard() && Instant::now() < until {
            thread::sleep(LOOK);
        }
        self.heard()
    }

    /// A stop, as if it were heard: for the tests, whose `Interrupt::default()`
    /// listens to no one (the Ctrl-C of whoever runs them is theirs).
    #[cfg(test)]
    pub fn raise(&self) {
        self.heard.store(true, Ordering::SeqCst);
    }

    /// An interrupt that listens to no one and waits `grace` for a stop after a failure.
    #[cfg(test)]
    pub fn graced(grace: Duration) -> Self {
        Self {
            grace,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests;

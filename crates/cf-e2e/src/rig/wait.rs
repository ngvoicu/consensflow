//! Waiting with a deadline: every wait in the suites ends, with an error that
//! says what was waited for and what the daemon and the pane host had said
//! meanwhile, and none is for ever.

use std::thread;
use std::time::{Duration, Instant};

use super::Rig;
use crate::{Error, Result};

/// How often what is waited for is looked at again.
const INTERVAL: Duration = Duration::from_millis(25);

impl Rig {
    /// Waits until `predicate` says yes, looking again every 25 ms, for up to
    /// `within`. A predicate that fails ends the wait with its error. `what` is
    /// what is waited for, in the words of the error that says it did not
    /// happen.
    pub fn wait_for(
        &self,
        what: &str,
        within: Duration,
        mut predicate: impl FnMut() -> Result<bool>,
    ) -> Result<()> {
        let started = Instant::now();
        loop {
            if predicate()? {
                return Ok(());
            }
            if started.elapsed() >= within {
                return Err(Error::Timeout(format!(
                    "timed out after {} s waiting for {what}{}",
                    within.as_secs(),
                    self.what_they_said()
                )));
            }
            thread::sleep(INTERVAL);
        }
    }
}

/// Waits until `predicate` is true, for up to `within`: whether it was.
pub(super) fn until(within: Duration, mut predicate: impl FnMut() -> bool) -> bool {
    let started = Instant::now();
    loop {
        if predicate() {
            return true;
        }
        if started.elapsed() >= within {
            return false;
        }
        thread::sleep(INTERVAL);
    }
}

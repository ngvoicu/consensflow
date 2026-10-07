//! How a pane's program ended. Whoever first reads its status (`try_wait`
//! from a listing, a retirement or a kill, `wait` from the thread that reaps
//! it) keeps it here, because it can be read only once on some systems and
//! because the pane has left the table by the time the output thread, which
//! tells the daemon the pane ended, wants it.

use std::sync::{Mutex, PoisonError};

use portable_pty::ExitStatus;

/// How a program ended: the exit code it returned, and the signal that ended
/// it where one did (a program ended by a signal has the code 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ended {
    pub code: u32,
    pub signal: Option<String>,
}

/// One pane's [`Ended`], empty until its status is read.
#[derive(Debug, Default)]
pub struct EndedSlot {
    ended: Mutex<Option<Ended>>,
}

impl EndedSlot {
    /// The program's status was read. The first reading stays: a later one
    /// of the same program says the same.
    pub(crate) fn record(&self, status: &ExitStatus) {
        let mut ended = self.ended.lock().unwrap_or_else(PoisonError::into_inner);
        if ended.is_none() {
            *ended = Some(Ended {
                code: status.exit_code(),
                signal: status.signal().map(str::to_owned),
            });
        }
    }

    /// How the program ended, if its status was read.
    pub fn get(&self) -> Option<Ended> {
        self.ended
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_status_read_is_the_one_kept() {
        let slot = EndedSlot::default();
        assert_eq!(slot.get(), None);
        slot.record(&ExitStatus::with_exit_code(3));
        slot.record(&ExitStatus::with_exit_code(0));
        assert_eq!(
            slot.get(),
            Some(Ended {
                code: 3,
                signal: None
            })
        );
    }

    #[test]
    fn a_signal_is_kept_with_the_code_one_the_system_gives_it() {
        let slot = EndedSlot::default();
        slot.record(&ExitStatus::with_signal("Hangup: 1"));
        assert_eq!(
            slot.get(),
            Some(Ended {
                code: 1,
                signal: Some("Hangup: 1".to_owned())
            })
        );
    }
}

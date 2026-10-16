//! When a window's output has ended, and what comes of its program then. The pane host
//! reads a window's output until it ends, and then tells the daemon the window closed
//! (`pane.exit`), which forgets it, sends no kill, and may open another window on its
//! conversation at once. So the output ends only where it has ended for good, and the
//! daemon is told only on proof that the program has gone ([`PaneTable::output_ended`]).
//! Left running, a program went on unseen beside the window a resume opened on its
//! conversation, and held the update back; told closed before it had gone, the daemon
//! could open that window while it still ran (the reviews of 2026-10-16).
//!
//! **The proof.** The program's status was read, or the system said its child was reaped
//! elsewhere (`EndedSlot::proven_gone`); on Windows the status is read once its process
//! handle is signalled. A pane out of the table is no proof: a close takes it out before
//! it has ended its programs, and a program that ignores the request to end, closes its
//! terminal and cleans up for six seconds of the close's grace was told gone while it ran
//! (both reviews of round 3). A program that has not gone a moment after its output ended
//! ([`EXIT_GRACE`]) is ended as a close ends one, on a thread of its own, and the proof is
//! waited for until [`PROOF_BOUND`] from the end of the output, and no longer: a close
//! waits for the exit with no bound, and a Windows program that waits on I/O it cannot
//! cancel is not gone when it is terminated, so a bound that waited on the close did not
//! hold (both reviews of round 4). Past it the daemon is told the window closed with the
//! program named as one that could not be ended (`survivor`), and the note says it may
//! still run. Nothing is refused for it: a registry of such programs refused a window of
//! the same name for ever where the proof never came of a program that had gone, and
//! forgot them on a restart (the same reviews).
//!
//! **Windows.** The output is the pseudoconsole's: its host (`conhost`, or the
//! `OpenConsole` the app ships) writes it, and it ends when the host has gone, which
//! nothing brings back. The host stays while a program runs, and after it (the exit
//! watcher's retirement closes it, `pty`), so an output that ends while the program
//! runs is a host that died: the end is for good, and the program is ended.
//!
//! **Unix.** The output is the terminal's, and it ends (an end of file on macOS, an error
//! on Linux) whenever no process holds the terminal: a program that closes it (or revokes
//! it) and opens it again goes on, and so does its output. So the reader takes the end for
//! the window's only once the program has gone (it exited, or its pane left the table),
//! and it asks that on a schedule of its own ([`LOOK`]), whatever it reads: a helper the
//! program left can hold the terminal with nothing to say, and a read waited for ever on
//! its silence (Sol's sequence); or write a byte every twenty milliseconds, and a reader
//! that asked only when nothing came never asked (both reviews of round 3). Once the
//! program has gone, what the terminal still has is read and sent on before the output
//! ends, for [`DRAIN`] at most: the program can open its terminal again, say its last
//! words and exit between the end the reader saw and its look at the program (Astra's
//! race), and a helper that goes on writing is cut off there. A program that let go of
//! its terminal for good and runs on keeps its window, silent, until it exits or the
//! window is closed.

use std::io::Read;
use std::time::{Duration, Instant};

use super::{has_exited, PaneError, PaneKey, PaneTable, EXIT_GRACE};
use crate::ended::EndedSlot;

/// How often the reader asks whether the program has gone (Unix), whatever it reads, and
/// how long it waits before it reads again a terminal that nothing holds.
#[cfg(unix)]
const LOOK: Duration = Duration::from_millis(100);

/// How long what the terminal still has is read, once the program has gone, before the
/// output is taken to have ended: a helper the program left can go on writing.
#[cfg(unix)]
const DRAIN: Duration = Duration::from_secs(1);

/// How long, from the end of a window's output, the host waits for the proof that its
/// program has gone before the daemon is told it could not be ended: past a close's grace
/// (ten seconds) and the force after it.
const PROOF_BOUND: Duration = Duration::from_secs(30);

/// What the terminal a window's output is read from is asked whether it has anything
/// to read by, on Unix: its own descriptor, held for as long as the output is read.
#[cfg(unix)]
pub(super) type Terminal = std::os::fd::OwnedFd;
/// Nothing, elsewhere: the pseudoconsole's output ends only for good.
#[cfg(not(unix))]
pub(super) struct Terminal;

/// What a read of a window's output gave.
pub(super) enum Got {
    /// So many bytes.
    Bytes(usize),
    /// The output has ended for good.
    End,
}

/// Whether a window's program has gone, asked with whether the terminal has just come to
/// an end (nothing holds it), or not.
pub(super) type Gone = Box<dyn Fn(bool) -> bool + Send>;

/// A window's output, as the host reads it until it has ended for good.
pub(super) struct Output {
    reader: Box<dyn Read + Send>,
    #[cfg_attr(not(unix), allow(dead_code))]
    terminal: Terminal,
    #[cfg_attr(not(unix), allow(dead_code))]
    gone: Gone,
    /// When the program was last asked after.
    #[cfg_attr(not(unix), allow(dead_code))]
    asked: Instant,
    /// Once the program has gone: until when what is left is read.
    #[cfg_attr(not(unix), allow(dead_code))]
    ending: Option<Instant>,
}

impl Output {
    pub(super) fn new(reader: Box<dyn Read + Send>, terminal: Terminal, gone: Gone) -> Self {
        Self {
            reader,
            terminal,
            gone,
            asked: Instant::now(),
            ending: None,
        }
    }

    /// The next of the output, into `bytes`: what the terminal has, or that the output
    /// has ended for good.
    #[cfg(unix)]
    pub(super) fn read(&mut self, bytes: &mut [u8]) -> Got {
        loop {
            if self.ending.is_some_and(|until| Instant::now() >= until) {
                return Got::End;
            }
            // On the reader's own schedule, whatever the terminal gives.
            if self.ending.is_none() && self.asked.elapsed() >= LOOK {
                self.ask(false);
                continue;
            }
            let wait = if self.ending.is_some() {
                Duration::ZERO
            } else {
                LOOK.saturating_sub(self.asked.elapsed())
            };
            if !readable(&self.terminal, wait) {
                if self.ending.is_some() {
                    return Got::End;
                }
                continue;
            }
            match self.reader.read(bytes) {
                Ok(count) if count > 0 => return Got::Bytes(count),
                _ if self.ending.is_some() => return Got::End,
                // Nothing holds the terminal: once the program has gone, what is left is
                // read; while it runs, it may open its terminal again.
                _ => {
                    if !self.ask(true) {
                        std::thread::sleep(LOOK);
                    }
                }
            }
        }
    }

    /// The next of the output, into `bytes`: what the pseudoconsole passes on, until its
    /// output ends, which is for good.
    #[cfg(not(unix))]
    pub(super) fn read(&mut self, bytes: &mut [u8]) -> Got {
        match self.reader.read(bytes) {
            Ok(count) if count > 0 => Got::Bytes(count),
            _ => Got::End,
        }
    }

    /// Whether the program has gone, `at_an_end` saying whether nothing holds the terminal
    /// now; once it has, what is left is read for [`DRAIN`].
    #[cfg(unix)]
    fn ask(&mut self, at_an_end: bool) -> bool {
        self.asked = Instant::now();
        let gone = (self.gone)(at_an_end);
        if gone {
            self.ending = Some(Instant::now() + DRAIN);
        }
        gone
    }
}

/// Whether `terminal` has something to read (or has ended), waiting up to `wait` for it.
#[cfg(unix)]
fn readable(terminal: &Terminal, wait: Duration) -> bool {
    use std::os::fd::AsRawFd;

    let mut asked = libc::pollfd {
        fd: terminal.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let millis = i32::try_from(wait.as_millis()).unwrap_or(i32::MAX);
    // SAFETY: one structure, writable, for a descriptor the output holds open.
    unsafe { libc::poll(&mut asked, 1, millis) > 0 }
}

/// What the daemon is told of a window's program, as its exit goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Handed {
    /// Its number, where it could not be proven gone (0 where the number is not known).
    pub survivor: Option<u32>,
    /// Whether the host ended it: how it ended is then the host's doing, not its own.
    pub ended_by_host: bool,
}

impl PaneTable {
    /// Whether a pane's program has gone, as far as the reader of its output needs to
    /// know: its end was read (into `slot`), or it has exited, or its status cannot be
    /// read, which no read will change. A pane a close took out of the table is being
    /// closed, and its output has ended once nothing holds its terminal (`at_an_end`),
    /// which the close brings about. This is no proof for the daemon
    /// ([`PaneTable::output_ended`] asks the slot for that).
    pub(super) fn program_gone(&self, key: &PaneKey, slot: &EndedSlot, at_an_end: bool) -> bool {
        if slot.proven_gone() {
            return true;
        }
        let Ok(mut panes) = self.lock_panes() else {
            return true;
        };
        match panes.get_mut(key) {
            Some(pane) => has_exited(pane).unwrap_or(true),
            None => at_an_end,
        }
    }

    /// Whether the proof that a pane's program has gone comes within `wait`: the slot
    /// says so, or the program's status, read where the pane is in the table, does.
    fn proven_within(&self, key: &PaneKey, slot: &EndedSlot, wait: Duration) -> bool {
        let deadline = Instant::now() + wait;
        loop {
            if slot.proven_gone() {
                return true;
            }
            if let Ok(mut panes) = self.lock_panes() {
                if let Some(pane) = panes.get_mut(key) {
                    let _ = has_exited(pane);
                }
            }
            if slot.proven_gone() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// A pane whose output has ended for good, its program's end kept in `slot`: the
    /// daemon is told (`tell`, `pane.exit`) once the program is proven gone. One that has
    /// not gone a moment after ([`EXIT_GRACE`]) is ended as a close ends one, on a thread
    /// of its own ([`PaneTable::kill_detached`]), and the proof is waited for until
    /// [`PROOF_BOUND`]; then the daemon is told all the same, given the program's number
    /// (`pid`) as one that could not be ended. Then the pane leaves the table, where the
    /// close has not taken it out. `true` once it is out.
    pub(crate) fn output_ended(
        &self,
        key: &PaneKey,
        slot: &EndedSlot,
        pid: Option<u32>,
        tell: impl FnOnce(Handed),
    ) -> Result<bool, PaneError> {
        self.output_ended_within(key, slot, pid, PROOF_BOUND, tell)
    }

    pub(super) fn output_ended_within(
        &self,
        key: &PaneKey,
        slot: &EndedSlot,
        pid: Option<u32>,
        bound: Duration,
        tell: impl FnOnce(Handed),
    ) -> Result<bool, PaneError> {
        let deadline = Instant::now() + bound;
        let mut proven = self.proven_within(key, slot, EXIT_GRACE.min(bound));
        let mut ended_by_host = false;
        if !proven {
            // Ended as a close ends it: a pane out of the table is a close's already.
            if self.lock_panes()?.contains_key(key) {
                ended_by_host = true;
                let _ = self.kill_detached(key);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            proven = self.proven_within(key, slot, left);
        }
        let survivor = (!proven).then(|| pid.unwrap_or_default());
        tell(Handed {
            survivor,
            ended_by_host,
        });
        self.retire_exited(key)
    }
}

#[cfg(test)]
mod tests;

//! Ending a window: its pane taken out of the table, its programs ended and counted until they
//! are proven gone, and the exit of its program observed.

use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use super::close::{ask_all_to_exit, terminate, terminate_asked, terminate_detached};
use super::flow::close_output;
#[cfg(windows)]
use super::EXIT_FLUSH;
#[cfg(all(test, windows))]
use super::test_support::WATCH_FAILS;
use super::{Pane, PaneError, PaneKey, PaneTable, CANCELLED_KEPT, EXIT_GRACE};
use crate::ended::{Ended, EndedSlot};

/// Panes whose child is still being torn down. A pane leaves `finishing` only
/// on positive evidence that its process is gone; anything unproven moves to
/// `unconfirmed`, which holds admission closed until ConsensFlow restarts.
#[derive(Default)]
pub(super) struct Teardown {
    finishing: AtomicU64,
    unconfirmed: AtomicU64,
}

impl Teardown {
    pub(super) fn begin(&self) {
        self.finishing.fetch_add(1, Ordering::AcqRel);
    }

    /// Raising `unconfirmed` before lowering `finishing` keeps the two counts
    /// from summing to zero mid-handover, so no installer slips through.
    pub(super) fn resolve(&self, child_is_gone: bool) {
        if !child_is_gone {
            self.unconfirmed.fetch_add(1, Ordering::AcqRel);
        }
        self.finishing.fetch_sub(1, Ordering::AcqRel);
    }

    pub(super) fn counts(&self) -> (u64, u64) {
        (
            self.finishing.load(Ordering::Acquire),
            self.unconfirmed.load(Ordering::Acquire),
        )
    }
}

/// The windows whose programs are being ended now, by their keys: a kill of one that has
/// left the table already waits for that end, so that its answer says the programs are
/// gone (the daemon asks again for the end of a window whose open it gave up on where
/// its first kill went unanswered, which may still be waiting out the grace).
#[derive(Default)]
pub(super) struct Ending {
    keys: Mutex<Vec<PaneKey>>,
    ended: Condvar,
}

impl Ending {
    /// `key`'s programs are being ended. Called with `panes`' lock held, as the pane
    /// leaves the table.
    fn begin(&self, key: &PaneKey) {
        self.keys
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(key.clone());
    }

    /// `key`'s programs are ended, or what could be done to end them was.
    fn end(&self, key: &PaneKey) {
        let mut keys = self.keys.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(at) = keys.iter().position(|ending| ending == key) {
            keys.swap_remove(at);
        }
        self.ended.notify_all();
    }

    /// Waits until nothing ends `key`'s programs, for up to `within`: whether nothing does.
    fn wait(&self, key: &PaneKey, within: Duration) -> bool {
        let deadline = Instant::now() + within;
        let mut keys = self.keys.lock().unwrap_or_else(PoisonError::into_inner);
        while keys.contains(key) {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            keys = self
                .ended
                .wait_timeout(keys, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }
}
impl PaneTable {
    /// Whether the exit of every window whose output is read has been told, waited for
    /// until `within`: the exit of a window that was ended is told when its output has
    /// ended and its program is proven gone, a moment after the close that ended it, and a
    /// host that stops closes its own output only once they are.
    pub fn exits_told_within(&self, within: Duration) -> bool {
        self.exits.paid_within(within)
    }

    /// How a pane's program ended, waiting up to `wait` for its status to be
    /// readable: a program closes its terminal a moment before it has gone.
    /// A pane that left the table is being taken down by whoever took it out
    /// (a kill, a retirement), who keeps its status in `slot` as they read it,
    /// and so it is waited for there. None when the status was not read in
    /// time, or could not be.
    pub fn wait_ended(&self, key: &PaneKey, slot: &EndedSlot, wait: Duration) -> Option<Ended> {
        let deadline = Instant::now() + wait;
        loop {
            if let Some(ended) = slot.get() {
                return Some(ended);
            }
            {
                let mut panes = self.lock_panes().ok()?;
                if let Some(pane) = panes.get_mut(key) {
                    if has_exited(pane).ok()? {
                        return slot.get();
                    }
                }
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Ends a pane and takes it out of the table. One that is gone already,
    /// killed or retired when its program ended, is closed: the caller cannot
    /// tell the two apart, and does not need to. Its programs are asked to end
    /// first and given [`CLOSE_GRACE`](super::CLOSE_GRACE) to do it in; on Windows they are
    /// ended at once (see [`close`](super::close)).
    pub fn kill(&self, key: &PaneKey) -> Result<(), PaneError> {
        self.close_pane(key, true)
    }

    /// [`PaneTable::kill`] for a quit, where no one is left to be told what the window
    /// left running (the daemon has stopped): its tree is not looked at, so a window with
    /// work in it does not make the quit wait for the second look ([`crate::background`]).
    pub(crate) fn reap(&self, key: &PaneKey) -> Result<(), PaneError> {
        self.close_pane(key, false)
    }

    fn close_pane(&self, key: &PaneKey, told: bool) -> Result<(), PaneError> {
        let mut panes = self.lock_panes()?;
        let Some(pane) = panes.remove(key) else {
            // Gone already, or not open yet: an open of it that comes now is one
            // the daemon gave up on (its answer came too late), and it never opens.
            // One being ended by a close before this is gone once that close is over.
            self.cancel(key);
            drop(panes);
            if self.ending.wait(key, self.close_grace + EXIT_GRACE) {
                return Ok(());
            }
            return Err(PaneError::Ending(key.clone()));
        };
        self.tear_down(panes, key, pane, told)
    }

    /// `key` is closed before its open reached the table: that open is refused.
    /// Called with `panes`' lock held, as the open checks it with that lock held,
    /// so a kill and an open of one window are the one or the other way round.
    fn cancel(&self, key: &PaneKey) {
        let mut cancelled = self
            .cancelled
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if cancelled.contains(key) {
            return;
        }
        if cancelled.len() == CANCELLED_KEPT {
            cancelled.pop_front();
        }
        cancelled.push_back(key.clone());
    }

    /// Whether `key` was closed before its open reached the table, which then
    /// takes it off the list. Called with `panes`' lock held.
    pub(super) fn uncancel(&self, key: &PaneKey) -> bool {
        let mut cancelled = self
            .cancelled
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let at = cancelled.iter().position(|cancelled| cancelled == key);
        at.and_then(|at| cancelled.remove(at)).is_some()
    }

    /// [`PaneTable::kill`], with the close on a thread of its own: the pane is out of the
    /// table when this returns, and its programs' end comes into its slot when the close
    /// has read it. The end of an output a program outlived waits on that for as long as
    /// its bound allows (`output_end`): a kill waits for the exit with no bound, and a
    /// Windows program that waits on I/O it cannot cancel is not gone when it is
    /// terminated, so neither `pane.exit` nor the word that the program could not be
    /// ended came (both reviews of round 4).
    pub(super) fn kill_detached(&self, key: &PaneKey) -> Result<(), PaneError> {
        let mut panes = self.lock_panes()?;
        let Some(pane) = panes.remove(key) else {
            return Ok(());
        };
        self.teardown.begin();
        self.ending.begin(key);
        drop(panes);
        let (teardown, grace) = (Arc::clone(&self.teardown), self.close_grace);
        let (ender, ending, ended) = (Arc::clone(&teardown), Arc::clone(&self.ending), key.clone());
        std::thread::Builder::new()
            .name("consensflow-pty-ender".to_string())
            .spawn(move || {
                let _ = end_pane(pane, &ender, grace, true);
                ending.end(&ended);
            })
            .map(|_| ())
            .map_err(|error| {
                teardown.resolve(false);
                self.ending.end(key);
                PaneError::Io(error)
            })
    }

    /// A pane whose program has ended leaves the table by a kill's rules:
    /// what the harness left running ends with it, its master goes and its
    /// child is reaped, and a write still in progress hands the exit to the
    /// reaper. Nothing else would take it out: the daemon closes a window
    /// that ended on its own without a `pane.kill`, and the pane kept its
    /// master, a zombie child and the update blocked until the app quit.
    ///
    /// `true` once the pane is out of the table (now, or already); `false`
    /// while its program still runs, so the pane stays for a kill (the one that
    /// follows the end of an output its program outlived:
    /// [`PaneTable::output_ended`]). The output ends a moment before the exit can
    /// be read, so it looks again for a while before it says so.
    pub(crate) fn retire_exited(&self, key: &PaneKey) -> Result<bool, PaneError> {
        self.retire_exited_within(key, EXIT_GRACE)
    }

    pub(super) fn retire_exited_within(
        &self,
        key: &PaneKey,
        grace: Duration,
    ) -> Result<bool, PaneError> {
        let deadline = Instant::now() + grace;
        loop {
            let mut panes = self.lock_panes()?;
            let Some(pane) = panes.get_mut(key) else {
                return Ok(true);
            };
            if has_exited(pane)? {
                if let Some(pane) = panes.remove(key) {
                    self.tear_down(panes, key, pane, true)?;
                }
                return Ok(true);
            }
            drop(panes);
            if Instant::now() >= deadline {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Ends a pane just taken out of the table. Its teardown is counted before
    /// the table's lock goes, so no installer is admitted in between. `told`: whether
    /// what the window leaves running is kept for the exit that is sent.
    fn tear_down(
        &self,
        panes: MutexGuard<'_, HashMap<PaneKey, Pane>>,
        key: &PaneKey,
        pane: Pane,
        told: bool,
    ) -> Result<(), PaneError> {
        self.teardown.begin();
        self.ending.begin(key);
        drop(panes);
        let ended = end_pane(pane, &self.teardown, self.close_grace, told);
        self.ending.end(key);
        ended
    }

    /// The built-in ConPTY keeps a pane's output open after its program exits
    /// (node-pty closes it itself a second after the exit), so the end of the
    /// output, which says on a Unix PTY that the window has ended, never came:
    /// the chief's `/exit` or a crash went unseen. One watcher per pane waits
    /// on the program, gives the pseudoconsole a moment to pass on what it
    /// printed last, and retires the pane; its master's drop closes the
    /// pseudoconsole, the output ends, and `pane.exit` follows as anywhere.
    /// A window whose end could not be seen does not open: its opener ends it.
    #[cfg(windows)]
    pub(super) fn watch_exit(self: &Arc<Self>, key: &PaneKey) -> Result<(), PaneError> {
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::System::Threading::{
            OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE,
        };

        struct Program(HANDLE);
        // SAFETY: a process handle is a kernel handle, usable from any thread.
        unsafe impl Send for Program {}
        impl Drop for Program {
            fn drop(&mut self) {
                // SAFETY: the handle is ours and closed once.
                unsafe { CloseHandle(self.0) };
            }
        }

        #[cfg(test)]
        if WATCH_FAILS.get() {
            return Err(PaneError::Io(io::Error::other("planted: no watcher")));
        }
        let pid = self
            .lock_panes()?
            .get(key)
            .and_then(|pane| pane.child.process_id());
        let watched = pid
            .ok_or_else(|| io::Error::other("the harness has no process id"))
            .and_then(|pid| {
                // SAFETY: a wait-only handle; the pane's own child handle keeps
                // the pid from being reused while the pane is open.
                let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
                if handle.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let program = Program(handle);
                let table = Arc::downgrade(self);
                let key = key.clone();
                std::thread::Builder::new()
                    .name("consensflow-pty-exit".to_string())
                    .spawn(move || {
                        // SAFETY: the handle stays open until `program` drops.
                        unsafe { WaitForSingleObject(program.0, INFINITE) };
                        drop(program);
                        std::thread::sleep(EXIT_FLUSH);
                        if let Some(table) = table.upgrade() {
                            table.retire_after_exit(&key);
                        }
                    })
                    .map(|_| ())
            });
        watched.map_err(PaneError::Io)
    }

    /// What an exit watcher does once its program has gone: the pane is retired. Where that
    /// fails, or the program is found to be running still, the pane is ended as a close ends
    /// one ([`PaneTable::kill_detached`]), which counts what it cannot prove gone as
    /// unconfirmed, and the host says so on its stderr: left in the table, the pane's
    /// output would never end on Windows.
    #[cfg(windows)]
    fn retire_after_exit(&self, key: &PaneKey) {
        self.retire_after_exit_by(key, Self::retire_exited);
    }

    #[cfg(any(windows, test))]
    #[allow(clippy::print_stderr)] // The pane host's log is its error output.
    pub(super) fn retire_after_exit_by(
        &self,
        key: &PaneKey,
        retire: impl FnOnce(&Self, &PaneKey) -> Result<bool, PaneError>,
    ) {
        let failed = match retire(self, key) {
            Ok(true) => return,
            Ok(false) => "its program still runs".to_owned(),
            Err(error) => error.to_string(),
        };
        let ended = self.kill_detached(key);
        eprintln!(
            "consensflow-bridge: pane {} generation {}: its exit could not retire it ({failed}); {}",
            key.id,
            key.generation,
            match ended {
                Ok(()) => "it is ended as a close ends it".to_owned(),
                Err(error) => format!("ending it failed too: {error}"),
            }
        );
    }
}

impl Drop for PaneTable {
    fn drop(&mut self) {
        let panes = match self.panes.get_mut() {
            Ok(panes) => panes,
            Err(poisoned) => poisoned.into_inner(),
        };
        // Every window is asked to end before any is waited for, so one whose
        // programs ignore the request does not make the others wait for it too.
        for pane in panes.values_mut() {
            close_output(pane);
        }
        for (pane, _) in ask_all_to_exit(panes.values_mut(), self.close_grace) {
            let _ = terminate_asked(pane);
        }
    }
}

/// Ends a pane just taken out of the table, whose teardown is counted in `teardown`:
/// its programs are given `grace` and what is left of them is killed. `told`: whether
/// what the window leaves running is kept for the exit that is sent.
fn end_pane(
    mut pane: Pane,
    teardown: &Arc<Teardown>,
    grace: Duration,
    told: bool,
) -> Result<(), PaneError> {
    close_output(&pane);
    // What the close ends is what the window leaves running: the exit it
    // sends says so, and by then the processes are gone.
    if told {
        pane.keep_background();
    }
    if pane.active_writes.load(Ordering::Acquire) == 0 {
        let result = terminate(&mut pane, grace);
        teardown.resolve(result.is_ok());
        result
    } else {
        terminate_detached(pane, Arc::clone(teardown), grace)
    }
}

/// `ECHILD` says the kernel has no such child left to reap, which is proof the
/// process is gone rather than a doubt. Every other error leaves its fate open.
#[cfg(unix)]
fn already_reaped(error: &io::Error) -> bool {
    const ECHILD: i32 = 10;
    error.raw_os_error() == Some(ECHILD)
}

#[cfg(not(unix))]
fn already_reaped(_error: &io::Error) -> bool {
    false
}

/// Whether the pane's program has exited. The status that says so is kept
/// ([`EndedSlot`]) for whoever tells how the program ended, and so is the
/// system's word that it was reaped elsewhere: either is the proof it has gone.
pub(super) fn has_exited(pane: &mut Pane) -> Result<bool, PaneError> {
    match pane.child.try_wait() {
        Ok(Some(status)) => {
            pane.ended.record(&status);
            Ok(true)
        }
        Ok(None) => Ok(false),
        Err(error) if already_reaped(&error) => {
            pane.ended.mark_reaped();
            Ok(true)
        }
        Err(error) => Err(PaneError::Io(error)),
    }
}

pub(super) fn wait_for_exit(pane: &mut Pane) -> Result<(), PaneError> {
    match pane.child.wait() {
        Ok(status) => {
            pane.ended.record(&status);
            Ok(())
        }
        Err(error) if already_reaped(&error) => {
            pane.ended.mark_reaped();
            Ok(())
        }
        Err(error) => Err(PaneError::Io(error)),
    }
}

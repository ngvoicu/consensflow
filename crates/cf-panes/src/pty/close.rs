//! Closing a window, on Unix, the way a terminal closes one. Its programs are
//! asked to end: TERM and then HUP to the pane's process group and, on macOS,
//! to every process of its tree wherever it went (a job the agent started in a
//! session of its own is in no group of the pane's), which is what is under its
//! program, or in the session it leads, or was seen so (`process_tree`'s bound, which
//! signals the pane's group itself only while the program runs). They are given
//! [`CLOSE_GRACE`] to do it in, and what is still there once they have ended or
//! the grace is over is ended by force. A job that traps the signals gets to
//! release what it holds; one that ignores them is killed after the grace, as
//! every one was at once before.
//!
//! Windows keeps the close it had: nothing is asked and nothing is waited for,
//! and the pane's job object ends the whole tree at once. **A job the agent
//! left running there is ended with no chance to release what it holds.** The
//! request it would be given is Ctrl+Break to the pane's console, and that does
//! not stay in the pane's console. For a program behind a `.cmd` shim (as npm
//! installs every harness: `cmd.exe /d /s /c x.cmd`) the Break reached the
//! console of the host itself: it ended the process that closed the pane
//! (`STATUS_CONTROL_C_EXIT`, 0xc000013a), which in the rig was the pane host and
//! the daemon, and left the shim asking "Terminate batch job (Y/N)?" on the
//! host's own console.
//!
//! Every close of a window goes through here: a kill, the retirement of a
//! window whose program ended, a table dropped ([`terminate`],
//! [`terminate_detached`], and [`ask_all_to_exit`] for the many a table holds).
//!
//! What a turn left running is ended the same way, with the window staying open
//! (`turn`'s `end_background`): on macOS the same asking and the same ending of
//! what is left, by the same [`crate::process_tree`] of the pane, of the turn's work
//! the baseline proves instead of the whole tree and the group
//! ([`wait_until_gone`] is the wait they share). On Windows they cannot share the
//! ending: a job object ends all of its tree or none of it, which is what a close
//! wants, and an ending of some of it ends each process by itself, at once, and
//! waits for it.

#[cfg(unix)]
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
use crate::process_tree::Subject;

use super::{has_exited, wait_for_exit, Pane, PaneError, Teardown};

/// How long a window's programs are given, once asked to end, before what is
/// left of them is killed. A close that finds them all gone sooner does not wait,
/// and on Windows, where nothing is asked, there is no grace.
pub const CLOSE_GRACE: Duration = Duration::from_secs(10);
/// How often a close that waits looks.
const LOOK_EVERY: Duration = Duration::from_millis(25);

/// What a close asked to end, and waits for.
#[derive(Default)]
pub(super) struct Asked {
    #[cfg(unix)]
    group: Option<i32>,
    #[cfg(target_os = "macos")]
    tree: Option<crate::process_tree::Asked>,
}

impl Pane {
    /// Asks the window's programs to end. A program that cannot be asked
    /// (already gone, or not ours to signal) is not waited for, and the close
    /// that follows ends what is left. On Windows nothing is asked.
    pub(super) fn ask_to_exit(&self) -> Asked {
        #[cfg_attr(not(unix), allow(unused_mut))]
        let mut asked = Asked::default();
        #[cfg(unix)]
        if let Some(group) = self.process_group_id {
            let _ = signal_group(self, group, SIGTERM);
            let _ = signal_group(self, group, SIGHUP);
            asked.group = Some(group);
        }
        #[cfg(target_os = "macos")]
        {
            // A process that could not be asked is one that is ending, or not ours
            // to signal: none of them is waited for.
            asked.tree = self
                .process_tree
                .ask_to_exit(Subject::Whole, self.process_group_id)
                .ok()
                .filter(|asked| !asked.failed());
        }
        asked
    }

    /// Whether nothing asked to end can still answer: the pane's process group
    /// and the rest of its tree have no process left to signal. A program that
    /// has ended is reaped here, which is how the group comes to be empty. One
    /// that cannot be signalled is one that is ending (it waits on a write the
    /// pane has blocked), as good as gone to a close that ends it next. On
    /// Windows nothing was asked, so there is nothing to wait for.
    #[cfg_attr(not(unix), allow(unused_variables))]
    fn is_gone(&mut self, asked: &Asked) -> bool {
        let _ = has_exited(self);
        #[cfg(unix)]
        if asked.group.is_some_and(group_exists) {
            return false;
        }
        #[cfg(target_os = "macos")]
        if asked.tree.as_ref().is_some_and(|tree| tree.any_alive()) {
            return false;
        }
        true
    }
}

/// Waits until everything asked to end has, or `deadline` passes.
fn wait_until(pane: &mut Pane, asked: &Asked, deadline: Instant) {
    wait_until_gone(deadline, || pane.is_gone(asked));
}

/// Waits until `gone` says everything asked to end has, or `deadline` passes: what
/// a close waits with, and an ending of what a turn left.
pub(super) fn wait_until_gone(deadline: Instant, mut gone: impl FnMut() -> bool) {
    loop {
        if gone() {
            return;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        std::thread::sleep(LOOK_EVERY.min(left));
    }
}

/// Asks every pane to end, and gives them the grace together: one that ignores
/// the signals does not make the rest wait for it twice. What is left after is
/// each pane's to end ([`terminate_asked`]).
pub(super) fn ask_all_to_exit<'a>(
    panes: impl Iterator<Item = &'a mut Pane>,
    grace: Duration,
) -> Vec<(&'a mut Pane, Asked)> {
    let deadline = Instant::now() + grace;
    let mut asked: Vec<(&mut Pane, Asked)> = panes
        .map(|pane| {
            let asked = pane.ask_to_exit();
            (pane, asked)
        })
        .collect();
    for (pane, asked) in &mut asked {
        wait_until(pane, asked, deadline);
    }
    asked
}

/// Ends a pane's programs: asked to end, given `grace`, and what is left killed.
pub(super) fn terminate(pane: &mut Pane, grace: Duration) -> Result<(), PaneError> {
    let asked = pane.ask_to_exit();
    wait_until(pane, &asked, Instant::now() + grace);
    terminate_asked(pane)
}

/// Ends what is left of a pane's programs after they were asked to end.
pub(super) fn terminate_asked(pane: &mut Pane) -> Result<(), PaneError> {
    let child_exited = signal_for_termination(pane, false)?;

    if !child_exited {
        wait_for_exit(pane)?;
    }
    pane.alive = false;
    Ok(())
}

/// [`terminate`] for a pane a write is still in progress in: asked and given
/// `grace` as any, but what is left is ended without waiting for the exit,
/// which a reaper confirms later (`teardown` says when).
pub(super) fn terminate_detached(
    mut pane: Pane,
    teardown: Arc<Teardown>,
    grace: Duration,
) -> Result<(), PaneError> {
    let asked = pane.ask_to_exit();
    wait_until(&mut pane, &asked, Instant::now() + grace);
    let child_exited = match signal_for_termination(&mut pane, true) {
        Ok(exited) => exited,
        Err(error) => {
            teardown.resolve(false);
            return Err(error);
        }
    };
    if child_exited {
        pane.alive = false;
        teardown.resolve(true);
        return Ok(());
    }

    let reaper = Arc::clone(&teardown);
    if let Err(error) = std::thread::Builder::new()
        .name("consensflow-pty-reaper".to_string())
        .spawn(move || reaper.resolve(wait_for_exit(&mut pane).is_ok()))
    {
        teardown.resolve(false);
        return Err(PaneError::Io(error));
    }
    Ok(())
}

/// What is left of a pane's programs, ended by force. `detached`: the caller
/// does not wait for the exit (a write is still in progress in the pane), and
/// a reaper confirms it later.
fn signal_for_termination(pane: &mut Pane, detached: bool) -> Result<bool, PaneError> {
    #[cfg(not(target_os = "macos"))]
    let _ = detached;
    #[cfg(target_os = "macos")]
    pane.process_tree.terminate(Subject::Whole)?;

    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut child_exited = has_exited(pane)?;

    #[cfg(unix)]
    if let Some(process_group_id) = pane.process_group_id {
        if let Err(error) = signal_group(pane, process_group_id, SIGKILL) {
            // macOS answers EPERM when a member of the group cannot take the
            // signal: here one already killed with the tree above, whose exit
            // waits on the pane's blocked write. A caller that does not wait
            // hands that exit to the reaper; one that waits keeps the check.
            #[cfg(target_os = "macos")]
            if detached && is_permission_denied(&error) {
                return Ok(child_exited);
            }
            if is_permission_denied(&error) {
                let deadline = Instant::now() + Duration::from_millis(100);
                while !child_exited && Instant::now() < deadline {
                    child_exited = has_exited(pane)?;
                    if !child_exited {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
            }
            if !child_exited || !is_permission_denied(&error) {
                return Err(error);
            }
        }
    }

    #[cfg(windows)]
    if !child_exited {
        pane.job.terminate()?;
    }

    Ok(child_exited)
}

#[cfg(unix)]
fn is_permission_denied(error: &PaneError) -> bool {
    matches!(error, PaneError::Io(error) if error.kind() == io::ErrorKind::PermissionDenied)
}

#[cfg(unix)]
const SIGHUP: i32 = 1;
#[cfg(unix)]
const SIGKILL: i32 = 9;
#[cfg(unix)]
const SIGTERM: i32 = 15;

/// Signals the pane's process group with `signal`. On macOS it goes through the pane's
/// tree and its bound, which sends it to the group whole while the pane's program runs
/// and leads it, and else to each member of the group the bound lets through: the
/// number of a program that has gone may be another process's now, which leads a group of
/// its own. Elsewhere there is no table of processes to look at, and the group is
/// signalled whole.
#[cfg(unix)]
fn signal_group(pane: &Pane, group: i32, signal: i32) -> Result<(), PaneError> {
    #[cfg(target_os = "macos")]
    {
        pane.process_tree
            .signal_group(group, signal)
            .map_err(PaneError::Io)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pane;
        signal_process_group(group, signal)
    }
}

/// Signals the pane's process group with `signal`, whole.
#[cfg(all(unix, not(target_os = "macos")))]
fn signal_process_group(process_group_id: i32, signal: i32) -> Result<(), PaneError> {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }

    // SAFETY: portable-pty created the child as its own session leader. A
    // negative pid addresses that process group, and the signals sent here
    // need no handler of ours.
    if unsafe { kill(-process_group_id, signal) } == 0 {
        return Ok(());
    }

    let error = io::Error::last_os_error();
    const ESRCH: i32 = 3;
    if error.kind() == io::ErrorKind::NotFound || error.raw_os_error() == Some(ESRCH) {
        Ok(())
    } else {
        Err(PaneError::Io(error))
    }
}

/// Whether the process group still has a process in it that takes a signal.
#[cfg(unix)]
fn group_exists(process_group_id: i32) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }

    // SAFETY: signal 0 delivers nothing: it only asks whether the group has a
    // process to deliver to.
    unsafe { kill(-process_group_id, 0) == 0 }
}

#[cfg(test)]
mod tests;

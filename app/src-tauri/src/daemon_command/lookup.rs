//! The login shells a start of the daemon asks, held so that the app's quit can end them.

use std::path::Path;
use std::process::{Child, ChildStdout, Command, ExitStatus};
use std::sync::{Mutex, MutexGuard};

use crate::diag;

/// The login shell a start of the daemon is asking for its PATH, held while it
/// asks, so that the app's quit ends it: a shell that never answered outlived
/// the app (the reviews of 2026-10-16). Only the shell, the lookup's own child,
/// is ever ended: what its login files started (an `ssh-agent -D`, a
/// `gpg-agent --no-detach` other programs use) is the user's, as when a
/// terminal's shell ends, and so is anything a login file put in a session of
/// its own to go on with.
#[derive(Default)]
pub(crate) struct LoginLookup {
    asking: Mutex<Asking>,
}

/// The shells asked, each by the number its start was given, and whether the app is
/// quitting.
#[derive(Default)]
pub(super) struct Asking {
    pub(super) shells: Vec<(u64, Child)>,
    next: u64,
    ended: bool,
}

/// A shell a lookup started: the number it is held by, and its output, to read.
pub(crate) struct Started {
    pub(crate) asked: u64,
    pub(crate) output: ChildStdout,
}

impl LoginLookup {
    pub(super) fn lock(&self) -> MutexGuard<'_, Asking> {
        self.asking
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    /// Starts `shell` and holds it beside any other asked, in one step against the
    /// app's quit; none once the app is quitting, when no shell is started, or where it
    /// cannot be.
    pub(super) fn start(&self, shell: &mut Command) -> Option<Started> {
        let mut asking = self.lock();
        if asking.ended {
            return None;
        }
        let mut started = match shell.spawn() {
            Ok(started) => started,
            Err(error) => {
                diag::say(format!(
                    "consensflow: the login shell could not be started ({}): {error}",
                    Path::new(shell.get_program()).display()
                ));
                return None;
            }
        };
        let Some(output) = started.stdout.take() else {
            let _ = started.kill();
            let _ = started.wait();
            return None;
        };
        let asked = asking.next;
        asking.next += 1;
        asking.shells.push((asked, started));
        Some(Started { asked, output })
    }

    /// How the shell held as `asked` ended: none while it runs.
    pub(super) fn status(&self, asked: u64) -> std::io::Result<Option<ExitStatus>> {
        match self
            .lock()
            .shells
            .iter_mut()
            .find(|(held, _)| *held == asked)
        {
            Some((_, shell)) => shell.try_wait(),
            None => Err(std::io::ErrorKind::NotFound.into()),
        }
    }

    /// The lookup is over: the shell is ended if it still runs, under the lock,
    /// and only then let go and waited for. A shell let go first was one a quit
    /// could find nowhere, and the app could end before anything ended it (the
    /// third review of 2026-10-16).
    pub(super) fn finish(&self, asked: u64) {
        self.finish_with(asked, || {});
    }

    /// [`LoginLookup::finish`], with `between` run once the lock is let go and
    /// before the shell is waited for: where the app's quit may come, and the
    /// app end.
    pub(super) fn finish_with(&self, asked: u64, between: impl FnOnce()) {
        let shell = {
            let mut asking = self.lock();
            let at = asking.shells.iter().position(|(held, _)| *held == asked);
            at.map(|at| asking.shells.remove(at).1).map(|mut shell| {
                let _ = shell.kill();
                shell
            })
        };
        between();
        if let Some(mut shell) = shell {
            let _ = shell.wait();
        }
    }

    /// The app is quitting: every shell asked ends now, and no other is started.
    pub(crate) fn end(&self) {
        let mut asking = self.lock();
        asking.ended = true;
        for (_, shell) in &mut asking.shells {
            let _ = shell.kill();
        }
    }
}

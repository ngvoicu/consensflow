//! What a program run to its end (`capture`, and `execute` with it) leads: on
//! Unix a process group of its own, so that ending it ends what it started
//! too (the children of Homebrew and of npm, which an update runs through),
//! not the program alone. Windows has no groups: `taskkill /T` ends a tree,
//! and the job ends every child this process has when this process ends.
//!
//! The group is the program's to end until the program has been waited for,
//! and not after: a program that is waited for is reaped, and the id it led
//! the group by is free for the system to give to another as soon as nothing
//! is left of the group. Until then every way the program ends reaches the
//! group: this process's exit path (the [`Ender`] handed out), its time
//! running out, and the wait for it dropped. What the program left running
//! once it was waited for is let go of, as with a program that exited after
//! it detached its children.

use std::cell::Cell;
use std::rc::Rc;

use crate::terminate::{end, Reach};
use crate::{Ender, Ending};

/// Starts `command` as the leader of a process group of its own, which has
/// its pid for an id.
#[cfg(unix)]
pub(crate) fn lead(command: &mut tokio::process::Command) {
    command.process_group(0);
}

/// Windows has no groups: its job and `taskkill /T` hold the tree.
#[cfg(not(unix))]
pub(crate) fn lead(_command: &mut tokio::process::Command) {}

/// The program a capture started, and the group it leads: the capture's to
/// end until it has [released](Group::release) them, and ended by this one's
/// drop before.
pub(crate) struct Group {
    pid: Option<u32>,
    /// Whether the program has been waited for: its pid, and the id of its
    /// group, may be another's now. The enders hold it weakly, so that a
    /// capture dropped is told to them as no program to end.
    exited: Rc<Cell<bool>>,
}

impl Group {
    /// The group of the program `pid`, which has just started.
    pub(crate) fn new(pid: Option<u32>) -> Self {
        Self {
            pid,
            exited: Rc::default(),
        }
    }

    /// What ends the group later, should this process be ending.
    pub(crate) fn ender(&self) -> Ender {
        Ender::new(self.pid, Reach::Group, &self.exited)
    }

    /// Asks or forces the program, and what it started, to end: on Unix every
    /// process of the group gets the signal `how` names (SIGTERM is what
    /// Node's `kill` sends, to the program alone); on Windows the whole tree
    /// goes whatever `how` asks, where Node ends the program alone (and a
    /// `.cmd`'s own program, left running, keeps its streams open). Nothing
    /// is sent once it was waited for.
    pub(crate) fn end(&self, how: Ending) {
        if let (Some(pid), false) = (self.pid, self.exited.get()) {
            end(pid, how, Reach::Group, true);
        }
    }

    /// The program has been waited for: nothing is sent to its pid, nor to
    /// its group's id, after this.
    pub(crate) fn release(&self) {
        self.exited.set(true);
    }
}

impl Drop for Group {
    /// A capture dropped while its program runs: `kill_on_drop` ends the
    /// program, and this ends what it started with it. Dropped before the
    /// child that holds the program (declared after it), the group is
    /// signalled while the program is not yet reaped. On Windows the program
    /// alone goes, as `kill_on_drop` ends it.
    fn drop(&mut self) {
        #[cfg(unix)]
        self.end(Ending::Forced);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::process::Child;
    use std::time::Duration;

    use super::*;
    use crate::testing::Survivors;

    /// A sleep that leads a group of its own: a signal sent to the process or
    /// to its group ends it.
    #[allow(clippy::disallowed_methods, clippy::unwrap_used)] // The test starts what it ends.
    fn sleeper() -> Child {
        std::process::Command::new("/bin/sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .unwrap()
    }

    #[test]
    fn what_a_group_holds_is_ended_by_its_end_and_by_its_drop() {
        let mut ended = sleeper();
        Group::new(Some(ended.id())).end(Ending::Forced);
        assert_eq!(ended.wait().unwrap().signal(), Some(libc::SIGKILL));

        // Dropped before it was released: the capture was cancelled.
        let mut dropped = sleeper();
        drop(Group::new(Some(dropped.id())));
        assert_eq!(dropped.wait().unwrap().signal(), Some(libc::SIGKILL));
    }

    #[test]
    fn once_released_nothing_is_sent_by_its_end_its_ender_or_its_drop() {
        let mut waited_for = sleeper();
        let _left = Survivors(vec![waited_for.id()]);
        let group = Group::new(Some(waited_for.id()));
        let ender = group.ender();
        assert!(ender.running());
        group.release();
        assert!(!ender.running());
        group.end(Ending::Forced);
        ender.force();
        drop(group);
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            waited_for.try_wait().unwrap().is_none(),
            "it was sent nothing: its id may be another's"
        );
        waited_for.kill().unwrap();
        waited_for.wait().unwrap();
    }

    #[test]
    fn a_group_dropped_is_no_one_s_to_end() {
        let mut sleeper = sleeper();
        let _left = Survivors(vec![sleeper.id()]);
        let ender = Group::new(Some(sleeper.id())).ender();
        // The group went with its drop, which ended the program: the ender
        // that outlives it has no program to end.
        assert_eq!(sleeper.wait().unwrap().signal(), Some(libc::SIGKILL));
        assert!(!ender.running());
        ender.force();
    }
}

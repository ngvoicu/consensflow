//! What the windows of a live case left running once they were closed, found and ended by
//! the kit and said: a Monitor armed on `tail -f … | grep tick` outlived its Claude Code by
//! seventeen hours (T-491), and nothing of a live case looked, for a live window has no stand-in
//! to note its processes (`pids.jsonl` of the rig is the stand-ins' own).
//!
//! A process is a window's by descent from the window's own process, which the pane host named
//! as it opened the window, read once and kept ([`Watch::look`]: the tree is taken while the
//! window runs, each process by its number and when it began); or, where the window has gone and
//! what it ran has no parent left (a Mac's init has it, 1), by being an orphan of the folder the
//! window ran in that began no earlier than the window ([`orphans_in_folder`]). It is ended by
//! which process it is ([`end_chosen`]), never by a pattern or a name, and what is said is what
//! was found, what was ended and what could not be.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::json;

use super::descent::{
    descending_from, end_chosen, orphans_in_folder, working_folder, Ended, Identity, Machine,
    Running, ThisMachine,
};
use crate::live::cut;
use crate::rig::Rig;

/// How long what the closed windows left is looked for again while it is still ending: a
/// process the pane host has killed is listed for a moment longer, and is no leftover.
const SETTLE: Duration = Duration::from_secs(3);

/// How often it is looked at while it settles.
const LOOK: Duration = Duration::from_millis(250);

/// One window the case opened: its process, the folder it ran in, and what descended from it
/// at the looks made while it ran.
#[derive(Debug, Clone)]
struct Watched {
    root: Identity,
    folder: PathBuf,
    tree: Vec<Running>,
}

/// The windows of a case, watched until they are closed ([`Watch::sweep`] then says and ends
/// what is left of them).
#[derive(Debug, Clone, Default)]
pub struct Watch {
    windows: Vec<Watched>,
}

impl Watch {
    /// A watch of no window yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// A watch of every window the pane host of `rig` opened a process for: the daemon's
    /// (their folders are the `cwd` of the `pane.open` it asked) and the kit's own, whose folder
    /// is `own_folder` where the rig has no request of its (a window the case opened itself);
    /// looked at once now, while they run. A window whose process is not known, or whose folder
    /// is not, is not watched.
    pub fn of_rig(rig: &Rig, own_folder: Option<&Path>) -> Self {
        let frames = rig.open_frames();
        let mut watch = Self::new();
        for (id, root) in rig.window_roots() {
            let asked = frames
                .iter()
                .rev()
                .find(|frame| frame["id"] == json!(id))
                .and_then(|frame| frame["cwd"].as_str())
                .map(PathBuf::from);
            if let Some(folder) = asked.or_else(|| own_folder.map(Path::to_path_buf)) {
                watch.add(root, &folder);
            }
        }
        watch.look_now();
        watch
    }

    /// How many windows are watched.
    pub fn windows(&self) -> usize {
        self.windows.len()
    }

    /// How many processes the looks saw so far, the windows' own among them.
    pub fn seen(&self) -> usize {
        self.windows.iter().map(|window| window.tree.len()).sum()
    }

    /// A [`Swept`] that says how much was watched: nothing found, nothing ended.
    fn counted(&self) -> Swept {
        Swept {
            watched: (self.windows(), self.seen()),
            ..Swept::default()
        }
    }

    /// Watches the window whose process is `root`, which runs in `folder`.
    pub fn add(&mut self, root: Identity, folder: &Path) {
        if !self.windows.iter().any(|window| window.root == root) {
            self.windows.push(Watched {
                root,
                folder: folder.to_path_buf(),
                tree: Vec::new(),
            });
        }
    }

    /// One look at the machine: what descends from each window's process now is kept, each
    /// process by which one it is. A machine that cannot be read adds nothing.
    pub fn look_now(&mut self) {
        if let Ok(all) = ThisMachine.processes() {
            self.look(&all);
        }
    }

    /// [`Watch::look_now`], as `all` lists the machine's processes.
    pub fn look(&mut self, all: &[Running]) {
        for window in &mut self.windows {
            for process in all
                .iter()
                .filter(|process| process.identity == window.root)
                .cloned()
                .chain(descending_from(all, window.root))
            {
                if !window
                    .tree
                    .iter()
                    .any(|known| known.identity == process.identity)
                {
                    window.tree.push(process);
                }
            }
        }
    }

    /// What of the watched windows is among `all`, the machine's processes, now: each process the
    /// looks saw descend from a window (or the window's own) that still is the process it was,
    /// and each orphan of a window's folder that began no earlier than the window, as `folder_of`
    /// says where a process runs, with what it started; each once. A process of another run,
    /// whatever its command line says, is none of them.
    pub fn left(
        &self,
        all: &[Running],
        folder_of: impl Fn(u32) -> Option<PathBuf>,
    ) -> Vec<Running> {
        let mut left: Vec<Running> = Vec::new();
        let mut add = |process: Running| {
            if !left.iter().any(|known| known.identity == process.identity) {
                left.push(process);
            }
        };
        for window in &self.windows {
            for seen in &window.tree {
                if let Some(now) = all.iter().find(|process| process.identity == seen.identity) {
                    add(now.clone());
                }
            }
            for orphan in
                orphans_in_folder(all, "", &window.folder, window.root.started, &folder_of)
            {
                add(orphan);
            }
        }
        left
    }

    /// Finds what the windows left, after they were closed, ends it and says it. What is
    /// listed is looked at again for a moment while it ends, so that a process on its way out
    /// is not one that was left.
    pub fn sweep(&self) -> Swept {
        self.sweep_by(&ThisMachine, working_folder, end_chosen, SETTLE)
    }

    /// [`Watch::sweep`], the machine listed by `machine`, a process's folder said by `folder_of`,
    /// each ended by `end`, and `settle` the time it is looked at again.
    pub fn sweep_by(
        &self,
        machine: &dyn Machine,
        folder_of: impl Fn(u32) -> Option<PathBuf>,
        end: impl Fn(&[Running]) -> Ended,
        settle: Duration,
    ) -> Swept {
        let until = Instant::now() + settle;
        loop {
            match machine.processes() {
                Err(failed) => {
                    return Swept {
                        unread: Some(failed.to_string()),
                        ..self.counted()
                    }
                }
                Ok(all) => {
                    let found = self.left(&all, &folder_of);
                    if found.is_empty() || Instant::now() >= until {
                        let ended = end(&found);
                        return Swept {
                            found,
                            ended,
                            ..self.counted()
                        };
                    }
                }
            }
            thread::sleep(LOOK);
        }
    }
}

/// What the closed windows left: the processes found, what came of ending them, and why the
/// machine could not be read where it could not.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Swept {
    pub found: Vec<Running>,
    pub ended: Ended,
    pub unread: Option<String>,
    /// How many windows were watched, and how many processes the looks saw under them while
    /// they ran: a watch that saw none says nothing of what was left.
    pub watched: (usize, usize),
}

impl Swept {
    /// Whether the windows left nothing, and the machine could be read to say so.
    pub fn is_clean(&self) -> bool {
        self.found.is_empty() && self.unread.is_none()
    }

    /// What it came to, in a line: nothing left, or what was left and what the kit did with it.
    pub fn said(&self) -> String {
        if let Some(why) = &self.unread {
            return format!(
                "the machine's processes could not be read to see what the windows left: {why}"
            );
        }
        if self.found.is_empty() {
            return format!(
                "the windows left nothing running ({} window(s) watched, {} process(es) seen under them while they ran)",
                self.watched.0, self.watched.1
            );
        }
        let list = |processes: &[Running]| {
            processes
                .iter()
                .map(|process| format!("{} {}", process.identity.pid, cut(&process.command, 70)))
                .collect::<Vec<_>>()
                .join("; ")
        };
        let mut said = format!(
            "the windows left {} process(es) running after they were closed: {}; the kit ended them by which process each is",
            self.found.len(),
            list(&self.found)
        );
        said.push_str(&format!(
            ": {} ended, {} gone already",
            self.ended.ended.len(),
            self.ended.gone.len()
        ));
        for problem in self.ended.problems() {
            said.push_str(&format!("; {problem}"));
        }
        said
    }

    /// What may still run or could not be told, a problem each.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = self.ended.problems();
        if let Some(why) = &self.unread {
            problems.push(format!(
                "the machine's processes could not be read to see what the windows left: {why}"
            ));
        }
        problems
    }
}

#[cfg(test)]
mod tests;

//! What the windows of a live case left running once they were closed, found by the kit and
//! said, and never ended: a Monitor armed on `tail -f … | grep tick` outlived its Claude Code by
//! seventeen hours (T-491), and nothing of a live case looked, for a live window has no stand-in
//! to note its processes (`pids.jsonl` of the rig is the stand-ins' own).
//!
//! A process is a window's by descent from the window's own process, which the pane host named
//! as it opened the window, read once and kept ([`Watch::look`]: the tree is taken while the
//! window runs, each process by its number and when it began); or, where the window has gone and
//! what it ran has no parent left (a Mac's init has it, 1), by being an orphan of the folder the
//! window ran in that began no earlier than the window ([`orphans_in_folder`]), where that folder
//! is under the run's own temp root ([`RunFolder`]). What is said is what was found. The sweep
//! ends nothing: at 18:58 on 2026-11-12 it took a live rig's home, the owner's own, for a word of
//! the run and ended 22 processes of the owner's and of other tasks. The kit ends a process only
//! by descent from the run's window, the stand-in's jobs or the run's own word, where a case chose
//! it.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::json;

use super::descent::{
    descending_from, orphans_in_folder, working_folder, Identity, Machine, RunFolder, Running,
    ThisMachine,
};
use crate::live::cut;
use crate::rig::Rig;

/// How long what the closed windows left is looked for again while it is still ending: a
/// process the pane host has killed is listed for a moment longer, and is no leftover.
const SETTLE: Duration = Duration::from_secs(3);

/// How often it is looked at while it settles.
const LOOK: Duration = Duration::from_millis(250);

/// One window the case opened: its process, the folder it ran in where that is the run's own
/// ([`RunFolder`]), and what descended from it at the looks made while it ran.
#[derive(Debug, Clone)]
struct Watched {
    root: Identity,
    folder: Option<RunFolder>,
    tree: Vec<Running>,
}

/// The windows of a case, watched until they are closed ([`Watch::sweep`] then says what is
/// left of them).
#[derive(Debug, Clone, Default)]
pub struct Watch {
    windows: Vec<Watched>,
    /// Words of the run's own, which the command line of what the kit asked a window to run or
    /// watch holds: the folders of the run's it was made to run in, under the run's temp root
    /// ([`RunFolder`]), and the word made for the run. A process whose parent has gone, that
    /// began no earlier than the run's first window and that holds one is what the windows left,
    /// wherever it went, on every platform ([`Watch::add_word`]).
    words: Vec<String>,
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
    /// is not, is not watched. A folder (the window's, the rig's home) is a ground of the run's
    /// only where it is under the rig's own temp root ([`Rig::run_folder`]): a live rig's home and
    /// project folder are the person's, and the window is then watched by descent and the run's
    /// word alone.
    pub fn of_rig(rig: &Rig, own_folder: Option<&Path>) -> Self {
        let frames = rig.open_frames();
        let windows: Vec<(Identity, PathBuf)> = rig
            .window_roots()
            .into_iter()
            .filter_map(|(id, root)| {
                let asked = frames
                    .iter()
                    .rev()
                    .find(|frame| frame["id"] == json!(id))
                    .and_then(|frame| frame["cwd"].as_str())
                    .map(PathBuf::from);
                asked
                    .or_else(|| own_folder.map(Path::to_path_buf))
                    .map(|folder| (root, folder))
            })
            .collect();
        let home = rig.var("HOME").filter(|home| !home.is_empty());
        let mut watch =
            Self::of_windows(&|path| rig.run_folder(path), &windows, home.map(Path::new));
        watch.look_now();
        watch
    }

    /// The watch of `windows` (each window's process and the folder it runs in) and of the rig's
    /// `home`, a folder being the run's where `run_folder` says it is ([`Rig::run_folder`]): its
    /// words, and the folder rule of its window, are the run's then alone. And the word made for
    /// the run ([`super::run_word`]).
    fn of_windows(
        run_folder: &dyn Fn(&Path) -> Option<RunFolder>,
        windows: &[(Identity, PathBuf)],
        home: Option<&Path>,
    ) -> Self {
        let mut watch = Self::new();
        for (root, folder) in windows {
            let folder = run_folder(folder);
            if let Some(folder) = &folder {
                watch.add_folder_words(folder);
            }
            watch.add(*root, folder);
        }
        if let Some(home) = home.and_then(run_folder) {
            watch.add_folder_words(&home);
        }
        watch.add_word(super::run_word());
        watch
    }

    /// Counts a folder of the run's as words: its path as it was given, its real path, and the
    /// path as Claude Code names a project's folder in the files it keeps (every character that
    /// is no letter or digit a dash), which the command line of a program that reads one holds.
    /// A [`RunFolder`] alone: a path outside the run's temp root is never a word of the run's.
    fn add_folder_words(&mut self, folder: &RunFolder) {
        for path in [folder.given(), folder.real()] {
            let text = path.to_string_lossy().into_owned();
            self.push_word(&text);
            self.push_word(
                &text
                    .chars()
                    .map(|at| if at.is_alphanumeric() { at } else { '-' })
                    .collect::<String>(),
            );
        }
    }

    /// How many windows are watched.
    pub fn windows(&self) -> usize {
        self.windows.len()
    }

    /// How many processes the looks saw so far, the windows' own among them.
    pub fn seen(&self) -> usize {
        self.windows.iter().map(|window| window.tree.len()).sum()
    }

    /// A [`Swept`] that says how much was watched: nothing found.
    fn counted(&self) -> Swept {
        Swept {
            watched: (self.windows(), self.seen()),
            ..Swept::default()
        }
    }

    /// Counts `word` for the run's own, as the command line of a process that holds it: the word
    /// made for the run ([`super::run_word`]). One that holds a path, which is a folder's word
    /// and a [`RunFolder`]'s alone ([`Watch::add_folder_words`]), is not counted.
    fn add_word(&mut self, word: &str) {
        if !word.contains(['/', '\\']) {
            self.push_word(word);
        }
    }

    /// Counts `word`, unless it is too short to be nobody else's.
    fn push_word(&mut self, word: &str) {
        if word.len() >= 8 && !self.words.iter().any(|known| known == word) {
            self.words.push(word.to_owned());
        }
    }

    /// Watches the window whose process is `root`, which runs in `folder` where that is the
    /// run's own.
    pub fn add(&mut self, root: Identity, folder: Option<RunFolder>) {
        if !self.windows.iter().any(|window| window.root == root) {
            self.windows.push(Watched {
                root,
                folder,
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
    /// and each orphan of a window's folder of the run's that began no earlier than the earliest
    /// window of the run, as `folder_of` says where a process runs, with what it started; and, on
    /// every platform, each process whose parent has gone, that began no earlier than that window
    /// and whose command line holds a word of the run's own ([`Watch::add_word`]), with what it
    /// started; each once. A process of another run, or one that began before the run's first
    /// window, whatever else its command line says, is none of them.
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
        // A reopened member's old window is a window of the run: the bound is the earliest.
        let since = self
            .windows
            .iter()
            .map(|window| window.root.started)
            .min()
            .unwrap_or(u64::MAX);
        for window in &self.windows {
            for seen in &window.tree {
                if let Some(now) = all.iter().find(|process| process.identity == seen.identity) {
                    add(now.clone());
                }
            }
            if let Some(folder) = &window.folder {
                for orphan in orphans_in_folder(all, "", folder, since, &folder_of) {
                    add(orphan);
                }
            }
        }
        for process in all.iter().filter(|process| {
            parent_gone(all, process)
                && process.identity.started >= since
                && self
                    .words
                    .iter()
                    .any(|word| process.command.contains(word.as_str()))
        }) {
            for one in
                std::iter::once(process.clone()).chain(descending_from(all, process.identity))
            {
                add(one);
            }
        }
        left
    }

    /// Finds what the windows left, after they were closed, and says it; it ends nothing. What
    /// is listed is looked at again for a moment while it ends, so that a process on its way out
    /// is not one that was left.
    pub fn sweep(&self) -> Swept {
        self.sweep_by(&ThisMachine, working_folder, SETTLE)
    }

    /// [`Watch::sweep`], the machine listed by `machine`, a process's folder said by `folder_of`,
    /// and `settle` the time it is looked at again.
    pub fn sweep_by(
        &self,
        machine: &dyn Machine,
        folder_of: impl Fn(u32) -> Option<PathBuf>,
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
                        return Swept {
                            found,
                            ..self.counted()
                        };
                    }
                }
            }
            thread::sleep(LOOK);
        }
    }
}

/// Whether nothing runs that started `process`: its parent is init's, or is not among `all`, or
/// began after it (the number has another process now). A Mac's init has what is left; Windows
/// leaves the dead parent's number in the child's row.
fn parent_gone(all: &[Running], process: &Running) -> bool {
    process.ppid <= 1
        || !all.iter().any(|parent| {
            parent.identity.pid == process.ppid
                && parent.identity.started <= process.identity.started
        })
}

/// What the closed windows left: the processes found, none of them ended, and why the machine
/// could not be read where it could not.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Swept {
    pub found: Vec<Running>,
    pub unread: Option<String>,
    /// How many windows were watched, and how many processes the looks saw under them while
    /// they ran: a watch that saw none says nothing of what was left.
    pub watched: (usize, usize),
}

impl Swept {
    /// Whether a look that could have found a leftover was made: the machine was read, and a
    /// window was watched. Where none was, nothing is said of what was left.
    pub fn looked(&self) -> bool {
        self.unread.is_none() && self.watched.0 > 0
    }

    /// Whether the windows left nothing, and a look that could have found it was made
    /// ([`Swept::looked`]): never where the sweep could not look.
    pub fn is_clean(&self) -> bool {
        self.found.is_empty() && self.looked()
    }

    /// What it came to, in a line: nothing left, or what was left, which the kit left as it is.
    pub fn said(&self) -> String {
        if let Some(why) = &self.unread {
            return format!(
                "the machine's processes could not be read to see what the windows left: {why}"
            );
        }
        if self.watched.0 == 0 {
            return "the sweep could not look: no window of the run was watched (the pane host named no process of any), so nothing is said of what the windows left".to_owned();
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
        format!(
            "the windows left {} process(es) running after they were closed: {}; the kit ended none of them (it ends a process only by descent from the run's window, the stand-in's jobs or the run's own word)",
            self.found.len(),
            list(&self.found)
        )
    }

    /// What could not be told, a problem each: the machine that could not be read.
    pub fn problems(&self) -> Vec<String> {
        self.unread
            .iter()
            .map(|why| {
                format!(
                    "the machine's processes could not be read to see what the windows left: {why}"
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;

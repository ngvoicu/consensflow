//! Git Bash's own programs and the parents Git Bash says they have.
//!
//! A program of Git Bash's own kind (`sleep`, `grep`, `ls`: they run on Cygwin's layer) is
//! started from a script by a forked copy of the shell, which starts it and has gone when
//! anyone looks (zeewin, 2026-10-21: the sleep of `sh ./script` names a parent that no process
//! has; `exec sleep` and a job in the background do the same). So the parent Windows names for
//! it is no process of the window's tree, and the chain of processes that run, up from it to
//! the shell the harness started, which is what the host takes for a turn's work
//! ([`crate::background`]), is broken: a stop leaves it running, and nothing names it, until
//! the window closes. `ps -W` of Git Bash's own folder says what its parent was (its Cygwin
//! number, and so the shell that is its parent, with the number Windows has for it); where
//! Windows lists that shell, the shell is the parent it has.
//!
//! This is the whole of it, read where the system says nothing: a parent is given only on
//! what Git Bash says, never guessed from when the program began or what it runs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One row of `ps -W`: a process by its Cygwin number, its parent's, and its Windows number.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub pid: u32,
    pub ppid: u32,
    pub winpid: u32,
}

/// What `ps -W` says, read for the first four of its columns (`PID PPID PGID WINPID`) of
/// each row: the header and any line that is no row say nothing. Cygwin's `ps` begins a row
/// with a status letter (`S` stopped, `I` reading its terminal, `O` writing it) or a space,
/// then the number in seven columns (`"%c %7d %7d %7d %10u …"`), so a first word of one letter
/// is the status and the number is the next: a process that reads its terminal is a row all
/// the same, and so is the shell above it (T-194, P3-1).
pub(crate) fn table(listing: &str) -> Vec<Row> {
    listing
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let first = words.next()?;
            let first =
                if first.len() == 1 && first.chars().all(|letter| letter.is_ascii_alphabetic()) {
                    words.next()?
                } else {
                    first
                };
            let mut numbers = std::iter::once(first)
                .chain(words)
                .map(|word| word.parse::<u32>().ok());
            let (pid, ppid, _group, winpid) = (
                numbers.next()??,
                numbers.next()??,
                numbers.next()??,
                numbers.next()??,
            );
            Some(Row { pid, ppid, winpid })
        })
        .collect()
}

/// Whether `ps -W` is run from `folder`: a `ps.exe` is there and so is the Cygwin layer it
/// runs on, `msys-2.0.dll` or `cygwin1.dll`, beside it (`exists` says what is there). A
/// `ps.exe` without it is no Git Bash's, and is not run (T-194, P3-7).
pub(crate) fn runs_ps_in(folder: &Path, exists: impl Fn(&Path) -> bool) -> bool {
    exists(&folder.join("ps.exe"))
        && ["msys-2.0.dll", "cygwin1.dll"]
            .iter()
            .any(|layer| exists(&folder.join(layer)))
}

/// The Windows number of the shell that `table` says started the process with the Windows
/// number `winpid`: none where Git Bash does not know the process, or its parent has none.
pub(crate) fn parent_of(table: &[Row], winpid: u32) -> Option<u32> {
    let own = table.iter().find(|row| row.winpid == winpid)?;
    let parent = table.iter().find(|row| row.pid == own.ppid)?;
    (parent.winpid != 0).then_some(parent.winpid)
}

/// The parent Git Bash says for each of `orphans` (a process by its Windows number, and the
/// folder its program is in): `ps` is asked once for each folder, and says its text or none
/// (no `ps` there, or it did not answer). A process it says nothing of is not in the answer.
pub(crate) fn parents_of(
    orphans: &[(u32, PathBuf)],
    mut ps: impl FnMut(&Path) -> Option<String>,
) -> HashMap<u32, u32> {
    let mut tables: HashMap<&Path, Vec<Row>> = HashMap::new();
    let mut parents = HashMap::new();
    for (winpid, folder) in orphans {
        let rows = tables
            .entry(folder.as_path())
            .or_insert_with(|| ps(folder).map(|text| table(&text)).unwrap_or_default());
        if let Some(parent) = parent_of(rows, *winpid) {
            parents.insert(*winpid, parent);
        }
    }
    parents
}

#[cfg(test)]
mod tests;

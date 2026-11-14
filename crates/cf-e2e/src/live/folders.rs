//! The project folders a live case makes in the live folder (`~/.consensflow-candidate/live`),
//! and when they go.
//!
//! Every case asks for a folder of its own ([`super::machine::live_folder_in`]: `<name>-<random>`,
//! made through the door), and a second ask of the same name in this process is given the same
//! folder, so that a restart, a resumed window or a second step of the case works where the first
//! did. A run ([`super::Log`], with every scope of it) holds a [`Lease`] on this process's [`Pool`]
//! for as long as it lives. When the last lease is let go the folders made while one was held are
//! dealt with, as a whole and not before: a sub-case or a resume that still holds a lease is still
//! working in the folder.
//!
//! - Every run came to a pass ([`Lease::passed`]: the report of the run says so, or the case
//!   does) and `--keep` is not given: each folder is taken away by the door's receipt
//!   ([`crate::door::Door::remove_made`]), and the folders are asked for anew by the next run.
//! - A run that did not come to a pass (a panic, an error it returned, checks that did not hold:
//!   nothing says it passed), a say-only run, or `--keep`: each folder stays, marked kept
//!   ([`crate::door::KEPT`]), and the run says where.
//!
//! Only the live folder of the machine's own home is dealt with: a folder in a home that a test
//! made for itself goes with that home.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::door::Door;

/// What this process's runs share: the folders allocated, and who still works in them.
pub struct Pool {
    state: Mutex<State>,
}

struct State {
    /// The folders allocated, by the live folder (by its real path) and the first name asked.
    allocated: BTreeMap<(PathBuf, String), PathBuf>,
    /// Those of them that are dealt with when the last lease is let go, in the order they were made.
    dealt_with: Vec<PathBuf>,
    /// The leases held now.
    leases: usize,
    /// Whether a run that let its lease go did not come to a pass since the leases were none.
    not_passed: bool,
}

impl Pool {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(State {
                allocated: BTreeMap::new(),
                dealt_with: Vec::new(),
                leases: 0,
                not_passed: false,
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A lease on the pool, held until it is dropped. The run it is of passes with
    /// [`Lease::passed`].
    pub fn lease(&'static self) -> Lease {
        self.state().leases += 1;
        Lease {
            pool: self,
            passed: AtomicBool::new(false),
        }
    }

    /// The folder for `first` in the live folder `root`: the one this process allocated for it
    /// where it is still there, else the one `make` makes. `deal_with` says that the folder is
    /// dealt with when the last lease is let go, if a lease is held now.
    pub fn folder<E>(
        &self,
        root: &Path,
        first: &str,
        deal_with: bool,
        make: impl FnOnce() -> Result<PathBuf, E>,
    ) -> Result<PathBuf, E> {
        let key = (
            std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf()),
            first.to_owned(),
        );
        let mut state = self.state();
        if let Some(folder) = state.allocated.get(&key) {
            if crate::door::is_made(folder) {
                return Ok(folder.clone());
            }
        }
        let folder = make()?;
        state.allocated.insert(key, folder.clone());
        if deal_with && state.leases > 0 {
            state.dealt_with.push(folder.clone());
        }
        Ok(folder)
    }

    /// The folders that will be dealt with, as a test reads them.
    #[cfg(test)]
    pub fn waiting(&self) -> Vec<PathBuf> {
        self.state().dealt_with.clone()
    }

    fn release(&self, passed: bool, kept_by_flag: bool) {
        let (folders, not_passed) = {
            let mut state = self.state();
            state.leases = state.leases.saturating_sub(1);
            state.not_passed |= !passed;
            if state.leases > 0 {
                return;
            }
            let not_passed = std::mem::take(&mut state.not_passed);
            let folders = std::mem::take(&mut state.dealt_with);
            // What is dealt with is asked for anew by the next run; the folder a test made in a
            // home of its own is no run's to deal with, and a test's other threads still ask for it.
            state
                .allocated
                .retain(|_, folder| !folders.contains(folder));
            (folders, not_passed)
        };
        if folders.is_empty() {
            return;
        }
        let door = Door::of_run();
        let says_only = door.says_only();
        if not_passed || kept_by_flag || says_only {
            for folder in &folders {
                door.keep_made(folder);
            }
            let why = if kept_by_flag {
                "--keep is given"
            } else if says_only {
                "the run is say-only"
            } else {
                "the run did not come to a pass"
            };
            say(&format!(
                "the live folders of this run stay, marked kept ({why}): {}",
                list(&folders)
            ));
            return;
        }
        for folder in &folders {
            let removed = door.remove_made(folder);
            if !removed.is_gone() {
                say(&format!("a live folder is left: {removed:?}"));
            }
        }
    }
}

impl Default for Pool {
    fn default() -> Self {
        Self::new()
    }
}

fn list(folders: &[PathBuf]) -> String {
    folders
        .iter()
        .map(|folder| folder.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn say(text: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "{text}");
}

/// A run's hold on the pool: the folders made while one is held wait until every one is let go.
pub struct Lease {
    pool: &'static Pool,
    passed: AtomicBool,
}

impl Lease {
    /// Says that the run came to a pass: its folders may be taken once the last run is done.
    pub fn passed(&self) {
        self.passed.store(true, Ordering::SeqCst);
    }

    /// Whether the run said that it came to a pass, as a test reads it.
    #[cfg(test)]
    pub fn has_passed(&self) -> bool {
        self.passed.load(Ordering::SeqCst)
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.pool
            .release(self.passed.load(Ordering::SeqCst), super::keep_requested());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool() -> &'static Pool {
        Box::leak(Box::new(Pool::new()))
    }

    /// A folder made through the door, in the folder `root`, as a case's first ask is.
    fn made_in(root: &Path, first: &str) -> std::io::Result<PathBuf> {
        Ok(crate::door::made_in(root, &format!("{first}-"))?.hand_on())
    }

    #[test]
    fn a_folder_asked_for_again_is_the_same_one_and_a_second_name_is_another() {
        let pool = pool();
        let root = crate::door::made("cf-test-").unwrap();
        let _lease = pool.lease();
        let a = pool
            .folder(root.path(), "a", true, || made_in(root.path(), "a"))
            .unwrap();
        let again = pool
            .folder(root.path(), "a", true, || -> std::io::Result<PathBuf> {
                panic!("asked for the same folder")
            })
            .unwrap();
        let b = pool
            .folder(root.path(), "b", true, || made_in(root.path(), "b"))
            .unwrap();
        assert_eq!(a, again);
        assert_ne!(a, b);
        assert_eq!(pool.waiting(), vec![a, b]);
    }

    #[test]
    fn a_run_that_passed_takes_its_folders_when_it_is_done() {
        let pool = pool();
        let root = crate::door::made("cf-test-").unwrap();
        let lease = pool.lease();
        let folder = pool
            .folder(root.path(), "case", true, || made_in(root.path(), "case"))
            .unwrap();
        std::fs::write(folder.join("left.txt"), "x").unwrap();
        lease.passed();
        drop(lease);
        assert!(!folder.exists(), "a passed run left {folder:?}");
        assert!(root.path().is_dir());
        assert!(pool.waiting().is_empty());
    }

    #[test]
    fn a_run_that_did_not_pass_leaves_its_folders_marked_kept() {
        let pool = pool();
        let root = crate::door::made("cf-test-").unwrap();
        let lease = pool.lease();
        let folder = pool
            .folder(root.path(), "case", true, || made_in(root.path(), "case"))
            .unwrap();
        drop(lease);
        assert!(folder.is_dir(), "a run that did not pass took {folder:?}");
        assert!(folder.join(crate::door::KEPT).is_file(), "not marked kept");
    }

    #[test]
    fn the_folders_wait_for_every_run_and_a_run_that_did_not_pass_keeps_them_all() {
        let pool = pool();
        let root = crate::door::made("cf-test-").unwrap();
        let first = pool.lease();
        let second = pool.lease();
        let folder = pool
            .folder(root.path(), "case", true, || made_in(root.path(), "case"))
            .unwrap();
        first.passed();
        drop(first);
        assert!(folder.is_dir(), "a folder a second run works in was taken");
        drop(second);
        assert!(folder.is_dir(), "the run that did not pass lost its folder");
        assert!(folder.join(crate::door::KEPT).is_file());
    }

    #[test]
    fn a_folder_not_dealt_with_is_still_the_same_when_another_run_lets_its_lease_go() {
        let pool = pool();
        let root = crate::door::made("cf-test-").unwrap();
        let elsewhere = pool
            .folder(root.path(), "own", false, || made_in(root.path(), "own"))
            .unwrap();
        let lease = pool.lease();
        let dealt = pool
            .folder(root.path(), "case", true, || made_in(root.path(), "case"))
            .unwrap();
        lease.passed();
        drop(lease);
        assert!(!dealt.exists());
        let again = pool
            .folder(root.path(), "own", false, || -> std::io::Result<PathBuf> {
                panic!("asked for the same folder")
            })
            .unwrap();
        assert_eq!(again, elsewhere);
    }

    #[test]
    fn a_pass_after_a_failure_is_asked_anew_and_a_second_run_takes_its_own() {
        let pool = pool();
        let root = crate::door::made("cf-test-").unwrap();
        let failed = pool.lease();
        let kept = pool
            .folder(root.path(), "case", true, || made_in(root.path(), "case"))
            .unwrap();
        drop(failed);
        let lease = pool.lease();
        let next = pool
            .folder(root.path(), "case", true, || made_in(root.path(), "case"))
            .unwrap();
        assert_ne!(kept, next, "a run was given the folder another left");
        lease.passed();
        drop(lease);
        assert!(!next.exists());
        assert!(kept.is_dir(), "the kept folder of the run before went");
    }

    #[test]
    fn a_folder_made_with_no_lease_or_not_to_be_dealt_with_is_left_alone() {
        let pool = pool();
        let root = crate::door::made("cf-test-").unwrap();
        let unled = pool
            .folder(root.path(), "unled", true, || made_in(root.path(), "unled"))
            .unwrap();
        let lease = pool.lease();
        let elsewhere = pool
            .folder(root.path(), "elsewhere", false, || {
                made_in(root.path(), "elsewhere")
            })
            .unwrap();
        lease.passed();
        drop(lease);
        assert!(unled.is_dir() && elsewhere.is_dir());
    }

    #[test]
    fn what_a_folder_of_the_run_is_not_is_never_taken() {
        let pool = pool();
        let root = crate::door::made("cf-test-").unwrap();
        let lease = pool.lease();
        let other = tempfile::tempdir().unwrap();
        let theirs = other.path().join("theirs");
        std::fs::create_dir(&theirs).unwrap();
        let folder = pool
            .folder(root.path(), "case", true, || {
                Ok::<_, std::io::Error>(theirs.clone())
            })
            .unwrap();
        lease.passed();
        drop(lease);
        assert!(folder.is_dir(), "a folder no run made was taken");
    }
}

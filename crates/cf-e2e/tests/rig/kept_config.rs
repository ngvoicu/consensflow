//! A Claude config folder a case keeps for the real Claude Code's windows of a rig, as a
//! live run keeps one ([`cf_e2e::live::login::Login`]): the rig is asked for it when it
//! starts and what the run came to when it ends, which this counts and answers as the case says.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use cf_e2e::rig::ClaudeConfig;
use cf_e2e::Error;

/// A Claude config folder that says `verdict` when the rig that used it ends.
#[derive(Debug)]
pub struct Kept {
    pub path: PathBuf,
    asked_for: AtomicUsize,
    asked_what_it_came_to: AtomicUsize,
    verdict: Option<&'static str>,
    /// What the rig asked of it after the windows were gone, in order: where it was to keep the
    /// run's records, and that it was asked what the run came to.
    asked: Mutex<Vec<String>>,
}

impl Kept {
    /// The folder `path`, whose rig is told `verdict` is what the run came to: none is no
    /// complaint.
    pub fn new(path: PathBuf, verdict: Option<&'static str>) -> Arc<Self> {
        Arc::new(Self {
            path,
            asked_for: AtomicUsize::new(0),
            asked_what_it_came_to: AtomicUsize::new(0),
            verdict,
            asked: Mutex::new(Vec::new()),
        })
    }

    /// How many times the rig asked for the folder.
    pub fn asked_for(&self) -> usize {
        self.asked_for.load(Ordering::SeqCst)
    }

    /// What the rig asked of it once the windows were gone: `keep <where>` and `finish`, in order.
    pub fn asked(&self) -> Vec<String> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// How many times the rig asked what the run came to.
    pub fn asked_what_it_came_to(&self) -> usize {
        self.asked_what_it_came_to.load(Ordering::SeqCst)
    }
}

impl ClaudeConfig for Kept {
    fn folder(&self) -> cf_e2e::Result<PathBuf> {
        self.asked_for.fetch_add(1, Ordering::SeqCst);
        std::fs::create_dir_all(&self.path).map_err(|source| Error::File {
            action: "make",
            path: self.path.clone(),
            source,
        })?;
        Ok(self.path.clone())
    }

    fn described(&self) -> String {
        "the test".to_owned()
    }

    fn keep_records(&self, into: &Path) -> cf_e2e::Result {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(format!("keep {}", into.display()));
        Ok(())
    }

    fn finish(&self) -> cf_e2e::Result {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push("finish".to_owned());
        self.asked_what_it_came_to.fetch_add(1, Ordering::SeqCst);
        match self.verdict {
            Some(said) => Err(Error::Daemon(said.to_owned())),
            None => Ok(()),
        }
    }
}

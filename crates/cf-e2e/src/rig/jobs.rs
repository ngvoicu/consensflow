//! The jobs a window's stand-in agent left running: what an agent starts in the
//! background and ends its turn with, and the servers a harness starts as it
//! opens a window, and what they start. The stand-ins write their files in `jobs/`
//! beside the daemon's home, `<label>-<window's process id>-<n>` with a kind after
//! a dot, and a case reads them here:
//!
//! - `.pid`: its process, written once it can be asked to end;
//! - `.released`: it was asked to end (a signal) and said so before it went, as a
//!   tool that releases a lock does;
//! - `.finished`: it ran to its end by itself;
//! - `.monitor-may-end`: written by a case, it lets the monitor that watches the job end
//!   (a `CLAUDE-MONITOR`'s, [`Job::let_its_monitor_end`]).
//!
//! A job that was killed outright wrote neither of the last two. That is every
//! job of a window on Windows, which is ended at once with its window and never
//! asked to end.

use std::path::{Path, PathBuf};

use super::Rig;
use crate::files;

/// What a job's files are in, beside the daemon's home.
const FOLDER: &str = "jobs";

/// A job a window's stand-in agent started, as its files say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    /// Its file, without the kind.
    file: PathBuf,
    pid: u32,
}

impl Job {
    /// Its process id.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Its file, without the kind: the job was started with it (`--background-job
    /// <seconds> <file>`), and it is in the rig's own folder, so that no process but the
    /// job has it in its command line.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// Whether it was asked to end and said so before it went.
    pub fn released(&self) -> bool {
        self.beside("released").exists()
    }

    /// Whether it ran to its end by itself.
    pub fn finished(&self) -> bool {
        self.beside("finished").exists()
    }

    /// Lets the monitor that watches it end, as a `CLAUDE-MONITOR`'s stand-in waits to be
    /// let: the case has seen what the turn its end woke led to, which the monitor's end
    /// would otherwise race.
    pub fn let_its_monitor_end(&self) -> crate::Result {
        files::write(&self.beside("monitor-may-end"), "")
    }

    /// Its file of the kind `kind`: `<file>.<kind>`.
    fn beside(&self, kind: &str) -> PathBuf {
        let mut name = self.file.clone().into_os_string();
        name.push(".");
        name.push(kind);
        PathBuf::from(name)
    }
}

impl Rig {
    /// The jobs named `label` that the windows' stand-in agents started and
    /// that can be asked to end (their process id is written), in the order of
    /// their names. A window's own are `job`; the server it starts as it opens,
    /// when `CF_TEST_MCP_SERVER` is set, is `server`, and the browser that server
    /// launches (`CF_TEST_MCP_BROWSER`) is `browser`. A stand-in Codex's windows
    /// open `plugin` servers (`CF_TEST_CODEX_SERVERS`), the first of which starts
    /// `worker` processes of its own (`CF_TEST_CODEX_WORKERS`). The runs of a stand-in
    /// Claude's status line (`CF_TEST_STATUS_LINE`) are `statusline`.
    pub fn jobs(&self, label: &str) -> Vec<Job> {
        listed(&self.root.join(FOLDER), label)
    }
}

/// The jobs named `label` whose files are in `folder`.
fn listed(folder: &Path, label: &str) -> Vec<Job> {
    let Ok(names) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let prefix = format!("{label}-");
    let mut found: Vec<Job> = names
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().to_string_lossy().into_owned();
            let stem = name
                .strip_suffix(".pid")
                .filter(|stem| stem.starts_with(&prefix))?;
            // A file the job is writing is read whole or not yet: not a process id.
            let pid = files::read_string(&folder.join(&name))
                .ok()?
                .trim()
                .parse()
                .ok()?;
            Some(Job {
                file: folder.join(stem),
                pid,
            })
        })
        .collect();
    found.sort_by(|a, b| a.file.cmp(&b.file));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_jobs_of_a_label_are_those_with_a_process_id_written_and_what_they_wrote_is_read() {
        let folder = tempfile::tempdir().unwrap();
        let write =
            |name: &str, text: &str| std::fs::write(folder.path().join(name), text).unwrap();
        write("job-4242-1.pid", "77\n");
        write("job-4242-1.released", "RELEASED\n");
        // Not yet written whole, not a number, another label, another kind of file.
        write("job-4242-2.pid", "");
        write("job-4243-1.pid", "soon");
        write("server-4242-1.pid", "9");
        write("job-4242-3.finished", "FINISHED\n");

        let jobs = listed(folder.path(), "job");

        assert_eq!(jobs.len(), 1, "{jobs:?}");
        assert_eq!(jobs[0].pid(), 77);
        assert!(jobs[0].released());
        assert!(!jobs[0].finished());
        assert_eq!(listed(folder.path(), "server").len(), 1);
        assert_eq!(listed(folder.path(), "nothing"), Vec::new());
        // A folder nothing was written in has no jobs.
        assert_eq!(listed(&folder.path().join("none"), "job"), Vec::new());
    }
}

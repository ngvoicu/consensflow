//! Whether a process works in a folder or names it, as the machine says now: what `live sweep`
//! says of each folder it lists, so that a person who sees a process there knows the folder may
//! be a run's at work. It reads and takes nothing. A process uses a folder where it runs in it,
//! where its command line names it, or, on a Mac, where its environment does (a harness a run
//! detached holds its login copy in `CLAUDE_CONFIG_DIR`, and in nothing else). Windows' processes
//! have no environment the kit reads, so only a command line names a folder there. An answer that
//! cannot be read is an error, and the listing says so.

use std::path::Path;

/// The processes of the machine as it says them at one moment: the folders they work in and
/// their command lines (with the environment where the machine lets it be read).
pub struct Table {
    working: Vec<String>,
    said: Vec<String>,
    fold: bool,
}

impl Table {
    /// The machine's processes, read once: an error where they cannot be read.
    pub fn read() -> Result<Table, String> {
        machine::read()
    }

    /// Whether a process works in `folder` or names it.
    pub fn uses(&self, folder: &Path) -> bool {
        let names = names_of(folder);
        used(&self.working, &self.said, &names, self.fold)
    }
}

/// The names a process may know `folder` by: as given, and its real path.
fn names_of(folder: &Path) -> Vec<String> {
    let mut names = vec![folder.to_string_lossy().into_owned()];
    if let Ok(real) = folder.canonicalize() {
        let real = real.to_string_lossy().into_owned();
        let real = real
            .strip_prefix(r"\\?\")
            .map_or(real.clone(), str::to_owned);
        if !names.contains(&real) {
            names.push(real);
        }
    }
    names.retain(|name| !name.is_empty());
    names
}

/// Whether a working folder of `working` is in any of `names`, or a command line of `said`
/// names any of them (in any case where `fold` is set: Windows' paths are the same in any).
fn used(working: &[String], said: &[String], names: &[String], fold: bool) -> bool {
    working
        .iter()
        .any(|cwd| names.iter().any(|name| Path::new(cwd).starts_with(name)))
        || said.iter().any(|line| {
            names.iter().any(|name| {
                if fold {
                    line.to_lowercase().contains(&name.to_lowercase())
                } else {
                    line.contains(name.as_str())
                }
            })
        })
}

#[cfg(target_os = "macos")]
mod machine {
    use std::time::Duration;

    use crate::process::Run;

    /// How long the machine has to list its processes.
    const LISTING: Duration = Duration::from_secs(30);

    /// What `program` wrote of itself with `args` (`lsof` says 1 for the processes it may not
    /// look at): an error where it did not start, ran out of time, or wrote nothing.
    fn listed(program: &str, args: &[&str]) -> Result<String, String> {
        let ran = Run::new(program)
            .args(args)
            .inheriting_env()
            .limit(LISTING)
            .run()
            .map_err(|failed| format!("{program} did not run: {failed}"))?;
        if ran.timed_out || ran.stdout.trim().is_empty() {
            return Err(format!("{program} said nothing"));
        }
        Ok(ran.stdout)
    }

    pub(super) fn read() -> Result<super::Table, String> {
        let folders = listed("/usr/sbin/lsof", &["-d", "cwd", "-Fn"])?;
        let working = folders
            .lines()
            .filter_map(|line| line.strip_prefix('n'))
            .map(str::to_owned)
            .collect();
        // `-E` adds each process's environment to its command line, for the processes of this
        // user whose environment the system lets be read.
        let table = listed("/bin/ps", &["-axwwE", "-o", "command="])?;
        Ok(super::Table {
            working,
            said: table.lines().map(str::to_owned).collect(),
            fold: false,
        })
    }
}

#[cfg(windows)]
mod machine {
    use crate::live::members::{Machine as _, ThisMachine};

    pub(super) fn read() -> Result<super::Table, String> {
        let all = ThisMachine
            .processes()
            .map_err(|failed| format!("the processes could not be listed: {failed}"))?;
        Ok(super::Table {
            working: Vec::new(),
            said: all.into_iter().map(|process| process.command).collect(),
            fold: true,
        })
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod machine {
    pub(super) fn read() -> Result<super::Table, String> {
        Err("the kit reads no machine's processes but a Mac's and Windows'".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_is_used_where_a_process_works_in_it_or_a_command_line_names_it() {
        let names = vec!["/tmp/consensflow-live-claude-7-abc".to_owned()];
        let said = vec![
            "/bin/zsh -l".to_owned(),
            "node claude CLAUDE_CONFIG_DIR=/tmp/consensflow-live-claude-7-abc HOME=/x".to_owned(),
        ];
        assert!(used(&[], &said, &names, false));
        assert!(!used(&[], &said[..1], &names, false));
        let working = vec!["/tmp/consensflow-live-claude-7-abc/projects".to_owned()];
        assert!(used(&working, &[], &names, false));
        let beside = vec!["/tmp/consensflow-live-claude-7-abcd".to_owned()];
        assert!(!used(&beside, &[], &names, false), "by whole components");
        let windows = vec![r"C:\Temp\Consensflow-Live-Home-7-abc".to_owned()];
        let said = vec![r"c:\temp\consensflow-live-home-7-abc\codex.exe".to_owned()];
        assert!(used(&[], &said, &windows, true));
        assert!(!used(&[], &said, &windows, false));
    }
}

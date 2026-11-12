//! Whether a process uses a folder, as the machine says now: what the door asks before it takes
//! away a folder that is no run's ([`super::Door::remove_unused`], [`super::Door::remove_stale_rig`]).
//! A process uses it where it runs in it, where its command line names it, or, on a Mac, where its
//! environment does (a harness a run detached holds its login copy in `CLAUDE_CONFIG_DIR`, and in
//! nothing else); on Windows, whose processes' environments are not read, where it is a child of
//! the run that named the folder (Windows keeps a child's parent number when the parent ends). An
//! answer that cannot be read is an error, and the folder is left.

use std::path::Path;

/// Whether a process uses `folder`, or is a child of `owner`: an error where it cannot be read.
pub(super) fn in_use(folder: &Path, owner: Option<u32>) -> Result<bool, String> {
    let names = names_of(folder);
    machine::in_use(&names, owner)
}

/// The names a process may know `folder` by: as given, and its real path.
fn names_of(folder: &Path) -> Vec<String> {
    let mut names = vec![folder.to_string_lossy().into_owned()];
    if let Some(real) = super::canonical(folder) {
        let real = real.to_string_lossy().into_owned();
        if !names.contains(&real) {
            names.push(real);
        }
    }
    names.retain(|name| !name.is_empty());
    names
}

/// Whether any of `listed` (a command line, with its environment where the machine says it)
/// names any of `names`, or whether a `parents` number is `owner`.
fn named(listed: &[(u32, String)], names: &[String], owner: Option<u32>, fold: bool) -> bool {
    listed.iter().any(|(ppid, said)| {
        owner.is_some_and(|owner| *ppid == owner)
            || names.iter().any(|name| {
                if fold {
                    said.to_lowercase().contains(&name.to_lowercase())
                } else {
                    said.contains(name.as_str())
                }
            })
    })
}

#[cfg(target_os = "macos")]
mod machine {
    use std::path::Path;
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

    pub(super) fn in_use(names: &[String], owner: Option<u32>) -> Result<bool, String> {
        let folders = listed("/usr/sbin/lsof", &["-d", "cwd", "-Fn"])?;
        let working = folders
            .lines()
            .filter_map(|line| line.strip_prefix('n'))
            .any(|cwd| names.iter().any(|name| Path::new(cwd).starts_with(name)));
        if working {
            return Ok(true);
        }
        // `-E` adds each process's environment to its command line, for the processes of this
        // user whose environment the system lets be read.
        let table = listed("/bin/ps", &["-axwwE", "-o", "ppid=,command="])?;
        let listed: Vec<(u32, String)> = table
            .lines()
            .filter_map(|line| {
                let (ppid, said) = line.trim_start().split_once(' ')?;
                Some((ppid.parse().ok()?, said.to_owned()))
            })
            .collect();
        Ok(super::named(&listed, names, owner, false))
    }
}

#[cfg(windows)]
mod machine {
    use crate::live::members::{Machine as _, ThisMachine};

    pub(super) fn in_use(names: &[String], owner: Option<u32>) -> Result<bool, String> {
        let all = ThisMachine
            .processes()
            .map_err(|failed| format!("the processes could not be listed: {failed}"))?;
        let listed: Vec<(u32, String)> = all
            .into_iter()
            .map(|process| (process.ppid, process.command))
            .collect();
        // Windows' paths are the same in any case.
        Ok(super::named(&listed, names, owner, true))
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod machine {
    pub(super) fn in_use(_names: &[String], _owner: Option<u32>) -> Result<bool, String> {
        Err("the kit reads no machine's processes but a Mac's and Windows'".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_is_used_where_a_command_line_names_it_or_a_child_of_its_run_is_there() {
        let names = vec!["/tmp/consensflow-live-claude-7-abc".to_owned()];
        let listed = vec![
            (1, "/bin/zsh -l".to_owned()),
            (
                1,
                "node claude CLAUDE_CONFIG_DIR=/tmp/consensflow-live-claude-7-abc HOME=/x"
                    .to_owned(),
            ),
        ];
        assert!(named(&listed, &names, None, false));
        assert!(!named(&listed[..1], &names, None, false));
        assert!(named(&[(7, "anything".to_owned())], &names, Some(7), false));
        assert!(!named(
            &[(8, "anything".to_owned())],
            &names,
            Some(7),
            false
        ));
        let windows = vec![r"C:\Temp\Consensflow-Live-Home-7-abc".to_owned()];
        let said = vec![(
            1,
            r"c:\temp\consensflow-live-home-7-abc\codex.exe".to_owned(),
        )];
        assert!(named(&said, &windows, None, true));
        assert!(!named(&said, &windows, None, false));
    }
}

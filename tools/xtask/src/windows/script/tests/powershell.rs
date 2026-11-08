//! The machine's own PowerShell for the tests of the scripts that run it on a folder made for the
//! case: the Mac's (`pwsh`) or Windows' own.

use std::fs;
use std::path::Path;

use cf_base::env::Env;

use super::said;
use crate::process::{self, Invocation};

/// A PowerShell of this machine: the Mac's (`pwsh`) or Windows' own; none where the
/// machine has neither on its PATH, and the tests that run it say so. One that is there
/// and does not run is a failure of the test, not a reason to skip it: a PowerShell
/// whose startup cache was corrupted by others starting at once ended every test of
/// these as "no PowerShell", and every plant of them passed (2026-11-08).
pub(super) fn shell() -> Option<&'static str> {
    let path = Env::from_process();
    let folders: Vec<std::path::PathBuf> = std::env::split_paths(path.search_path()?).collect();
    ["pwsh", "pwsh.exe", "powershell.exe"]
        .into_iter()
        .find(|name| folders.iter().any(|folder| folder.join(name).is_file()))
}

/// What `script` says when `shell` runs it from a file in `folder`, its lines' ends as `\n`.
pub(super) fn run(shell: &str, folder: &Path, script: &str) -> String {
    let file = folder.join("case.ps1");
    fs::write(&file, script).unwrap();
    let ran = process::capture(
        &Invocation::new(shell, folder)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(file.display().to_string()),
        // PowerShell keeps a cache of how it started in the user's cache folder, which several
        // starting at once write over each other: each run has a folder of its own for it.
        &Env::from_process().with_var(
            "XDG_CACHE_HOME",
            &folder.join("cache").display().to_string(),
        ),
    )
    .unwrap();
    assert_eq!(ran.code, 0, "{}", said(&ran));
    ran.stdout.replace("\r\n", "\n")
}

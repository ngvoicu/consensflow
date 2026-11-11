//! The machine's own PowerShell for the tests of the scripts that run it on a folder made for the
//! case: the Mac's (`pwsh`) or Windows' own.

use std::fs;
use std::path::Path;

use cf_base::env::Env;

use super::said;
use crate::process::{self, Captured, Invocation};

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
/// A run that does not end 0 fails the test with all that can say why.
pub(super) fn run(shell: &str, folder: &Path, script: &str) -> String {
    told(shell, folder, script).unwrap_or_else(|why| panic!("{why}"))
}

/// The file the shell is given to run: the case's script held in a `try` under `'Stop'`,
/// where the file's own scope is the script's, so a `param`, a function or an `exit` of the
/// script's means what `-File` of it alone meant. A terminating error says its whole record
/// on the error stream before the run ends 1; a non-terminating one ends the run too where
/// the script does not set a preference of its own (the tests' scripts all do); and a clean
/// end is 0, as `-File` of the script alone ended — the last native word's code was never
/// the run's own.
const WRAPPED: &str = r#"$ErrorActionPreference = 'Stop'
try {
«case»
    exit 0
} catch {
    $record = $_ | Out-String
    [Console]::Error.WriteLine($record)
    [Console]::Error.WriteLine($_.ScriptStackTrace)
    [Console]::Error.WriteLine("LASTEXITCODE: $LASTEXITCODE")
    exit 1
}
"#;

/// `text` without the escapes a PowerShell draws its errors with where it thinks a terminal
/// is there (`ESC [` to the sequence's last byte): a report is read as text.
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((before, after)) = rest.split_once('\u{1b}') {
        out.push_str(before);
        rest = match after.strip_prefix('[') {
            Some(csi) => match csi.find(|c: char| ('@'..='~').contains(&c)) {
                Some(end) => &csi[end + 1..],
                None => after,
            },
            None => after,
        };
    }
    out.push_str(rest);
    out
}

/// The case as it was written for the run: the file, the `wrapped` text it was written
/// with, and the `script` the run was given.
struct Written<'a> {
    file: &'a Path,
    wrapped: &'a str,
    script: &'a str,
}

/// The script `written.file` as the run left it.
fn left(written: &Written) -> String {
    match fs::read_to_string(written.file) {
        Ok(kept) if kept == written.wrapped => {
            format!("still there and unchanged: {}", written.file.display())
        }
        Ok(kept) => format!(
            "changed under the run: {} now holds\n{kept}",
            written.file.display()
        ),
        Err(_) => format!("gone after the run: {}", written.file.display()),
    }
}

/// All a run that did not end 0 can still tell of why: the command as it ran, its code and
/// streams, the run's file as it left it, the script it was given, and what the same shell
/// alone on the same folder answers — a shell that cannot start says so itself, and one that
/// starts says its version, so a second sighting can be told from a script's own failing.
fn why(
    shell: &str,
    folder: &Path,
    env: &Env,
    invocation: &Invocation,
    written: &Written,
    ran: &Captured,
) -> String {
    let probe = Invocation::new(shell, folder).args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "Write-Output ('PWSH-OK ' + $PSVersionTable.PSVersion.ToString())",
    ]);
    let alone = match process::capture(&probe, env) {
        Ok(alone) => format!("{}\n{}", probe.display(), said(&alone)),
        Err(failed) => format!("{}\ncould not be started: {failed}", probe.display()),
    };
    format!(
        "{}\n--- the command, in {}\n{}\nwith XDG_CACHE_HOME of its own: {}\n--- the file of the run, as it left it\n{}\n--- the script the run was given\n{}\n--- the same shell alone on the folder\n{}",
        said(ran),
        folder.display(),
        invocation.display(),
        folder.join("cache").display(),
        left(written),
        written.script,
        alone,
    )
}

/// What `run` has to tell of `script` on `folder`: the output it gives, or all a run that
/// did not end can still say of why.
fn told(shell: &str, folder: &Path, script: &str) -> Result<String, String> {
    let file = folder.join("case.ps1");
    let wrapped = WRAPPED.replace("«case»", script);
    fs::write(&file, &wrapped).unwrap();
    // PowerShell keeps a cache of how it started in the user's cache folder, which several
    // starting at once write over each other: each run has a folder of its own for it.
    let env = Env::from_process().with_var(
        "XDG_CACHE_HOME",
        &folder.join("cache").display().to_string(),
    );
    let invocation = Invocation::new(shell, folder)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(file.display().to_string());
    let ran = process::capture(&invocation, &env).unwrap();
    if ran.code == 0 {
        return Ok(ran.stdout.replace("\r\n", "\n"));
    }
    Err(plain(&why(
        shell,
        folder,
        &env,
        &invocation,
        &Written {
            file: &file,
            wrapped: &wrapped,
            script,
        },
        &ran,
    )))
}

#[test]
fn a_script_that_ends_1_with_no_word_is_said_with_all_that_can_tell_why() {
    let Some(shell) = shell() else {
        eprintln!("skipped: no PowerShell on this machine");
        return;
    };
    let folder = tempfile::tempdir().unwrap();
    let why = told(shell, folder.path(), "exit 1  # a-script-with-no-word\n").unwrap_err();
    // The run's own ending.
    assert!(why.contains("exit 1"), "{why}");
    // The exact command and arguments, and the script file it names.
    assert!(why.contains("-File"), "{why}");
    assert!(why.contains("case.ps1"), "{why}");
    // Whether the script file still stood after the run, and the text it was given.
    assert!(why.contains("unchanged"), "{why}");
    assert!(why.contains("a-script-with-no-word"), "{why}");
    // The same shell alone on the same folder: a start that fails names the shell, not the
    // script, and a start that runs says the shell's own version.
    assert!(why.contains("PWSH-OK"), "{why}");
    // A terminating error's record reaches the error stream: the why is the record, not 1.
    let thrown = told(shell, folder.path(), "throw 'a spoken why'\n").unwrap_err();
    let (streams, _) = thrown.split_once("\n--- the command").unwrap();
    assert!(streams.contains("a spoken why"), "{thrown}");
    assert!(thrown.contains("LASTEXITCODE"), "{thrown}");
}

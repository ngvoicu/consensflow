//! The machine's own PowerShell for the tests of the scripts that run it on a folder made for the
//! case: the Mac's (`pwsh`) or Windows' own.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};

use cf_base::env::Env;
use cf_base::time::{iso, Clock, SystemClock};

use super::said;
use crate::process::{self, Captured, Invocation};

#[path = "../../../../../../tests/support/powershell_start_lock.rs"]
mod powershell_start_lock;

pub(super) fn with_shell_turn<T>(shell: &str, start: impl FnOnce() -> T) -> T {
    if shell.eq_ignore_ascii_case("pwsh") || shell.eq_ignore_ascii_case("pwsh.exe") {
        powershell_start_lock::one_at_a_time(start)
    } else {
        start()
    }
}

pub(super) fn shell_environment(shell: &str, env: Env) -> Env {
    if shell.eq_ignore_ascii_case("pwsh") || shell.eq_ignore_ascii_case("pwsh.exe") {
        env.with_var("DOTNET_MultiCoreJitMinNumCpus", "65535")
            .with_var("COMPlus_MultiCoreJitMinNumCpus", "65535")
            .with_var("POWERSHELL_TELEMETRY_OPTOUT", "1")
    } else {
        env
    }
}

#[test]
fn pwsh_environment_disables_the_shared_startup_profile() {
    let original = Env::from_process().with_var("DOTNET_MultiCoreJitMinNumCpus", "0");
    let pwsh = shell_environment("pwsh", original.clone());
    assert_eq!(pwsh.os("DOTNET_MultiCoreJitMinNumCpus").unwrap(), "65535");
    assert_eq!(pwsh.os("POWERSHELL_TELEMETRY_OPTOUT").unwrap(), "1");
    assert_eq!(
        shell_environment("powershell.exe", original)
            .os("DOTNET_MultiCoreJitMinNumCpus")
            .unwrap(),
        "0"
    );
}

/// A PowerShell of this machine: the Mac's (`pwsh`) or Windows' own; none where the
/// machine has neither on its PATH, and the tests that run it say so. One that is there
/// and does not run is a failure of the test, not a reason to skip it: a PowerShell
/// whose startup cache was corrupted by others starting at once ended every test of
/// these as "no PowerShell", and every plant of them passed (2026-11-08).
pub(super) fn shell() -> Option<&'static str> {
    let path = Env::from_process();
    let folders: Vec<std::path::PathBuf> = std::env::split_paths(path.search_path()?).collect();
    shell_in(&folders, cfg!(windows))
}

fn shell_in(folders: &[PathBuf], windows: bool) -> Option<&'static str> {
    // PowerShell 7 on Windows resolves its cache through the user's Windows
    // special folder, not XDG_CACHE_HOME. These Windows scripts run in the
    // built-in Windows PowerShell that the runner itself uses.
    let names: &[&str] = if windows {
        &["powershell.exe"]
    } else {
        &["pwsh", "pwsh.exe", "powershell.exe"]
    };
    names
        .iter()
        .copied()
        .find(|name| folders.iter().any(|folder| folder.join(name).is_file()))
}

#[test]
fn windows_script_tests_choose_windows_powershell_not_the_users_pwsh_cache() {
    let folder = tempfile::tempdir().unwrap();
    for name in ["pwsh.exe", "powershell.exe"] {
        fs::write(folder.path().join(name), "").unwrap();
    }
    let path = [folder.path().to_path_buf()];
    assert_eq!(shell_in(&path, true), Some("powershell.exe"));
    fs::remove_file(folder.path().join("powershell.exe")).unwrap();
    assert_eq!(shell_in(&path, true), None);
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
pub(super) struct Written<'a> {
    pub(super) file: &'a Path,
    pub(super) wrapped: &'a str,
    pub(super) script: &'a str,
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
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            format!("gone after the run: {}", written.file.display())
        }
        Err(error) => format!(
            "cannot read {} after the run: {error}",
            written.file.display()
        ),
    }
}

/// A run and the evidence kept immediately after it. The probe is deferred until a failed
/// assertion prints the report: a bootstrap that correctly exits 1 is not itself a failure.
pub(super) struct Report {
    shell: String,
    folder: PathBuf,
    env: Env,
    invocation: Invocation,
    left: String,
    script: String,
    started: String,
    ended: String,
    pub(super) ran: Result<Output, String>,
}

/// Keep the actual OS status: `process::capture` turns a signal into code 1.
fn output(invocation: &Invocation, env: &Env) -> Result<Output, String> {
    process::spawn(invocation, env, Stdio::null(), false)
        .map_err(|error| format!("could not be started: {error}"))?
        .wait_with_output()
        .map_err(|error| format!("could not wait for the shell: {error}"))
}

fn said_output(output: &Output) -> String {
    let said = said(&Captured {
        code: output.status.code().unwrap_or(1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    });
    match output.status.code() {
        Some(_) => said,
        None => format!(
            "terminated without an exit code: {}\n{}",
            output.status,
            said.replacen("exit 1", "test code 1 (normalized)", 1)
        ),
    }
}

/// Trace even successful runs to the CI log, so a failing run's times can be compared with
/// other shells. Direct stderr is intentional: libtest captures ordinary test output on success.
fn trace(time: &str, event: &str, invocation: &Invocation) {
    let thread = std::thread::current();
    let name = thread.name().unwrap_or("unnamed test");
    let _ = writeln!(
        std::io::stderr().lock(),
        "PowerShell {event} {time} [{name}] in {}: {}",
        invocation.cwd.display(),
        invocation.display()
    );
}

/// Capture the shell and the file it leaves. The times bound the capture call, which starts
/// and waits for the child; they are UTC with milliseconds, not times read from the child.
pub(super) fn capture(
    shell: &str,
    folder: &Path,
    env: Env,
    invocation: Invocation,
    written: &Written,
) -> Report {
    let env = shell_environment(shell, env);
    // Take the turn before recording the start: the reported interval must not
    // include time spent waiting for another test's PowerShell.
    let (started, ended, ran) = with_shell_turn(shell, || {
        let started = iso(SystemClock.now_ms());
        trace(&started, "starting", &invocation);
        let ran = output(&invocation, &env);
        let ended = iso(SystemClock.now_ms());
        trace(&ended, "ended", &invocation);
        (started, ended, ran)
    });
    Report {
        shell: shell.to_owned(),
        folder: folder.to_owned(),
        env,
        invocation,
        left: left(written),
        script: written.script.to_owned(),
        started,
        ended,
        ran,
    }
}

/// All that can tell why: the command, times, streams and file, then the same shell alone.
impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&plain(&why(self)))
    }
}

fn why(report: &Report) -> String {
    let probe = Invocation::new(&report.shell, &report.folder).args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "Write-Output ('PWSH-OK ' + $PSVersionTable.PSVersion.ToString())",
    ]);
    let alone = with_shell_turn(&report.shell, || match output(&probe, &report.env) {
        Ok(alone) => format!("{}\n{}", probe.display(), said_output(&alone)),
        Err(failed) => format!("{}\n{failed}", probe.display()),
    });
    format!(
        "{}\n--- the command, in {}\n{}\nwith XDG_CACHE_HOME of its own: {}\nstarted (capture called): {}\nended (capture returned): {}\n--- the file of the run, as it left it\n{}\n--- the script the run was given\n{}\n--- the same shell alone on the folder\n{}",
        match &report.ran {
            Ok(ran) => said_output(ran),
            Err(error) => error.clone(),
        },
        report.folder.display(),
        report.invocation.display(),
        report.env.os("XDG_CACHE_HOME").map_or_else(
            || "(unset)".to_owned(),
            |value| value.to_string_lossy().into_owned()
        ),
        report.started,
        report.ended,
        report.left,
        report.script,
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
    let report = capture(
        shell,
        folder,
        env,
        invocation,
        &Written {
            file: &file,
            wrapped: &wrapped,
            script,
        },
    );
    if let Ok(ran) = &report.ran {
        if ran.status.success() {
            return Ok(String::from_utf8_lossy(&ran.stdout).replace("\r\n", "\n"));
        }
    }
    Err(report.to_string())
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
    for label in ["started (capture called): ", "ended (capture returned): "] {
        let time = why.split_once(label).unwrap().1.lines().next().unwrap();
        assert!(cf_base::time::parse(time).is_some(), "{why}");
        assert!(time.ends_with('Z') && time.len() == 24, "{why}");
    }
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

    // A signal is not an exit 1: preserve the operating system's termination status.
    #[cfg(unix)]
    {
        let killed = told(shell, folder.path(), "& /bin/kill -TERM $PID\n").unwrap_err();
        assert!(
            killed.contains("terminated without an exit code"),
            "{killed}"
        );
        assert!(killed.contains("signal: 15"), "{killed}");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn a_running_powershell_holds_the_shared_turn_until_it_ends() {
    use std::time::{Duration, Instant};

    let Some(shell) = shell() else {
        eprintln!("skipped: no PowerShell on this machine");
        return;
    };
    let folder = tempfile::tempdir().unwrap();
    let ready = folder.path().join("ready");
    let release = folder.path().join("release");
    let quoted = |path: &Path| path.display().to_string().replace('\'', "''");
    let script = format!(
        "[IO.File]::WriteAllText('{}', 'ready')\nwhile (-not (Test-Path -LiteralPath '{}')) {{ [Threading.Thread]::Sleep(25) }}\n",
        quoted(&ready),
        quoted(&release),
    );
    let path = folder.path().to_owned();
    let run = std::thread::spawn(move || told(shell, &path, &script));
    let until = Instant::now() + Duration::from_secs(30);
    while !ready.exists() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    let started = ready.exists();
    let env = Env::from_process();
    let lock = fs::File::options()
        .read(true)
        .write(true)
        .open(env.os("CF_PWSH_START_LOCK").expect("this run's lock"))
        .unwrap();
    let held = matches!(lock.try_lock(), Err(std::fs::TryLockError::WouldBlock));
    // Release and join before asserting, even on a red: no test-owned shell is left behind.
    fs::write(&release, "go").unwrap();
    let ended = run.join().unwrap();
    assert!(
        started,
        "the shell never reached its ready marker: {ended:?}"
    );
    assert!(held, "the running shell did not hold the shared turn");
    assert!(ended.is_ok(), "{ended:?}");
}

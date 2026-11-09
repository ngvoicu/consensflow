//! No run of the Windows runner takes anything away from the user's Temp (T-1368).
//!
//! The runner's sweep once took the test families out of it (T-1333): by their names, a day old,
//! when no program's command line named them. The review of that landing (T-1367) showed what a
//! name cannot tell: another program's `.tmpA1b2C3` that holds a `data` folder or a `sessions.db`
//! was taken, and a person's `T-42-holiday`; a Temp that is a link to a folder outside the profile
//! had that folder swept; a program that runs from a folder without its name on its command line
//! (a working folder, a short name, a process that hides its command line) held nothing; and a folder
//! that was old itself took a file written a minute ago with it. zeewin2 is Gabriel's own laptop,
//! so the sweep is gone, and with it every way for a run to delete in Temp. What the tests leave
//! behind is the tests' to drop (T-1333's teardown, which stays); the Temp folders of earlier days
//! are cleaned by a person, by hand, after a review.
//!
//! Each case here runs the whole sweep, as the bootstrap runs it, in a session whose Temp is a
//! folder made for the case, and compares what Temp holds before and after.

use std::fs;

use super::powershell::shell;
use super::runs::{sweep_as, write};
use super::*;

/// What no script of the runner holds, in the text of each: a way to Temp.
const NO_TEMP: [&str; 6] = [
    "GetTempPath",
    "Clear-TestTemp",
    "$env:TEMP",
    "$env:TMP",
    "TMPDIR",
    "[IO.Path]::GetTempFileName",
];

#[test]
fn no_script_of_the_runner_names_the_users_temp() {
    let scripts = [
        ("the sweep", sweep_script()),
        ("the bootstrap", bootstrap()),
        ("the run script", plain(&[])),
    ];
    for (which, text) in &scripts {
        for word in NO_TEMP {
            assert!(
                !text.to_lowercase().contains(&word.to_lowercase()),
                "{which} names the user's Temp ({word}): {text}"
            );
        }
        assert!(
            !text.contains("== temp"),
            "{which} says a Temp sweep: {text}"
        );
    }
}

/// PowerShell that sets the session's Temp to `$temp` under the profile `$fx`, and `Assert-TempIsCases`,
/// which each case calls once its folders are made and before the sweep: it ends the session unless the
/// Temp of the process is the case's. `[IO.Path]::GetTempPath()` reads the environment of the process,
/// `TMPDIR` on the Mac and `TMP` and `TEMP` on Windows, and a case whose override had not taken would run
/// on the Temp of the person running the tests. The folder is told by a marker file made in the case's
/// folder and looked for through the process's Temp, and not by comparing names: GitHub's Windows runner
/// writes its temp folder as 8.3 short names (`RUNNER~1`) and the process's Temp as long ones. Then `Make`,
/// `Listing` and `Lost`, which the cases use.
const SESSION: &str = r#"
$temp = Join-Path $fx 'Temp'
$env:TEMP = $temp; $env:TMP = $temp; $env:TMPDIR = $temp
function Assert-TempIsCases([string]$cases) {
  $marker = 'cf-marker-' + [guid]::NewGuid().ToString('N')
  Set-Content -LiteralPath (Join-Path $cases $marker) -Value 'x'
  try { $seen = Test-Path -LiteralPath (Join-Path ([IO.Path]::GetTempPath()) $marker) } finally { Remove-Item -LiteralPath (Join-Path $cases $marker) -Force }
  if (-not $seen) { throw "the session's Temp is not the case's: $([IO.Path]::GetTempPath())" }
}
# The folder `$path` holding `$files` (a name ending in / is a folder), last written `$hours` ago, files included.
function Make([string]$path, [string[]]$files, [double]$hours = 48) {
  New-Item -ItemType Directory -Path $path -Force | Out-Null
  foreach ($file in $files) {
    $to = Join-Path $path $file
    if ($file.EndsWith('/')) { New-Item -ItemType Directory -Path $to -Force | Out-Null; continue }
    New-Item -ItemType Directory -Path (Split-Path $to) -Force | Out-Null
    Set-Content -LiteralPath $to -Value 'UNRELATED SENTINEL'
    (Get-Item -Force -LiteralPath $to).LastWriteTimeUtc = (Get-Date).ToUniversalTime().AddHours(-$hours)
  }
  (Get-Item -Force -LiteralPath $path).LastWriteTimeUtc = (Get-Date).ToUniversalTime().AddHours(-$hours)
}
# Every name under `$root`, a file with its time (a folder's is not told: NTFS keeps a folder's time in the folder's own record and
# in its parent's listing, and a listing may show the older of the two for a while). Programs the sweep starts (git, the compiler of
# Add-Type) write their scratch files in the session's Temp, so what Temp holds afterwards may be more than it held, and never less.
function Listing([string]$root) { @(Get-ChildItem -LiteralPath $root -Recurse -Force | ForEach-Object { $_.FullName.Substring($root.Length) + '|' + $(if ($_.PSIsContainer) { 'folder' } else { $_.LastWriteTimeUtc.Ticks }) }) }
# The lines of `$was` that `$now` does not have: a name taken away, or one written to.
function Lost($was, $now) { @($was | Where-Object { $now -cnotcontains $_ }) }
"#;

/// A copy with one file the sweep takes (`stray.txt`, which its tree does not list) and one it
/// keeps, under the profile `profile` as the runner's is.
fn copy_in(profile: &std::path::Path) -> std::path::PathBuf {
    let dir = profile.join("consensflow-build");
    fs::create_dir_all(&dir).unwrap();
    write(&dir, "listed.txt");
    write(&dir, "stray.txt");
    dir
}

#[test]
fn a_run_leaves_every_folder_in_the_users_temp_where_it_is_whatever_its_name_and_age() {
    let Some(shell) = shell() else {
        eprintln!("skipped: no PowerShell on this machine");
        return;
    };
    let profile = tempfile::tempdir().unwrap();
    let dir = copy_in(profile.path());
    let before = format!(
        r#"$fx = '{}'
{SESSION}
New-Item -ItemType Directory -Path $temp | Out-Null
Assert-TempIsCases $temp
# Another program's, and a person's: the names the review found taken (two days old, nothing of ours in them).
Make (Join-Path $temp '.tmpA1b2C3') @('data/draft.txt')
Make (Join-Path $temp '.tmpD4e5F6') @('sessions.db')
Make (Join-Path $temp 'T-42-holiday') @('photograph.txt')
# Old itself, and a file written now in it.
Make (Join-Path $temp '.tmpQ4r5S6') @('data/')
Set-Content -LiteralPath (Join-Path $temp '.tmpQ4r5S6/data/just-written.txt') -Value 'WRITTEN NOW'
(Get-Item -Force -LiteralPath (Join-Path $temp '.tmpQ4r5S6')).LastWriteTimeUtc = (Get-Date).ToUniversalTime().AddHours(-48)
# What the tests of this program name their folders, which a run once took: it takes none now.
Make (Join-Path $temp 'cf-dispatch-AbC123') @('consensflow.db', 'consensflow.db-wal')
Make (Join-Path $temp 'consensflow-integration-Zz9Yy8') @('workspace/')
Make (Join-Path $temp 'cf-pane-probe-abc') @()
Make (Join-Path $temp '.tmpHhHhH3') @('data/', 'home/')
Make (Join-Path $temp '.tmpEmpty1') @()
Set-Content -LiteralPath (Join-Path $temp 'consensflow-integration-Zz9Yy8-left.txt') -Value 'x'
(Get-Item -Force -LiteralPath (Join-Path $temp 'consensflow-integration-Zz9Yy8-left.txt')).LastWriteTimeUtc = (Get-Date).ToUniversalTime().AddHours(-48)
$was = Listing $temp
"#,
        profile.path().display()
    );
    let after = r#"
$lost = Lost $was (Listing $temp)
Write-Output "TEMP-HELD=$($was.Count)"
Write-Output "TEMP-LOST=$($lost.Count)"
foreach ($line in $lost) { Write-Output "LOST $line" }
Write-Output "FRESH-FILE=$(Test-Path -LiteralPath (Join-Path $temp '.tmpQ4r5S6/data/just-written.txt'))"
"#;
    let out = sweep_as(shell, &dir, &["listed.txt"], profile.path(), &before, after);
    assert!(
        out.contains("== removed stray.txt"),
        "the sweep ran, and took its own stray: {out}"
    );
    assert!(dir.join("listed.txt").exists() && !dir.join("stray.txt").exists());
    // The case's own folders and files, all of them: ten entries and the eleven in them.
    assert!(
        out.contains("TEMP-HELD=21\n"),
        "the case made what it meant to: {out}"
    );
    assert!(
        out.contains("TEMP-LOST=0\n"),
        "every name Temp held is there, with its time: {out}"
    );
    assert!(out.contains("FRESH-FILE=True\n"), "{out}");
    assert!(!out.contains("== temp"), "no Temp sweep is said: {out}");
}

#[test]
fn a_run_whose_users_temp_is_a_link_to_a_folder_outside_the_profile_leaves_that_folder_whole() {
    let Some(shell) = shell() else {
        eprintln!("skipped: no PowerShell on this machine");
        return;
    };
    let profile = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let dir = copy_in(profile.path());
    // A link a person made, and a folder behind it that is not the profile's: a junction on Windows
    // (a symbolic link takes a privilege there), a symbolic link on the Mac.
    let before = format!(
        r#"$fx = '{profile}'
$outside = '{outside}'
{SESSION}
Make (Join-Path $outside '.tmpN1o2P3') @('data/personal.txt')
Make (Join-Path $outside 'cf-dispatch-M4n5O6') @('consensflow.db')
Make (Join-Path $outside 'T-77-thesis') @('chapter.txt')
if ($PSVersionTable.PSEdition -eq 'Core' -and -not $IsWindows) {{ New-Item -ItemType SymbolicLink -Path $temp -Target $outside | Out-Null }}
else {{ New-Item -ItemType Junction -Path $temp -Target $outside | Out-Null }}
if (-not ((Get-Item -Force -LiteralPath $temp).Attributes -band [IO.FileAttributes]::ReparsePoint)) {{ throw 'Temp is not a link' }}
Assert-TempIsCases $outside
$was = Listing $outside
"#,
        profile = profile.path().display(),
        outside = outside.path().display()
    );
    let after = r#"
$lost = Lost $was (Listing $outside)
Write-Output "OUTSIDE-HELD=$($was.Count)"
Write-Output "OUTSIDE-LOST=$($lost.Count)"
foreach ($line in $lost) { Write-Output "LOST $line" }
Write-Output "TEMP-IS-LINK=$([bool]((Get-Item -Force -LiteralPath $temp).Attributes -band [IO.FileAttributes]::ReparsePoint))"
"#;
    let out = sweep_as(shell, &dir, &["listed.txt"], profile.path(), &before, after);
    assert!(
        out.contains("== removed stray.txt"),
        "the sweep ran, and took its own stray: {out}"
    );
    assert!(
        out.contains("OUTSIDE-HELD=7\n") && out.contains("OUTSIDE-LOST=0\n"),
        "what the link leads to is whole: {out}"
    );
    assert!(out.contains("TEMP-IS-LINK=True\n"), "{out}");
    for kept in [
        ".tmpN1o2P3/data/personal.txt",
        "cf-dispatch-M4n5O6/consensflow.db",
        "T-77-thesis/chapter.txt",
    ] {
        assert!(outside.path().join(kept).is_file(), "{kept}: {out}");
    }
}

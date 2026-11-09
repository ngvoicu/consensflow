//! The sweep takes away the test families a run left in the user's Temp, and nothing else
//! (T-1333). Windows will not remove a folder from under a file a program has open, so the
//! tests that kept a ledger open past their `TempDir`, and the runs that were killed, left
//! their folders for good: zeewin and zeewin2 held 45,000 of them on 2026-11-07. zeewin2 is
//! Gabriel's laptop, and its Temp holds a Codex's, a Chrome's, a Muse's and an installer's
//! folders beside them: the families are told by the whole of their names, and a `.tmp` folder
//! by what is in it.

use std::fs;

use super::powershell::{self, shell};
use super::*;

/// The marks the sweep puts around the function that takes the folders away.
const BEGIN: &str = "# temp: begin\n";
const END: &str = "# temp: end\n";

/// The function as the sweep holds it, between its marks.
fn function_in(text: &str) -> String {
    let (_, after) = text.split_once(BEGIN).unwrap_or_else(|| {
        panic!(
            "the sweep holds no function for Temp: {} characters",
            text.len()
        )
    });
    let (function, _) = after
        .split_once(END)
        .unwrap_or_else(|| panic!("the function for Temp has no end"));
    function.to_owned()
}

/// PowerShell that makes the folder `$name` in `$temp`, holding `$kids` (a name ending in `/` is
/// a folder, `@()` makes none), last written `$hours` ago; a file when `$kind` is `file`.
const MAKE: &str = r#"
function Make([string]$name, [string[]]$kids, [double]$hours, [string]$kind = 'folder') {
  $path = Join-Path $temp $name
  if ($kind -eq 'file') { Set-Content -LiteralPath $path -Value 'x'; (Get-Item -Force -LiteralPath $path).LastWriteTimeUtc = (Get-Date).ToUniversalTime().AddHours(-$hours); return }
  New-Item -ItemType Directory -Path $path | Out-Null
  foreach ($kid in $kids) {
    if ($kid.EndsWith('/')) { New-Item -ItemType Directory -Path (Join-Path $path $kid) | Out-Null }
    else { Set-Content -LiteralPath (Join-Path $path $kid) -Value 'x' }
  }
  (Get-Item -Force -LiteralPath $path).LastWriteTimeUtc = (Get-Date).ToUniversalTime().AddHours(-$hours)
}
"#;

/// What the function makes of `make` (PowerShell that calls `Make` for each folder of a Temp),
/// given `processes` (PowerShell: `$null`, or the objects that stand for the programs there are)
/// and a `cap`: the line it ends by, and what is left in the Temp.
fn swept(shell: &str, make: &str, processes: &str, cap: u32) -> (String, Vec<String>) {
    swept_as(shell, make, processes, cap, false)
}

/// [`swept`], and only said what would be taken when `dry`.
fn swept_as(
    shell: &str,
    make: &str,
    processes: &str,
    cap: u32,
    dry: bool,
) -> (String, Vec<String>) {
    let dry = if dry { " -DryRun" } else { "" };
    let folder = tempfile::tempdir().unwrap();
    let temp = folder.path().join("Temp");
    fs::create_dir(&temp).unwrap();
    let function = function_in(&sweep_script());
    let out = powershell::run(
        shell,
        folder.path(),
        &format!(
            "$ErrorActionPreference = 'Stop'\n$temp = '{}'\n{MAKE}\n{function}\n{make}\n$processes = {processes}\n$r = Clear-TestTemp -Temp $temp -Processes $processes -Now (Get-Date) -Cap {cap}{dry}\nWrite-Output \"RESULT removed=$($r.Removed) held=$($r.Held) failed=$($r.Failed.Count) left=$($r.Left) skipped=$($r.Skipped)\"\n",
            temp.display()
        ),
    );
    let result = out
        .lines()
        .find(|line| line.starts_with("RESULT "))
        .unwrap_or_else(|| panic!("the function did not end: {out}"))
        .to_owned();
    let mut left: Vec<String> = fs::read_dir(&temp)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    (result, left)
}

#[test]
fn the_sweep_takes_the_test_families_a_day_old_and_ends_with_them_and_never_fails_the_run() {
    let text = sweep_script();
    let function = function_in(&text);
    // Called last, with the user's Temp and the programs there are, after the tarballs.
    let after = text.split_once(END).expect("the function is called").1;
    assert!(
        after.contains("Clear-TestTemp -Temp ([IO.Path]::GetTempPath()) -Processes $running -Now (Get-Date) -Cap 3000\n"),
        "{after}"
    );
    // Only the Temp under the user's profile: a case of the tests of this program is given a profile
    // of its own, and the Temp of the person running them is none of its.
    let guard = after
        .find("GetTempPath().StartsWith($env:USERPROFILE.TrimEnd('\\') + '\\', [StringComparison]::OrdinalIgnoreCase)")
        .expect("only the Temp under the profile is swept");
    assert!(
        guard < after.find("Clear-TestTemp -Temp").unwrap(),
        "{after}"
    );
    assert!(
        text.find("Remove-Item -Force -ErrorAction SilentlyContinue } catch {}")
            .unwrap()
            < text.find(BEGIN).unwrap(),
        "{text}"
    );
    // A day, by name and whole name: the families of the page of the issue.
    for family in [
        "'^(cf-dispatch|consensflow-integration)-[A-Za-z0-9]{6}$'",
        "'^consensflow-integration-[A-Za-z0-9]{6}-left\\.txt$'",
        "'^\\.tmp[A-Za-z0-9]{6}$'",
        "'^T-[0-9]{2,5}-[A-Za-z0-9._-]+$'",
        "'^cf-pane-probe-[A-Za-z0-9._-]+$'",
    ] {
        assert!(function.contains(family), "{family}: {function}");
    }
    assert!(function.contains("AddDays(-1)"), "a day: {function}");
    // What it could not take never ends the run: its failures are its own, said, and the run's
    // list of what could not be removed is not touched.
    assert!(
        !text.split_once(BEGIN).unwrap().1.contains("$failed"),
        "{text}"
    );
}

#[test]
fn only_the_named_families_a_day_old_that_no_program_holds_are_taken_and_everything_else_stays() {
    let Some(shell) = shell() else {
        eprintln!("skipped: no PowerShell on this machine");
        return;
    };
    let make = r#"
$old = 48
# Ours, and old: each of the families, and the `.tmp` folders that hold what the tests make.
Make 'cf-dispatch-AbC123' @('consensflow.db', 'consensflow.db-wal') $old
Make 'consensflow-integration-Zz9Yy8' @('workspace/') $old
Make 'consensflow-integration-Zz9Yy8-left.txt' @() $old 'file'
Make '.tmpAbCdE1' @() $old
Make '.tmpQwErT2' @('consensflow.db') $old
Make '.tmpHhHhH3' @('data/', 'home/') $old
Make '.tmpEvEnTs' @('consensflow.db', 'events.jsonl', 'events.jsonl.lock', 'daemon.log') $old
Make 'T-1179-packaged-x1' @('bundle/') $old
Make 'cf-pane-probe-abc' @() $old
# Ours, and a day not yet (23 hours), or held by a program that runs from it.
Make 'cf-dispatch-Young1' @('consensflow.db') 23
Make 'cf-dispatch-Held12' @('consensflow.db') $old
# Not ours: the neighbours in a person's Temp, a `.tmp` folder with a stranger's file in it, a name
# that is not the whole of a family's, a family's name on a file, and a folder of the right name
# that is older than a day and not a folder of a test.
Make 'chrome_BITS_1234_abc' @() $old
Make 'codex-artifact-session-Ab12' @('notes.md') $old
Make 'muse-workspace-probe-AbCdEf' @() $old
Make '.tmpNotes1' @('notes.txt') $old
Make '.tmpAbC12' @() $old
Make '.tmpAbCdEfG' @() $old
Make 'cf-dispatch-AbCdEf' @() $old 'file'
Make 'cf-dispatch-AbCdEf1' @() $old
Make 'xcf-dispatch-AbCdE1' @() $old
Make 'T-shirt' @() $old
Make 'T-12345678-x' @() $old
Make 'Documents-old' @('file.txt') $old
"#;
    // A program that runs from nowhere here, and one whose command line names a file in a folder.
    let processes = r#"@([pscustomobject]@{ ExecutablePath = 'C:\Windows\notepad.exe'; CommandLine = $null }, [pscustomobject]@{ ExecutablePath = $null; CommandLine = ('a.exe ' + (Join-Path $temp 'cf-dispatch-Held12') + '\consensflow.db') })"#;
    // Said what would be taken, nothing is.
    let (said, all) = swept_as(shell, make, processes, 3000, true);
    assert_eq!(
        said,
        "RESULT removed=9 held=1 failed=0 left=0 skipped=False"
    );
    assert_eq!(all.len(), 23, "{all:?}");
    let (result, left) = swept(shell, make, processes, 3000);
    assert_eq!(
        result, "RESULT removed=9 held=1 failed=0 left=0 skipped=False",
        "{left:?}"
    );
    let mut expected = vec![
        "cf-dispatch-AbCdEf",
        "cf-dispatch-AbCdEf1",
        "cf-dispatch-Held12",
        "cf-dispatch-Young1",
        "chrome_BITS_1234_abc",
        "codex-artifact-session-Ab12",
        "muse-workspace-probe-AbCdEf",
        ".tmpAbC12",
        ".tmpAbCdEfG",
        ".tmpNotes1",
        "Documents-old",
        "T-12345678-x",
        "T-shirt",
        "xcf-dispatch-AbCdE1",
    ];
    expected.sort_unstable();
    let expected: Vec<String> = expected.into_iter().map(str::to_owned).collect();
    assert_eq!(left, expected);
}

#[test]
fn where_the_programs_cannot_be_listed_nothing_is_taken_and_a_sweep_takes_at_most_its_cap() {
    let Some(shell) = shell() else {
        eprintln!("skipped: no PowerShell on this machine");
        return;
    };
    let make = r#"
Make 'cf-dispatch-One111' @('consensflow.db') 72
Make 'cf-dispatch-Two222' @('consensflow.db') 60
Make 'cf-dispatch-Three3' @('consensflow.db') 48
"#;
    let (result, left) = swept(shell, make, "$null", 3000);
    assert_eq!(
        result,
        "RESULT removed=0 held=0 failed=0 left=0 skipped=True"
    );
    assert_eq!(left.len(), 3, "{left:?}");
    // Two at most: the oldest two.
    let (result, left) = swept(shell, make, "@()", 2);
    assert_eq!(
        result,
        "RESULT removed=2 held=0 failed=0 left=1 skipped=False"
    );
    assert_eq!(left, ["cf-dispatch-Three3"]);
}

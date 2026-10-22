//! The sweep that removes from the copy what is no file of the tree: each of its rules
//! read in its text, and, on Windows, the sweep run by the machine's own PowerShell on
//! a folder made for it.

use super::*;

/// The one line of the sweep that holds `what`.
fn line_with<'a>(text: &'a str, what: &str) -> &'a str {
    let lines: Vec<&str> = text.lines().filter(|line| line.contains(what)).collect();
    assert_eq!(lines.len(), 1, "one line holds {what}: {text}");
    lines[0]
}

#[test]
fn the_copy_keeps_the_runs_own_files_and_the_builds_outputs_and_nothing_else() {
    assert_eq!(
        KEPT[..],
        [
            ".synced-files",
            ".windows-sweep.ps1",
            ".windows-run.pid",
            ".windows-run.cmd",
            "node_modules",
            "app/node_modules",
            "app/src-tauri/target",
            "app/.cache",
            "app/src-tauri/resources",
            "app/src-tauri/gen/schemas",
            "bin/cf.exe",
            "bin/cf.exe.old-*",
            "evals/reports",
        ][..]
    );
    // The sweep is given each of them, as PowerShell reads a string between single quotes.
    let text = sweep_script();
    let kept: Vec<String> = KEPT.iter().map(|name| format!("'{name}'")).collect();
    assert_eq!(
        line_with(&text, "$kept = "),
        format!("$kept = @({})", kept.join(", "))
    );
    // A name of it is matched whole, or by its `*`, in any case, and a folder of it is
    // not walked: what is kept is passed over before anything is looked into.
    let condition = line_with(&text, "$kept | Where-Object");
    assert!(
        condition.contains("($kept | Where-Object { $name -like $_ }) -or "),
        "{condition}"
    );
    assert!(
        condition.find("$kept").unwrap() < condition.find("Clear-Stale $item").unwrap(),
        "{condition}"
    );
}

#[test]
fn a_file_the_list_names_is_kept_in_any_case_as_the_list_was_written() {
    let text = sweep_script();
    let list = line_with(&text, "$now = ");
    // Read as UTF-8, which the list is written in; compared as Windows finds a file.
    assert!(
        list.contains("Get-Content -Encoding UTF8 (Join-Path $dir '.synced-files')"),
        "{list}"
    );
    assert!(
        list.ends_with(", [StringComparer]::OrdinalIgnoreCase)"),
        "{list}"
    );
    let condition = line_with(&text, "$kept | Where-Object");
    assert!(
        condition.contains("($item -is [IO.FileInfo] -and $now.Contains($name))"),
        "{condition}"
    );
    // Its name as the list has it: from the copy's root, with `/`.
    assert!(
        text.contains("$name = $item.FullName.Substring($base).Replace('\\', '/')"),
        "{text}"
    );
}

#[test]
fn a_folder_that_is_a_link_is_not_looked_into_and_one_left_empty_is_removed() {
    let text = sweep_script();
    // Neither walk looks into a link: the tree's, and the reports' that is pruned by age.
    let walks = text
        .matches(
            "$deep = $item -is [IO.DirectoryInfo] -and -not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)"
        )
        .count();
    assert_eq!(walks, 2, "{text}");
    // Nor is the reports folder itself, when it is one: it would be walked to where it goes
    // (T-141, P3-8).
    assert!(
        text.contains(
            "  if (-not ($folder.Attributes -band [IO.FileAttributes]::ReparsePoint)) { $null = Clear-Old $folder }\n"
        ),
        "{text}"
    );
    // A folder is walked, and kept only where something is left in it.
    let condition = line_with(&text, "$kept | Where-Object");
    assert!(
        condition.ends_with(" -or ($deep -and (Clear-Stale $item))) {"),
        "{condition}"
    );
    assert!(text.contains("\n  $left\n}\n"), "{text}");
}

#[test]
fn what_is_removed_is_removed_without_a_question_and_said_and_what_is_not_ends_the_run() {
    let text = sweep_script();
    // PowerShell's Remove-Item asks before it takes a folder, which a run cannot answer.
    assert!(!text.contains("Remove-Item"), "{text}");
    assert!(
        text.contains(
            "      $item.Attributes = $item.Attributes -band -bnot [IO.FileAttributes]::ReadOnly\n      $item.Delete()\n      $removed.Add($name)\n"
        ),
        "{text}"
    );
    assert!(
        text.contains("foreach ($name in $removed) { Write-Output \"== removed $name\" }"),
        "{text}"
    );
    // What could not be removed is kept, and said, and the run's script ends with it.
    assert!(
        text.contains("      $failed.Add(\"${name}: $($_.Exception.Message)\")\n"),
        "{text}"
    );
    assert!(
        text.contains("$failed.Add(\"$($folder.FullName): $($_.Exception.Message)\"); return 1"),
        "{text}"
    );
    assert!(
        text.contains("if ($failed.Count) { Write-Output \"What left the tree could not be removed from the copy: $($failed -join '; ')\" }"),
        "{text}"
    );
    let run = plain(&[]);
    let after_unpack = run.split_once("tar.exe -xmzf").unwrap().1;
    assert!(
        after_unpack.contains(
            "\n. ([scriptblock]::Create([IO.File]::ReadAllText((Join-Path $dir '.windows-sweep.ps1'))))\nif ($failed.Count) { Exit-Run 1 }\n"
        ),
        "{run}"
    );
    // Before the build and the command: they see the tree and nothing else.
    assert!(run.find("Exit-Run 1").unwrap() < run.find("foreach ($step in").unwrap());
}

/// The reports a run leaves (`evals/reports`) are kept from one run to the next, and there
/// were no bounds to what that kept (T-34, P3-6): the sweep removes, in that folder alone,
/// what was last written more than two weeks before it. It runs under the lock, before
/// the run's script does anything else, so no run is going on; and what is newer than the
/// limit is never touched, a run's own files being written now.
#[test]
fn the_reports_a_run_left_are_kept_for_two_weeks_and_nothing_newer_is_touched() {
    let text = sweep_script();
    assert!(
        text.contains("$cutoff = (Get-Date).ToUniversalTime().AddDays(-14)"),
        "{text}"
    );
    // Only that folder is walked, and only what is in it is removed, never the folder.
    assert!(
        text.contains("$reports = Join-Path $dir 'evals/reports'\n"),
        "{text}"
    );
    // After the tree's stale files, and before what was removed is said.
    let (stale, old, said) = (
        text.find("$null = Clear-Stale $root").unwrap(),
        text.find("$null = Clear-Old $folder").unwrap(),
        text.find("foreach ($name in $removed)").unwrap(),
    );
    assert!(stale < old && old < said, "{text}");
    // A file or a folder as new as the limit stays, and so does a folder with something
    // left in it. Every folder is walked first, whatever its own time, which is when an entry
    // was last added to it: a `long` folder a recent run wrote into holds old reports too.
    assert_eq!(
        line_with(&text, "Clear-Old $item"),
        "    $inside = if ($deep) { Clear-Old $item } else { 0 }"
    );
    assert_eq!(
        line_with(&text, "$inside -or"),
        "    if ($inside -or $written -ge $cutoff) {"
    );
    assert!(
        text.find("$written = ").unwrap() < text.find("$inside = ").unwrap(),
        "the folder's own time is read before its entries are removed: {text}"
    );
    // A report that cannot be removed is said and the run goes on: it is no file of the tree.
    let body = text
        .split_once("function Clear-Old")
        .and_then(|(_, rest)| rest.split_once("\n}\n"))
        .map(|(body, _)| body)
        .unwrap();
    assert!(!body.contains("$failed"), "{body}");
    assert!(
        text.contains("$unpruned = [Collections.Generic.List[string]]::new()"),
        "{text}"
    );
    assert!(
        text.contains("if ($unpruned.Count) { Write-Output \"What the reports held past their age could not be removed: $($unpruned -join '; ')\" }"),
        "{text}"
    );
    // The limit is the sweep's own to say.
    assert!(!text.contains("«days»"), "{text}");
}

#[test]
fn the_sweep_has_no_mark_left_and_is_plain_text() {
    let text = sweep_script();
    assert!(!text.contains('«'), "{text}");
    assert!(text.is_ascii(), "{text}");
}

/// A file of the copy `name` (its path from `root`, with `/`), made with its folders.
#[cfg(windows)]
fn write(root: &std::path::Path, name: &str) {
    let path = root.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, name).unwrap();
}

/// A script run by the machine's PowerShell, as the sweep's own test runs it.
#[cfg(windows)]
fn powershell(script: &str) -> crate::process::Captured {
    use cf_base::env::Env;

    use crate::process::{self, Invocation};

    process::capture(
        &Invocation::new("powershell", ".")
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
            .arg(encode_script(script).unwrap()),
        &Env::from_process(),
    )
    .unwrap()
}

/// The sweep run on the copy `dir`, which holds the list and the sweep it reads.
#[cfg(windows)]
fn run_sweep(dir: &std::path::Path) -> crate::process::Captured {
    powershell(&format!(
        "$ErrorActionPreference = 'Stop'\n$dir = '{}'\n. ([scriptblock]::Create([IO.File]::ReadAllText((Join-Path $dir '{SWEEP}'))))\nif ($failed.Count) {{ exit 1 }}\n",
        dir.display()
    ))
}

/// The sweep run by the machine's PowerShell on a copy made for it: what the tree has
/// stays (in another case too), what it has not goes (a file, a folder it leaves empty,
/// a read-only file, a link to a folder elsewhere, whose folder stays), what is kept is
/// not looked into, and a file that cannot be removed ends the run, said.
#[cfg(windows)]
#[test]
fn powershell_removes_from_the_copy_what_the_tree_has_not_and_says_what_it_could_not() {
    use std::fs;
    use std::os::windows::fs::OpenOptionsExt;

    use cf_base::env::Env;

    use crate::process::{self, Invocation};

    let copy = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let dir = copy.path();
    for name in [
        "src/Lib.rs",
        "crates/cf-e2e/tests/stale_probe.rs",
        "crates/stale/src/lib.rs",
        "stale-read-only.txt",
        "notes.txt",
        "app/src-tauri/target/debug/deps/old.rlib",
        "node_modules/a/index.js",
        "bin/cf.exe.old-1700000000",
        ".windows-run.pid",
        "evals/reports/long/2026-10-20T12-25-35-830Z-long-ledger.db",
        "evals/reports/runs/run.json",
    ] {
        write(dir, name);
    }
    let read_only = dir.join("stale-read-only.txt");
    let mut marks = fs::metadata(&read_only).unwrap().permissions();
    marks.set_readonly(true);
    fs::set_permissions(&read_only, marks).unwrap();
    write(elsewhere.path(), "outside.txt");
    let junction = Invocation::new("cmd", dir)
        .args(["/c", "mklink", "/J", "linked"])
        .arg(elsewhere.path());
    assert_eq!(
        process::capture(&junction, &Env::from_process())
            .unwrap()
            .code,
        0
    );
    // The tree, as the list names it: `src/lib.rs`, which the copy has as `src/Lib.rs`.
    fs::write(dir.join(SYNCED), "src/lib.rs\nnotes.txt\n").unwrap();
    fs::write(dir.join(SWEEP), sweep_script()).unwrap();

    // A file of the tree's held open with no sharing: it cannot go, and the run ends.
    let stale_held = dir.join("crates/cf-e2e/tests/stale_probe.rs");
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&stale_held)
        .unwrap();
    let ran = run_sweep(dir);
    assert_eq!(ran.code, 1, "{}{}", ran.stdout, ran.stderr);
    assert!(
        ran.stdout.contains("What left the tree could not be removed from the copy: crates/cf-e2e/tests/stale_probe.rs: "),
        "{}",
        ran.stdout
    );
    drop(held);

    let ran = run_sweep(dir);
    assert_eq!(ran.code, 0, "{}{}", ran.stdout, ran.stderr);
    for gone in [
        "crates/cf-e2e/tests/stale_probe.rs",
        "crates/stale/src/lib.rs",
        "crates/stale",
        "stale-read-only.txt",
        "linked",
    ] {
        assert!(!dir.join(gone).exists(), "{gone}: {}", ran.stdout);
    }
    assert!(
        ran.stdout
            .contains("== removed crates/cf-e2e/tests/stale_probe.rs"),
        "{}",
        ran.stdout
    );
    for kept in [
        "src/Lib.rs",
        "notes.txt",
        "app/src-tauri/target/debug/deps/old.rlib",
        "node_modules/a/index.js",
        "bin/cf.exe.old-1700000000",
        ".windows-run.pid",
        // What a run wrote for its reader, a long run's report and ledger among it.
        "evals/reports/long/2026-10-20T12-25-35-830Z-long-ledger.db",
        "evals/reports/runs/run.json",
        SYNCED,
        SWEEP,
    ] {
        assert!(dir.join(kept).exists(), "{kept}: {}", ran.stdout);
    }
    // The folder the link pointed at is no file of the copy.
    assert!(elsewhere.path().join("outside.txt").exists());
    // And a second sweep finds nothing to remove.
    let again = run_sweep(dir);
    assert_eq!(again.code, 0, "{}{}", again.stdout, again.stderr);
    assert!(!again.stdout.contains("== removed"), "{}", again.stdout);
}

/// The sweep's prune of the reports run by the machine's PowerShell: what was last written
/// more than the limit ago goes (a file, and a folder left empty that was old itself, but not
/// `evals/reports`); every folder is walked, one written within the limit too (a `long` folder
/// that a recent run wrote into holds old reports), and stays where something newer is left in
/// it; and nothing outside the reports is judged by its age.
#[cfg(windows)]
#[test]
fn powershell_removes_reports_past_their_age_and_nothing_newer_or_outside_them() {
    use std::fs;

    use cf_base::env::Env;

    use crate::process::{self, Invocation};

    let copy = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let dir = copy.path();
    let old = [
        "evals/reports/2026-09-25T20-09-20-351Z-six-decisions.json",
        "evals/reports/long/2026-09-26T04-46-26-635Z-long.md",
        "evals/reports/long/2026-09-26T04-46-26-635Z-long.json",
        "evals/reports/long/2026-09-26T04-46-26-635Z-long-ledger.db",
        "evals/reports/runs/2026-09-26T10-02-54-255Z/run.json",
        "evals/reports/runs/2026-09-27T10-02-54-255Z/old.json",
        "evals/reports/runs/2026-09-28T10-02-54-255Z/old.json",
    ];
    let new = [
        "evals/reports/2026-10-20T20-09-20-351Z-six-decisions.json",
        "evals/reports/long/2026-10-20T12-25-35-830Z-long.md",
        "evals/reports/long/2026-10-20T12-25-35-830Z-long-ledger.db",
        "evals/reports/runs/2026-09-27T10-02-54-255Z/new.json",
        "evals/reports/runs/2026-09-28T10-02-54-255Z/new.json",
    ];
    for name in old.iter().chain(&new) {
        write(dir, name);
    }
    // Not a report, and old: outside the reports it is the list's to keep.
    write(dir, "notes.txt");
    fs::create_dir_all(dir.join("evals/reports/runs/empty-old")).unwrap();
    fs::create_dir_all(dir.join("evals/reports/runs/empty-new")).unwrap();
    fs::write(dir.join(SYNCED), "notes.txt\n").unwrap();
    fs::write(dir.join(SWEEP), sweep_script()).unwrap();
    // A link under the reports, to a folder elsewhere that holds an old file: the prune does
    // not walk into it, and what it points at is no report of the copy (T-141, P3-8).
    write(elsewhere.path(), "outside-old.txt");
    let junction = Invocation::new("cmd", dir)
        .args(["/c", "mklink", "/J", "evals\\reports\\linked"])
        .arg(elsewhere.path());
    assert_eq!(
        process::capture(&junction, &Env::from_process())
            .unwrap()
            .code,
        0
    );
    // Their times as runs of days ago left them. The folders are aged last: making a file
    // in one wrote it now. `long`, `runs` and `2026-09-27...` stay as the newer files made
    // in them left them, as they are where a recent run has written.
    let aged = |names: &[&str]| -> String {
        let list: Vec<String> = names.iter().map(|name| format!("'{name}'")).collect();
        format!(
            "foreach ($name in @({})) {{ (Get-Item -LiteralPath (Join-Path $dir $name)).LastWriteTime = (Get-Date).AddDays(-40) }}\n",
            list.join(", ")
        )
    };
    let times = format!(
        "$ErrorActionPreference = 'Stop'\n$dir = '{}'\n(Get-Item -LiteralPath '{}').LastWriteTime = (Get-Date).AddDays(-40)\n{}{}{}",
        dir.display(),
        elsewhere.path().join("outside-old.txt").display(),
        aged(&old),
        aged(&["notes.txt"]),
        aged(&[
            "evals/reports/runs/empty-old",
            "evals/reports/runs/2026-09-26T10-02-54-255Z",
            "evals/reports/runs/2026-09-28T10-02-54-255Z",
        ])
    );
    let set = powershell(&times);
    assert_eq!(set.code, 0, "{}{}", set.stdout, set.stderr);

    let ran = run_sweep(dir);
    assert_eq!(ran.code, 0, "{}{}", ran.stdout, ran.stderr);
    for gone in [
        "evals/reports/2026-09-25T20-09-20-351Z-six-decisions.json",
        "evals/reports/long/2026-09-26T04-46-26-635Z-long.md",
        "evals/reports/long/2026-09-26T04-46-26-635Z-long.json",
        "evals/reports/long/2026-09-26T04-46-26-635Z-long-ledger.db",
        "evals/reports/runs/2026-09-26T10-02-54-255Z/run.json",
        // The folder it leaves empty, old itself, goes with it; so does an empty old one.
        "evals/reports/runs/2026-09-26T10-02-54-255Z",
        "evals/reports/runs/empty-old",
        // An old file in a folder that something newer is left in, whether the folder is old
        // itself (`2026-09-28`) or was written within the limit (`2026-09-27`).
        "evals/reports/runs/2026-09-28T10-02-54-255Z/old.json",
        "evals/reports/runs/2026-09-27T10-02-54-255Z/old.json",
    ] {
        assert!(!dir.join(gone).exists(), "{gone}: {}", ran.stdout);
    }
    assert!(
        ran.stdout
            .contains("== removed evals/reports/long/2026-09-26T04-46-26-635Z-long-ledger.db"),
        "{}",
        ran.stdout
    );
    for kept in [
        "evals/reports/2026-10-20T20-09-20-351Z-six-decisions.json",
        "evals/reports/long/2026-10-20T12-25-35-830Z-long.md",
        "evals/reports/long/2026-10-20T12-25-35-830Z-long-ledger.db",
        "evals/reports/long",
        "evals/reports/runs/2026-09-28T10-02-54-255Z/new.json",
        "evals/reports/runs/2026-09-28T10-02-54-255Z",
        "evals/reports/runs/2026-09-27T10-02-54-255Z/new.json",
        "evals/reports/runs",
        // New, though nothing is in it.
        "evals/reports/runs/empty-new",
        "evals/reports",
        // The link is new, and what it points at was not walked: its old file is there.
        "evals/reports/linked",
        "notes.txt",
        SYNCED,
        SWEEP,
    ] {
        assert!(dir.join(kept).exists(), "{kept}: {}", ran.stdout);
    }
    assert!(
        elsewhere.path().join("outside-old.txt").exists(),
        "{}",
        ran.stdout
    );
    // And a second sweep finds nothing more to remove.
    let again = run_sweep(dir);
    assert_eq!(again.code, 0, "{}{}", again.stdout, again.stderr);
    assert!(!again.stdout.contains("== removed"), "{}", again.stdout);
}

/// The reports folder itself a link (a junction to a folder elsewhere): the prune does not walk
/// where it goes, whatever old it finds there (T-141, P3-8).
#[cfg(windows)]
#[test]
fn powershell_does_not_prune_what_a_reports_folder_that_is_a_link_goes_to() {
    use std::fs;

    use cf_base::env::Env;

    use crate::process::{self, Invocation};

    let copy = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let dir = copy.path();
    write(elsewhere.path(), "old.json");
    write(elsewhere.path(), "folder/old.json");
    // The list of what went over is never empty (a run sends the tree): one file is in it.
    write(dir, "keep.txt");
    fs::create_dir_all(dir.join("evals")).unwrap();
    let junction = Invocation::new("cmd", dir)
        .args(["/c", "mklink", "/J", "evals\\reports"])
        .arg(elsewhere.path());
    assert_eq!(
        process::capture(&junction, &Env::from_process())
            .unwrap()
            .code,
        0
    );
    fs::write(dir.join(SYNCED), "keep.txt\n").unwrap();
    fs::write(dir.join(SWEEP), sweep_script()).unwrap();
    let aged = powershell(&format!(
        "$ErrorActionPreference = 'Stop'\nforeach ($path in @('{0}\\old.json', '{0}\\folder\\old.json', '{0}\\folder')) {{ (Get-Item -LiteralPath $path).LastWriteTime = (Get-Date).AddDays(-40) }}\n",
        elsewhere.path().display()
    ));
    assert_eq!(aged.code, 0, "{}{}", aged.stdout, aged.stderr);

    let ran = run_sweep(dir);
    assert_eq!(ran.code, 0, "{}{}", ran.stdout, ran.stderr);
    assert!(!ran.stdout.contains("== removed"), "{}", ran.stdout);
    for kept in ["old.json", "folder/old.json", "folder"] {
        assert!(elsewhere.path().join(kept).exists(), "{kept}");
    }
    assert!(dir.join("evals/reports").exists());
}

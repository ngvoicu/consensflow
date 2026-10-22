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
            ".windows-tree",
            ".windows-sweep.ps1",
            ".windows-run.pid",
            ".windows-run.id",
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
    assert!(
        text.contains(
            "$deep = $item -is [IO.DirectoryInfo] -and -not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)"
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
    assert!(
        !text
            .lines()
            .any(|line| line.starts_with("      ") && line.contains("Remove-Item")),
        "{text}"
    );
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

#[test]
fn the_tarballs_runs_left_beside_the_copy_are_taken_away_an_hour_after_they_were_written() {
    let text = sweep_script();
    let at = |what: &str| {
        text.find(what)
            .unwrap_or_else(|| panic!("{what} is not there: {text}"))
    };
    // Named by the copy's folder, with a dot in it a dot: the names `upload_name` makes,
    // and the one the runs had before each had its own.
    assert!(
        text.contains(
            r"$upload = '^' + [regex]::Escape($root.Name) + '(\.[0-9a-f]{8}-[0-9a-f]{8})?\.tgz$'"
        ),
        "{text}"
    );
    let line = line_with(&text, "Get-ChildItem");
    assert!(
        line.contains("-LiteralPath $root.Parent.FullName -File"),
        "{line}"
    );
    assert!(
        line.contains("$_.Name -match $upload -and $_.LastWriteTime -lt (Get-Date).AddHours(-1)"),
        "{line}"
    );
    // It never fails the run, and it is the last thing the sweep does: what the copy is
    // told of its files is said before it.
    assert!(
        line.starts_with("try { ") && line.ends_with(" } catch {}"),
        "{line}"
    );
    assert!(at("== removed $name") < at("Get-ChildItem"));
    assert!(at("$failed.Count") < at("Get-ChildItem"));
    // And it only removes: no other name than a file of the folder the copy is in.
    assert_eq!(text.matches("Remove-Item").count(), 1, "{text}");
}

#[test]
fn the_sweep_has_no_mark_left_and_is_plain_text() {
    let text = sweep_script();
    assert!(!text.contains('«'), "{text}");
    assert!(text.is_ascii(), "{text}");
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
    use std::path::Path;

    use cf_base::env::Env;

    use crate::process::{self, Invocation};

    fn write(root: &Path, name: &str) {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, name).unwrap();
    }

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
        "evals/reports/windows/run.json",
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

    let run = |dir: &Path| {
        let script = format!(
            "$ErrorActionPreference = 'Stop'\n$dir = '{}'\n. ([scriptblock]::Create([IO.File]::ReadAllText((Join-Path $dir '{SWEEP}'))))\nif ($failed.Count) {{ exit 1 }}\n",
            dir.display()
        );
        process::capture(
            &Invocation::new("powershell", ".")
                .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
                .arg(encode_script(&script).unwrap()),
            &Env::from_process(),
        )
        .unwrap()
    };

    // A file of the tree's held open with no sharing: it cannot go, and the run ends.
    let stale_held = dir.join("crates/cf-e2e/tests/stale_probe.rs");
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&stale_held)
        .unwrap();
    let ran = run(dir);
    assert_eq!(ran.code, 1, "{}{}", ran.stdout, ran.stderr);
    assert!(
        ran.stdout.contains("What left the tree could not be removed from the copy: crates/cf-e2e/tests/stale_probe.rs: "),
        "{}",
        ran.stdout
    );
    drop(held);

    let ran = run(dir);
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
        "evals/reports/windows/run.json",
        SYNCED,
        SWEEP,
    ] {
        assert!(dir.join(kept).exists(), "{kept}: {}", ran.stdout);
    }
    // The folder the link pointed at is no file of the copy.
    assert!(elsewhere.path().join("outside.txt").exists());
    // And a second sweep finds nothing to remove.
    let again = run(dir);
    assert_eq!(again.code, 0, "{}{}", again.stdout, again.stderr);
    assert!(!again.stdout.contains("== removed"), "{}", again.stdout);
}

/// The sweep run by the machine's PowerShell beside tarballs that runs left: only those of
/// the copy's folder (this run's own among them, which is an hour young), the ones the
/// runs sent before each had its own, and only when they were written an hour ago.
#[cfg(windows)]
#[test]
fn powershell_takes_away_the_tarballs_runs_left_an_hour_ago_and_no_other_file() {
    use std::fs;
    use std::time::{Duration, SystemTime};

    use cf_base::env::Env;

    use crate::process::{self, Invocation};

    let profile = tempfile::tempdir().unwrap();
    let dir = profile.path().join("consensflow-build");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(SYNCED), "keep.txt\n").unwrap();
    fs::write(dir.join("keep.txt"), "kept").unwrap();
    // Written long ago (2001), or just now: the time of day is read in one place, and the
    // case needs only a file that is far older than an hour and one that is not.
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    let make = |name: &str, written: Option<SystemTime>| {
        let path = profile.path().join(name);
        fs::write(&path, name).unwrap();
        if let Some(written) = written {
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(written)
                .unwrap();
        }
    };
    let (removed, stays) = (
        [
            "consensflow-build.0000abcd-12345678.tgz",
            "consensflow-build.tgz",
        ],
        [
            // A run's own tarball, written just now: it is being unpacked, or is on its way.
            ("consensflow-build.0000abce-12345678.tgz", None),
            // Not of this copy's folder, not a name a run gives, not a tarball.
            ("other.0000abcd-12345678.tgz", Some(old)),
            ("consensflow-build2.0000abcd-12345678.tgz", Some(old)),
            ("consensflow-build.0000abcd-1234567.tgz", Some(old)),
            ("consensflow-build.0000abcd-12345678.tgz.part", Some(old)),
            ("consensflow-build.x.tgz", Some(old)),
            ("consensflow-build.0000abcd-12345678.txt", Some(old)),
        ],
    );
    for name in removed {
        make(name, Some(old));
    }
    for (name, written) in stays {
        make(name, written);
    }
    fs::write(dir.join(SWEEP), sweep_script()).unwrap();
    let script = format!(
        "$ErrorActionPreference = 'Stop'\n$dir = '{}'\n. ([scriptblock]::Create([IO.File]::ReadAllText((Join-Path $dir '{SWEEP}'))))\nif ($failed.Count) {{ exit 1 }}\n",
        dir.display()
    );
    let ran = process::capture(
        &Invocation::new("powershell", ".")
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
            .arg(encode_script(&script).unwrap()),
        &Env::from_process(),
    )
    .unwrap();
    assert_eq!(ran.code, 0, "{}{}", ran.stdout, ran.stderr);
    for name in removed {
        assert!(!profile.path().join(name).exists(), "{name} stayed");
    }
    for (name, _) in stays {
        assert!(profile.path().join(name).exists(), "{name} went");
    }
    assert!(dir.join("keep.txt").exists());
}

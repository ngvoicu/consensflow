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
        KEPT,
        [
            ".synced-files",
            ".windows-sweep.ps1",
            ".windows-run.pid",
            ".windows-run.cmd",
            ".git",
            "node_modules",
            "app/node_modules",
            "app/src-tauri/target",
            "app/.cache",
            "app/src-tauri/resources",
            "app/src-tauri/gen/schemas",
            "bin/cf.exe",
            "bin/cf.exe.old-*",
        ]
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

#[test]
fn the_copy_is_made_a_checkout_whose_one_commit_is_what_went_over_and_nothing_else() {
    // Every plant on zeewin was UNSTABLE: the copy held no checkout, and the plants'
    // runner could not tell what a run changed in it (2026-10-17).
    let text = sweep_script();
    let steps = line_with(&text, "foreach ($words in @(");
    // Made once and kept; git's line ends left as they are, in the repository's own
    // config, which what the plants' runner asks of git reads; the index emptied, then
    // given the files the list names, an ignored one too, each name as it is; committed.
    for step in [
        "@('init', '-q')",
        "@('config', 'core.autocrlf', 'false')",
        "@('read-tree', '--empty')",
        "@('--literal-pathspecs', 'add', '-f', '--pathspec-from-file=.synced-files')",
        "@('commit', '-q', '--allow-empty', '--no-verify', '-m', 'what-went-over')",
    ] {
        assert!(steps.contains(step), "{step}: {steps}");
    }
    assert!(
        steps.find("read-tree").unwrap() < steps.find("'add'").unwrap()
            && steps.find("'add'").unwrap() < steps.find("'commit'").unwrap(),
        "{steps}"
    );
    assert!(!text.contains("add -A") && !text.contains("'-A'"), "{text}");
    // Committed as no one, signed by nothing a machine's config asks for.
    assert!(
        text.contains(
            "$as = @('-c', 'user.name=windows-run', '-c', 'user.email=windows-run@localhost', '-c', 'commit.gpgsign=false')"
        ),
        "{text}"
    );
    // What the run and the build write is left out of what git says of the copy.
    let excluded: Vec<String> = KEPT.iter().map(|name| format!("'/{name}'")).collect();
    assert!(
        line_with(&text, ".git\\info\\exclude").contains(&format!("@({})", excluded.join(", "))),
        "{text}"
    );
    // A git that is not there, or fails, ends the run, said; a copy the sweep could not
    // make the tree's is not committed.
    assert!(
        text.contains(
            "      $said = & git @as @words 2>&1\n      if ($LASTEXITCODE -ne 0) { $failed.Add(\"git $($words -join ' '): $($said -join ' ')\"); return }\n"
        ),
        "{text}"
    );
    assert!(text.contains("if (-not $failed.Count) {\n  if (Get-Command git -ErrorAction SilentlyContinue) { Save-Tree } else { $failed.Add('git is not on the PATH, so the copy cannot be made a checkout of what went over') }\n"), "{text}");
    // git's words on its standard error are no error of PowerShell's.
    assert!(
        text.contains("function Save-Tree {\n  $ErrorActionPreference = 'Continue'\n"),
        "{text}"
    );
}

/// The sweep run by the machine's PowerShell and git on a copy made for it: once swept,
/// the copy is a checkout whose one commit is what the list names (a file an ignore rule
/// matches too) and of which git says nothing changed, though the build's folders and
/// the run's files are there; a file a run writes is said; swept again after the tree
/// changed, it is the new tree's.
#[cfg(windows)]
#[test]
fn powershell_makes_the_copy_a_checkout_whose_one_commit_is_what_went_over() {
    use std::fs;
    use std::path::Path;

    use cf_base::env::Env;

    use crate::process::{self, Invocation};

    fn write(root: &Path, name: &str) {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, name).unwrap();
    }

    let copy = tempfile::tempdir().unwrap();
    let dir = copy.path();
    for name in [
        "src/lib.rs",
        "notes.txt",
        "keep.log",
        "app/src-tauri/target/debug/out.rlib",
        "node_modules/a/index.js",
        ".windows-run.pid",
        ".windows-run.cmd",
    ] {
        write(dir, name);
    }
    fs::write(dir.join(".gitignore"), "*.log\n").unwrap();
    fs::write(dir.join(SWEEP), sweep_script()).unwrap();
    let sweep = |listed: &str| {
        fs::write(dir.join(SYNCED), listed).unwrap();
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
    let git = |words: &[&str]| {
        let ran = process::capture(
            &Invocation::new("git", dir).args(words.iter().copied()),
            &Env::from_process(),
        )
        .unwrap();
        assert_eq!(ran.code, 0, "git {words:?}: {}", ran.stderr);
        ran.stdout
    };
    let status = || git(&["status", "--porcelain=v1", "--untracked-files=all"]);
    let committed = || git(&["ls-tree", "-r", "--name-only", "HEAD"]);

    let ran = sweep(".gitignore\nkeep.log\nnotes.txt\nsrc/lib.rs\n");
    assert_eq!(ran.code, 0, "{}{}", ran.stdout, ran.stderr);
    assert_eq!(
        status(),
        "",
        "nothing changed, the build's and the run's files there too"
    );
    assert_eq!(committed(), ".gitignore\nkeep.log\nnotes.txt\nsrc/lib.rs\n");
    // What a run writes is said.
    write(dir, "src/new.rs");
    assert_eq!(status(), "?? src/new.rs\n");
    // The tree changed: a file left it, the new one is in it.
    let ran = sweep(".gitignore\nkeep.log\nsrc/lib.rs\nsrc/new.rs\n");
    assert_eq!(ran.code, 0, "{}{}", ran.stdout, ran.stderr);
    assert!(!dir.join("notes.txt").exists());
    assert_eq!(status(), "");
    assert_eq!(
        committed(),
        ".gitignore\nkeep.log\nsrc/lib.rs\nsrc/new.rs\n"
    );
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

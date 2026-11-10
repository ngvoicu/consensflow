//! The sweep that removes from the copy what is no file of the tree: each of its rules
//! read in its text, and, on Windows, the sweep run by the machine's own PowerShell on
//! a folder made for it.

use super::*;

/// Writes the tree's identity for `listed` as the files stand under `dir`, as the tarball
/// holds it (no commit), so that the sweep finds the files it unpacked to be the tree.
#[cfg(windows)]
fn say_the_tree(dir: &std::path::Path, listed: &[&str]) {
    use cf_base::env::Env;

    use crate::context::Context;
    use crate::windows::tree;

    let context = Context {
        root: dir.to_path_buf(),
        env: Env::from_process(),
    };
    let names: Vec<String> = listed.iter().map(|name| (*name).to_owned()).collect();
    let hash = tree::content(&context, &names).unwrap();
    std::fs::write(dir.join(TREE), format!("none {hash}\n")).unwrap();
}

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
            ".windows-run.ps1",
            ".windows-run.pid",
            ".windows-run.id",
            ".windows-run.lock",
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
            "evals/reports",
            ".runs",
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
    // The same line guards the logs' folder (`.runs`), so it stands twice: counted, or the one that
    // is left would satisfy a `contains` for the one that is gone.
    assert_eq!(
        text.matches(
            "  if (-not ($folder.Attributes -band [IO.FileAttributes]::ReparsePoint)) { $null = Clear-Old $folder }\n"
        )
        .count(),
        2,
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
    let run = bootstrap();
    let after_unpack = run.split_once("& $tar -xmzf").unwrap().1;
    assert!(
        after_unpack.contains(
            "\n. ([scriptblock]::Create([IO.File]::ReadAllText((Join-Path $dir '.windows-sweep.ps1'))))\nif ($failed.Count) { Exit-Run 1 }\n"
        ),
        "{run}"
    );
    // Before the run script (the build and the command): they see the tree and nothing else.
    let read =
        ". ([scriptblock]::Create([IO.File]::ReadAllText((Join-Path $dir '.windows-run.ps1'))))";
    assert!(run.find("Exit-Run 1").unwrap() < run.find(read).unwrap());
}

#[test]
fn the_files_unpacked_are_held_to_the_tree_that_was_sent_and_a_run_whose_files_differ_ends() {
    let text = sweep_script();
    let at = |what: &str| {
        text.find(what)
            .unwrap_or_else(|| panic!("{what} is not there: {text}"))
    };
    // The same hash as the Mac makes (`tree::content`): the list's names in its order, each
    // as its length (8 bytes, little endian), its name in UTF-8, its kind and its bytes.
    assert!(
        text.contains(
            "foreach ($name in @(Get-Content -Encoding UTF8 (Join-Path $dir '.synced-files'))) {\n"
        ),
        "{text}"
    );
    for part in [
        "$bytes = $utf8.GetBytes($name)\n",
        "$null = $sha.TransformBlock([BitConverter]::GetBytes([uint64]$bytes.Length), 0, 8, $null, 0)\n",
        "$kind = 'missing'\n",
        "try { $kind = if ([IO.File]::GetAttributes($path) -band 1040) { 'other' } else { 'file' } } catch {}\n",
        "if ($kind -eq 'file') { $bytes = [IO.File]::ReadAllBytes($path);",
        "$unpacked = ($sha.Hash[0..7] | ForEach-Object { $_.ToString('x2') }) -join ''\n",
    ] {
        assert!(text.contains(part), "{part}: {text}");
    }
    // The identity the tarball held is the second word of its line; a difference is said, with
    // both, and ends the run through the list that ends it.
    assert!(
        text.contains("$sent = (Get-Content -LiteralPath (Join-Path $dir '.windows-tree') -TotalCount 1).Split(' ')[1]\n"),
        "{text}"
    );
    assert!(
        text.contains("if ($unpacked -ne $sent) { Write-Output \"The files unpacked are not the tree that was sent: they hash to $unpacked, and its identity says $sent.\"; $failed.Add('the files unpacked are not the tree that was sent') }\n"),
        "{text}"
    );
    // After what the sweep removed is said (what is removed is not in the list), and before the
    // copy is made a checkout, with which the sweep ends.
    assert!(at("== removed $name") < at("$sha = "));
    assert!(at("if ($unpacked -ne $sent)") < at("Save-Tree }"));
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
        // The tarball holds the tree's identity beside the list; the sweep holds the files it
        // unpacked to it before it makes them a checkout.
        let names: Vec<&str> = listed.lines().collect();
        say_the_tree(dir, &names);
        run_sweep(dir)
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
    assert_eq!(ran.code, 0, "{}", said(&ran));
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
    assert_eq!(ran.code, 0, "{}", said(&ran));
    assert!(!dir.join("notes.txt").exists());
    assert_eq!(status(), "");
    assert_eq!(
        committed(),
        ".gitignore\nkeep.log\nsrc/lib.rs\nsrc/new.rs\n"
    );
}

/// The reports a run leaves (`evals/reports`) are kept from one run to the next, and there
/// were no bounds to what that kept (T-34, P3-6): the sweep removes, in that folder alone,
/// what was last written more than two weeks before it. It runs under the lock, before
/// the run's script does anything else, so no run is going on; and what is newer than the
/// limit is never touched, a run's own files being written now.
#[test]
fn the_reports_a_run_left_are_kept_for_two_weeks_and_nothing_newer_is_touched() {
    let text = sweep_script();
    // The reports' limit stands before the walk's own function. The logs' limit (`runs.rs`) is as
    // old and stands after the reports' block, and must not stand in for this one.
    let (before_walk, _) = text.split_once("function Clear-Old").unwrap();
    assert!(
        before_walk.contains("$cutoff = (Get-Date).ToUniversalTime().AddDays(-14)"),
        "{text}"
    );
    // Only that folder is walked, and only what is in it is removed, never the folder.
    assert!(
        text.contains("$reports = Join-Path $dir 'evals/reports'\n"),
        "{text}"
    );
    // After the tree's stale files, and before what was removed is said: the reports' own prune,
    // which stands before the logs' block, where the same line stands again.
    let reports = text
        .find("$reports = Join-Path $dir 'evals/reports'\n")
        .unwrap();
    let (stale, old, runs, said) = (
        text.find("$null = Clear-Stale $root").unwrap(),
        reports + text[reports..].find("$null = Clear-Old $folder").unwrap(),
        text.find("$runs = Join-Path $dir '.runs'").unwrap(),
        text.find("foreach ($name in $removed)").unwrap(),
    );
    assert!(stale < reports && old < runs && runs < said, "{text}");
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
        text.contains("if ($unpruned.Count) { Write-Output \"What the reports and the logs of the tasks held past their age could not be removed: $($unpruned -join '; ')\" }"),
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

/// The sweep run on the copy `dir`, which holds the list and the sweep it reads, as the
/// bootstrap runs it: with the progress records it would otherwise write on its error
/// stream (`Preparing modules for first use`) turned off, so that what is there is an error.
#[cfg(windows)]
fn run_sweep(dir: &std::path::Path) -> crate::process::Captured {
    powershell(&format!(
        "$ProgressPreference = 'SilentlyContinue'\n$ErrorActionPreference = 'Stop'\n$dir = '{}'\n. ([scriptblock]::Create([IO.File]::ReadAllText((Join-Path $dir '{SWEEP}'))))\nif ($failed.Count) {{ exit 1 }}\n",
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
    say_the_tree(dir, &["src/lib.rs", "notes.txt"]);
    fs::write(dir.join(SWEEP), sweep_script()).unwrap();

    // A file of the tree's held open with no sharing: it cannot go, and the run ends.
    let stale_held = dir.join("crates/cf-e2e/tests/stale_probe.rs");
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&stale_held)
        .unwrap();
    let ran = run_sweep(dir);
    assert_eq!(ran.code, 1, "{}", said(&ran));
    assert!(
        ran.stdout.contains("What left the tree could not be removed from the copy: crates/cf-e2e/tests/stale_probe.rs: "),
        "{}",
        ran.stdout
    );
    drop(held);

    let ran = run_sweep(dir);
    assert_eq!(ran.code, 0, "{}", said(&ran));
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
    assert_eq!(again.code, 0, "{}", said(&again));
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
    say_the_tree(dir, &["notes.txt"]);
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
    assert_eq!(set.code, 0, "{}", said(&set));

    let ran = run_sweep(dir);
    assert_eq!(ran.code, 0, "{}", said(&ran));
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
    assert_eq!(again.code, 0, "{}", said(&again));
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
    say_the_tree(dir, &["keep.txt"]);
    fs::write(dir.join(SWEEP), sweep_script()).unwrap();
    let aged = powershell(&format!(
        "$ErrorActionPreference = 'Stop'\nforeach ($path in @('{0}\\old.json', '{0}\\folder\\old.json', '{0}\\folder')) {{ (Get-Item -LiteralPath $path).LastWriteTime = (Get-Date).AddDays(-40) }}\n",
        elsewhere.path().display()
    ));
    assert_eq!(aged.code, 0, "{}", said(&aged));

    let ran = run_sweep(dir);
    assert_eq!(ran.code, 0, "{}", said(&ran));
    assert!(!ran.stdout.contains("== removed"), "{}", ran.stdout);
    for kept in ["old.json", "folder/old.json", "folder"] {
        assert!(elsewhere.path().join(kept).exists(), "{kept}");
    }
    assert!(dir.join("evals/reports").exists());
}

/// The sweep's hash, made by the machine's PowerShell, is the one the Mac makes of the same
/// files: names with a space and an accent (read from the list as UTF-8), a file with nothing
/// in it, one with carriage returns, one of bytes that are no text. A file that differs is
/// refused with both hashes, and so is one that is gone.
#[cfg(windows)]
#[test]
fn powershell_makes_the_hash_the_mac_makes_of_names_and_bytes_and_refuses_files_that_differ() {
    use std::fs;
    use std::path::Path;

    let copy = tempfile::tempdir().unwrap();
    let dir = copy.path();
    let files: [(&str, &[u8]); 5] = [
        ("keep.txt", b"kept"),
        ("sub dir/été.txt", b"accent"),
        ("empty", b""),
        ("crlf.txt", b"one\r\ntwo\r\n"),
        ("bytes.bin", &[0, 255, 13, 10, 26, 128]),
    ];
    let names: Vec<&str> = files.iter().map(|(name, _)| *name).collect();
    for (name, bytes) in files {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    fs::write(dir.join(SYNCED), format!("{}\n", names.join("\n"))).unwrap();
    fs::write(dir.join(SWEEP), sweep_script()).unwrap();
    say_the_tree(dir, &names);
    let run = |dir: &Path| run_sweep(dir);
    let ran = run(dir);
    assert_eq!(ran.code, 0, "{}", said(&ran));
    assert!(
        !ran.stdout.contains("not the tree that was sent"),
        "{}",
        ran.stdout
    );
    // A file changed by one byte, a file gone, a file added to the list that is not there.
    let identity = fs::read_to_string(dir.join(TREE)).unwrap();
    let sent = identity.trim().split(' ').nth(1).unwrap().to_owned();
    fs::write(dir.join("keep.txt"), b"kept!").unwrap();
    let changed = run(dir);
    assert_eq!(changed.code, 1, "{}", said(&changed));
    assert!(
        changed
            .stdout
            .contains("The files unpacked are not the tree that was sent: they hash to ")
            && changed
                .stdout
                .contains(&format!("and its identity says {sent}.")),
        "{}",
        changed.stdout
    );
    fs::write(dir.join("keep.txt"), b"kept").unwrap();
    fs::remove_file(dir.join("empty")).unwrap();
    // The sweep removes what the list does not name, so the list's file that is gone is the difference.
    let gone = run(dir);
    assert_eq!(gone.code, 1, "{}", said(&gone));
    assert!(
        gone.stdout.contains("not the tree that was sent"),
        "{}",
        gone.stdout
    );
}

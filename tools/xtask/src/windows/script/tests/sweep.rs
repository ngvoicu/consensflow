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
    let run = bootstrap();
    let after_unpack = run.split_once("tar.exe -xmzf").unwrap().1;
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
            r"$own = '^' + [regex]::Escape($root.Name) + '(\.[0-9a-f]{8}-[0-9a-f]{8})?\.tgz$'"
        ),
        "{text}"
    );
    assert!(text.contains("$beside = $root.Parent.FullName\n"), "{text}");
    let line = line_with(&text, "$_.Name -match $own");
    assert!(
        line.contains("Get-ChildItem -LiteralPath $beside -File"),
        "{line}"
    );
    assert!(
        line.contains("$_.Name -match $own -and $_.LastWriteTime -lt (Get-Date).AddHours(-1)"),
        "{line}"
    );
    // It never fails the run, and it comes after the sweep has said what it removed.
    assert!(
        line.starts_with("try { ") && line.ends_with(" } catch {}"),
        "{line}"
    );
    assert!(at("== removed $name") < at("$_.Name -match $own"));
    assert!(at("$failed.Count") < at("$_.Name -match $own"));
}

#[test]
fn the_uploads_of_a_folder_that_is_gone_are_taken_away_a_day_after_they_were_written() {
    let text = sweep_script();
    // Any `<folder>.<id>.tgz` beside the copy whose folder is no folder there any more,
    // written to more than a day ago (a folder in use keeps its uploads an hour; these
    // belong to runs of folders nobody removes the tarballs of).
    assert!(
        text.contains(r"$left = '^(?<folder>.+)\.[0-9a-f]{8}-[0-9a-f]{8}\.tgz$'"),
        "{text}"
    );
    let line = line_with(&text, "$_.Name -match $left");
    assert!(
        line.contains("Get-ChildItem -LiteralPath $beside -File"),
        "{line}"
    );
    assert!(
        line.contains("-and -not (Test-Path -LiteralPath (Join-Path $beside $Matches.folder))"),
        "{line}"
    );
    assert!(
        line.contains("-and $_.LastWriteTime -lt (Get-Date).AddDays(-1)"),
        "{line}"
    );
    assert!(
        line.starts_with("try { ") && line.ends_with(" } catch {}"),
        "{line}"
    );
    // And these two lines are all the sweep removes with `Remove-Item`.
    assert_eq!(text.matches("Remove-Item").count(), 2, "{text}");
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
    // tarballs beside the copy are looked at.
    assert!(at("== removed $name") < at("$sha = "));
    assert!(at("if ($unpacked -ne $sent)") < at("$beside = "));
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
    say_the_tree(dir, &["src/lib.rs", "notes.txt"]);
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
fn powershell_takes_away_the_tarballs_runs_left_an_hour_ago_or_of_a_folder_gone_a_day_ago_and_no_other_file(
) {
    use std::fs;
    use std::time::{Duration, SystemTime};

    use cf_base::env::Env;

    use crate::process::{self, Invocation};

    let profile = tempfile::tempdir().unwrap();
    let dir = profile.path().join("consensflow-build");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(SYNCED), "keep.txt\n").unwrap();
    fs::write(dir.join("keep.txt"), "kept").unwrap();
    say_the_tree(&dir, &["keep.txt"]);
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
    // Folders beside the copy, of runs that went on: one is there, one is gone.
    fs::create_dir_all(profile.path().join("other")).unwrap();
    let (removed, stays) = (
        [
            "consensflow-build.0000abcd-12345678.tgz",
            "consensflow-build.tgz",
            // The upload of a folder that is gone, written two days ago (the system's
            // way of saying "a day" is a time in 2001, which is further).
            "gone.0000abcd-12345678.tgz",
            "gone.with.dots.0000abcd-12345678.tgz",
            // A folder of a name like the copy's, which is not there: its upload is gone's too.
            "consensflow-build2.0000abcd-12345678.tgz",
        ],
        [
            // A run's own tarball, written just now: it is being unpacked, or is on its way.
            ("consensflow-build.0000abce-12345678.tgz", None),
            // The upload of a folder that is gone, as old as it should be to go if it were
            // not just now: it stays, written less than a day ago.
            ("gone-a-moment-ago.0000abcd-12345678.tgz", None),
            // Another folder's, which is there: its own runs take theirs away an hour after.
            ("other.0000abcd-12345678.tgz", Some(old)),
            // Not of this copy's folder, not a name a run gives, not a tarball.
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

/// The sweep's hash, made by the machine's PowerShell, is the one the Mac makes of the same
/// files: names with a space and an accent (read from the list as UTF-8), a file with nothing
/// in it, one with carriage returns, one of bytes that are no text. A file that differs is
/// refused with both hashes, and so is one that is gone.
#[cfg(windows)]
#[test]
fn powershell_makes_the_hash_the_mac_makes_of_names_and_bytes_and_refuses_files_that_differ() {
    use std::fs;
    use std::path::Path;

    use cf_base::env::Env;

    use crate::process::{self, Invocation};

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
    let ran = run(dir);
    assert_eq!(ran.code, 0, "{}{}", ran.stdout, ran.stderr);
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
    assert_eq!(changed.code, 1, "{}{}", changed.stdout, changed.stderr);
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
    assert_eq!(gone.code, 1, "{}{}", gone.stdout, gone.stderr);
    assert!(
        gone.stdout.contains("not the tree that was sent"),
        "{}",
        gone.stdout
    );
}

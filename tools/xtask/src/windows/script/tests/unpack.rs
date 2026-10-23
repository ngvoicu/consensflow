//! How the script unpacks the tree: with each file given the time of the unpack, which
//! the line of it says and the `tar` of the machine the tests run on does as it says.

use std::fs;
use std::time::{Duration, SystemTime};

use cf_base::env::Env;

use super::*;
use crate::process::{self, Invocation};

/// The line of a script that unpacks the tree, as its words after the tar it runs
/// (`& $tar`): its flags, and what it is given.
fn unpacking(text: &str) -> Vec<&str> {
    let lines: Vec<&str> = text
        .lines()
        .filter_map(|line| line.strip_prefix("& $tar "))
        .collect();
    assert_eq!(lines.len(), 1, "one line unpacks the tree: {text}");
    lines[0].split(' ').collect()
}

#[test]
fn the_tree_is_unpacked_by_tar_with_each_file_given_the_time_of_the_unpack() {
    let text = bootstrap();
    assert_eq!(unpacking(&text), ["-xmzf", "$tarball", "-C", "$dir"]);
    // A folder with a space in its name is one word of the PowerShell, as it is of the others.
    let other = remote_script(&Script {
        id: ID,
        dir: "build two",
        inside: ".",
        steps: &[],
        command: "x",
        env: &[],
    });
    assert!(
        other.contains("$tarball = Join-Path $env:USERPROFILE 'build two.0000abcd-12345678.tgz'\n"),
        "{other}"
    );
    assert!(other.contains("-xmzf $tarball -C $dir\n"), "{other}");
}

/// What a file packed with a time long past has for a time once `tar` has unpacked
/// it with `flags`: the `tar` of the machine the test runs on (GNU tar or bsdtar,
/// the two that the script's flags are for), run in the folder with the archive
/// named bare, as a GNU tar takes `C:\…` for a host.
fn time_after_unpacking(flags: &str) -> SystemTime {
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("old.txt");
    fs::write(&file, "changed long ago").unwrap();
    fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000))
        .unwrap();
    let env = Env::from_process();
    let tar = |words: &[&str]| {
        let step = Invocation::new("tar", folder.path()).args(words);
        let ran = process::capture(&step, &env).unwrap();
        assert_eq!(ran.code, 0, "{}: {}", step.display(), ran.stderr);
    };
    tar(&["czf", "old.tgz", "old.txt"]);
    // What is there after the unpack is what the unpack made.
    fs::remove_file(&file).unwrap();
    tar(&[flags, "old.tgz"]);
    fs::metadata(&file).unwrap().modified().unwrap()
}

#[test]
fn a_file_changed_long_ago_is_unpacked_with_the_time_of_the_unpack_and_without_the_flag_with_its_own(
) {
    let flags = unpacking(&bootstrap())[0].to_owned();
    // Packed in 2001: the script's flags give it the time of now.
    let age = |time: SystemTime| time.elapsed().unwrap_or_default();
    assert!(
        age(time_after_unpacking(&flags)) < Duration::from_secs(3600),
        "{flags} left the time the tarball keeps"
    );
    // What the script did before keeps the time the Mac had it at: the thing the flag is for.
    assert!(
        age(time_after_unpacking("-xzf")) > Duration::from_secs(10 * 365 * 24 * 3600),
        "this machine's tar does not keep the times a tarball holds, so the test proves nothing"
    );
}

/// What every script the machine is sent says, the bootstrap first.
fn every_script() -> Vec<String> {
    vec![
        bootstrap(),
        plain(&[]),
        sweep_script(),
        stop_script(DIR, ID),
        reports_script(DIR),
    ]
}

#[test]
fn the_tree_is_unpacked_by_windows_own_tar_named_by_its_path_and_never_by_the_first_tar_on_the_path(
) {
    // CI's Windows steps run in Git Bash, which puts Git's usr\bin first on the PATH, and a
    // GNU tar is there: given `C:\…` it takes `C:` for a host to reach, and ends 2 before
    // anything is unpacked (CI run 39896583469, six lock cases).
    let text = bootstrap();
    let at = |what: &str| {
        text.find(what)
            .unwrap_or_else(|| panic!("{what} is not there: {text}"))
    };
    let named = "\n$tar = Join-Path $env:SystemRoot 'System32\\tar.exe'\n";
    let missing = "\nif (-not (Test-Path -LiteralPath $tar -PathType Leaf)) { Write-Output \"Windows' own tar is not there ($tar), so the tree cannot be unpacked.\"; Exit-Run 1 }\n";
    assert!(text.contains(named), "{text}");
    assert!(text.contains(missing), "{text}");
    // Named, then looked for, then run; all of it before the sweep.
    assert!(at(named) < at(missing));
    assert!(at(missing) < at("\n& $tar -xmzf $tarball -C $dir\n"));
    assert!(at("\n& $tar -xmzf") < at("'.windows-sweep.ps1'"));
    // No script calls a tar by its bare name, which the PATH would choose: no word of a
    // line outside its quotes is one. (The bare call the script had is one.)
    let calls_a_bare_tar = |line: &str| {
        outside_quotes(line)
            .split(|c: char| c.is_whitespace() || "(){};|&".contains(c))
            .any(|word| word.eq_ignore_ascii_case("tar") || word.eq_ignore_ascii_case("tar.exe"))
    };
    for script in every_script() {
        let bare = script.lines().find(|line| calls_a_bare_tar(line));
        assert_eq!(bare, None, "{script}");
    }
    assert!(calls_a_bare_tar("tar.exe -xmzf $tarball -C $dir"));
}

/// `line` with what it holds between quotes, single or double, taken out.
fn outside_quotes(line: &str) -> String {
    let mut quote = None;
    line.chars()
        .filter(|&c| {
            match quote {
                Some(open) if c == open => quote = None,
                Some(_) => {}
                None if c == '\'' || c == '"' => quote = Some(c),
                None => return true,
            }
            false
        })
        .collect()
}

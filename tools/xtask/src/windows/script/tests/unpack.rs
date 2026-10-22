//! How the script unpacks the tree: with each file given the time of the unpack, which
//! the line of it says and the `tar` of the machine the tests run on does as it says.

use std::fs;
use std::time::{Duration, SystemTime};

use cf_base::env::Env;

use super::*;
use crate::process::{self, Invocation};

/// The line of a script that unpacks the tree, as its words: `tar.exe`, its
/// flags, and what it is given.
fn unpacking(text: &str) -> Vec<&str> {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("tar.exe "))
        .collect();
    assert_eq!(lines.len(), 1, "one line unpacks the tree: {text}");
    lines[0].split(' ').collect()
}

#[test]
fn the_tree_is_unpacked_by_tar_with_each_file_given_the_time_of_the_unpack() {
    let text = plain(&[]);
    assert_eq!(
        unpacking(&text),
        ["tar.exe", "-xmzf", "$tarball", "-C", "$dir"]
    );
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
    let flags = unpacking(&plain(&[]))[1].to_owned();
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

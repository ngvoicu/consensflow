//! Every `ssh` and `scp` the tools start is built in one place, which gives it the bounds that end
//! a connection to a peer that went silent (`src/windows/reach.rs`). A connection built anywhere
//! else has none: ssh waits for a peer that is gone until the system's keepalive gives up (two
//! hours and ten minutes on macOS), and the run that waits holds the machine's queue with it
//! (zeewin2, 2026-10-27: the reports of a run that was refused, listed over a connection that had
//! died, held both machines for two hours). So this reads the sources: a program named `ssh` or
//! `scp` that is started from anywhere but that place fails here, and so does a script that runs
//! one without the bounds.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

/// The one file that builds `ssh` and `scp`, from the package's folder.
const THE_PLACE: &str = "src/windows/reach.rs";

/// The words a script's `ssh` or `scp` is given, each of which ends a connection to a silent peer.
const BOUNDS: [&str; 4] = [
    "BatchMode=",
    "ConnectTimeout=",
    "ServerAliveInterval=",
    "ServerAliveCountMax=",
];

/// Every file under `folder` that ends with one of `extensions`.
fn files(folder: &Path, extensions: &[&str], found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files(&path, extensions, found);
        } else if extensions
            .iter()
            .any(|extension| path.extension().is_some_and(|found| found == *extension))
        {
            found.push(path);
        }
    }
}

fn package() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Whether a file of the package is a test's or a stand-in's: what it writes as `"ssh"` is the
/// name of a stand-in program, and what starts one is held to the same rule below.
fn is_test(path: &Path) -> bool {
    path.components().any(|part| {
        let part = part.as_os_str().to_string_lossy();
        part == "tests" || part == "tests.rs" || part == "testing.rs" || part == "fake_child.rs"
    })
}

/// The names this rule watches over.
const PROGRAMS: [&str; 2] = ["ssh", "scp"];

#[test]
fn ssh_and_scp_are_built_in_the_one_place_that_gives_them_the_bounds() {
    let package = package();
    let mut sources = Vec::new();
    files(&package.join("src"), &["rs"], &mut sources);
    files(&package.join("tests"), &["rs"], &mut sources);
    let (here, place) = (
        package.join("tests/ssh_and_scp.rs"),
        package.join(THE_PLACE),
    );
    let mut found = Vec::new();
    for path in sources
        .iter()
        .filter(|path| **path != here && **path != place)
    {
        let text = fs::read_to_string(path).unwrap();
        // Whitespace is no part of the pattern: rustfmt may break the call anywhere.
        let squeezed: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        for program in PROGRAMS {
            for builder in ["Invocation::new", "Command::new", "Command::new_async"] {
                if squeezed.contains(&format!("{builder}(\"{program}\"")) {
                    found.push(format!("{}: {builder}(\"{program}\"", path.display()));
                }
            }
            // Outside the tests the name is written nowhere else: a variable that holds it
            // would build the same program from a place that is not the one.
            if !is_test(path) && squeezed.contains(&format!("\"{program}\"")) {
                found.push(format!("{}: the name \"{program}\"", path.display()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "a program that reaches a machine is built in {THE_PLACE} alone, which gives it the bounds:\n{}",
        found.join("\n")
    );
    assert!(
        place.is_file(),
        "{THE_PLACE} is the place, and is not there"
    );
}

/// Whether `line` runs `ssh` or `scp`: one of them is a word of it, and it is not a comment.
fn runs_ssh_or_scp(line: &str) -> bool {
    let line = line.trim();
    !line.starts_with('#')
        && line
            .split(|c: char| c.is_whitespace() || "(`;|&\"'".contains(c))
            .any(|word| PROGRAMS.contains(&word))
}

#[test]
fn a_script_that_runs_ssh_or_scp_gives_it_the_bounds_too() {
    let mut scripts = Vec::new();
    files(&package(), &["sh"], &mut scripts);
    assert!(!scripts.is_empty(), "no script of the package was found");
    let mut found = Vec::new();
    for path in scripts {
        let text = fs::read_to_string(&path).unwrap();
        for (at, line) in text
            .lines()
            .enumerate()
            .filter(|(_, line)| runs_ssh_or_scp(line))
        {
            let missing: Vec<&str> = BOUNDS
                .into_iter()
                .filter(|bound| !line.contains(bound))
                .collect();
            if !missing.is_empty() {
                found.push(format!(
                    "{}:{}: without {missing:?}",
                    path.display(),
                    at + 1
                ));
            }
        }
    }
    assert!(
        found.is_empty(),
        "an ssh or scp of a script waits for ever for a peer that went silent:\n{}",
        found.join("\n")
    );
}

#[test]
fn what_the_rule_calls_a_command_that_runs_ssh_is_one_and_a_comment_is_not() {
    for line in [
        r#"x=$(ssh -o BatchMode=yes "$machine" "$ASK" 2>&1)"#,
        "  scp -q a b",
        "ssh host",
        "while ssh host; do :; done",
    ] {
        assert!(runs_ssh_or_scp(line), "{line}");
    }
    for line in [
        "# ssh host",
        "echo sshd is up",
        "the-ssh-tool --x",
        "grep -q scp.rs x",
    ] {
        assert!(!runs_ssh_or_scp(line), "{line}");
    }
}

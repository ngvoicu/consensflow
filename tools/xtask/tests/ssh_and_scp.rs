//! Every `ssh` and `scp` the tools start is built in one place, which gives it the bounds that end
//! a connection to a peer that went silent (`src/windows/reach.rs`). A connection built anywhere
//! else has none: ssh waits for a peer that is gone until the system's keepalive gives up (two
//! hours and ten minutes on macOS), and the run that waits holds the machine's queue with it
//! (zeewin2, 2026-10-27: the reports of a run that was refused, listed over a connection that had
//! died, held both machines for two hours). So this reads the sources: a program named `ssh` or
//! `scp` that is started from anywhere but that place fails here, and so does a script that runs
//! one without the bounds.
#![allow(clippy::disallowed_methods, clippy::unwrap_used)]

mod common;

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

/// The line a script says it that gives no bounds of its own, because whoever runs it gives them: a
/// wrapper that hands its words on to the real ssh (windows-silent.sh puts one first on the PATH
/// to freeze a connection), whose bounds, put first, would win over the runner's own.
const BOUNDS_ARE_THE_CALLERS: &str = "ssh-bounds: given by the caller";

/// The words after which a shell runs another program, and the rest of the command may name it: a
/// program that runs what it is given (`env`, `xargs`, `sudo`, `timeout`…), and what finds a
/// program (`command -v`, `which`, `type`), whose answer is run from a variable or a substitution.
const RUNS_ANOTHER: [&str; 19] = [
    "env",
    "exec",
    "command",
    "nohup",
    "xargs",
    "sudo",
    "time",
    "timeout",
    "nice",
    "ionice",
    "setsid",
    "caffeinate",
    "parallel",
    "eval",
    "watch",
    "which",
    "type",
    "hash",
    "whereis",
];

/// The words after which a command starts, as if a line had.
const STARTS_A_COMMAND: [&str; 8] = ["while", "until", "if", "then", "do", "else", "elif", "!"];

/// The shells, which run what `-c` gives them.
const SHELLS: [&str; 4] = ["bash", "sh", "zsh", "dash"];

/// Whether `word` is `ssh` or `scp`, by its name or by a path to it (`/usr/bin/ssh`).
fn is_program(word: &str) -> bool {
    let word = word.trim_matches(|c| c == '"' || c == '\'');
    PROGRAMS.contains(&word.rsplit('/').next().unwrap_or(word))
}

/// Whether `name` is the name of a variable: what is before the `=` of an assignment.
fn is_variable_name(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether `line` runs `ssh` or `scp`, or names one to be run: as the command a shell starts
/// (after a `;`, `|`, `&`, `(`, `$(` or a backquote, a keyword such as `while`, or an assignment
/// before it), by its name or by a path (`/usr/bin/ssh`); named by a program that runs another
/// (`env ssh`, `timeout 10 ssh`, `command -v ssh`, `bash -c "ssh …"`, `find -exec scp`); or as the
/// value of a variable (`SSH=/usr/bin/ssh`). A word that is only an argument is not one (`echo
/// ssh`, a file called `ssh`), nor is a comment.
fn runs_ssh_or_scp(line: &str) -> bool {
    let line = line.trim();
    if line.starts_with('#') {
        return false;
    }
    // What ends a command is a word of its own, `;`, so that what follows it is one that starts.
    let mut spaced = String::new();
    for c in line.chars() {
        if ";|&()`".contains(c) {
            spaced.push_str(" ; ");
        } else {
            spaced.push(c);
        }
    }
    let tokens: Vec<&str> = spaced.split_whitespace().collect();
    // Whether a command starts at the word, and whether every word of this one may be the program.
    let (mut starts, mut any_word) = (true, false);
    for (at, token) in tokens.iter().enumerate() {
        let word = token.trim_matches(|c| c == '"' || c == '\'');
        if word == ";" {
            (starts, any_word) = (true, false);
        } else if word == "-exec" || word == "-execdir" {
            any_word = true;
        } else if any_word {
            if is_program(word) {
                return true;
            }
        } else if starts {
            // An assignment before the command, or the whole of one: its value may be the program.
            if let Some((_, value)) = word
                .split_once('=')
                .filter(|(name, _)| is_variable_name(name))
            {
                if is_program(value) {
                    return true;
                }
                continue;
            }
            if STARTS_A_COMMAND.contains(&word) {
                continue;
            }
            if is_program(word) {
                return true;
            }
            if RUNS_ANOTHER.contains(&word)
                || (SHELLS.contains(&word) && tokens.get(at + 1) == Some(&"-c"))
            {
                (starts, any_word) = (false, true);
            } else {
                starts = false;
            }
        }
    }
    false
}

/// What of the bounds `line` lacks, if it runs ssh or scp and does not say its caller gives them.
fn lacks_the_bounds(line: &str) -> Option<Vec<&'static str>> {
    if !runs_ssh_or_scp(line) || line.contains(BOUNDS_ARE_THE_CALLERS) {
        return None;
    }
    let missing: Vec<&'static str> = BOUNDS
        .into_iter()
        .filter(|bound| !line.contains(bound))
        .collect();
    (!missing.is_empty()).then_some(missing)
}

/// What a script (or a package script) of the tree under `root` runs without the bounds, one
/// `file:line: …` each: every shell script under its `tools` folder, and the scripts of its two
/// `package.json`.
fn unbounded_in(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut scripts = Vec::new();
    let tools = root.join("tools");
    if tools.is_dir() {
        files(&tools, &["sh"], &mut scripts);
    }
    for path in scripts {
        let text = fs::read_to_string(&path).unwrap();
        for (at, line) in text.lines().enumerate() {
            if let Some(missing) = lacks_the_bounds(line) {
                found.push(format!(
                    "{}:{}: without {missing:?}",
                    path.display(),
                    at + 1
                ));
            }
        }
    }
    for package in ["package.json", "app/package.json"] {
        let path = root.join(package);
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        for (name, command) in json["scripts"].as_object().into_iter().flatten() {
            if let Some(missing) = command.as_str().and_then(lacks_the_bounds) {
                found.push(format!("{}:{name}: without {missing:?}", path.display()));
            }
        }
    }
    found
}

#[test]
fn a_script_that_runs_ssh_or_scp_gives_it_the_bounds_too() {
    let found = unbounded_in(&common::checkout());
    assert!(
        found.is_empty(),
        "an ssh or scp of a script waits for ever for a peer that went silent:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_rule_calls_a_command_that_runs_ssh_whatever_way_it_is_named() {
    for line in [
        // By its name, in the places a shell starts a command.
        r#"x=$(ssh -o BatchMode=yes "$machine" "$ASK" 2>&1)"#,
        "  scp -q a b",
        "ssh host",
        "while ssh host; do :; done",
        "if ! ssh host true; then echo no; fi",
        // By a path.
        "/usr/bin/ssh -o BatchMode=yes host",
        r#""/usr/bin/scp" a host:b"#,
        "exec /usr/bin/ssh host",
        // By the path the shell finds it at.
        r#""$(command -v ssh)" host true"#,
        "$(which scp) a host:b",
        // Through a program that runs another.
        "env ssh host",
        "env -i HOME=/x /usr/bin/ssh host",
        "exec scp a b",
        "nohup ssh host &",
        "xargs -n1 ssh < hosts",
        "sudo -u root /usr/bin/scp a b",
        "timeout 10 ssh host",
        "find . -exec scp {} host: \\;",
        r#"bash -c "ssh host true""#,
        // Named in a variable, to be run from it.
        "SSH=/usr/bin/ssh",
        "ssh=$(command -v ssh)",
    ] {
        assert!(runs_ssh_or_scp(line), "{line}");
    }
    for line in [
        "# ssh host",
        "echo sshd is up",
        "echo ssh",
        "the-ssh-tool --x",
        "grep -q scp.rs x",
        "ls ~/.ssh",
        r#"cat > "$out/bin/ssh" <<WRAPPER"#,
        r#"chmod +x "$out/bin/relay.sh" "$out/bin/ssh""#,
        r#"bash "$tool" "$tree" --machine zeewin cargo test"#,
    ] {
        assert!(!runs_ssh_or_scp(line), "{line}");
    }
}

#[test]
fn the_rule_reads_the_shell_scripts_of_the_tools_folder_and_the_package_scripts() {
    let root = tempfile::tempdir().unwrap();
    let write = |name: &str, text: &str| {
        let path = root.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    };
    write(
        "tools/other/by-path.sh",
        "#!/bin/bash\n/usr/bin/ssh -o BatchMode=yes host true\n",
    );
    write(
        "tools/other/by-name.sh",
        "echo hi\nssh -o BatchMode=yes -o ConnectTimeout=8 -o ServerAliveInterval=5 -o ServerAliveCountMax=1 host true\nscp a host:b\n",
    );
    write(
        "tools/other/found.sh",
        "x=$(command -v ssh)\n\"$(command -v ssh)\" -o BatchMode=yes -o ConnectTimeout=8 -o ServerAliveInterval=5 -o ServerAliveCountMax=1 host\n",
    );
    write(
        "package.json",
        r#"{"scripts": {"deploy": "ssh host ./install", "build": "cargo build"}}"#,
    );
    write(
        "app/package.json",
        r#"{"scripts": {"copy": "/usr/bin/scp a host:b"}}"#,
    );
    // A script elsewhere than the tools folder is not this rule's.
    write("elsewhere/x.sh", "ssh host\n");
    let found = unbounded_in(root.path());
    let said = found.join("\n");
    for (file, at) in [
        ("tools/other/by-path.sh", "2"),
        ("tools/other/by-name.sh", "3"),
        ("tools/other/found.sh", "1"),
        ("package.json", "deploy"),
        ("app/package.json", "copy"),
    ] {
        assert!(
            found
                .iter()
                .any(|line| line.contains(file) && line.contains(&format!(":{at}:"))),
            "{file}:{at} was not found in\n{said}"
        );
    }
    assert_eq!(found.len(), 5, "{said}");
}

#[test]
fn a_line_that_says_who_gives_the_bounds_is_the_callers_to_say() {
    // A wrapper that hands its own words on to the real ssh (the experiment of windows-silent.sh
    // puts one first on the PATH) is given the bounds by whoever runs it, and must not give its own,
    // which would win over theirs: its line says so.
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("tools")).unwrap();
    fs::write(
        root.path().join("tools/wrapper.sh"),
        "exec /usr/bin/ssh -o ProxyCommand=x \"$@\" # ssh-bounds: given by the caller\n",
    )
    .unwrap();
    assert_eq!(unbounded_in(root.path()), Vec::<String>::new());
}

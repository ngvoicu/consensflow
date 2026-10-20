//! The long run signals only what it started itself, through the handle it holds (a
//! program's `Child`, the rig's daemon and pane host): no line of its code sends a
//! signal by a process's number or runs a program that ends processes. A window it
//! crashes is ended by the pane host, which holds that window's handle (`pane.kill`);
//! a process its windows left behind is said, and left running. A number the system
//! gives again to another process can then never be ended in its place.

use std::path::{Path, PathBuf};

/// The Rust files at `path`: the file, or every one in the folder and the folders in it.
fn rust_files(path: &Path) -> Vec<PathBuf> {
    if path.is_file() {
        return if path.extension().is_some_and(|kind| kind == "rs") {
            vec![path.to_owned()]
        } else {
            Vec::new()
        };
    }
    let mut found: Vec<PathBuf> = std::fs::read_dir(path)
        .unwrap()
        .flat_map(|entry| rust_files(&entry.unwrap().path()))
        .collect();
    found.sort();
    found
}

/// Whether a line of Rust ends a process by its number: sends a signal (`signal`, a
/// system's `kill`), or runs a program that ends processes. A program's end through
/// its own handle (`.kill()`) and a window's through the pane host's (`pane.kill`) go
/// by no number. A comment says.
fn ends_a_process_by_its_number(line: &str) -> bool {
    let code = line.trim_start();
    if code.starts_with("//") {
        return false;
    }
    [
        "signal(",
        "Signal::",
        "::kill(",
        "taskkill",
        "pkill",
        "killall",
        "Stop-Process",
        "kill -",
        "end_checked",
    ]
    .iter()
    .any(|word| code.contains(word))
}

#[test]
fn no_line_of_the_long_run_ends_a_process_by_its_number() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let folder = root.join("src").join("long_run");
    let mut files = rust_files(&folder);
    files.extend(rust_files(&root.join("src").join("long_run.rs")));
    for read in ["watcher.rs", "processes.rs", "stage.rs", "tracker.rs"] {
        assert!(files.contains(&folder.join(read)), "{read} is read");
    }
    let ending: Vec<String> = files
        .iter()
        .flat_map(|file| {
            let text = std::fs::read_to_string(file).unwrap();
            let shown = file
                .strip_prefix(root)
                .unwrap_or(file)
                .display()
                .to_string();
            text.lines()
                .enumerate()
                .filter(|(_, line)| ends_a_process_by_its_number(line))
                .map(|(at, line)| format!("{shown}:{}: {}", at + 1, line.trim()))
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(ending.is_empty(), "{}", ending.join("\n"));
    // The words it looks for are found where they are.
    for line in [
        "let _ = signal(pid, Signal::Kill);",
        "Run::new(\"taskkill\").args([\"/F\", \"/PID\"])",
        "nix::sys::signal::kill(pid, None)",
    ] {
        assert!(ends_a_process_by_its_number(line), "{line}");
    }
    for line in [
        "window.kill();",
        "\"pane.kill\",",
        "// a signal() by its number is never sent",
    ] {
        assert!(!ends_a_process_by_its_number(line), "{line}");
    }
}

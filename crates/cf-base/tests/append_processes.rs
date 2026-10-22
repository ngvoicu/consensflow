//! Two processes that append to one file at its limit (T-285, T-290): a second
//! daemon starting on a home writes its first line to the log before its ledger
//! refuses it, beside the daemon that runs there. Each append takes an advisory
//! lock beside the file (`lock_beside`), so the file is moved aside once, and
//! neither the previous file nor a line is lost. The test is this binary run
//! twice more, each run the one test that appends as another process.

use std::fs;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use cf_base::file::{append_rotating, aside};

/// The files the two processes append to, one a round, each past its limit.
const ROUNDS: usize = 300;
const LIMIT: u64 = 10;
/// The variables that make a run of this binary one of the two processes.
const DIR: &str = "CF_BASE_APPEND_DIR";
const NAME: &str = "CF_BASE_APPEND_NAME";

fn before() -> String {
    "x".repeat(11)
}

#[test]
fn two_processes_that_append_at_the_limit_move_the_file_aside_once_and_lose_no_line() {
    let dir = tempfile::tempdir().unwrap();
    for round in 0..ROUNDS {
        fs::write(dir.path().join(format!("{round}.log")), before()).unwrap();
    }
    let exe = std::env::current_exe().unwrap();
    let children: Vec<_> = ["a", "b"]
        .into_iter()
        .map(|name| {
            // cf-process, the one place a process starts, is built on this crate.
            #[allow(clippy::disallowed_methods)]
            let mut command = std::process::Command::new(&exe);
            // Its own report is not this one's: only what it says of a failure is kept.
            command
                .args(["--exact", "appends_as_another_process", "--quiet"])
                .env(DIR, dir.path())
                .env(NAME, name)
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    fs::write(dir.path().join("go"), "").unwrap();
    for child in children {
        let ended = child.wait_with_output().unwrap();
        assert!(
            ended.status.success(),
            "a process failed to append: {}",
            String::from_utf8_lossy(&ended.stderr)
        );
    }
    for round in 0..ROUNDS {
        let file = dir.path().join(format!("{round}.log"));
        assert_eq!(
            fs::read_to_string(aside(&file)).unwrap(),
            before(),
            "round {round}: the previous file"
        );
        let mut lines: Vec<String> = fs::read_to_string(&file)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        lines.sort();
        assert_eq!(lines, ["a", "b"], "round {round}");
    }
}

/// One of the two processes: once told to go, it appends its name to each
/// round's file. Run any other way, it does nothing.
#[test]
fn appends_as_another_process() {
    // The test's own process is told what it is by its environment, read nowhere else.
    #[allow(clippy::disallowed_methods)]
    let (Some(dir), Some(name)) = (std::env::var_os(DIR), std::env::var_os(NAME)) else {
        return;
    };
    let dir = Path::new(&dir);
    let deadline = Instant::now() + Duration::from_secs(60);
    while !dir.join("go").exists() {
        assert!(Instant::now() < deadline, "never told to go");
        std::thread::sleep(Duration::from_millis(1));
    }
    let line = format!("{}\n", name.to_string_lossy());
    for round in 0..ROUNDS {
        append_rotating(&dir.join(format!("{round}.log")), &line, LIMIT).unwrap();
    }
}

//! The native `cf`, built from this workspace by the test and run as a window
//! runs it: the arguments and the input the trace recorded, an environment
//! that is this process's less every `CONSENSFLOW_*`, `CF_*` and `CHISEL_*`
//! (a window has none of its own) with what the trace added, and what it
//! printed and how it ended compared by the player.
//!
//! A run is a thread's work, so the API under test, which the test's own
//! thread runs, goes on answering while `cf` waits for it.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use serde_json::Value;
use tokio::sync::oneshot;

/// What a run printed, and how it ended.
pub struct Ran {
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
}

/// `cf`, built once for the whole test process.
pub fn cf() -> &'static Path {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT.get_or_init(build)
}

/// `cargo build -p cf` in this workspace, in the target folder these tests
/// were built in, and the program it made: cargo says where it put it.
fn build() -> PathBuf {
    let tests = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let target = tests.parent().expect("a target folder");
    let mut command = Command::new(env!("CARGO"));
    command
        .args(["build", "--offline", "--package", "cf", "--bin", "cf"])
        .arg("--message-format=json-render-diagnostics")
        .env("CARGO_TARGET_DIR", target)
        .stderr(Stdio::inherit());
    if !cfg!(debug_assertions) {
        command.arg("--release");
    }
    let output = command.output().expect("cargo runs");
    assert!(output.status.success(), "cargo build -p cf failed");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|message| message["reason"] == "compiler-artifact")
        .filter(|message| message["target"]["name"] == "cf")
        .filter(|message| {
            message["target"]["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
        })
        .find_map(|message| message["executable"].as_str().map(PathBuf::from))
        .expect("cargo built the cf program")
}

/// Starts `cf argv`, with `stdin` on its standard input and `env` added to
/// its environment, and says what it came to once it has ended.
pub fn start(
    argv: Vec<String>,
    env: Vec<(String, String)>,
    stdin: String,
) -> oneshot::Receiver<Ran> {
    let (ended, ran) = oneshot::channel();
    // The program is found before the thread is: a build is not a run's.
    let program = cf().to_owned();
    std::thread::spawn(move || {
        let mut command = Command::new(program);
        for (name, _) in std::env::vars_os() {
            let name = name.to_string_lossy();
            if ["CONSENSFLOW_", "CF_", "CHISEL_"]
                .iter()
                .any(|prefix| name.starts_with(prefix))
            {
                command.env_remove(&*name);
            }
        }
        let mut child = command
            .args(&argv)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("cf starts");
        let mut input = child.stdin.take().expect("its standard input");
        // A command that reads no input may have ended before it is written.
        let _ = input.write_all(stdin.as_bytes());
        drop(input);
        let output = child.wait_with_output().expect("cf ends");
        let _ = ended.send(Ran {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            code: output.status.code(),
        });
    });
    ran
}

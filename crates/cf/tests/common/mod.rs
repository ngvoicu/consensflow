//! `cf` as the tests run it: the binary, with none of ConsensFlow's
//! variables from a window this test may itself run in, a home of its own
//! unless the case names one, and only the variables the case gives.

// The tests' own helper: a failure in it is the test's.
#![allow(clippy::expect_used)]

use cf_board::scripted::{reply, Reply};
use serde_json::{json, Value};
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// The caller lookup made by `tell` and a plain `note`, followed by the reply
/// to the command. The table's numbered tell is the chief's; its other is a member's.
#[allow(dead_code)]
pub fn replies_for<S: AsRef<str>>(args: &[S], answer: Value) -> Vec<Reply> {
    let first = args.first().map(AsRef::as_ref);
    let explicit_human = args.iter().any(|word| word.as_ref() == "--human");
    let needs_caller = first == Some("tell") || (first == Some("note") && !explicit_human);
    let mut replies = Vec::new();
    if needs_caller {
        let role = if first == Some("tell") && args.get(1).map(AsRef::as_ref) != Some("T-3") {
            "worker"
        } else {
            "chief"
        };
        replies.push(reply(200, json!({ "participant": { "role": role } })));
    }
    replies.push(reply(201, answer));
    replies
}

/// `cf args` with `env` added and `input` on its standard input. An argument
/// is given as the system takes it, text or not.
#[allow(dead_code)] // Each test file takes what it needs of this.
pub fn cf<S: AsRef<OsStr>>(args: &[S], env: &[(&str, &str)], input: &str) -> Output {
    cf_at(Path::new(env!("CARGO_BIN_EXE_cf")), args, env, input)
}

/// `cf args` with `env` added and `input` on its standard input as bytes, which need be no text
/// (a file Windows PowerShell 5.1 wrote with `>` is UTF-16).
#[allow(dead_code)] // Each test file takes what it needs of this.
pub fn cf_bytes<S: AsRef<OsStr>>(args: &[S], env: &[(&str, &str)], input: &[u8]) -> Output {
    cf_at_bytes(Path::new(env!("CARGO_BIN_EXE_cf")), args, env, input)
}

/// The binary under test as the system spells its place, which a program run
/// by it reads back as its own: a copy of it elsewhere is another `cf`, as a
/// second install of ConsensFlow is.
#[allow(dead_code)] // Not every test that runs `cf` asks where it is.
pub fn own_cf() -> PathBuf {
    plain(std::fs::canonicalize(env!("CARGO_BIN_EXE_cf")).expect("cf is there"))
}

/// A resolved `path` as a program reports it: Windows resolves a path to its
/// verbatim form (`\\?\C:\…`), which is not what a program says of where it
/// runs, so that prefix goes (but not a share's, `\\?\UNC\…`).
#[allow(dead_code)] // Not every test that runs `cf` resolves a place.
pub fn plain(path: PathBuf) -> PathBuf {
    path.to_string_lossy()
        .strip_prefix(r"\\?\")
        .filter(|plain| !plain.starts_with(r"UNC\"))
        .map_or(path.clone(), PathBuf::from)
}

/// `cf args` as the binary at `program` runs it, with `env` added and `input`
/// on its standard input. The home is a folder of this run's own unless the
/// case names one: the machine's own `~/.consensflow` is nobody's to write in.
/// And the system's folders of programs are none unless it names some: a `cf`
/// takes Homebrew's otherwise, and finds the machine's harnesses there.
#[allow(clippy::disallowed_methods)] // The tests start cf themselves.
pub fn cf_at<S: AsRef<OsStr>>(
    program: &Path,
    args: &[S],
    env: &[(&str, &str)],
    input: &str,
) -> Output {
    cf_at_bytes(program, args, env, input.as_bytes())
}

/// [`cf_at`] with `input` as bytes.
#[allow(clippy::disallowed_methods)] // The tests start cf themselves.
pub fn cf_at_bytes<S: AsRef<OsStr>>(
    program: &Path,
    args: &[S],
    env: &[(&str, &str)],
    input: &[u8],
) -> Output {
    let home = tempfile::tempdir().expect("a home");
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
    command.env("CONSENSFLOW_HOME", home.path());
    command.env("CONSENSFLOW_TOOL_FOLDERS", "");
    // The tests run side by side: a `cf` left what another test had open would hold it.
    cf_process::hold_only_the_streams(&mut command);
    let mut child = command
        .args(args.iter().map(AsRef::as_ref))
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("cf starts");
    let mut stdin = child.stdin.take().expect("its standard input");
    // A command that reads no input may have ended before it is written.
    let _ = stdin.write_all(input);
    drop(stdin);
    child.wait_with_output().expect("cf ends")
}

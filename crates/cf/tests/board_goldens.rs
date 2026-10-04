//! The Rust `cf` held to what Node's board commands said and asked
//! (tests/goldens/cf-board.mjs wrote goldens/board.json): each case's replies
//! served by a scripted API, the binary run as a window runs it, and its
//! requests, output, errors and exit code compared byte for byte.

// The tests start cf themselves, keeping their own window's variables from it.
#![allow(clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};

use cf_board::scripted::{reply_text, scripted};
use serde::Deserialize;

#[derive(Deserialize)]
struct Golden {
    name: String,
    args: Vec<String>,
    env: BTreeMap<String, String>,
    stdin: Option<String>,
    replies: Vec<Reply>,
    requests: Vec<Request>,
    stdout: String,
    stderr: String,
    code: i32,
}

#[derive(Deserialize)]
struct Reply {
    status: u16,
    text: String,
}

#[derive(Debug, PartialEq, Deserialize)]
struct Request {
    method: String,
    path: String,
    authorization: Option<String>,
    body: Option<String>,
}

/// What the binary did with one case.
#[derive(Debug, PartialEq)]
struct Ran {
    requests: Vec<Request>,
    stdout: String,
    stderr: String,
    code: Option<i32>,
}

fn run(golden: &Golden) -> Ran {
    let replies = golden
        .replies
        .iter()
        .map(|reply| reply_text(reply.status, reply.text.clone()));
    let api = scripted(replies.collect());
    let mut command = Command::new(env!("CARGO_BIN_EXE_cf"));
    // Nothing of a window this test may itself run in reaches the case.
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
        .args(&golden.args)
        .env("CONSENSFLOW_URL", &api.url)
        .env("CONSENSFLOW_TOKEN", "tok")
        // A proxy in the window's environment has no part in a call on loopback.
        .env("HTTP_PROXY", "http://127.0.0.1:9")
        .env("ALL_PROXY", "http://127.0.0.1:9")
        .envs(&golden.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("cf starts");
    let mut stdin = child.stdin.take().expect("its standard input");
    // A command that reads no input may have ended before it is written.
    let _ = stdin.write_all(golden.stdin.as_deref().unwrap_or_default().as_bytes());
    drop(stdin);
    let output = child.wait_with_output().expect("cf ends");
    Ran {
        requests: api
            .received()
            .into_iter()
            .map(|received| Request {
                method: received.method,
                path: received.path,
                authorization: received.authorization,
                body: received.body,
            })
            .collect(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code(),
    }
}

#[test]
fn every_case_asks_says_and_exits_as_node_did() {
    let goldens: Vec<Golden> =
        serde_json::from_str(include_str!("goldens/board.json")).expect("the goldens");
    assert!(goldens.len() > 60, "the goldens are all there");
    let mut differ = Vec::new();
    for golden in &goldens {
        let ran = run(golden);
        let expected = Ran {
            requests: golden
                .requests
                .iter()
                .map(|request| Request {
                    method: request.method.clone(),
                    path: request.path.clone(),
                    authorization: request.authorization.clone(),
                    body: request.body.clone(),
                })
                .collect(),
            stdout: golden.stdout.clone(),
            stderr: golden.stderr.clone(),
            code: Some(golden.code),
        };
        if ran != expected {
            differ.push(format!(
                "{}:\n  node: {expected:?}\n  rust: {ran:?}",
                golden.name
            ));
        }
    }
    assert!(
        differ.is_empty(),
        "{} of {} cases differ:\n\n{}",
        differ.len(),
        goldens.len(),
        differ.join("\n\n")
    );
}

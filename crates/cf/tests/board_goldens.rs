//! The Rust `cf` held to what Node's board commands said and asked: Node's
//! `runCoreCli` (src/core/cli.js) ran each case of goldens/board.json against
//! a scripted API (tests/goldens/cf-board.mjs, both in the history at
//! b54361c), and recorded it. Each case's replies are served again, the
//! binary run as a window runs it, and its requests, output, errors and exit
//! code compared byte for byte.

mod common;

use std::collections::BTreeMap;

use cf_board::scripted::{reply_text, scripted};
use common::cf;
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
    let mut env: Vec<(&str, &str)> = vec![
        ("CONSENSFLOW_URL", &api.url),
        ("CONSENSFLOW_TOKEN", "tok"),
        // A proxy in the window's environment has no part in a call on loopback.
        ("HTTP_PROXY", "http://127.0.0.1:9"),
        ("ALL_PROXY", "http://127.0.0.1:9"),
    ];
    env.extend(
        golden
            .env
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str())),
    );
    let output = cf(
        &golden.args,
        &env,
        golden.stdin.as_deref().unwrap_or_default(),
    );
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
    // Every case, each once: a case is added or dropped here on purpose.
    let names: std::collections::HashSet<_> = goldens.iter().map(|golden| &golden.name).collect();
    assert_eq!(
        (goldens.len(), names.len()),
        (72, 72),
        "the goldens are all there, each once"
    );
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

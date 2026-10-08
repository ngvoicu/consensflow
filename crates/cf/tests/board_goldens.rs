//! The Rust `cf` held to what Node's board commands said and asked: Node's
//! `runCoreCli` ran each case of goldens/board.json against a scripted API
//! (both in the history at b54361c), and recorded it. Each case's replies are
//! served again, the binary run as a window runs it, and its requests, output,
//! errors and exit code compared byte for byte.

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

/// What a hook says of an answer it handed over (`POST /api/answers/<n>/receipt`)
/// is a route Node's board never had. A daemon of Node's answers 404 to any
/// route it lacks, and so does this one, once what was recorded has been
/// served: a receipt nobody answered would be said again, and the case would
/// wait for it. What the hook asks of a board that knows no such route is not
/// among the requests Node's recording holds.
fn is_receipt(request: &cf_board::scripted::Received) -> bool {
    request.method == "POST"
        && request.path.starts_with("/api/answers/")
        && request.path.ends_with("/receipt")
}

fn run(golden: &Golden) -> Ran {
    let node_has_no_receipt = reply_text(
        404,
        r#"{"error":"unknown-route","message":"no such command: POST /api/answers/receipt"}"#,
    );
    let replies = golden
        .replies
        .iter()
        .map(|reply| reply_text(reply.status, reply.text.clone()))
        .chain([node_has_no_receipt]);
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
            .filter(|received| !is_receipt(received))
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

/// What Node's recording said of a case, as a run of the binary would have.
fn node_ran(golden: &Golden) -> Ran {
    Ran {
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
    }
}

/// The cases the receipt and stop redesign moved on purpose: each is named
/// with the requests the binary makes now, and why they are not Node's. What
/// it prints and its exit code stay as Node's recording has them. Found by
/// running every case against the binary, never by guessing; a case that is
/// answered as Node answered it now is a line to take out.
const DEPARTED: &[(&str, &[&str], &str)] = &[(
    "a transcript of no items is asked for after the task",
    &[],
    "a command written wrong is said before the board is asked anything: Node asked for the task first, and the board took the answers in its thread as read, though nothing of them was printed",
)];

// The recordings that cannot be read are the test's failure, as in the tests that read them.
#[allow(clippy::expect_used)]
fn goldens() -> Vec<Golden> {
    let goldens: Vec<Golden> =
        serde_json::from_str(include_str!("goldens/board.json")).expect("the goldens");
    // Every case, each once: a case is added or dropped here on purpose.
    let names: std::collections::HashSet<_> = goldens.iter().map(|golden| &golden.name).collect();
    assert_eq!(
        (goldens.len(), names.len()),
        (72, 72),
        "the goldens are all there, each once"
    );
    goldens
}

#[test]
fn every_case_but_the_departed_asks_says_and_exits_as_node_did() {
    let goldens = goldens();
    let mut differ = Vec::new();
    let mut held = 0;
    for golden in goldens
        .iter()
        .filter(|golden| !DEPARTED.iter().any(|(name, _, _)| *name == golden.name))
    {
        held += 1;
        let ran = run(golden);
        let expected = node_ran(golden);
        if ran != expected {
            differ.push(format!(
                "{}:\n  node: {expected:?}\n  rust: {ran:?}",
                golden.name
            ));
        }
    }
    println!("{held} cases held to Node's, {} departed", DEPARTED.len());
    for (name, _, why) in DEPARTED {
        println!("  departed {name}: {why}");
    }
    assert!(
        differ.is_empty(),
        "{} of {} cases differ:\n\n{}",
        differ.len(),
        goldens.len(),
        differ.join("\n\n")
    );
}

/// A case is named in `DEPARTED` because it differs from Node's, in the one
/// thing it says, and for no other reason.
#[test]
fn every_departed_case_is_there_and_still_departs_in_the_requests_and_in_nothing_else() {
    let goldens = goldens();
    for (name, requests, why) in DEPARTED {
        let golden = goldens
            .iter()
            .find(|golden| golden.name == *name)
            .unwrap_or_else(|| panic!("{name} is not a recorded case ({why})"));
        let ran = run(golden);
        let asked: Vec<String> = ran
            .requests
            .iter()
            .map(|request| format!("{} {}", request.method, request.path))
            .collect();
        assert_eq!(asked, *requests, "{name}: what it asks now");
        let node = node_ran(golden);
        assert_ne!(
            ran.requests, node.requests,
            "{name} asks as Node's did now ({why})"
        );
        assert_eq!(
            (&ran.stdout, &ran.stderr, ran.code),
            (&node.stdout, &node.stderr, node.code),
            "{name}: what it prints and its exit are Node's"
        );
    }
}

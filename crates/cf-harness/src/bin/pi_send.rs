//! One message sent through Pi's channel, on the system's clock and
//! randomness, for `tests/pi-message.test.mjs`, which runs the channel's
//! cases once with JavaScript's `send` and once with this one's. It is no
//! adapter, and ships with nothing: it is built only with the `test-support`
//! feature.
//!
//! The send is one JSON object on stdin:
//! `{"channel": {"launchId", "inbox", "ack", "ackTimeoutMs"}, "session",
//! "pane", "generation", "claim", "text"}`, `claim` being what the pane host
//! answers the claim with, or `{"throws": message, "error": word}` for a host
//! that never answered. What the send answered is one JSON object on stdout,
//! as JavaScript's answer reads, or `{"threw": message}` for a failure that
//! JavaScript threw, and the exit status is then 1.

#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::process::ExitCode;

use cf_harness::contract::{HostError, Pane, PaneHost, Work};
use cf_harness::pi::{send, Answer, Target};
use cf_harness::seams::{SystemEntropy, SystemTime};
use serde_json::{json, Map, Value};

/// A pane host that answers every claim as it was told to.
struct Claiming(Result<Value, HostError>);

impl PaneHost for Claiming {
    fn request<'a>(&'a self, _op: &'a str, _body: Value) -> Work<'a, Result<Value, HostError>> {
        let answer = self.0.clone();
        Box::pin(async move { answer })
    }
}

/// A text field of `object`.
fn text<'a>(object: &'a Value, name: &str) -> Result<&'a str, String> {
    object
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("the send names no {name}"))
}

/// What a send answered, as JavaScript's object reads.
fn written(answer: &Answer) -> Value {
    let mut fields = Map::new();
    fields.insert("ok".to_owned(), json!(answer.ok));
    fields.insert("admitted".to_owned(), json!(answer.admitted));
    if let Some(error) = answer.error {
        fields.insert("error".to_owned(), json!(error));
    }
    if answer.zero_bytes {
        fields.insert("bytesWritten".to_owned(), json!(0));
    }
    if let Some(cause) = &answer.cause {
        fields.insert("cause".to_owned(), json!(cause));
    }
    if let Some(ack) = &answer.ack {
        fields.insert("ack".to_owned(), ack.clone());
    }
    Value::Object(fields)
}

/// The send `input` asks for, played on a runtime of its own.
fn run(input: &str) -> Result<Result<Answer, String>, String> {
    let asked: Value = serde_json::from_str(input).map_err(|failed| failed.to_string())?;
    let channel = asked.get("channel").ok_or("the send names no channel")?;
    let claim = asked.get("claim").ok_or("the send names no claim")?;
    let claimed = match claim.get("throws").and_then(Value::as_str) {
        Some(message) => Err(HostError {
            error: claim
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_owned),
            message: message.to_owned(),
        }),
        None => Ok(claim.clone()),
    };
    let pane = Pane {
        id: text(&asked, "pane")?.to_owned(),
        generation: asked
            .get("generation")
            .and_then(Value::as_u64)
            .ok_or("the send names no generation")?,
    };
    let host = Claiming(claimed);
    let target = Target {
        launch_id: text(channel, "launchId")?,
        inbox: text(channel, "inbox")?,
        ack: text(channel, "ack")?,
        ack_timeout_ms: channel
            .get("ackTimeoutMs")
            .and_then(Value::as_u64)
            .ok_or("the channel names no ackTimeoutMs")?,
        session: text(&asked, "session")?,
        pane: &pane,
        host: &host,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|failed| failed.to_string())?;
    Ok(runtime.block_on(send(
        &SystemTime,
        &SystemEntropy,
        &target,
        text(&asked, "text")?,
    )))
}

fn main() -> ExitCode {
    let mut input = String::new();
    let outcome = std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|failed| failed.to_string())
        .and_then(|_| run(&input));
    let (answer, code) = match outcome {
        Ok(Ok(answer)) => (written(&answer), ExitCode::SUCCESS),
        Ok(Err(threw)) | Err(threw) => (json!({ "threw": threw }), ExitCode::FAILURE),
    };
    match writeln!(std::io::stdout(), "{answer}") {
        Ok(()) => code,
        Err(_) => ExitCode::FAILURE,
    }
}

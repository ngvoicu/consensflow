//! Codex's channel on the system's clock and loopback, for
//! `tests/codex-channel.test.mjs` and `tests/delivery-contract.test.mjs`,
//! which run the channel's cases once with JavaScript's `send` and
//! `sessionState` and once with this one's. It is no adapter, and ships with
//! nothing: it is built only with the `test-support` feature.
//!
//! The question is one JSON object on the first line of stdin, as `common`
//! says, and the pane host is asked as `pane_host` says: `{"op": "send" |
//! "shown", "channel": {"launchId", "sessionBridge":
//! {"endpoint", "token"}}}`, and for a send also `"session"`, `"pane"`,
//! `"generation"` and `"text"`. A send answers as JavaScript's does, and
//! the broker's word as `sessionState` does: `{"sessionId", "available"}`,
//! or null where the broker does not answer for this launch.

#![forbid(unsafe_code)]

mod common;
mod pane_host;

use cf_harness::codex::{send, Answer, Channel, Session, Shown, Target};
use cf_harness::seams::{SystemLoopback, SystemTime};
use cf_harness::tooling::text;
use common::serve;
use pane_host::{pane, Asking};
use serde_json::{json, Map, Value};

/// The broker the question names.
fn channel(asked: &Value) -> Result<Channel, String> {
    let channel = asked
        .get("channel")
        .ok_or("the question names no channel")?;
    let bridge = channel
        .get("sessionBridge")
        .ok_or("the channel names no sessionBridge")?;
    Ok(Channel::new(
        text(channel, "launchId")?,
        text(bridge, "endpoint")?.to_owned(),
        text(bridge, "token")?.to_owned(),
    ))
}

/// What a send answered, as JavaScript's object reads.
fn written(answer: &Answer) -> Value {
    let mut fields = Map::new();
    fields.insert("ok".to_owned(), json!(answer.ok));
    fields.insert("admitted".to_owned(), json!(answer.admitted));
    if let Some(error) = &answer.error {
        fields.insert("error".to_owned(), json!(error));
    }
    if answer.zero_bytes {
        fields.insert("bytesWritten".to_owned(), json!(0));
    }
    if let Some(cause) = &answer.cause {
        fields.insert("cause".to_owned(), json!(cause));
    }
    Value::Object(fields)
}

/// The broker's word on the window, as `sessionState` returns it.
async fn shown(channel: &Channel) -> Result<Value, String> {
    let Some(Shown { session, available }) = channel.shown(&SystemTime, &SystemLoopback).await
    else {
        return Ok(Value::Null);
    };
    let session = match session {
        Session::Unnamed => Value::Null,
        Session::Thread(thread) => json!(thread),
        Session::Wrapped => {
            return Err(
                "the broker named the thread in a list, which JSON gives back as text".to_owned(),
            );
        }
    };
    Ok(json!({ "sessionId": session, "available": available }))
}

/// The send the question asks for.
async fn sent(asked: &Value, channel: &Channel) -> Result<Value, String> {
    let pane = pane(asked)?;
    let target = Target {
        channel,
        thread: asked.get("session").and_then(Value::as_str),
        pane: &pane,
        host: &Asking,
    };
    let answer = send(&SystemTime, &SystemLoopback, &target, text(asked, "text")?).await?;
    Ok(written(&answer))
}

async fn run(asked: Value) -> Result<Value, String> {
    let channel = channel(&asked)?;
    match text(&asked, "op")? {
        "shown" => shown(&channel).await,
        "send" => sent(&asked, &channel).await,
        other => Err(format!(
            "the question asks for {other}, which is none of shown and send"
        )),
    }
}

fn main() -> std::process::ExitCode {
    serve(run)
}

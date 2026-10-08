//! OpenCode's channel on the system's clock, loopback and processes, for
//! `tests/opencode-launch.test.mjs` and `tests/delivery-contract.test.mjs`,
//! which run the channel's cases once with JavaScript's `createSession`,
//! `seedSession` and `send` and once with this one's. It is no adapter, and
//! ships with nothing: it is built only with the `test-support` feature.
//!
//! The question is one JSON object on the first line of stdin, as `common`
//! says, and the pane host is asked as `pane_host` says; its `"op"` one of:
//! - `"create"`: `{"executable", "directory", "env", "configuration":
//!   {"args", "env", "channel"}}`, answered with the new conversation's id;
//! - `"seed"`: `{"channel", "session", "directory", "text", "model",
//!   "variant", "resume"}`, answered with null;
//! - `"send"`: `{"channel", "session", "pane", "generation", "text"}`,
//!   answered as JavaScript's `send` does.
//!
//! A `"channel"` is JavaScript's own: `{"launchId", "endpoint", "password",
//! "sessionBridge": {"endpoint", "token"}}`, of which an operation reads what
//! it needs and the rest is empty (a throwaway server is never given the
//! plugin's, nor a send the window's own).

#![forbid(unsafe_code)]

mod common;
mod pane_host;

use cf_base::env::Env;
use cf_harness::opencode::{
    create_session, seed_session, send, Bridge, Channel, Launched, Seed, Sent, Serve, Target,
    Wires, LIFETIME_MS, TIMEOUT_MS,
};
use cf_harness::seams::{SystemLoopback, SystemProcesses, SystemTime};
use cf_harness::tooling::text;
use common::serve;
use pane_host::{pane, Asking};
use serde_json::{json, Map, Value};

/// A text field of `object`, empty where there is none.
fn field(object: Option<&Value>, name: &str) -> String {
    object
        .and_then(|object| object.get(name))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// The channel the question names.
fn channel(object: Option<&Value>) -> Channel {
    let bridge = object.and_then(|channel| channel.get("sessionBridge"));
    Channel {
        launch_id: field(object, "launchId"),
        endpoint: field(object, "endpoint"),
        password: field(object, "password"),
        bridge: Bridge {
            endpoint: field(bridge, "endpoint"),
            token: field(bridge, "token"),
        },
    }
}

/// The variables of an object of texts, in the order it lists them.
fn variables(object: Option<&Value>) -> Result<Vec<(String, String)>, String> {
    let Some(Value::Object(object)) = object else {
        return Err("the question names no environment".to_owned());
    };
    object
        .iter()
        .map(|(name, value)| {
            let value = value.as_str().ok_or("an environment variable is no text")?;
            Ok((name.clone(), value.to_owned()))
        })
        .collect()
}

/// What a send answered, as JavaScript's object reads. A refusal is one
/// before the hand-over, which wrote nothing.
fn written(sent: &Sent) -> Value {
    let admitted = if sent.ok {
        json!(true)
    } else if sent.refused {
        json!(false)
    } else {
        Value::Null
    };
    let mut fields = Map::new();
    fields.insert("ok".to_owned(), json!(sent.ok));
    fields.insert("admitted".to_owned(), admitted);
    if sent.refused {
        fields.insert("bytesWritten".to_owned(), json!(0));
    }
    if let Some(error) = &sent.error {
        fields.insert("error".to_owned(), json!(error));
    }
    if let Some(cause) = &sent.cause {
        fields.insert("cause".to_owned(), json!(cause));
    }
    Value::Object(fields)
}

/// The conversation `create` makes on a throwaway server.
async fn created(wires: Wires<'_>, asked: &Value) -> Result<Value, String> {
    let configuration = asked
        .get("configuration")
        .ok_or("the question names no configuration")?;
    let args = configuration
        .get("args")
        .and_then(Value::as_array)
        .ok_or("the configuration names no args")?
        .iter()
        .map(|arg| arg.as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()
        .ok_or("an argument is no text")?;
    let launched = Launched {
        args,
        env: variables(configuration.get("env"))?,
        channel: channel(configuration.get("channel")),
    };
    let env = Env::from_vars(variables(asked.get("env"))?);
    let serve = Serve {
        executable: text(asked, "executable")?,
        directory: text(asked, "directory")?,
        env: &env,
        launched: &launched,
        // What the case asks for (`createSession`'s `timeoutMs`), else a window's.
        timeout_ms: asked
            .get("timeoutMs")
            .and_then(Value::as_i64)
            .unwrap_or(TIMEOUT_MS),
    };
    Ok(json!(create_session(wires, &serve).await?))
}

/// The first message `seed` posts.
async fn seeded(wires: Wires<'_>, asked: &Value) -> Result<Value, String> {
    let seed = Seed {
        session: text(asked, "session")?,
        directory: text(asked, "directory")?,
        text: text(asked, "text")?,
        model: asked.get("model").and_then(Value::as_str),
        variant: asked.get("variant").and_then(Value::as_str),
        resume: asked.get("resume") == Some(&Value::Bool(true)),
        // What the case asks for (`seedSession`'s `timeoutMs`), else a window's.
        lifetime_ms: asked
            .get("timeoutMs")
            .and_then(Value::as_u64)
            .unwrap_or(LIFETIME_MS),
    };
    seed_session(wires, &channel(asked.get("channel")), &seed).await?;
    Ok(Value::Null)
}

/// The message `send` hands to the plugin.
async fn sent(wires: Wires<'_>, asked: &Value) -> Result<Value, String> {
    let pane = pane(asked)?;
    let target = Target {
        session: text(asked, "session")?,
        pane: &pane,
        host: &Asking,
    };
    let sent = send(
        wires,
        &channel(asked.get("channel")),
        &target,
        text(asked, "text")?,
    )
    .await?;
    Ok(written(&sent))
}

/// The question, answered. `SystemProcesses::end_all` is not called on the way
/// out: a child the channel never asked to end is forced when it is dropped
/// (`cf_process::Child`), inside the call.
async fn run(asked: Value, env: Env) -> Result<Value, String> {
    let processes = SystemProcesses::new(env);
    let wires = Wires {
        time: &SystemTime,
        loopback: &SystemLoopback,
        processes: &processes,
    };
    match text(&asked, "op")? {
        "create" => created(wires, &asked).await,
        "seed" => seeded(wires, &asked).await,
        "send" => sent(wires, &asked).await,
        other => Err(format!(
            "the question asks for {other}, which is none of create, seed and send"
        )),
    }
}

fn main() -> std::process::ExitCode {
    let env = Env::from_process();
    serve(|asked| run(asked, env))
}

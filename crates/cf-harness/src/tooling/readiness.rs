//! The daemon's readiness of a Devin window a live test opened, driven as the engine
//! drives it: the adapter's own window ([`Window::ready`]) asked again and again until
//! it is ready, unfit or the time is up. It asks the pane host what the engine's
//! window asks it (the screen, and the `/bypass` and Enter that switch a window in
//! the normal mode), and the pane host is the live test's: each request is a line of
//! this program's output, `{"host": {"op", "body"}}`, and its answer the next line of
//! its input, `{"answer": value}` or `{"error": message}` for one never answered. So
//! what the window is given is what the product gives it, and a test that held a
//! window to it held the product (Astra and Sol on the B1 fix: a test that pasted
//! the command itself passed whatever the product did).

use std::io::{BufRead, Write};
use std::rc::Rc;
use std::time::{Duration, Instant};

use cf_base::env::Env;
use serde_json::{json, Value};

use super::text;
use crate::contract::{HostError, Pane, PaneHost, Readiness, Work};
use crate::seams::{Services, SystemTime};
use crate::testing::Fakes;

/// The pane host of the live test that asked, through this program's own streams.
struct Relay;

impl PaneHost for Relay {
    fn request<'a>(&'a self, op: &'a str, body: Value) -> Work<'a, Result<Value, HostError>> {
        Box::pin(async move { relayed(op, &body) })
    }
}

/// `op` with `body` asked of the live test's pane host, and its answer.
fn relayed(op: &str, body: &Value) -> Result<Value, HostError> {
    let lost = |message: String| HostError {
        error: Some("eof".to_owned()),
        message,
    };
    let request = json!({ "host": { "op": op, "body": body } });
    let mut out = std::io::stdout().lock();
    writeln!(out, "{request}")
        .and_then(|()| out.flush())
        .map_err(|cause| lost(cause.to_string()))?;
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|cause| lost(cause.to_string()))?;
    let answer: Value =
        serde_json::from_str(&line).map_err(|_| lost(format!("no answer to {op}: {line:?}")))?;
    match (answer.get("answer"), answer.get("error")) {
        (Some(answer), _) => Ok(answer.clone()),
        (None, error) => Err(lost(
            error
                .and_then(Value::as_str)
                .unwrap_or("no answer")
                .to_owned(),
        )),
    }
}

/// What one look answered, as the answer says it: `ready`, `held: …` or `unfit: …`.
fn said(readiness: &Readiness) -> String {
    match readiness {
        Readiness::Ready => "ready".to_owned(),
        Readiness::Held(held) => format!("held: {}", held.sentence()),
        Readiness::Unfit(why) => format!("unfit: {why}"),
    }
}

/// The Devin window of the question (`wire`, the wire log of its launch, `session`,
/// the conversation it is on, `pane` with its `id` and `generation`) asked whether it
/// is ready every `everyMs` until it is ready or unfit, or `withinMs` are up. Each
/// look's answer, the same one said once for as many looks as gave it in a row.
pub(super) fn devin_readiness(question: &Value) -> Result<Value, String> {
    let pane = question.get("pane").ok_or("the question names no pane")?;
    let pane = Pane {
        id: text(pane, "id")?.to_owned(),
        generation: pane
            .get("generation")
            .and_then(Value::as_u64)
            .ok_or("the question's pane has no generation")?,
    };
    let millis = |name: &str| {
        question
            .get(name)
            .and_then(Value::as_u64)
            .map(Duration::from_millis)
            .ok_or_else(|| format!("the question names no {name}"))
    };
    let (within, every) = (millis("withinMs")?, millis("everyMs")?);
    let env = Env::from_process();
    let services = Services {
        time: Rc::new(SystemTime),
        ..Fakes::new(&env).services(&env, std::path::Path::new(""))
    };
    let window = crate::devin::window_on(
        &services,
        text(question, "wire")?,
        text(question, "session")?,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|failed| failed.to_string())?;
    runtime.block_on(async {
        let started = Instant::now();
        let mut answers: Vec<String> = Vec::new();
        loop {
            let readiness = window.ready(&Relay, &pane).await?;
            let now = said(&readiness);
            if answers.last() != Some(&now) {
                answers.push(now);
            }
            let over = !matches!(readiness, Readiness::Held(_)) || started.elapsed() >= within;
            if over {
                return Ok(json!({
                    "answers": answers,
                    "ready": readiness == Readiness::Ready,
                }));
            }
            tokio::time::sleep(every).await;
        }
    })
}

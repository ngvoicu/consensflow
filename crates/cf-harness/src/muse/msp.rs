//! Muse's wire (MSP, "Muse Session Protocol": JSON-RPC 2.0, one object a line, over the
//! stdio of `muse serve`) for what needs no window and no model turn: one bounded
//! conversation, `initialize`, `initialized`, then each request answered, and the process
//! ended. ConsensFlow reads the models Muse lists this way (`models`), and no member is ever
//! driven over it (the research, T-702: the TUI a human watches cannot be attached to).
//!
//! The server rejects everything before `initialize` and `initialized`. A request is
//! answered by the reply of its own `id`; the notifications and replies of other ids that
//! come between are passed over.

use serde_json::{json, Value};

use crate::seams::processes::{Child, Processes, Program, Streams};
use crate::seams::{arm, Time};
use crate::shared::child::stop;

/// How long a conversation has to answer every request: Muse answers `model/list` in a few
/// seconds on a login (a fetch of its catalog), and past this it is given up on.
pub(crate) const BOUND_MS: u64 = 30_000;

/// The longest line a reply may be.
const LINE_LIMIT: usize = 4 << 20;

/// What `muse serve` is started with: its host with no log of its own session, which keeps
/// it from writing a session's folder for a conversation that has no session. Muse's own
/// login and config are read as the human's `muse` reads them.
pub(crate) fn serve_args() -> Vec<String> {
    ["serve", "--provider", "meta", "--no-session-log"]
        .map(str::to_owned)
        .to_vec()
}

/// `requests` (a method and its params each) put to the host `program` starts, and the
/// result of each, in order. Fails with the reason where the host does not start, ends or
/// stays silent past `bound_ms`, or answers a request with an error. The host is ended
/// whichever way.
pub(crate) async fn conversation(
    processes: &dyn Processes,
    time: &dyn Time,
    program: Program,
    requests: &[(&str, Value)],
    bound_ms: u64,
) -> Result<Vec<Value>, String> {
    let child = processes.spawn(program, Streams::Lines)?;
    let armed = arm(time, bound_ms);
    let answered = armed.bound(talk(&*child, requests)).await;
    // Ended whichever way: a host that will not end is the process's to be given up on.
    let _ = stop(time, &*child).await;
    answered.unwrap_or_else(|| {
        Err(format!(
            "muse serve did not answer within {} s",
            bound_ms / 1000
        ))
    })
}

async fn talk(child: &dyn Child, requests: &[(&str, Value)]) -> Result<Vec<Value>, String> {
    let initialize = json!({
        "clientInfo": { "name": "consensflow", "version": env!("CARGO_PKG_VERSION") },
        "capabilities": { "experimentalApi": false },
    });
    let mut id = 0_u64;
    let mut results = Vec::new();
    ask(child, &mut id, "initialize", &initialize).await?;
    // A notification: no id, no answer.
    send(child, json!({ "jsonrpc": "2.0", "method": "initialized" })).await?;
    for (method, params) in requests {
        results.push(ask(child, &mut id, method, params).await?);
    }
    Ok(results)
}

async fn send(child: &dyn Child, message: Value) -> Result<(), String> {
    child.write_line(&message.to_string()).await
}

/// The `result` of `method`, asked with `params`.
async fn ask(
    child: &dyn Child,
    id: &mut u64,
    method: &str,
    params: &Value,
) -> Result<Value, String> {
    *id += 1;
    let wanted = *id;
    send(
        child,
        json!({ "jsonrpc": "2.0", "id": wanted, "method": method, "params": params }),
    )
    .await?;
    loop {
        let line = child
            .read_line(LINE_LIMIT)
            .await?
            .ok_or_else(|| format!("muse serve ended before it answered {method}"))?;
        let Ok(reply) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if reply.get("id").and_then(Value::as_u64) != Some(wanted) || reply.get("method").is_some()
        {
            continue;
        }
        if let Some(error) = reply.get("error") {
            let said = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("no reason given");
            return Err(format!("muse serve refused {method}: {said}"));
        }
        return Ok(reply.get("result").cloned().unwrap_or(Value::Null));
    }
}

//! A Codex window's role. Codex is given the role's text itself, appended to
//! the instructions it would have read anyway: the native resolver is asked for
//! them, so the profile and project layering is kept, and no version is read.

use std::path::PathBuf;

use cf_base::js;
use cf_base::json::from_slice_lossy;
use serde_json::{json, Value};

use crate::seams::processes::{Child, Ending, Program, Streams};
use crate::seams::{arm, Services};

/// Why a launch is refused when Codex's own instructions could not be read.
const UNREAD: &str = "Cannot read native Codex instructions safely";

/// How long Codex's app-server has to answer, from the moment it starts.
const ANSWER_WITHIN_MS: u64 = 10_000;

/// The most a line of the app-server's may say.
const LINE_LIMIT: usize = 2 * 1024 * 1024;

/// The arguments that give a window of `role` its role: Codex's own
/// instructions for a window opened in `cwd`, then the role's `content`, as
/// the developer's instructions.
pub(super) async fn arguments(
    services: &Services,
    role: &str,
    executable: &str,
    cwd: &str,
    content: &str,
) -> Result<Vec<String>, String> {
    if content.is_empty() {
        return Err(format!("the {role} window needs its role text"));
    }
    let existing = instructions(services, executable, cwd).await?;
    let text = format!(
        "{existing}\n\nYour ConsensFlow role is {role}. The following role instructions are already loaded; follow them for app coordination. This is context, not a task; wait for the user's request.\n\n{content}"
    );
    Ok(vec![
        "-c".to_owned(),
        format!(
            "developer_instructions={}",
            js::stringify(&Value::String(text))
        ),
    ])
}

/// The instructions Codex itself would give a window opened in `cwd`, asked of
/// its app-server, which is then asked to end whatever came of it.
async fn instructions(services: &Services, executable: &str, cwd: &str) -> Result<String, String> {
    let program = Program {
        executable: PathBuf::from(executable),
        args: vec!["app-server".to_owned()],
        cwd: Some(PathBuf::from(cwd)),
        env: services.env.clone(),
    };
    let child = services
        .processes
        .spawn(program, Streams::Lines)
        .map_err(|_| UNREAD.to_owned())?;
    let timer = arm(&*services.time, ANSWER_WITHIN_MS);
    let said = timer.bound(converse(&*child, cwd)).await;
    child.terminate(Ending::Asked);
    said.flatten().ok_or_else(|| UNREAD.to_owned())
}

/// The app-server's dialogue: it is initialized, then asked for its
/// configuration, and the developer instructions in that are the answer. None
/// where it fails to be: it ends or says too much, speaks no JSON, refuses a
/// request, or has no configuration or instructions that are text.
///
/// Kept from Node on purpose: a line of `null` fails here, where reading its
/// `id` threw a `TypeError` out of the stream's handler; and JSON nested past
/// 127 levels, or holding a number past a double's range, is no JSON here.
async fn converse(child: &dyn Child, cwd: &str) -> Option<String> {
    let send =
        |message: Value| async move { child.write_line(&js::stringify(&message)).await.ok() };
    send(json!({
        "id": 1,
        "method": "initialize",
        "params": {
            "clientInfo": { "name": "consensflow-role-config", "version": "3.0.0" },
            "capabilities": { "experimentalApi": true },
        },
    }))
    .await?;
    loop {
        let line = child.read_line(LINE_LIMIT).await.ok()??;
        let message = from_slice_lossy(line.as_bytes())
            .ok()
            .filter(|message| !message.is_null())?;
        match message.get("id").and_then(Value::as_f64) {
            Some(1.0) => {
                if js::truthy(message.get("error")) {
                    return None;
                }
                send(json!({ "method": "initialized", "params": {} })).await?;
                let read = json!({
                    "id": 2,
                    "method": "config/read",
                    "params": { "cwd": cwd, "includeLayers": false },
                });
                send(read).await?;
            }
            Some(2.0) => return configured(&message),
            _ => {}
        }
    }
}

/// The developer instructions the app-server's answer to the read of its
/// configuration holds: none, as empty text, or none at all where it is no
/// answer or the instructions are no text.
fn configured(message: &Value) -> Option<String> {
    let configuration = message
        .get("result")
        .and_then(|result| result.get("config"));
    if js::truthy(message.get("error")) || !js::truthy(configuration) {
        return None;
    }
    match configuration?.get("developer_instructions") {
        None | Some(Value::Null) => Some(String::new()),
        Some(Value::String(text)) => Some(text.clone()),
        Some(_) => None,
    }
}

#[cfg(test)]
mod tests;

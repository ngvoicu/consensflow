//! What the board page may ask of the app: the daemon's operations it forwards,
//! its panes' input, size and acknowledgements, its output subscription, and
//! the address of the daemon's harnesses screen.

use std::sync::Arc;

use serde_json::{json, Value};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::daemon::RosterHandle;
use crate::runtime::{AppRuntime, PAGE_STATE_EVENT};
use cf_bridge::Bridge;
use cf_panes::input_queue::{InputWork, NoticeBody, PageInputCompletion, PendingNotices};
use cf_panes::output_hub::PaneOutputMessage;
use cf_panes::page;
use cf_panes::validation::{decode_page_bytes, pane_key, validate_text};

pub(crate) fn window_command_allowed(window: &str, _command: &str) -> bool {
    window == "main"
}

fn request_daemon(connection: Result<Bridge, String>, operation: String, body: Value) -> Value {
    let bridge = match connection {
        Ok(bridge) => bridge,
        Err(cause) => return not_available(&operation, &cause),
    };
    let deadline = cf_proto::page::request_deadline_ms(&operation);
    match bridge.request(operation.clone(), body, deadline) {
        Ok(response) => response,
        Err(error) => json!({"ok":false,"error":error.to_string(),"operation":operation}),
    }
}

fn not_available(operation: &str, detail: &str) -> Value {
    json!({
        "ok":false,
        "error":"not-available-yet",
        "operation":operation,
        "detail":detail,
    })
}

async fn run_blocking<F>(operation: &'static str, task: F) -> Value
where
    F: FnOnce() -> Value + Send + 'static,
{
    match tauri::async_runtime::spawn_blocking(task).await {
        Ok(value) => value,
        Err(error) => {
            json!({"ok":false,"error":format!("{operation} worker failed: {error}"),"operation":operation})
        }
    }
}

async fn input_result(result: Result<PageInputCompletion, String>) -> Value {
    let completion = match result {
        Ok(completion) => completion,
        Err(error) => return json!({"ok":false,"error":error}),
    };
    match completion.receiver.clone().await {
        Ok(()) => json!({"ok":true}),
        Err(error) => json!({
            "ok":false,
            "error":error.to_string(),
            "uncertain":error.is_uncertain(),
            "failedUntilRestart":error.failed_until_restart(),
            // The host recorded this call's outcome itself: the page confirms its number once
            // it has said so.
            "loss":completion.loss(),
        }),
    }
}

fn page_input_finished(
    human: bool,
    mut result: Value,
    emit: impl FnOnce(&str, Value) -> tauri::Result<()>,
    log: impl FnOnce(&str),
) -> Value {
    if human && result["ok"] == true {
        if let Err(error) = emit(PAGE_STATE_EVENT, json!({"reason":"human-input"})) {
            let warning = format!(
                "ConsensFlow could not send the page the {PAGE_STATE_EVENT} event after human input reached the pane: {error}; the page's state may be out of date"
            );
            log(&warning);
            result["warning"] = warning.into();
        }
    }
    result
}

/// A page announces itself with a nonce of its document's own; the host answers with its
/// epoch for this document, and the page echoes that exact pair to activate.
#[tauri::command]
pub fn pane_page_prepare<R: Runtime>(app: AppHandle<R>, page_id: String, nonce: String) -> Value {
    let state = app.state::<AppRuntime>();
    match state.inputs.prepare_page(&page_id, &nonce) {
        Ok(epoch) => json!({"ok":true,"epoch":epoch,"nonce":nonce}),
        Err(error) => json!({"ok":false,"error":error}),
    }
}

/// The event that tells the page that is current that typing may not have reached a
/// window: its `id` and `generation`, and the `loss` number the page confirms.
pub(crate) const PANE_INPUT_LOST_EVENT: &str = "pane-input-lost";

/// Tells the page what the host gives up on of its input: the windows whose typing may
/// not have arrived, and a line for the app's log.
pub(crate) struct PageInputObserver<R: Runtime>(pub AppHandle<R>);

impl<R: Runtime> cf_panes::input_queue::InputObserver for PageInputObserver<R> {
    fn lost(&self, key: &cf_panes::pty::PaneKey, loss: u64) {
        if let Err(error) = self.0.emit(
            PANE_INPUT_LOST_EVENT,
            json!({"id":key.id,"generation":key.generation,"loss":loss}),
        ) {
            crate::diag::say(format!(
                "could not tell the page its typing was lost: {error}"
            ));
        }
    }

    fn say(&self, line: &str) {
        crate::diag::say(line);
    }
}

fn notices_json(notices: &PendingNotices) -> Value {
    json!({
        "notices":notices.iter().map(|notice| match &notice.body {
            NoticeBody::Window { key, name, closed, failed, dropped } => json!({
                "id":notice.id,
                "kind":"window",
                "window":key.id,
                "generation":key.generation,
                "name":name,
                "closed":closed,
                "failed":failed,
                "dropped":dropped,
            }),
            NoticeBody::Folded { count } => json!({
                "id":notice.id,
                "kind":"folded",
                "count":count,
            }),
        }).collect::<Vec<_>>(),
    })
}

/// The person dismissed the notice with this id: it goes if it is still outstanding under that
/// id, on the current page of the current epoch. A notice the host replaced has a new id.
#[tauri::command]
pub fn pane_lost_dismiss<R: Runtime>(
    app: AppHandle<R>,
    page_id: String,
    epoch: u64,
    id: u64,
) -> Value {
    let state = app.state::<AppRuntime>();
    match state.inputs.dismiss_notice(&page_id, epoch, id) {
        Ok(removed) => json!({"ok":true,"removed":removed}),
        Err(error) => json!({"ok":false,"error":error}),
    }
}

/// The page says what a window is called, so that a notice of a window that has closed names it.
#[tauri::command]
pub fn pane_window_name<R: Runtime>(
    app: AppHandle<R>,
    page_id: String,
    epoch: u64,
    id: String,
    generation: u64,
    name: String,
) -> Value {
    if name.is_empty() || name.len() > 200 {
        return json!({"ok":false,"error":"invalid window name"});
    }
    let key = match pane_key(&id, generation) {
        Ok(key) => key,
        Err(error) => return json!({"ok":false,"error":error}),
    };
    let state = app.state::<AppRuntime>();
    match state.inputs.name_window(&page_id, epoch, &key, &name) {
        Ok(()) => json!({"ok":true}),
        Err(error) => json!({"ok":false,"error":error}),
    }
}

/// What the host still owes the person, for the page that is current to show. The page asks at
/// its activation, after its board loads, on a timer and when it comes into view.
#[tauri::command]
pub fn pane_lost_pull<R: Runtime>(app: AppHandle<R>, page_id: String, epoch: u64) -> Value {
    let state = app.state::<AppRuntime>();
    match state.inputs.pending_losses(&page_id, epoch) {
        Ok(notices) => {
            let mut answer = notices_json(&notices);
            answer["ok"] = true.into();
            answer
        }
        Err(error) => json!({"ok":false,"error":error}),
    }
}

/// The page becomes current, echoing the epoch and nonce its preparation was answered. The
/// windows whose typing may not have reached them come back, for it to say so.
#[tauri::command]
pub fn pane_page_activate<R: Runtime>(
    app: AppHandle<R>,
    page_id: String,
    nonce: String,
    epoch: u64,
) -> Value {
    let state = app.state::<AppRuntime>();
    match state.inputs.activate_page(&page_id, &nonce, epoch) {
        Ok(notices) => {
            let mut answer = notices_json(&notices);
            answer["ok"] = true.into();
            answer["epoch"] = epoch.into();
            answer
        }
        Err(error) => json!({"ok":false,"error":error}),
    }
}

/// What a page call says of itself: its page, pane and number.
struct PageCall {
    page_id: String,
    epoch: u64,
    id: String,
    generation: u64,
    sequence: u64,
}

/// A call's bytes arrive as base64 text in the command's arguments, which both
/// of Tauri's transports carry, with the length the page counted: invalid
/// base64 or bytes of a different length are refused before writing.
fn enqueue_page_text<R: Runtime>(
    app: &AppHandle<R>,
    call: &PageCall,
    length: u64,
    text: &str,
    reply: bool,
) -> Value {
    match decode_page_bytes(length, text) {
        Ok(bytes) => enqueue_page_call(app, call, bytes, reply),
        Err(error) => {
            // A refusal is final for its number: the page goes on with the next, so the
            // order must not wait for this one.
            if let Ok(key) = pane_key(&call.id, call.generation) {
                let state = app.state::<AppRuntime>();
                match state.inputs.refuse_page_sequence(
                    &call.page_id,
                    call.epoch,
                    &key,
                    call.sequence,
                ) {
                    Ok(loss) => return json!({"ok":false,"error":error,"loss":loss}),
                    // Its number cannot be spent yet (the page's slots are full): the page skips
                    // it until the host has, and never takes this for a final refusal.
                    Err(pending) if pending == cf_panes::input_queue::SKIP_PENDING => {
                        return json!({"ok":false,"error":error,"spent":false});
                    }
                    Err(_) => {}
                }
            }
            json!({"ok":false,"error":error})
        }
    }
}

fn enqueue_page_call<R: Runtime>(
    app: &AppHandle<R>,
    call: &PageCall,
    bytes: Vec<u8>,
    reply: bool,
) -> Value {
    let key = match pane_key(&call.id, call.generation) {
        Ok(key) => key,
        Err(error) => return json!({"ok":false,"error":error}),
    };
    let inputs = {
        let state = app.state::<AppRuntime>();
        Arc::clone(&state.inputs)
    };
    let work = if reply {
        InputWork::Reply(bytes)
    } else {
        InputWork::Typed(bytes)
    };
    match inputs.enqueue_page_told(&call.page_id, call.epoch, key, call.sequence, work, !reply) {
        Ok(ticket) => json!({"ok":true,"ticket":ticket}),
        Err(refusal) => match refusal.loss {
            Some(loss) => {
                json!({"ok":false,"error":refusal.error,"loss":loss,"spent":refusal.spent})
            }
            None => json!({"ok":false,"error":refusal.error}),
        },
    }
}

/// What the human types or pastes, in one call: the bytes of a key, or of a
/// paste whole with its markers.
#[tauri::command]
// The page names each argument; a command cannot take them as one value.
#[allow(clippy::too_many_arguments)]
pub fn pane_input_enqueue<R: Runtime>(
    app: AppHandle<R>,
    page_id: String,
    epoch: u64,
    id: String,
    generation: u64,
    sequence: u64,
    length: u64,
    bytes: String,
) -> Value {
    let call = PageCall {
        page_id,
        epoch,
        id,
        generation,
        sequence,
    };
    enqueue_page_text(&app, &call, length, &bytes, false)
}

#[tauri::command]
// The page names each argument; a command cannot take them as one value.
#[allow(clippy::too_many_arguments)]
pub fn pane_reply_enqueue<R: Runtime>(
    app: AppHandle<R>,
    page_id: String,
    epoch: u64,
    id: String,
    generation: u64,
    sequence: u64,
    length: u64,
    bytes: String,
) -> Value {
    let call = PageCall {
        page_id,
        epoch,
        id,
        generation,
        sequence,
    };
    enqueue_page_text(&app, &call, length, &bytes, true)
}

/// A call's fate is unknown to the page, which will not send it again: the
/// pane's count moves past its number, so a late original is refused.
#[tauri::command]
pub fn pane_input_skip<R: Runtime>(
    app: AppHandle<R>,
    page_id: String,
    epoch: u64,
    id: String,
    generation: u64,
    sequence: u64,
) -> Value {
    let key = match pane_key(&id, generation) {
        Ok(key) => key,
        Err(error) => return json!({"ok":false,"error":error}),
    };
    let state = app.state::<AppRuntime>();
    match state
        .inputs
        .skip_page_sequence(&page_id, epoch, &key, sequence)
    {
        Ok(loss) => json!({"ok":true,"loss":loss}),
        // Its number cannot be spent yet: the page asks again, and the skip stays pending.
        Err(error) if error == cf_panes::input_queue::SKIP_PENDING => {
            json!({"ok":false,"error":error,"retry":true})
        }
        Err(error) => json!({"ok":false,"error":error}),
    }
}

/// A drop took this number at its gesture, before it looked up its path: the host holds the
/// place, and says so itself if the page reloads before the path is typed.
#[tauri::command]
pub fn pane_input_place<R: Runtime>(
    app: AppHandle<R>,
    page_id: String,
    epoch: u64,
    id: String,
    generation: u64,
    sequence: u64,
) -> Value {
    let key = match pane_key(&id, generation) {
        Ok(key) => key,
        Err(error) => return json!({"ok":false,"error":error}),
    };
    let state = app.state::<AppRuntime>();
    match state
        .inputs
        .place_page_sequence(&page_id, epoch, &key, sequence)
    {
        Ok(()) => json!({"ok":true}),
        Err(error) => json!({"ok":false,"error":error}),
    }
}

/// A number the page took for a gesture and never used is let go: spent, and nothing told.
#[tauri::command]
pub fn pane_input_release<R: Runtime>(
    app: AppHandle<R>,
    page_id: String,
    epoch: u64,
    id: String,
    generation: u64,
    sequence: u64,
) -> Value {
    let key = match pane_key(&id, generation) {
        Ok(key) => key,
        Err(error) => return json!({"ok":false,"error":error}),
    };
    let state = app.state::<AppRuntime>();
    match state
        .inputs
        .release_page_sequence(&page_id, epoch, &key, sequence)
    {
        Ok(()) => json!({"ok":true}),
        Err(error) if error == cf_panes::input_queue::SKIP_PENDING => {
            json!({"ok":false,"error":error,"retry":true})
        }
        Err(error) => json!({"ok":false,"error":error}),
    }
}

#[tauri::command]
pub async fn pane_input_wait<R: Runtime>(
    app: AppHandle<R>,
    page_id: String,
    ticket: String,
) -> Value {
    crate::panics::settled("pane_input_wait", async move {
        if let Err(error) = validate_text(&ticket, "pane input ticket") {
            return json!({"ok":false,"error":error});
        }
        let inputs = {
            let state = app.state::<AppRuntime>();
            Arc::clone(&state.inputs)
        };
        let completion = inputs.page_completion(&page_id, &ticket);
        let result = input_result(completion.clone()).await;
        let human = result["ok"] == true
            && completion
                .as_ref()
                .is_ok_and(PageInputCompletion::notify_human_once);
        page_input_finished(
            human,
            result,
            |event, body| app.emit(event, body),
            |said| crate::diag::say(said),
        )
    })
    .await
}

/// The page has received a wait's answer. Repeating this is harmless.
#[tauri::command]
pub fn pane_input_forget<R: Runtime>(app: AppHandle<R>, page_id: String, ticket: String) -> Value {
    if let Err(error) = validate_text(&ticket, "pane input ticket") {
        return json!({"ok":false,"error":error});
    }
    let inputs = {
        let state = app.state::<AppRuntime>();
        Arc::clone(&state.inputs)
    };
    match inputs.forget_page_completion(&page_id, &ticket) {
        Ok(()) => json!({"ok":true}),
        Err(error) => json!({"ok":false,"error":error}),
    }
}

#[tauri::command]
// A command's parameters are its arguments, so it takes as many as the page sends.
#[allow(clippy::too_many_arguments)]
pub async fn pane_resize<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    generation: u64,
    cols: u16,
    rows: u16,
    page_id: Option<String>,
    at: Option<u64>,
) -> Value {
    crate::panics::settled("pane_resize", async move {
        let (panes, inputs) = {
            let state = app.state::<AppRuntime>();
            (Arc::clone(&state.panes), Arc::clone(&state.inputs))
        };
        run_blocking("pane_resize", move || {
            page::resize(
                &inputs,
                &panes,
                page_id.as_deref(),
                &id,
                generation,
                (cols, rows),
                at,
            )
        })
        .await
    })
    .await
}

/// A window's history for the page now subscribed, from `from` (none: the page has nothing):
/// raw bytes, not a list of numbers ([`cf_panes::page::history`]).
#[tauri::command]
pub async fn pane_history<R: Runtime>(
    app: AppHandle<R>,
    page_id: String,
    id: String,
    generation: u64,
    from: Option<u64>,
) -> tauri::ipc::Response {
    let (panes, inputs) = {
        let state = app.state::<AppRuntime>();
        (Arc::clone(&state.panes), Arc::clone(&state.inputs))
    };
    let frame = tauri::async_runtime::spawn_blocking(move || {
        page::history(&inputs, &panes, &page_id, &id, generation, from)
    })
    .await
    .unwrap_or_else(|_| page::history_refusal(&crate::panics::failed("pane_history")));
    tauri::ipc::Response::new(frame)
}

/// The page's emulator has taken all of the history it pulled ([`cf_panes::page::history_done`]).
#[tauri::command]
pub async fn pane_history_done<R: Runtime>(
    app: AppHandle<R>,
    page_id: String,
    id: String,
    generation: u64,
    consumed: u64,
) -> Value {
    crate::panics::settled("pane_history_done", async move {
        let (panes, inputs) = {
            let state = app.state::<AppRuntime>();
            (Arc::clone(&state.panes), Arc::clone(&state.inputs))
        };
        page::history_done(&inputs, &panes, &page_id, &id, generation, consumed)
    })
    .await
}

#[tauri::command]
pub async fn pane_ack<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    generation: u64,
    seq: u64,
) -> Value {
    crate::panics::settled("pane_ack", async move {
        let panes = {
            let state = app.state::<AppRuntime>();
            Arc::clone(&state.panes)
        };
        page::ack(&panes, &id, generation, seq)
    })
    .await
}

async fn task_operation<R: Runtime>(
    app: AppHandle<R>,
    operation: &'static str,
    body: Value,
) -> Value {
    let daemon = Arc::clone(&app.state::<AppRuntime>().daemon);
    // Off the async runtime: asked while the daemon is starting, it waits.
    run_blocking(operation, move || {
        request_daemon(daemon.connection(), operation.to_string(), body)
    })
    .await
}

/// What the board page may ask the daemon. The page names the operation and
/// its body; anything else is refused here, before it reaches the daemon.
const DAEMON_OPERATIONS: &[&str] = &[
    "projects.list",
    "chief.switch",
    "project.open",
    "project.resume",
    "project.close",
    "project.delete",
    "project.gate",
    "board.get",
    "inbox.get",
    "member.add",
    "member.remove",
    "member.roles",
    "member.back",
    "session.open",
    "session.hide",
    "session.end",
    "task.get",
    "task.transcript",
    "task.cancel",
    "task.pause",
    "task.reassign",
    "task.finish",
    "task.resume",
    "tasks.delete",
    "message.read",
    "message.approve",
    "message.decline",
    "harnesses.missing",
    "staff.last",
    "wakeup.get",
    "wakeup.set",
    "project.specs",
    "project.rename",
    "project.describe",
    "specs.get",
    "specs.set",
    "specs.tree",
    "specs.file",
    "specs.present",
    "specs.open",
    "history.get",
    "history.part",
    "computer_use.get",
    "computer_use.open",
    "project.meeting",
    "project.rules",
    "project.compaction",
    "project.advisor_tool",
    "project.own_hands",
    "project.prompt_suggestion",
    "models.get",
    "member.edit",
    "backup.add",
    "backup.change",
    "backup.remove",
    "member.vacation",
    "member.vacation_back",
];

#[tauri::command]
pub async fn daemon_request<R: Runtime>(
    app: AppHandle<R>,
    operation: String,
    body: Value,
) -> Value {
    crate::panics::settled("daemon_request", async move {
        let Some(operation) = DAEMON_OPERATIONS
            .iter()
            .find(|allowed| **allowed == operation)
        else {
            return json!({"ok":false,"error":format!("unknown daemon operation {operation}")});
        };
        if !body.is_object() {
            return json!({"ok":false,"error":"a daemon request body is an object"});
        }
        task_operation(app, operation, body).await
    })
    .await
}

/// The page's pane-output subscription, taken ONCE for the life of the page.
///
/// It used to ride on every `state.list`, and that was silently destructive:
/// Tauri builds a fresh `Channel` for each invocation carrying one, and
/// dropping the previous one emits `{end:true}` to the JavaScript callback
/// the page reuses. So the SECOND refresh tore down the live subscription and
/// every pane went blank for the rest of the session — with no error
/// anywhere, because the sends that followed still returned ok into a channel
/// nothing was listening to. Only the packaged app could show it: a shimmed
/// page test has no real channel to end.
///
/// Page activation starts each pane's input count. The subscription is fenced
/// by that identity so a late subscription cannot replace the new page's sink.
///
/// It is also where the page before is let off what it never drew. A pane
/// stops reading once a window of its output is unacknowledged, and only the
/// page acknowledges; sends to a page that has gone still succeed (a
/// channel's send is a script the webview runs), so those bytes stayed owed,
/// and a pane printing faster than the page drew at the reload never printed
/// again, its harness waiting on its terminal for good. Only what the page
/// before was handed is settled: what waits for this page stays charged.
///
/// And it is when a new page hears where the daemon stands: what was told
/// before it listened (a first start that failed while the page loaded) is
/// told again, so the page listens to `daemon-status` before it subscribes.
#[tauri::command]
pub async fn subscribe_output<R: Runtime>(
    app: AppHandle<R>,
    page_id: String,
    on_output: Channel<PaneOutputMessage>,
) -> Value {
    crate::panics::settled("subscribe_output", async move {
        let (panes, output, inputs, daemon) = {
            let state = app.state::<AppRuntime>();
            (
                Arc::clone(&state.panes),
                Arc::clone(&state.output),
                Arc::clone(&state.inputs),
                Arc::clone(&state.daemon),
            )
        };
        // Once the new page listens: everything sent before it went to the old. The pane table
        // swaps the sink and settles what the page before was handed in one step, and tells the
        // journals of windows that keep them where the new page's history ends.
        let answer = page::subscribe(
            &inputs,
            &panes,
            &output,
            &page_id,
            Arc::new(move |message| on_output.send(message).is_ok()),
        );
        if answer["ok"] == true {
            daemon.tell_again();
        }
        answer
    })
    .await
}

/// The address of the daemon's screen (the harnesses; the saved agents' screen is no
/// more): the daemon's page with the UI token, which the board's page frames in a
/// dialog of its own.
#[tauri::command]
pub async fn agents_screen<R: Runtime>(app: AppHandle<R>, page: String) -> Value {
    crate::panics::settled("agents_screen", async move {
    let daemon = Arc::clone(&app.state::<AppRuntime>().daemon);
    // Off the async runtime: asked while the daemon is starting, it waits.
    run_blocking("agents_screen", move || {
        let Some(roster) = daemon.roster() else {
            return json!({"ok":false,"error":"the harnesses screen is not available: the daemon is not up"});
        };
        match agents_url(&roster, &page) {
            Ok(url) => json!({"ok":true,"url":url.as_str()}),
            Err(error) => json!({"ok":false,"error":error}),
        }
    })
    .await
})
    .await
}

const AGENTS_PAGES: &[&str] = &["harnesses"];

/// The daemon's page for its screen (the harnesses), carrying the UI token.
fn agents_url(roster: &RosterHandle, page: &str) -> Result<tauri::Url, String> {
    if !AGENTS_PAGES.contains(&page) {
        return Err(format!("no agents screen {page:?}"));
    }
    let mut url = tauri::Url::parse(&roster.url)
        .map_err(|error| format!("the daemon's handle is not an address: {error}"))?;
    url.set_path(&format!("/{page}"));
    url.query_pairs_mut()
        .clear()
        .append_pair("token", &roster.token);
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three commands of a reload as the page calls them, over the app's own state: a pull
    /// before the page has subscribed is refused, one after it is answered, and the history is
    /// asked to be let go only with the whole of it taken.
    #[test]
    fn a_reloading_pages_commands_subscribe_pull_and_settle_the_history() {
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), arbiter));
        let runtime = test_runtime(Arc::clone(&panes), Arc::clone(&inputs), None, None);
        let app = tauri::test::mock_builder()
            .manage(runtime)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let handle = app.handle().clone();
        inputs.begin_page("page").unwrap();
        let pulled = || {
            let response = tauri::async_runtime::block_on(pane_history(
                handle.clone(),
                "page".into(),
                "w".into(),
                1,
                None,
            ));
            let body = tauri::ipc::IpcResponse::body(response).unwrap();
            let tauri::ipc::InvokeResponseBody::Raw(frame) = body else {
                panic!("pane_history answers raw bytes");
            };
            let length = u32::from_le_bytes(frame[..4].try_into().unwrap()) as usize;
            serde_json::from_slice::<Value>(&frame[4..4 + length]).unwrap()
        };
        assert_eq!(pulled()["error"], "pane-not-subscribed");
        let subscribed = tauri::async_runtime::block_on(subscribe_output(
            handle.clone(),
            "page".into(),
            Channel::new(|_| Ok(())),
        ));
        assert_eq!(subscribed, json!({"ok":true}));
        assert_eq!(pulled()["error"], "pane-not-found");
        let done = tauri::async_runtime::block_on(pane_history_done(
            handle.clone(),
            "page".into(),
            "w".into(),
            1,
            0,
        ));
        assert_eq!(done, json!({"ok":false}), "there is no journal to forget");
        drop(app);
    }

    /// A page call as the page makes it: the command's JSON arguments, with the
    /// bytes as base64 text and their length. A test may give a list of bytes
    /// and have both made, or give its own `length` and `bytes` text.
    /// An answer without the number of the loss the host recorded for it: it counts the notices
    /// the host has made so far, which a test of something else does not.
    #[cfg(unix)]
    fn without_loss(mut answer: Value) -> Value {
        if let Some(object) = answer.as_object_mut() {
            assert!(
                object.get("loss").is_none_or(Value::is_u64),
                "a loss is a number: {answer}"
            );
            object.remove("loss");
        }
        answer
    }

    #[cfg(unix)]
    fn ipc_request(command: &str, mut args: Value) -> tauri::webview::InvokeRequest {
        use base64::Engine as _;
        if args["pageId"].is_null() {
            args["pageId"] = "test-page".into();
        }
        if args["epoch"].is_null() {
            args["epoch"] = 0.into();
        }
        if let Some(list) = args["bytes"].as_array() {
            let bytes = list
                .iter()
                .map(|byte| byte.as_u64().expect("a byte") as u8)
                .collect::<Vec<u8>>();
            args["length"] = bytes.len().into();
            args["bytes"] = base64::engine::general_purpose::STANDARD
                .encode(&bytes)
                .into();
        }
        tauri::webview::InvokeRequest {
            cmd: command.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "tauri://localhost".parse().expect("invoke URL"),
            body: tauri::ipc::InvokeBody::Json(args),
            headers: tauri::http::HeaderMap::new(),
            invoke_key: tauri::test::INVOKE_KEY.to_string(),
        }
    }

    /// A call made straight to the admission, once its bytes are decoded.
    #[cfg(unix)]
    fn page_input<R: Runtime>(
        app: &AppHandle<R>,
        page: &str,
        key: &PaneKey,
        sequence: u64,
        bytes: &[u8],
    ) -> Value {
        let epoch = app.state::<AppRuntime>().inputs.epoch();
        enqueue_page_call(
            app,
            &PageCall {
                page_id: page.into(),
                epoch,
                id: key.id.clone(),
                generation: key.generation,
                sequence,
            },
            bytes.to_vec(),
            false,
        )
    }

    #[cfg(unix)]
    use std::collections::HashMap;
    #[cfg(unix)]
    use std::io::Read;
    #[cfg(unix)]
    use std::path::Path;
    #[cfg(unix)]
    use std::sync::mpsc;
    #[cfg(unix)]
    use std::thread;
    #[cfg(unix)]
    use std::time::Duration;

    #[cfg(unix)]
    use portable_pty::PtySize;

    #[cfg(unix)]
    use crate::runtime::test_runtime;
    #[cfg(unix)]
    use cf_bridge::BridgeBuilder;
    #[cfg(unix)]
    use cf_panes::arbiter::{EnterTiming, InputArbiter};
    #[cfg(unix)]
    use cf_panes::input_queue::InputQueue;
    #[cfg(unix)]
    use cf_panes::pty::{PaneEnvironment, PaneKey, PaneTable};
    #[cfg(unix)]
    use cf_panes::validation::MAX_INPUT_BYTES;
    use cf_panes::validation::MAX_PAGE_INPUT_BYTES;
    #[cfg(unix)]
    use cf_proto::bridge::Role;

    /// A real macOS PTY EIO fails the generation. The two later keys are
    /// refused without writing, but still say the pane needs a restart.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_pty_write_failure_and_later_keys_report_the_failed_pane() {
        let _pty_guard = cf_panes::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        inputs.begin_page("test-page").unwrap();
        let key = PaneKey::new("failed-command", 1);
        let mut reader = panes
            .open_at(
                key.clone(),
                Path::new("/tmp"),
                &[
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "/bin/stty raw -echo; printf ready; /usr/bin/head -c 1 >/dev/null".to_string(),
                ],
                &HashMap::new(),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            )
            .expect("open the test pane");
        arbiter.register(&key).expect("register the pane");
        inputs.open(&key).expect("open the pane's input");
        let mut ready = [0; 5];
        reader
            .read_exact(&mut ready)
            .expect("read readiness marker");
        assert_eq!(&ready, b"ready");
        let runtime = test_runtime(Arc::clone(&panes), Arc::clone(&inputs), None, None);
        let app = tauri::test::mock_builder()
            .manage(runtime)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let handle = app.handle().clone();
        let mut answers = Vec::new();
        for (sequence, bytes) in [
            (1, vec![b'A'; 256 * 1024]),
            (2, vec![b'B']),
            (3, vec![b'C']),
        ] {
            let admitted = page_input(&handle, "test-page", &key, sequence, &bytes);
            assert_eq!(admitted["ok"], true, "{admitted}");
            answers.push(tauri::async_runtime::block_on(pane_input_wait(
                handle.clone(),
                "test-page".to_string(),
                admitted["ticket"].as_str().expect("ticket").to_string(),
            )));
        }
        inputs.close_and_drain();
        panes.kill(&key).expect("close the test pane");
        drop(app);
        println!("REAL_PTY_ANSWERS={}", json!(answers));
        assert_eq!(answers[0]["ok"], false, "{:?}", answers);
        assert!(
            answers[0]["error"].as_str().unwrap().contains("os error 5"),
            "{:?}",
            answers
        );
        assert_eq!(answers[0]["uncertain"], true, "{:?}", answers);
        assert_eq!(answers[0]["failedUntilRestart"], true, "{:?}", answers);
        for answer in &answers[1..] {
            assert_eq!(answer["error"], "pane input is failed until restart");
            assert_eq!(answer["uncertain"], false);
            assert_eq!(answer["failedUntilRestart"], true);
        }
    }

    #[test]
    fn notification_failure_reports_that_human_input_was_already_taken() {
        let said = std::cell::RefCell::new(Vec::new());
        let result = page_input_finished(
            true,
            json!({"ok":true}),
            |event, body| {
                assert_eq!(event, PAGE_STATE_EVENT);
                assert_eq!(body, json!({"reason":"human-input"}));
                Err(std::io::Error::other("webview gone").into())
            },
            |line| said.borrow_mut().push(line.to_owned()),
        );
        assert_eq!(result["ok"], true, "the pane already took the input");
        let warning = result["warning"].as_str().expect("notification warning");
        assert!(warning.contains(PAGE_STATE_EVENT), "{warning}");
        assert!(warning.contains("human input"), "{warning}");
        assert!(warning.contains("webview gone"), "{warning}");
        assert_eq!(said.into_inner(), [warning]);
    }

    #[test]
    fn only_successful_human_input_notifies_the_page() {
        for (human, result) in [
            (false, json!({"ok":true})),
            (true, json!({"ok":false,"error":"input failed"})),
        ] {
            let answer = page_input_finished(
                human,
                result.clone(),
                |_, _| panic!("no notification should be sent"),
                |_| panic!("no notification failure should be logged"),
            );
            assert_eq!(answer, result);
        }
        let result = page_input_finished(
            true,
            json!({"ok":true}),
            |event, body| {
                assert_eq!(event, PAGE_STATE_EVENT);
                assert_eq!(body, json!({"reason":"human-input"}));
                Ok(())
            },
            |_| panic!("a sent event should not be logged"),
        );
        assert_eq!(result, json!({"ok":true}));
    }

    #[test]
    fn only_main_window_has_application_command_authority() {
        assert!(window_command_allowed("main", "daemon_request"));
        assert!(!window_command_allowed("p1-zeus", "daemon_request"));
        assert!(!window_command_allowed("stranger", "pane_input_enqueue"));
    }

    /// What the page asks while the daemon is down is answered with why,
    /// under the operation it asked, and never reaches a daemon.
    #[test]
    fn a_request_while_the_daemon_is_down_says_why() {
        assert_eq!(
            request_daemon(
                Err("ConsensFlow's daemon stopped while the app was running".to_string()),
                "board.get".to_string(),
                json!({"project":1}),
            ),
            json!({
                "ok":false,
                "error":"not-available-yet",
                "operation":"board.get",
                "detail":"ConsensFlow's daemon stopped while the app was running",
            })
        );
    }

    /// The real app-side bridge must wait past its usual deadline for a
    /// model listing and pass the daemon's diagnosis on to the page.
    #[cfg(unix)]
    #[test]
    fn a_slow_model_listing_reaches_the_page_with_its_diagnosis() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;

        let (host, mut peer) = UnixStream::pair().unwrap();
        peer.write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .unwrap();
        let connected = BridgeBuilder::new(Role::Host, 1024 * 1024)
            .connect(host.try_clone().unwrap(), host)
            .unwrap();
        let daemon = thread::spawn(move || {
            let mut line = String::new();
            BufReader::new(peer.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["op"], "models.get");
            std::thread::sleep(Duration::from_millis(30_500));
            serde_json::to_writer(&mut peer, &json!({
                "v": 1, "id": request["id"], "kind": "res", "op": "models.get",
                "body": {"ok": true, "listing": {"freshness": {"why": "A package or extension of Pi's may be stuck"}}},
            })).unwrap();
            peer.write_all(b"\n").unwrap();
        });
        let answer = request_daemon(
            Ok(connected.bridge),
            "models.get".to_owned(),
            json!({"harness": "pi"}),
        );
        daemon.join().unwrap();
        assert_eq!(answer["ok"], true, "{answer}");
        assert!(answer["listing"]["freshness"]["why"]
            .as_str()
            .unwrap()
            .contains("package or extension"));
    }

    #[test]
    fn agents_screens_open_at_the_daemon_pages_with_the_token() {
        let roster = RosterHandle::from_value(json!({
            "url":"http://127.0.0.1:43123/",
            "token":"secret",
        }))
        .unwrap();
        assert!(
            agents_url(&roster, "").is_err(),
            "no screen of saved agents"
        );
        assert_eq!(
            agents_url(&roster, "harnesses").unwrap().as_str(),
            "http://localhost:43123/harnesses?token=secret"
        );
        assert!(agents_url(&roster, "admin").is_err());
        assert!(agents_url(&roster, "../etc").is_err());
    }

    /// The page refuses a paste over its cap before it sends anything: its
    /// number is the host's.
    #[test]
    fn the_pages_paste_cap_is_the_hosts() {
        let page = include_str!("../../ui/terminal-link.js");
        let declared = format!("const MAX_PASTE_BYTES = {MAX_PAGE_INPUT_BYTES}\n");
        let source_form = format!(
            "const MAX_PASTE_BYTES = {} * 1024 * 1024\n",
            MAX_PAGE_INPUT_BYTES / (1024 * 1024)
        );
        assert!(
            page.contains(&declared) || page.contains(&source_form),
            "terminal-link.js must declare MAX_PASTE_BYTES as the host's cap"
        );
    }

    #[test]
    fn every_tauri_command_that_waits_on_the_daemon_or_a_pty_is_async() {
        let source = include_str!("commands.rs");
        for command in ["pane_input_enqueue", "pane_reply_enqueue"] {
            assert!(
                source.contains(&format!("pub fn {command}")),
                "{command} must admit input synchronously on the IPC thread"
            );
            assert!(
                !source.contains(&format!("pub async fn {command}")),
                "{command} must not be scheduled before input admission"
            );
        }
        // `agents_screen` waits for the daemon's first start, as a request does (T-951).
        for command in [
            "pane_input_wait",
            "pane_resize",
            "pane_ack",
            "agents_screen",
        ] {
            assert!(
                source.contains(&format!("pub async fn {command}")),
                "{command} can wait and must leave Tauri's main thread"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn production_ipc_arrivals_admit_1000_human_writes_in_order() {
        let _pty_guard = cf_panes::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        inputs.begin_page("test-page").unwrap();
        let key = PaneKey::new("ordered-command", 1);
        let mut reader = panes
            .open_at(
                key.clone(),
                Path::new("/tmp"),
                &[
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "/bin/stty raw -echo; printf ready; /usr/bin/od -An -tx1 -N 5000".to_string(),
                ],
                &HashMap::new(),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            )
            .expect("open ordered pane");
        arbiter.register(&key).expect("register ordered pane");
        inputs.open(&key).expect("open the pane's input");
        let mut ready = [0; 5];
        reader
            .read_exact(&mut ready)
            .expect("read readiness marker");
        assert_eq!(&ready, b"ready");
        let output_reader = thread::spawn(move || {
            let mut output = Vec::new();
            reader.read_to_end(&mut output).expect("read ordered bytes");
            output
        });

        let runtime = test_runtime(Arc::clone(&panes), inputs, None, None);
        let app = tauri::test::mock_builder()
            .manage(runtime)
            .invoke_handler(tauri::generate_handler![pane_input_enqueue])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("build mock webview");
        let (responses, received) = mpsc::channel();
        for index in 0..1000 {
            let response_sender = responses.clone();
            webview.clone().on_message(
                ipc_request(
                    "pane_input_enqueue",
                    json!({
                        "id":key.id,
                        "generation":key.generation,
                        "sequence":index + 1,
                        "bytes":format!("{index:04}|").into_bytes(),
                    }),
                ),
                Box::new(move |_webview, _command, response, _callback, _error| {
                    response_sender
                        .send(response)
                        .expect("record command response");
                }),
            );
        }
        drop(responses);
        for response in received.iter().take(1000) {
            let value = match response {
                tauri::ipc::InvokeResponse::Ok(body) => {
                    body.deserialize::<Value>().expect("command response JSON")
                }
                tauri::ipc::InvokeResponse::Err(error) => {
                    panic!("pane_input_enqueue failed: {error:?}")
                }
            };
            assert_eq!(value["ok"], true, "pane_input_enqueue response: {value:?}");
            assert!(
                value["ticket"].is_string(),
                "missing input ticket: {value:?}"
            );
        }

        let output = output_reader.join().expect("join ordered output reader");
        // The page's keys are the human's own: what they leave stays unsent,
        // and holds a paste, until it is sent.
        let counted = std::time::Instant::now() + Duration::from_secs(5);
        while !arbiter.snapshot(&key).expect("snapshot").unsent {
            assert!(
                std::time::Instant::now() < counted,
                "the human's typing never counted"
            );
            thread::sleep(Duration::from_millis(10));
        }
        let actual = String::from_utf8(output)
            .expect("od output is UTF-8")
            .split_whitespace()
            .map(|byte| u8::from_str_radix(byte, 16).expect("hex byte"))
            .collect::<Vec<_>>();
        let expected = (0..1000)
            .flat_map(|index| format!("{index:04}|").into_bytes())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);

        drop(webview);
        drop(app);
    }

    #[cfg(unix)]
    #[test]
    fn production_ipc_refusal_is_final_for_its_sequence() {
        use tauri::Listener;

        let _pty_guard = cf_panes::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        inputs.begin_page("test-page").unwrap();
        let key = PaneKey::new("sequenced-command", 1);
        let mut reader = panes
            .open_at(
                key.clone(),
                Path::new("/tmp"),
                &[
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "/bin/stty raw -echo; printf ready; /usr/bin/od -An -tx1 -N 2".to_string(),
                ],
                &HashMap::new(),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            )
            .expect("open sequenced pane");
        arbiter.register(&key).expect("register sequenced pane");
        inputs.open(&key).expect("open the pane's input");
        let mut ready = [0; 5];
        reader
            .read_exact(&mut ready)
            .expect("read readiness marker");
        assert_eq!(&ready, b"ready");

        let runtime = test_runtime(Arc::clone(&panes), inputs, None, None);
        let app = tauri::test::mock_builder()
            .manage(runtime)
            .invoke_handler(tauri::generate_handler![
                pane_input_enqueue,
                pane_reply_enqueue,
                pane_input_skip,
                pane_input_wait,
                pane_input_forget
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("build mock webview");
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let heard = Arc::clone(&events);
        app.listen_any(PAGE_STATE_EVENT, move |event| {
            heard
                .lock()
                .expect("page events")
                .push(serde_json::from_str::<Value>(event.payload()).expect("page event JSON"));
        });
        let invoke = |command: &str, args: Value| {
            tauri::test::get_ipc_response(&webview, ipc_request(command, args))
                .expect("command succeeds")
                .deserialize::<Value>()
                .expect("command response JSON")
        };

        assert_eq!(
            without_loss(invoke(
                "pane_input_enqueue",
                json!({"id":key.id,"generation":1,"sequence":1,"length":MAX_PAGE_INPUT_BYTES + 1,"bytes":""}),
            )),
            json!({
                "ok":false,
                "error":format!("pane input exceeds {MAX_PAGE_INPUT_BYTES} bytes"),
            })
        );
        // A body of a different length, or one not in base64, is refused whole, and each
        // refusal is final for its number: the next call goes in without waiting for it.
        for (number, (length, bytes, error)) in [
            (2, "Sw==", "pane input bytes do not match their length"),
            (1, "Sw", "pane input bytes do not match their length"),
            (4, "Sw==", "pane input bytes do not match their length"),
            (1, "S!==", "pane input bytes are not base64"),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(
                without_loss(invoke(
                    "pane_input_enqueue",
                    json!({"id":key.id,"generation":1,"sequence":number + 2,"length":length,"bytes":bytes}),
                )),
                json!({"ok":false,"error":error})
            );
        }
        let first = invoke(
            "pane_input_enqueue",
            json!({"id":key.id,"generation":1,"sequence":6,"bytes":[75]}),
        );
        assert_eq!(first["ok"], true);
        // A number at or below the count is stale and writes nothing.
        let repeat = invoke(
            "pane_input_enqueue",
            json!({"id":key.id,"generation":1,"sequence":1,"bytes":[82]}),
        );
        assert_eq!(
            repeat,
            json!({"ok":false,"error":"pane-input-sequence-stale"})
        );
        assert_eq!(
            without_loss(invoke(
                "pane_input_skip",
                json!({"id":key.id,"generation":1,"sequence":7})
            )),
            json!({"ok":true})
        );
        let first_completed = invoke("pane_input_wait", json!({"ticket":first["ticket"]}));
        assert_eq!(first_completed["ok"], true);
        let repeated = invoke("pane_input_wait", json!({"ticket":first["ticket"]}));
        assert_eq!(
            repeated["ok"], true,
            "a lost completion answer is recoverable"
        );
        assert_eq!(
            *events.lock().expect("page events"),
            [json!({"reason":"human-input"})]
        );
        let forgotten = invoke("pane_input_forget", json!({"ticket":first["ticket"]}));
        assert_eq!(forgotten["ok"], true);
        let forgotten_again = invoke("pane_input_forget", json!({"ticket":first["ticket"]}));
        assert_eq!(forgotten_again["ok"], true);
        assert_eq!(
            invoke("pane_input_wait", json!({"ticket":first["ticket"]})),
            json!({"ok":false,"error":"pane-input-ticket-not-found"})
        );
        // A rejected page input spends its sequence, and the next goes on.
        let rejected = invoke(
            "pane_input_enqueue",
            json!({"id":key.id,"generation":1,"sequence":8,"length":MAX_PAGE_INPUT_BYTES + 1,"bytes":""}),
        );
        assert_eq!(rejected["ok"], false);
        let second = invoke(
            "pane_reply_enqueue",
            json!({"id":key.id,"generation":1,"sequence":9,"bytes":[67]}),
        );
        assert_eq!(second["ok"], true);

        let completed = invoke("pane_input_wait", json!({"ticket":second["ticket"]}));
        assert_eq!(completed["ok"], true);
        assert_eq!(events.lock().expect("page events").len(), 1);
        let mut output = Vec::new();
        reader
            .read_to_end(&mut output)
            .expect("read sequenced bytes");
        let actual = output
            .split(|byte| byte.is_ascii_whitespace())
            .filter(|field| !field.is_empty())
            .map(|byte| u8::from_str_radix(std::str::from_utf8(byte).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(actual, b"KC");

        drop(webview);
        drop(app);
    }

    /// The page's notice commands, called as the page calls them (`terminal-link.js`: camelCase
    /// names, numbers as numbers), through Tauri's own argument decoding: a wrong name or type
    /// fails here, not in the real app. Prepare and activate, the window's name, a drop's
    /// place and its release, a skip, the pull and the Dismiss.
    #[cfg(unix)]
    #[test]
    fn the_page_s_notice_commands_decode_as_the_page_calls_them() {
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        let key = PaneKey::new("notice-window", 3);
        inputs.open(&key).expect("open the window's input");
        let runtime = test_runtime(Arc::clone(&panes), Arc::clone(&inputs), None, None);
        let app = tauri::test::mock_builder()
            .manage(runtime)
            .invoke_handler(tauri::generate_handler![
                pane_page_prepare,
                pane_page_activate,
                pane_window_name,
                pane_input_place,
                pane_input_release,
                pane_input_skip,
                pane_lost_pull,
                pane_lost_dismiss
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("build mock webview");
        let invoke = |command: &str, args: Value| {
            tauri::test::get_ipc_response(&webview, ipc_request(command, args))
                .unwrap_or_else(|error| panic!("{command} decodes: {error:?}"))
                .deserialize::<Value>()
                .expect("command response JSON")
        };
        let page = json!({"pageId":"test-page"});
        let prepared = invoke(
            "pane_page_prepare",
            json!({"pageId":"test-page","nonce":"n-1"}),
        );
        assert_eq!(prepared["ok"], true, "{prepared}");
        let epoch = prepared["epoch"].as_u64().expect("the host's epoch");
        let activated = invoke(
            "pane_page_activate",
            json!({"pageId":"test-page","nonce":"n-1","epoch":epoch}),
        );
        assert_eq!(activated["ok"], true, "{activated}");
        assert_eq!(activated["notices"], json!([]));
        let at = |extra: Value| {
            let mut args = json!({"pageId":page["pageId"],"epoch":epoch,"id":key.id,"generation":key.generation});
            for (name, value) in extra.as_object().expect("an object") {
                args[name] = value.clone();
            }
            args
        };
        assert_eq!(
            invoke("pane_window_name", at(json!({"name":"Chief"}))),
            json!({"ok":true})
        );
        assert_eq!(
            invoke("pane_input_place", at(json!({"sequence":1}))),
            json!({"ok":true})
        );
        assert_eq!(
            invoke("pane_input_release", at(json!({"sequence":1}))),
            json!({"ok":true})
        );
        let skipped = invoke("pane_input_skip", at(json!({"sequence":2})));
        assert_eq!(skipped["ok"], true, "{skipped}");
        let pulled = invoke(
            "pane_lost_pull",
            json!({"pageId":"test-page","epoch":epoch}),
        );
        assert_eq!(pulled["ok"], true, "{pulled}");
        let notices = pulled["notices"].as_array().expect("the list");
        assert_eq!(notices.len(), 1, "{pulled}");
        let id = notices[0]["id"].as_u64().expect("an id");
        assert_eq!(
            notices[0],
            json!({"id":id,"kind":"window","window":"notice-window","generation":3,
                   "name":"Chief","closed":false,"failed":false,"dropped":false})
        );
        // A Dismiss of an id the host does not hold clears nothing; its own clears it.
        assert_eq!(
            invoke(
                "pane_lost_dismiss",
                json!({"pageId":"test-page","epoch":epoch,"id":id + 1000})
            ),
            json!({"ok":true,"removed":false})
        );
        assert_eq!(
            invoke(
                "pane_lost_dismiss",
                json!({"pageId":"test-page","epoch":epoch,"id":id})
            ),
            json!({"ok":true,"removed":true})
        );
        let pulled = invoke(
            "pane_lost_pull",
            json!({"pageId":"test-page","epoch":epoch}),
        );
        assert_eq!(pulled["notices"], json!([]));
        inputs.close_and_drain();
    }

    /// One emission of the page's terminal is one call (P4-A): exactly the cap of unbracketed
    /// input is admitted through Tauri's own argument decoding and reaches an owned PTY byte
    /// for byte, and one byte more is refused whole, writing nothing.
    #[cfg(unix)]
    #[test]
    fn production_ipc_admits_the_cap_of_plain_input_whole_and_refuses_one_byte_more() {
        use base64::Engine as _;

        let _pty_guard = cf_panes::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        inputs.begin_page("test-page").unwrap();
        let key = PaneKey::new("plain-cap", 1);
        let folder = tempfile::tempdir().expect("a folder for the program's copy");
        let copy = folder.path().join("received");
        let mut reader = panes
            .open_at(
                key.clone(),
                Path::new("/tmp"),
                &[
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    format!(
                        "/bin/stty raw -echo; printf ready; /usr/bin/head -c {} > '{}'; printf done",
                        MAX_PAGE_INPUT_BYTES,
                        copy.display()
                    ),
                ],
                &HashMap::new(),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            )
            .expect("open the pane");
        arbiter.register(&key).expect("register the pane");
        inputs.open(&key).expect("open the pane's input");
        let mut ready = [0; 5];
        reader
            .read_exact(&mut ready)
            .expect("read readiness marker");
        assert_eq!(&ready, b"ready");

        let runtime = test_runtime(Arc::clone(&panes), inputs, None, None);
        let app = tauri::test::mock_builder()
            .manage(runtime)
            .invoke_handler(tauri::generate_handler![
                pane_input_enqueue,
                pane_input_wait
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("build mock webview");
        let invoke = |command: &str, args: Value| {
            tauri::test::get_ipc_response(&webview, ipc_request(command, args))
                .expect("command succeeds")
                .deserialize::<Value>()
                .expect("command response JSON")
        };
        // Every byte value, none of them a paste marker's run, so that nothing in the bytes
        // is the arbiter's or the terminal's to change.
        let sent = (0..MAX_PAGE_INPUT_BYTES)
            .map(|index| ((index * 7 + index / 251) % 256) as u8)
            .collect::<Vec<u8>>();
        let text = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);

        let mut over = sent.clone();
        over.push(b'x');
        assert_eq!(
            without_loss(invoke(
                "pane_input_enqueue",
                json!({"id":key.id,"generation":1,"sequence":1,"length":over.len(),"bytes":text(&over)}),
            )),
            json!({
                "ok":false,
                "error":format!("pane input exceeds {MAX_PAGE_INPUT_BYTES} bytes"),
            })
        );
        // The refusal spent number 1; the cap's own size goes in at 2.
        let admitted = invoke(
            "pane_input_enqueue",
            json!({"id":key.id,"generation":1,"sequence":2,"length":sent.len(),"bytes":text(&sent)}),
        );
        assert_eq!(
            admitted["ok"], true,
            "the cap's own size is admitted: {admitted:?}"
        );
        let completed = invoke("pane_input_wait", json!({"ticket":admitted["ticket"]}));
        assert_eq!(completed["ok"], true);
        let mut rest = Vec::new();
        reader
            .read_to_end(&mut rest)
            .expect("read until the program ends");
        assert!(rest.ends_with(b"done"), "the program read the whole of it");
        assert!(
            std::fs::read(&copy).expect("the program's copy") == sent,
            "the program read the cap's bytes as sent"
        );

        drop(webview);
        drop(app);
    }

    /// A page reloads while its first call is still on the way (P4-B, by epoch). The program
    /// reads what the host wrote: the page `s:1` made calls 2 and 3 (B, C: number 1 never
    /// came), the host's epoch moved with the navigation, the page `s:2` prepared, echoed its
    /// epoch and typed D at once, and A (number 1 of `s:1`) came after. Returns what the
    /// program read, A's answer, and the windows the host said had lost typing. Every call
    /// goes through Tauri's own decoding.
    #[cfg(unix)]
    fn a_reload_with_a_held_first_call(expected: usize) -> (Vec<u8>, Value, Vec<Value>) {
        use std::sync::Mutex;
        use tauri::Listener as _;

        let _pty_guard = cf_panes::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        inputs.begin_page("s:1").unwrap();
        let key = PaneKey::new("reloaded", 1);
        let folder = tempfile::tempdir().expect("a folder for the program's copy");
        let copy = folder.path().join("received");
        let mut reader = panes
            .open_at(
                key.clone(),
                Path::new("/tmp"),
                &[
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    format!(
                        "/bin/stty raw -echo; printf ready; /usr/bin/head -c {expected} > '{}'; printf done",
                        copy.display()
                    ),
                ],
                &HashMap::new(),
                PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 },
            )
            .expect("open the pane");
        arbiter.register(&key).expect("register the pane");
        inputs.open(&key).expect("open the pane's input");
        let mut ready = [0; 5];
        reader
            .read_exact(&mut ready)
            .expect("read readiness marker");
        assert_eq!(&ready, b"ready");
        let runtime = test_runtime(Arc::clone(&panes), Arc::clone(&inputs), None, None);
        let app = tauri::test::mock_builder()
            .manage(runtime)
            .invoke_handler(tauri::generate_handler![
                pane_input_enqueue,
                pane_input_wait,
                pane_page_prepare,
                pane_page_activate
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        inputs.set_observer(Arc::new(PageInputObserver(app.handle().clone())));
        let notices = Arc::new(Mutex::new(Vec::<Value>::new()));
        let heard = Arc::clone(&notices);
        app.listen(PANE_INPUT_LOST_EVENT, move |event| {
            heard
                .lock()
                .unwrap()
                .push(serde_json::from_str(event.payload()).expect("the event's payload"));
        });
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("build mock webview");
        let invoke = |command: &str, args: Value| {
            tauri::test::get_ipc_response(&webview, ipc_request(command, args))
                .expect("command succeeds")
                .deserialize::<Value>()
                .expect("command response JSON")
        };
        let call = |page: &str, epoch: u64, sequence: u64, bytes: &[u8]| {
            invoke(
                "pane_input_enqueue",
                json!({"pageId":page,"epoch":epoch,"id":key.id,"generation":1,"sequence":sequence,"bytes":bytes}),
            )
        };
        assert_eq!(call("s:1", 0, 2, b"B")["ok"], true);
        assert_eq!(call("s:1", 0, 3, b"C")["ok"], true);
        // Navigation begins; the next page announces itself with a nonce of its own, is
        // answered the host's epoch for it, echoes that pair, and types at once.
        inputs.end_page().unwrap();
        let prepared = invoke("pane_page_prepare", json!({"pageId":"s:2","nonce":"n-2"}));
        assert_eq!(prepared["ok"], true, "{prepared:?}");
        assert_eq!(prepared["nonce"], "n-2");
        let epoch = prepared["epoch"].as_u64().expect("the host's epoch");
        assert_eq!(epoch, 1, "one navigation, one epoch");
        let activated = invoke(
            "pane_page_activate",
            json!({"pageId":"s:2","nonce":"n-2","epoch":epoch}),
        );
        assert_eq!(activated["ok"], true, "{activated:?}");
        assert_eq!(call("s:2", epoch, 1, b"D")["ok"], true);
        let a = call("s:1", 0, 1, b"A");
        // The program ends when it has read what it should have. One that never does is a
        // failure, not a hang.
        let (ended, ending) = mpsc::channel();
        thread::spawn(move || {
            let mut rest = Vec::new();
            let _ = reader.read_to_end(&mut rest);
            let _ = ended.send(rest);
        });
        let rest = ending.recv_timeout(Duration::from_secs(20));
        let received = std::fs::read(&copy).unwrap_or_default();
        assert!(
            rest.as_ref().is_ok_and(|rest| rest.ends_with(b"done")),
            "the program did not read all of what the host should have written: {received:?}"
        );
        let heard = notices.lock().unwrap().clone();
        drop(webview);
        drop(app);
        (received, a, heard)
    }

    /// The reload, through the commands: number 1 never came, B and C waited behind it and are
    /// cancelled as a loss when the page reloads, D of the new page is written at once, and A
    /// comes after the reload and is refused. The program reads D alone, and the page that is
    /// current is told which window.
    #[cfg(unix)]
    #[test]
    fn a_reload_cancels_what_waited_in_order_and_the_current_page_is_told() {
        let (received, a, notices) = a_reload_with_a_held_first_call(1);
        assert_eq!(received, b"D");
        assert_eq!(a["ok"], false, "A is refused: {a:?}");
        assert_eq!(a["error"], "pane-input-stale-page");
        assert!(
            !notices.is_empty()
                && notices
                    .iter()
                    .all(|each| each["id"] == "reloaded" && each["generation"] == 1),
            "{notices:?}"
        );
    }

    /// The page's terminal answers what the program asked of it (where its cursor is) past
    /// the human's keys held after their paste (T-1048): Codex on Windows asks as a paste comes
    /// and reads none of it until it has the answer, and the page's answer waited behind the
    /// human's held Enter. Here the program reads the paste and says so by a file (the Enter is
    /// held behind it), reads the answer, and draws only then: the answer sent once the paste
    /// and its Enter were admitted and the program has the paste is read before the Enter, or
    /// the program never draws and the Enter waits for its cap. No time of the machine's is in
    /// it: the events are the program's, and the cap is a minute, which only an answer held
    /// behind the Enter runs into.
    #[cfg(unix)]
    #[test]
    fn the_pages_reply_goes_past_the_humans_keys_held_after_their_paste() {
        let _pty_guard = cf_panes::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        inputs.begin_page("test-page").unwrap();
        let key = PaneKey::new("replied-command", 1);
        let paste: &[u8] = b"\x1b[200~ab\x1b[201~";
        let marker_folder = tempfile::tempdir().expect("a folder for the program's marker");
        let marker = marker_folder.path().join("pasted");
        // Raw and silent; it reads the paste and makes the marker, reads the answer's byte,
        // draws, reads the Enter, and says the hex of each.
        let script = format!(
            "/bin/stty raw -echo; printf ready; \
             p=$(dd bs=1 count={} 2>/dev/null | od -An -tx1); : > '{}'; \
             a=$(dd bs=1 count=1 2>/dev/null | od -An -tx1); printf drawn; \
             e=$(dd bs=1 count=1 2>/dev/null | od -An -tx1); echo \"$p $a $e\"; sleep 600",
            paste.len(),
            marker.display()
        );
        let mut reader = panes
            .open_at(
                key.clone(),
                Path::new("/tmp"),
                &["/bin/sh".to_string(), "-c".to_string(), script],
                &HashMap::new(),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            )
            .expect("open the replied pane");
        // Held until the window draws the paste, which the program does once it has read the
        // answer: the cap is a minute, past anything the few steps here take on a loaded
        // machine, and only an answer held behind the Enter waits for it.
        let silent = cf_proto::panes::SilentPaste {
            pace_us_per_character: 0,
            drawn_after_ms: 0,
            quiet_ms: 100,
            most_ms: 60_000,
            most_us_per_character: 0,
        };
        let output = arbiter
            .register_with(&key, Some(silent))
            .expect("register the replied pane");
        inputs.open(&key).expect("open the pane's input");
        let mut ready = [0; 5];
        reader
            .read_exact(&mut ready)
            .expect("read readiness marker");
        assert_eq!(&ready, b"ready");
        // What the pane prints is noted as the pane host's output thread notes it: the
        // program's "drawn" is the window's draw of the paste.
        let recorded = Arc::new(std::sync::Mutex::new(String::new()));
        let printing = {
            let recorded = Arc::clone(&recorded);
            thread::spawn(move || {
                let mut chunk = [0_u8; 256];
                while let Ok(read) = reader.read(&mut chunk) {
                    if read == 0 {
                        break;
                    }
                    output.note_output(true);
                    recorded
                        .lock()
                        .expect("what the program printed")
                        .push_str(&String::from_utf8_lossy(&chunk[..read]));
                }
            })
        };

        let runtime = test_runtime(Arc::clone(&panes), inputs, None, None);
        let app = tauri::test::mock_builder()
            .manage(runtime)
            .invoke_handler(tauri::generate_handler![
                pane_input_enqueue,
                pane_reply_enqueue,
                pane_input_wait
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("build mock webview");
        let invoke = |command: &str, args: Value| {
            tauri::test::get_ipc_response(&webview, ipc_request(command, args))
                .expect("command succeeds")
                .deserialize::<Value>()
                .expect("command response JSON")
        };

        let pasted = [paste, b"\r"].concat();
        let typed = invoke(
            "pane_input_enqueue",
            json!({"id":key.id,"generation":1,"sequence":1,"bytes":pasted}),
        );
        assert_eq!(typed["ok"], true, "{typed}");
        // The paste is in the program: the human's Enter is held behind it.
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while !marker.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "the program never read the paste"
            );
            thread::sleep(Duration::from_millis(5));
        }
        let replied = invoke(
            "pane_reply_enqueue",
            json!({"id":key.id,"generation":1,"sequence":2,"bytes":[82]}),
        );
        assert_eq!(replied["ok"], true, "{replied}");
        // Answered without the draw, which comes only once the program has read the answer:
        // an answer behind the held Enter is answered at the cap, a minute on.
        let answered = invoke("pane_input_wait", json!({"ticket":replied["ticket"]}));
        assert_eq!(answered["ok"], true, "{answered}");
        let sent = invoke("pane_input_wait", json!({"ticket":typed["ticket"]}));
        assert_eq!(sent["ok"], true, "{sent}");

        // The program says what it read, in the order it read it.
        let record = format!(
            "drawn{}520d",
            paste
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            let said: String = recorded
                .lock()
                .expect("what the program printed")
                .split_whitespace()
                .collect();
            if said.contains(&record) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the program's record is not the paste, the answer and then the Enter: {said}"
            );
            thread::sleep(Duration::from_millis(5));
        }
        panes.kill(&key).expect("kill the replied pane");
        printing.join().expect("the pane's output thread");

        drop(webview);
        drop(app);
    }

    /// A reloaded page counts every pane's input from 1 again: its
    /// subscription starts the count anew, and what the page before it was
    /// still owed answers goes with it.
    #[cfg(unix)]
    #[test]
    fn a_reloaded_page_types_into_the_panes_it_finds() {
        let _pty_guard = cf_panes::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        let key = PaneKey::new("reloaded-page", 1);
        let mut reader = panes
            .open_at(
                key.clone(),
                Path::new("/tmp"),
                &[
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "/bin/stty raw -echo; printf ready; /usr/bin/od -An -tx1 -N 3".to_string(),
                ],
                &HashMap::new(),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            )
            .expect("open the pane");
        arbiter.register(&key).expect("register the pane");
        inputs.open(&key).expect("open the pane's input");
        let mut ready = [0; 5];
        reader
            .read_exact(&mut ready)
            .expect("read readiness marker");
        assert_eq!(&ready, b"ready");

        let runtime = test_runtime(Arc::clone(&panes), inputs, None, None);
        let app = tauri::test::mock_builder()
            .manage(runtime)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let handle = app.handle().clone();
        let subscribe = |page: &str| {
            let prepared = pane_page_prepare(handle.clone(), page.to_string(), "nonce".into());
            assert_eq!(prepared["ok"], true);
            assert_eq!(
                pane_page_activate(
                    handle.clone(),
                    page.to_string(),
                    "nonce".into(),
                    prepared["epoch"].as_u64().expect("an epoch")
                )["ok"],
                true
            );
            let subscribed = tauri::async_runtime::block_on(subscribe_output(
                handle.clone(),
                page.to_string(),
                Channel::new(|_| Ok(())),
            ));
            assert_eq!(subscribed["ok"], true);
        };
        let wait = |page: &str, ticket: &Value| {
            tauri::async_runtime::block_on(pane_input_wait(
                handle.clone(),
                page.to_string(),
                ticket.as_str().expect("a ticket").to_string(),
            ))
        };

        subscribe("old");
        let first = page_input(&handle, "old", &key, 1, b"A");
        assert_eq!(wait("old", &first["ticket"]), json!({"ok":true}));
        // The page goes away with this one admitted and never waited for.
        let orphaned = page_input(&handle, "old", &key, 2, b"B");
        assert_eq!(orphaned["ok"], true);

        subscribe("new");
        let typed = page_input(&handle, "new", &key, 1, b"C");
        assert_eq!(typed["ok"], true, "the new page's first keystroke: {typed}");
        assert_eq!(wait("new", &typed["ticket"]), json!({"ok":true}));
        assert_eq!(
            wait("new", &orphaned["ticket"]),
            json!({"ok":false,"error":"pane-input-ticket-not-found"}),
            "the old page's tickets went with it"
        );

        let mut output = Vec::new();
        reader
            .read_to_end(&mut output)
            .expect("read what reached the pane");
        assert_eq!(
            String::from_utf8(output)
                .expect("od output is UTF-8")
                .split_whitespace()
                .collect::<String>(),
            "414243",
            "every admitted keystroke reached the pane, in order"
        );
        drop(app);
    }

    /// A reloaded page finds every pane still printing. A pane stops reading
    /// once a window of its output is unacknowledged, and what the page
    /// before was handed and never drew is never acknowledged: a pane that had
    /// filled its window printed nothing more, its harness waiting on its
    /// terminal for good.
    #[cfg(unix)]
    #[test]
    fn a_reloaded_page_finds_every_pane_printing() {
        const BACKLOG_BYTES: usize = 1024;
        let _pty_guard = cf_panes::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        panes.keep_journals();
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), arbiter));
        let streamed = panes
            .open_streamed_at(
                PaneKey::new("p1-zeus", 1),
                Path::new("/tmp"),
                &["/usr/bin/yes".to_string()],
                PaneEnvironment::new(&HashMap::new(), &[]),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
                BACKLOG_BYTES,
            )
            .expect("open a pane that never stops printing");
        let runtime = test_runtime(Arc::clone(&panes), inputs, None, None);
        let app = tauri::test::mock_builder()
            .manage(runtime)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let hub = Arc::clone(&app.state::<AppRuntime>().output);
        // The app's own consumer of the window's output.
        let journal = streamed.journal.clone().expect("a journal");
        let output = streamed.output;
        let consumer = thread::spawn(move || {
            cf_panes::pane_handlers::forward_journaled(&output, &journal, &hub, |_| {});
        });
        // A page subscribes and is handed a window's worth; it draws none of it.
        let page = |page: &str| {
            let (sender, taken) = mpsc::channel();
            let prepared = pane_page_prepare(app.handle().clone(), page.into(), "nonce".into());
            assert_eq!(prepared["ok"], true);
            assert_eq!(
                pane_page_activate(
                    app.handle().clone(),
                    page.into(),
                    "nonce".into(),
                    prepared["epoch"].as_u64().expect("an epoch")
                )["ok"],
                true
            );
            let subscribed = tauri::async_runtime::block_on(subscribe_output(
                app.handle().clone(),
                page.to_string(),
                Channel::new(move |_| {
                    let _ = sender.send(());
                    Ok(())
                }),
            ));
            assert_eq!(subscribed["ok"], true);
            taken
        };
        let old = page("old-page");
        old.recv_timeout(Duration::from_secs(5))
            .expect("output up to the window");
        // What more there is of the window is taken until it waits on the old page.
        while old.recv_timeout(Duration::from_millis(300)).is_ok() {}
        let new = page("new-page");
        new.recv_timeout(Duration::from_secs(5))
            .expect("the pane prints again for the new page");

        panes.kill(&streamed.key).expect("kill the pane");
        consumer.join().expect("the consumer");
        drop(app);
    }

    #[cfg(unix)]
    #[test]
    fn blocked_command_input_does_not_starve_another_pane_or_output_ack() {
        let _pty_guard = cf_panes::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        inputs.begin_page("test-page").unwrap();
        let size = PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let blocked_key = PaneKey::new("blocked-command", 1);
        let mut blocked_reader = panes
            .open_at(
                blocked_key.clone(),
                Path::new("/tmp"),
                &[
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "/bin/stty raw -echo; /bin/sleep 4; /bin/cat >/dev/null".to_string(),
                ],
                &HashMap::new(),
                size,
            )
            .expect("open blocked pane");
        // Like the production output pump, drain echoed startup bytes. Keeping an
        // unread PTY master open can block macOS child exit even after SIGKILL.
        let blocked_output = thread::spawn(move || {
            let _ = std::io::copy(&mut blocked_reader, &mut std::io::sink());
        });
        arbiter
            .register(&blocked_key)
            .expect("register blocked pane");
        inputs
            .open(&blocked_key)
            .expect("open the blocked pane's input");

        let responsive_key = PaneKey::new("responsive-command", 1);
        let responsive = panes
            .open_streamed_at(
                responsive_key.clone(),
                Path::new("/tmp"),
                &[
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "printf ready; sleep 30".to_string(),
                ],
                PaneEnvironment::new(&HashMap::new(), &[]),
                size,
                1024,
            )
            .expect("open responsive pane");
        arbiter
            .register(&responsive_key)
            .expect("register responsive pane");
        inputs
            .open(&responsive_key)
            .expect("open the responsive pane's input");
        let first_output = responsive
            .output
            .recv_timeout(Duration::from_secs(2))
            .expect("responsive pane output");

        let runtime = test_runtime(Arc::clone(&panes), Arc::clone(&inputs), None, None);
        let app = tauri::test::mock_builder()
            .manage(runtime)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let handle = app.handle().clone();

        let blocked_outcomes = (0..512)
            .map(|index| {
                page_input(
                    &handle,
                    "test-page",
                    &blocked_key,
                    index + 1,
                    &vec![b'x'; MAX_INPUT_BYTES],
                )
            })
            .collect::<Vec<_>>();
        thread::sleep(Duration::from_millis(500));

        let (responsive_sender, responsive_receiver) = mpsc::channel();
        let responsive_handle = handle.clone();
        let responsive_admission = page_input(&handle, "test-page", &responsive_key, 1, b"R");
        assert_eq!(responsive_admission["ok"], true);
        let responsive_ticket = responsive_admission["ticket"]
            .as_str()
            .expect("responsive input ticket")
            .to_string();
        let responsive_thread = thread::spawn(move || {
            let value = tauri::async_runtime::block_on(pane_input_wait(
                responsive_handle,
                "test-page".into(),
                responsive_ticket,
            ));
            responsive_sender
                .send(value)
                .expect("record responsive input result");
        });

        let (ack_sender, ack_receiver) = mpsc::channel();
        let ack_handle = handle;
        let ack_id = responsive_key.id.clone();
        let ack_generation = responsive_key.generation;
        let ack_thread = thread::spawn(move || {
            let value = tauri::async_runtime::block_on(pane_ack(
                ack_handle,
                ack_id,
                ack_generation,
                first_output.seq,
            ));
            ack_sender.send(value).expect("record ack result");
        });

        let responsive_before_cleanup = responsive_receiver
            .recv_timeout(Duration::from_secs(1))
            .ok();
        let ack_before_cleanup = ack_receiver.recv_timeout(Duration::from_secs(1)).ok();

        inputs.close_and_drain();
        let responsive_result = responsive_before_cleanup.clone().or_else(|| {
            responsive_receiver
                .recv_timeout(Duration::from_secs(5))
                .ok()
        });
        let ack_result = ack_before_cleanup
            .clone()
            .or_else(|| ack_receiver.recv_timeout(Duration::from_secs(5)).ok());
        responsive_thread.join().expect("responsive input thread");
        ack_thread.join().expect("ack thread");
        panes.kill(&blocked_key).expect("kill blocked pane");
        panes.kill(&responsive_key).expect("kill responsive pane");
        blocked_output.join().expect("drain blocked pane output");
        drop(app);

        assert_eq!(
            responsive_before_cleanup,
            Some(json!({"ok":true})),
            "one blocked pane consumed the shared blocking pool"
        );
        assert_eq!(
            ack_before_cleanup,
            Some(json!({"ok":true})),
            "output acks shared the blocked PTY pool"
        );
        assert_eq!(responsive_result, Some(json!({"ok":true})));
        assert_eq!(ack_result, Some(json!({"ok":true})));
        assert!(
            blocked_outcomes
                .iter()
                .any(|value| value["error"] == "the pane's input queue is full"),
            "a production-sized blocked burst must hit the bounded pane queue"
        );
    }

    #[cfg(unix)]
    #[test]
    fn tauri_commands_send_contract_operations_and_bodies_over_the_live_bridge() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;

        let routes = vec![
            (
                "daemon_request",
                json!({"operation":"board.get","body":{"project":1}}),
                "board.get",
                json!({"project":1}),
            ),
            (
                "daemon_request",
                json!({"operation":"task.cancel","body":{"project":1,"task":1}}),
                "task.cancel",
                json!({"project":1,"task":1}),
            ),
        ];

        let (rust_stream, mut daemon_stream) = UnixStream::pair().expect("bridge socket pair");
        daemon_stream
            .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("write bridge handle");
        daemon_stream.flush().expect("flush bridge handle");
        let connected = BridgeBuilder::new(Role::Host, 1024 * 1024)
            .connect(
                rust_stream.try_clone().expect("clone bridge socket"),
                rust_stream,
            )
            .expect("connect bridge");

        let expected = routes
            .iter()
            .map(|(_, _, operation, body)| ((*operation).to_string(), body.clone()))
            .collect::<Vec<_>>();
        let daemon = thread::spawn(move || {
            let mut reader =
                BufReader::new(daemon_stream.try_clone().expect("clone daemon reader"));
            for (operation, body) in expected {
                let mut line = String::new();
                reader.read_line(&mut line).expect("read command request");
                let frame: Value = serde_json::from_str(line.trim()).expect("command frame JSON");
                assert_eq!(frame["kind"], "req");
                assert_eq!(frame["op"], operation);
                assert_eq!(frame["body"], body);
                serde_json::to_writer(
                    &mut daemon_stream,
                    &json!({
                        "v":1,
                        "id":frame["id"],
                        "kind":"res",
                        "op":operation,
                        "body":{"ok":true},
                    }),
                )
                .expect("write command response");
                daemon_stream.write_all(b"\n").expect("terminate response");
                daemon_stream.flush().expect("flush command response");
            }
        });

        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), arbiter));
        let runtime = test_runtime(panes, inputs, None, Some(connected.bridge));
        let app = tauri::test::mock_builder()
            .manage(runtime)
            .invoke_handler(tauri::generate_handler![daemon_request])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("build mock webview");

        // These must be refused before requesting the bridge: the peer expects only valid routes.
        for (command, args) in [
            (
                "daemon_request",
                json!({"operation":"state.list","body":{}}),
            ),
            (
                "daemon_request",
                json!({"operation":"board.get","body":[1]}),
            ),
        ] {
            let response = tauri::test::get_ipc_response(
                &webview,
                tauri::webview::InvokeRequest {
                    cmd: command.into(),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: "tauri://localhost".parse().expect("invoke URL"),
                    body: tauri::ipc::InvokeBody::Json(args),
                    headers: Default::default(),
                    invoke_key: tauri::test::INVOKE_KEY.to_string(),
                },
            )
            .expect("validation response")
            .deserialize::<Value>()
            .expect("JSON");
            assert_eq!(
                response["ok"], false,
                "invalid {command} accepted: {response}"
            );
        }
        for (command, args, _, _) in routes {
            let response = tauri::test::get_ipc_response(
                &webview,
                tauri::webview::InvokeRequest {
                    cmd: command.into(),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: "tauri://localhost".parse().expect("invoke URL"),
                    body: tauri::ipc::InvokeBody::Json(args),
                    headers: Default::default(),
                    invoke_key: tauri::test::INVOKE_KEY.to_string(),
                },
            )
            .expect("command succeeds")
            .deserialize::<Value>()
            .expect("command response JSON");
            assert_eq!(response, json!({"ok":true}), "{command}");
        }

        daemon.join().expect("daemon peer");
        drop(webview);
        drop(app);
    }
}

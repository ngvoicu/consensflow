//! What the board page may ask of the app: the core's operations it forwards,
//! its panes' input, size and acknowledgements, its output subscription, and
//! the address of the agents screens.

use std::sync::Arc;

use serde_json::{json, Value};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::bridge::Bridge;
use crate::daemon::RosterHandle;
use crate::input_queue::{wait_for_input, InputWork, PageInputCompletion};
use crate::output_hub::PaneOutputMessage;
use crate::runtime::{AppRuntime, PAGE_STATE_EVENT};
use crate::validation::{pane_key, validate_seq, validate_size, validate_text};

pub(crate) fn window_command_allowed(window: &str, _command: &str) -> bool {
    window == "main"
}

fn request_node(connection: Result<Bridge, String>, operation: String, body: Value) -> Value {
    let bridge = match connection {
        Ok(bridge) => bridge,
        Err(cause) => return not_available(&operation, &cause),
    };
    match bridge.request(operation.clone(), body, None) {
        Ok(response) => normalize_node_response(&operation, response),
        Err(error) => json!({"ok":false,"error":error.to_string(),"operation":operation}),
    }
}

fn normalize_node_response(operation: &str, response: Value) -> Value {
    let unavailable = response
        .get("error")
        .and_then(Value::as_str)
        .is_some_and(|error| error == "unknown-op");
    if unavailable {
        not_available(operation, "the Node handler has not landed yet")
    } else {
        response
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
    match wait_for_input(completion.receiver).await {
        Ok(()) => json!({"ok":true}),
        Err(error) => json!({"ok":false,"error":error.to_string()}),
    }
}

fn enqueue_page_input<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    generation: u64,
    sequence: u64,
    work: InputWork,
    human: bool,
) -> Value {
    let key = match pane_key(&id, generation) {
        Ok(key) => key,
        Err(error) => return json!({"ok":false,"error":error}),
    };
    let inputs = {
        let state = app.state::<AppRuntime>();
        Arc::clone(&state.inputs)
    };
    match inputs.enqueue_page(key, sequence, work, human) {
        Ok(ticket) => json!({"ok":true,"ticket":ticket}),
        Err(error) => json!({"ok":false,"error":error}),
    }
}

#[tauri::command]
pub fn pane_input_enqueue<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    generation: u64,
    sequence: u64,
    bytes: Vec<u8>,
) -> Value {
    enqueue_page_input(app, id, generation, sequence, InputWork::Write(bytes), true)
}

#[tauri::command]
pub fn pane_reply_enqueue<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    generation: u64,
    sequence: u64,
    bytes: Vec<u8>,
) -> Value {
    enqueue_page_input(
        app,
        id,
        generation,
        sequence,
        InputWork::Write(bytes),
        false,
    )
}

#[tauri::command]
pub async fn pane_input_wait<R: Runtime>(app: AppHandle<R>, ticket: String) -> Value {
    if let Err(error) = validate_text(&ticket, "pane input ticket") {
        return json!({"ok":false,"error":error});
    }
    let inputs = {
        let state = app.state::<AppRuntime>();
        Arc::clone(&state.inputs)
    };
    let completion = inputs.take_page_completion(&ticket);
    let human = completion.as_ref().is_ok_and(|completion| completion.human);
    let result = input_result(completion).await;
    if human && result["ok"] == true {
        let _ = app.emit(PAGE_STATE_EVENT, json!({"reason":"human-input"}));
    }
    result
}

#[tauri::command]
pub async fn pane_resize<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    generation: u64,
    cols: u16,
    rows: u16,
) -> Value {
    if let Err(error) = validate_size(cols, rows) {
        return json!({"ok":false,"error":error});
    }
    let key = match pane_key(&id, generation) {
        Ok(key) => key,
        Err(error) => return json!({"ok":false,"error":error}),
    };
    let panes = {
        let state = app.state::<AppRuntime>();
        Arc::clone(&state.panes)
    };
    run_blocking("pane_resize", move || {
        match panes.resize(&key, rows, cols) {
            Ok(()) => json!({"ok":true}),
            Err(error) => json!({"ok":false,"error":error.to_string()}),
        }
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
    if let Err(error) = validate_seq(seq) {
        return json!({"ok":false,"error":error});
    }
    let key = match pane_key(&id, generation) {
        Ok(key) => key,
        Err(error) => return json!({"ok":false,"error":error}),
    };
    let panes = {
        let state = app.state::<AppRuntime>();
        Arc::clone(&state.panes)
    };
    match panes.ack(&key, seq) {
        Ok(()) => json!({"ok":true}),
        Err(error) => json!({"ok":false,"error":error.to_string()}),
    }
}

async fn task_operation<R: Runtime>(
    app: AppHandle<R>,
    operation: &'static str,
    body: Value,
) -> Value {
    let core = Arc::clone(&app.state::<AppRuntime>().core);
    // Off the async runtime: asked while the core is starting, it waits.
    run_blocking(operation, move || {
        request_node(core.connection(), operation.to_string(), body)
    })
    .await
}

/// What the board page may ask the new core. The page names the operation and
/// its body; anything else is refused here, before it reaches the daemon.
const CORE_OPERATIONS: &[&str] = &[
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
    "session.open",
    "session.close",
    "session.end",
    "task.get",
    "task.transcript",
    "task.cancel",
    "task.pause",
    "task.reassign",
    "task.resume",
    "tasks.delete",
    "message.read",
    "message.approve",
    "message.decline",
    "agents.list",
    "staff.last",
];

#[tauri::command]
pub async fn core_request<R: Runtime>(app: AppHandle<R>, operation: String, body: Value) -> Value {
    let Some(operation) = CORE_OPERATIONS
        .iter()
        .find(|allowed| **allowed == operation)
    else {
        return json!({"ok":false,"error":format!("unknown core operation {operation}")});
    };
    if !body.is_object() {
        return json!({"ok":false,"error":"a core request body is an object"});
    }
    task_operation(app, operation, body).await
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
/// Once per page is also what makes it the start of the page's input count.
/// The page numbers its input in its own memory, so a reload (WebKit's content
/// process replaced, or the human's Reload) starts again from 1; every pane's
/// next keystrokes were refused as a regression until the app restarted.
///
/// And it is when a new page hears where the core stands: what was told
/// before it listened (a first start that failed while the page loaded) is
/// told again, so the page listens to `core-status` before it subscribes.
#[tauri::command]
pub async fn subscribe_output<R: Runtime>(
    app: AppHandle<R>,
    on_output: Channel<PaneOutputMessage>,
) -> Value {
    let (output, inputs, core) = {
        let state = app.state::<AppRuntime>();
        (
            Arc::clone(&state.output),
            Arc::clone(&state.inputs),
            Arc::clone(&state.core),
        )
    };
    inputs.begin_page();
    output.register(on_output);
    core.tell_again();
    json!({"ok":true})
}

/// The address of one agents screen (the agents, the harnesses): the
/// daemon's page with the UI token, which the board's page frames in a
/// dialog of its own.
#[tauri::command]
pub fn agents_screen<R: Runtime>(app: AppHandle<R>, page: String) -> Value {
    let Some(roster) = app.state::<AppRuntime>().core.roster() else {
        return json!({"ok":false,"error":"the agents screens are not available: the daemon is not up"});
    };
    match agents_url(&roster, &page) {
        Ok(url) => json!({"ok":true,"url":url.as_str()}),
        Err(error) => json!({"ok":false,"error":error}),
    }
}

const AGENTS_PAGES: &[&str] = &["", "harnesses"];

/// The daemon's page for one agents screen, carrying the UI token.
fn agents_url(roster: &RosterHandle, page: &str) -> Result<tauri::Url, String> {
    if !AGENTS_PAGES.contains(&page) {
        return Err(format!("no agents screen {page:?}"));
    }
    let mut url = tauri::Url::parse(&roster.url)
        .map_err(|error| format!("the editor handle is not an address: {error}"))?;
    url.set_path(&format!("/{page}"));
    url.query_pairs_mut()
        .clear()
        .append_pair("token", &roster.token);
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
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
    use crate::arbiter::InputArbiter;
    #[cfg(unix)]
    use crate::bridge::BridgeBuilder;
    #[cfg(unix)]
    use crate::input_queue::InputQueue;
    #[cfg(unix)]
    use crate::pty::{PaneEnvironment, PaneKey, PaneTable};
    #[cfg(unix)]
    use crate::runtime::test_runtime;
    #[cfg(unix)]
    use crate::validation::MAX_INPUT_BYTES;

    #[test]
    fn only_main_window_has_application_command_authority() {
        assert!(window_command_allowed("main", "open_pm"));
        assert!(!window_command_allowed("pm-t-2", "open_pm"));
        assert!(!window_command_allowed("stranger", "pane_input_enqueue"));
    }

    #[test]
    fn unknown_node_operations_are_explicitly_not_available() {
        assert_eq!(
            normalize_node_response("answers.list", json!({"ok":false,"error":"unknown-op"})),
            json!({
                "ok":false,
                "error":"not-available-yet",
                "operation":"answers.list",
                "detail":"the Node handler has not landed yet",
            })
        );
    }

    /// What the page asks while the core is down is answered with why,
    /// under the operation it asked, and never reaches a daemon.
    #[test]
    fn a_request_while_the_core_is_down_says_why() {
        assert_eq!(
            request_node(
                Err("ConsensFlow's core stopped while the app was running".to_string()),
                "board.get".to_string(),
                json!({"project":1}),
            ),
            json!({
                "ok":false,
                "error":"not-available-yet",
                "operation":"board.get",
                "detail":"ConsensFlow's core stopped while the app was running",
            })
        );
    }

    #[test]
    fn agents_screens_open_at_the_daemon_pages_with_the_token() {
        let roster = RosterHandle::from_value(json!({
            "url":"http://127.0.0.1:43123/",
            "token":"secret",
        }))
        .unwrap();
        assert_eq!(
            agents_url(&roster, "").unwrap().as_str(),
            "http://localhost:43123/?token=secret"
        );
        assert_eq!(
            agents_url(&roster, "harnesses").unwrap().as_str(),
            "http://localhost:43123/harnesses?token=secret"
        );
        assert!(agents_url(&roster, "admin").is_err());
        assert!(agents_url(&roster, "../etc").is_err());
    }

    #[test]
    fn every_tauri_command_that_waits_on_node_or_a_pty_is_async() {
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
        for command in ["pane_input_wait", "pane_resize", "pane_ack"] {
            assert!(
                source.contains(&format!("pub async fn {command}")),
                "{command} can wait and must leave Tauri's main thread"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn production_ipc_arrivals_admit_1000_human_writes_in_order() {
        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(0));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
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
                tauri::webview::InvokeRequest {
                    cmd: "pane_input_enqueue".into(),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: "tauri://localhost".parse().expect("invoke URL"),
                    body: tauri::ipc::InvokeBody::Json(json!({
                        "id":key.id,
                        "generation":key.generation,
                        "sequence":index + 1,
                        "bytes":format!("{index:04}|").into_bytes(),
                    })),
                    headers: Default::default(),
                    invoke_key: tauri::test::INVOKE_KEY.to_string(),
                },
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
    fn production_ipc_consumes_sequence_before_size_refusal() {
        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(0));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
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
                pane_input_wait
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("build mock webview");
        let invoke = |command: &str, args: Value| {
            tauri::test::get_ipc_response(
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
            .expect("command response JSON")
        };

        let oversized = invoke(
            "pane_input_enqueue",
            json!({
                "id":key.id,
                "generation":1,
                "sequence":1,
                "bytes":vec![b'P'; MAX_INPUT_BYTES + 1],
            }),
        );
        assert_eq!(
            oversized,
            json!({
                "ok":false,
                "error":format!("pane input exceeds {MAX_INPUT_BYTES} bytes"),
            })
        );
        let first = invoke(
            "pane_input_enqueue",
            json!({"id":key.id,"generation":1,"sequence":2,"bytes":[75]}),
        );
        assert_eq!(first["ok"], true);
        let gap = invoke(
            "pane_input_enqueue",
            json!({"id":key.id,"generation":1,"sequence":4,"bytes":[66]}),
        );
        assert_eq!(gap, json!({"ok":false,"error":"pane-input-sequence-gap"}));
        let regression = invoke(
            "pane_input_enqueue",
            json!({"id":key.id,"generation":1,"sequence":2,"bytes":[82]}),
        );
        assert_eq!(
            regression,
            json!({"ok":false,"error":"pane-input-sequence-regression"})
        );
        let first_completed = invoke("pane_input_wait", json!({"ticket":first["ticket"]}));
        assert_eq!(first_completed["ok"], true);
        // A rejected page input still consumes its sequence, so the next
        // sequence remains valid.
        let rejected = invoke(
            "pane_input_enqueue",
            json!({
                "id":key.id,"generation":1,"sequence":3,"bytes":vec![b'P'; MAX_INPUT_BYTES + 1],
            }),
        );
        assert_eq!(rejected["ok"], false);
        let second = invoke(
            "pane_reply_enqueue",
            json!({"id":key.id,"generation":1,"sequence":4,"bytes":[67]}),
        );
        assert_eq!(second["ok"], true);

        let completed = invoke("pane_input_wait", json!({"ticket":second["ticket"]}));
        assert_eq!(completed["ok"], true);
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

    /// A reloaded page counts every pane's input from 1 again: its
    /// subscription starts the count anew, and what the page before it was
    /// still owed answers goes with it.
    #[cfg(unix)]
    #[test]
    fn a_reloaded_page_types_into_the_panes_it_finds() {
        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(0));
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
        let subscribe = || {
            let subscribed = tauri::async_runtime::block_on(subscribe_output(
                handle.clone(),
                Channel::new(|_| Ok(())),
            ));
            assert_eq!(subscribed["ok"], true);
        };
        let wait = |ticket: &Value| {
            tauri::async_runtime::block_on(pane_input_wait(
                handle.clone(),
                ticket.as_str().expect("a ticket").to_string(),
            ))
        };

        subscribe();
        let first = pane_input_enqueue(handle.clone(), key.id.clone(), 1, 1, b"A".to_vec());
        assert_eq!(wait(&first["ticket"]), json!({"ok":true}));
        // The page goes away with this one admitted and never waited for.
        let orphaned = pane_input_enqueue(handle.clone(), key.id.clone(), 1, 2, b"B".to_vec());
        assert_eq!(orphaned["ok"], true);

        subscribe();
        let typed = pane_input_enqueue(handle.clone(), key.id.clone(), 1, 1, b"C".to_vec());
        assert_eq!(typed["ok"], true, "the new page's first keystroke: {typed}");
        assert_eq!(wait(&typed["ticket"]), json!({"ok":true}));
        assert_eq!(
            wait(&orphaned["ticket"]),
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

    #[cfg(unix)]
    #[test]
    fn blocked_command_input_does_not_starve_another_pane_or_output_ack() {
        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(0));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
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
                pane_input_enqueue(
                    handle.clone(),
                    blocked_key.id.clone(),
                    blocked_key.generation,
                    index + 1,
                    vec![b'x'; MAX_INPUT_BYTES],
                )
            })
            .collect::<Vec<_>>();
        thread::sleep(Duration::from_millis(500));

        let (responsive_sender, responsive_receiver) = mpsc::channel();
        let responsive_handle = handle.clone();
        let responsive_admission = pane_input_enqueue(
            handle.clone(),
            responsive_key.id.clone(),
            responsive_key.generation,
            1,
            b"R".to_vec(),
        );
        assert_eq!(responsive_admission["ok"], true);
        let responsive_ticket = responsive_admission["ticket"]
            .as_str()
            .expect("responsive input ticket")
            .to_string();
        let responsive_thread = thread::spawn(move || {
            let value = tauri::async_runtime::block_on(pane_input_wait(
                responsive_handle,
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
                "core_request",
                json!({"operation":"board.get","body":{"project":1}}),
                "board.get",
                json!({"project":1}),
            ),
            (
                "core_request",
                json!({"operation":"task.cancel","body":{"project":1,"task":1}}),
                "task.cancel",
                json!({"project":1,"task":1}),
            ),
        ];

        let (rust_stream, mut node_stream) = UnixStream::pair().expect("bridge socket pair");
        node_stream
            .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("write bridge handle");
        node_stream.flush().expect("flush bridge handle");
        let connected = BridgeBuilder::new(1024 * 1024)
            .connect(
                rust_stream.try_clone().expect("clone bridge socket"),
                rust_stream,
            )
            .expect("connect bridge");

        let expected = routes
            .iter()
            .map(|(_, _, operation, body)| ((*operation).to_string(), body.clone()))
            .collect::<Vec<_>>();
        let node = thread::spawn(move || {
            let mut reader = BufReader::new(node_stream.try_clone().expect("clone node reader"));
            for (operation, body) in expected {
                let mut line = String::new();
                reader.read_line(&mut line).expect("read command request");
                let frame: Value = serde_json::from_str(line.trim()).expect("command frame JSON");
                assert_eq!(frame["kind"], "req");
                assert_eq!(frame["op"], operation);
                assert_eq!(frame["body"], body);
                serde_json::to_writer(
                    &mut node_stream,
                    &json!({
                        "v":1,
                        "id":frame["id"],
                        "kind":"res",
                        "op":operation,
                        "body":{"ok":true},
                    }),
                )
                .expect("write command response");
                node_stream.write_all(b"\n").expect("terminate response");
                node_stream.flush().expect("flush command response");
            }
        });

        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(0));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), arbiter));
        let runtime = test_runtime(panes, inputs, None, Some(connected.bridge));
        let app = tauri::test::mock_builder()
            .manage(runtime)
            .invoke_handler(tauri::generate_handler![core_request])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("build mock webview");

        // These must be refused before requesting the bridge: the peer expects only valid routes.
        for (command, args) in [
            ("core_request", json!({"operation":"state.list","body":{}})),
            ("core_request", json!({"operation":"board.get","body":[1]})),
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

        node.join().expect("Node peer");
        drop(webview);
        drop(app);
    }
}
